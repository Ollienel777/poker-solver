//! `GameConfig`: one struct that describes a game, loaded from TOML.
//!
//! A straddle is one more live blind and an ante is a dead forced bet, so nothing downstream
//! needs to ask whether a game has either.

use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use thiserror::Error;

use crate::position::Position;
use crate::tree::Street;

/// An amount of chips. All pot and stack arithmetic is in whole chips.
pub type Chips = u32;

pub const CHIPS_PER_BB: Chips = 100;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("could not read {}: {source}", .path.display())]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("invalid TOML: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("`{field}` {reason}")]
    Invalid { field: String, reason: String },
}

/// A field that holds a value out of range. The loader and the tree builders share the checks
/// that produce it, and each turns it into its own error type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Invalid {
    pub(crate) field: String,
    pub(crate) reason: String,
}

impl Invalid {
    pub(crate) fn new(field: impl Into<String>, reason: impl Into<String>) -> Invalid {
        Invalid {
            field: field.into(),
            reason: reason.into(),
        }
    }
}

impl From<Invalid> for ConfigError {
    fn from(invalid: Invalid) -> ConfigError {
        ConfigError::Invalid {
            field: invalid.field,
            reason: invalid.reason,
        }
    }
}

fn invalid(field: impl Into<String>, reason: impl Into<String>) -> ConfigError {
    Invalid::new(field, reason).into()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ForcedBetKind {
    /// A live bet: it counts toward a call, and its poster still has an option to act.
    Blind,
    /// Dead money: it goes to the pot and never counts toward a call.
    Ante,
}

/// One forced bet from one seat.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ForcedBet {
    pub position: Position,
    pub amount: Chips,
    pub kind: ForcedBetKind,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Rake {
    /// Percent of the pot, from 0 to 100.
    pub percent: f64,
    pub cap: Chips,
    /// When true, a hand that ends before the flop pays no rake.
    pub no_flop_no_drop: bool,
}

/// Preflop raise sizes.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreflopSizing {
    /// The open raise, as a multiple of the effective blind (the largest live forced bet).
    pub open: f64,
    /// The open raise from the small blind.
    pub open_sb: f64,
    /// The 3-bet in position, as a multiple of the raise faced.
    pub threebet_ip: f64,
    /// The 3-bet out of position.
    pub threebet_oop: f64,
    /// The 4-bet, as a multiple of the 3-bet faced.
    pub fourbet: f64,
    /// All-in is offered once a raise would commit more than this fraction of the stack.
    pub allin_threshold: f64,
    pub limps: bool,
    pub max_raises: u32,
}

/// Bet and raise sizes for one street. [`crate::tree`] states exactly how they become actions.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StreetSizing {
    /// Bet sizes as fractions of the pot.
    pub bets: Vec<f64>,
    /// Raise sizes: a raise to this multiple of the bet faced. Empty means no sized raises.
    pub raises: Vec<f64>,
    /// When true, all-in is offered as one more bet and one more raise.
    pub allin: bool,
}

/// The sizing of a postflop betting tree: one [`StreetSizing`] per street, and two rules that
/// apply to every street.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PostflopSizing {
    pub flop: StreetSizing,
    pub turn: StreetSizing,
    pub river: StreetSizing,
    /// The most raises a street may have after its opening bet, all-in raises included. Once a
    /// street has had this many, a player facing a bet may only fold or call. 0 means no raises.
    pub max_raises: u32,
    /// A bet or raise that would leave a player with more than this fraction of the tree's
    /// starting effective stack put in becomes all-in. More than 0 and at most 1, where 1 means
    /// no size is ever converted this way.
    pub allin_threshold: f64,
}

impl PostflopSizing {
    /// The same sizing on every street, as in `[abstract_postflop]`.
    pub fn uniform(street: StreetSizing, max_raises: u32, allin_threshold: f64) -> PostflopSizing {
        PostflopSizing {
            flop: street.clone(),
            turn: street.clone(),
            river: street,
            max_raises,
            allin_threshold,
        }
    }

    /// The sizing for one street.
    pub fn street(&self, street: Street) -> &StreetSizing {
        match street {
            Street::Flop => &self.flop,
            Street::Turn => &self.turn,
            Street::River => &self.river,
        }
    }

    /// Checks every size and both rules. Fields are named under `prefix`, such as `postflop`.
    pub(crate) fn check(&self, prefix: &str) -> Result<(), Invalid> {
        check_street(&format!("{prefix}.flop"), &self.flop)?;
        check_street(&format!("{prefix}.turn"), &self.turn)?;
        check_street(&format!("{prefix}.river"), &self.river)?;
        // `max_raises` needs no check: every count from 0 up is a rule that can be applied.
        let threshold = self.allin_threshold;
        if !(threshold > 0.0 && threshold <= 1.0) {
            let reason = format!("is {threshold}; it must be more than 0 and at most 1");
            return Err(Invalid::new(format!("{prefix}.allin_threshold"), reason));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct GameConfig {
    pub name: String,
    pub description: String,
    pub players: usize,
    /// Each seat's stack before any forced bet. Blinds and antes come out of it.
    pub stack: Chips,
    /// Every forced bet, one entry per seat. An ante from `ALL` becomes one entry per seat.
    pub forced_bets: Vec<ForcedBet>,
    pub rake: Rake,
    pub preflop: PreflopSizing,
    /// Sizes for the exact postflop solver.
    pub postflop: PostflopSizing,
    /// Sizes for the small postflop tree inside the preflop solve.
    pub abstract_postflop: StreetSizing,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawConfig {
    name: String,
    #[serde(default)]
    description: String,
    players: usize,
    stack_bb: f64,
    forced_bets: Vec<RawForcedBet>,
    rake: RawRake,
    preflop: PreflopSizing,
    postflop: PostflopSizing,
    abstract_postflop: StreetSizing,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawForcedBet {
    /// A position name, or `ALL` for every seat.
    position: String,
    amount_bb: f64,
    kind: ForcedBetKind,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRake {
    percent: f64,
    cap_bb: f64,
    no_flop_no_drop: bool,
}

/// The largest amount a config may hold, in big blinds. At 9 seats, every stack plus every
/// forced bet then sums to well under `Chips::MAX`, so pot arithmetic cannot overflow.
pub(crate) const MAX_BB: f64 = 1_000_000.0;

/// Converts big blinds to chips, rejecting amounts that are negative, above [`MAX_BB`] or not
/// whole chips. Range files share it with the config loader.
pub(crate) fn to_chips(field: &str, bb: f64) -> Result<Chips, Invalid> {
    if !(0.0..=MAX_BB).contains(&bb) {
        let reason = format!("is {bb}; an amount must be from 0 to {MAX_BB}bb");
        return Err(Invalid::new(field, reason));
    }
    let chips = bb * f64::from(CHIPS_PER_BB);
    if (chips - chips.round()).abs() > 1e-6 {
        let reason = format!("is {bb}bb, which is not a whole number of chips (1bb = 100 chips)");
        return Err(Invalid::new(field, reason));
    }
    Ok(chips.round() as Chips)
}

/// The largest bet size, as a fraction of the pot. A larger number is almost certainly a percent
/// typed where a fraction belongs, such as 33 for 0.33.
pub const MAX_BET_FRACTION: f64 = 10.0;

/// The largest raise size, as a multiple of the bet or blind it raises. A larger number is
/// almost certainly a percent typed where a multiple belongs, such as 250 for 2.5.
pub const MAX_RAISE_MULTIPLE: f64 = 20.0;

fn check_multiple(field: &str, value: f64) -> Result<(), Invalid> {
    if value > MAX_RAISE_MULTIPLE {
        let reason = format!(
            "is {value}; a raise size is a multiple, at most {MAX_RAISE_MULTIPLE} (write 2.5, not 250)"
        );
        Err(Invalid::new(field, reason))
    } else if value.is_finite() && value > 1.0 {
        Ok(())
    } else {
        Err(Invalid::new(
            field,
            format!("is {value}; a raise size must be more than 1"),
        ))
    }
}

fn check_street(prefix: &str, street: &StreetSizing) -> Result<(), Invalid> {
    if street.bets.is_empty() {
        return Err(Invalid::new(
            format!("{prefix}.bets"),
            "is empty; a street needs at least one bet size",
        ));
    }
    if let Some(bet) = street
        .bets
        .iter()
        .find(|bet| !(bet.is_finite() && **bet > 0.0))
    {
        let reason = format!("has {bet}; a bet size must be a positive fraction of the pot");
        return Err(Invalid::new(format!("{prefix}.bets"), reason));
    }
    if let Some(bet) = street.bets.iter().find(|bet| **bet > MAX_BET_FRACTION) {
        let reason = format!(
            "has {bet}; a bet size is a fraction of the pot, at most {MAX_BET_FRACTION} (write 0.33 for 33%)"
        );
        return Err(Invalid::new(format!("{prefix}.bets"), reason));
    }
    for &raise in &street.raises {
        check_multiple(&format!("{prefix}.raises"), raise)?;
    }
    Ok(())
}

impl GameConfig {
    pub fn load(path: impl AsRef<Path>) -> Result<GameConfig, ConfigError> {
        let path = path.as_ref();
        let text = fs::read_to_string(path).map_err(|source| ConfigError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        GameConfig::from_toml(&text)
    }

    pub fn from_toml(text: &str) -> Result<GameConfig, ConfigError> {
        let raw: RawConfig = toml::from_str(text)?;

        let table = Position::table(raw.players).ok_or_else(|| {
            invalid(
                "players",
                format!("is {}; a table has 2 to 9 players", raw.players),
            )
        })?;

        let stack = to_chips("stack_bb", raw.stack_bb)?;

        let mut forced_bets: Vec<ForcedBet> = Vec::new();
        for (index, bet) in raw.forced_bets.iter().enumerate() {
            let amount_field = format!("forced_bets[{index}].amount_bb");
            let amount = to_chips(&amount_field, bet.amount_bb)?;
            if amount == 0 {
                return Err(invalid(amount_field, "is zero; leave the bet out instead"));
            }

            let position_field = format!("forced_bets[{index}].position");
            let seats: Vec<Position> = if bet.position.eq_ignore_ascii_case("ALL") {
                table.to_vec()
            } else {
                let position: Position = bet.position.parse().map_err(|_| {
                    invalid(
                        &position_field,
                        format!("is `{}`, which is not a position", bet.position),
                    )
                })?;
                if !table.contains(&position) {
                    let reason = format!(
                        "is {position}, which does not exist at a {}-handed table",
                        raw.players
                    );
                    return Err(invalid(position_field, reason));
                }
                vec![position]
            };

            for position in seats {
                if forced_bets
                    .iter()
                    .any(|b| b.position == position && b.kind == bet.kind)
                {
                    let kind = match bet.kind {
                        ForcedBetKind::Blind => "blind",
                        ForcedBetKind::Ante => "ante",
                    };
                    let reason = format!("gives {position} a second {kind}");
                    return Err(invalid(position_field, reason));
                }
                forced_bets.push(ForcedBet {
                    position,
                    amount,
                    kind: bet.kind,
                });
            }
        }

        let rake_percent = raw.rake.percent;
        if !(rake_percent.is_finite() && (0.0..100.0).contains(&rake_percent)) {
            return Err(invalid(
                "rake.percent",
                format!("is {rake_percent}; it must be from 0 to 100"),
            ));
        }
        let rake = Rake {
            percent: rake_percent,
            cap: to_chips("rake.cap_bb", raw.rake.cap_bb)?,
            no_flop_no_drop: raw.rake.no_flop_no_drop,
        };

        let config = GameConfig {
            name: raw.name,
            description: raw.description,
            players: raw.players,
            stack,
            forced_bets,
            rake,
            preflop: raw.preflop,
            postflop: raw.postflop,
            abstract_postflop: raw.abstract_postflop,
        };
        config.check_forced_bets(table)?;
        config.check_sizing()?;
        Ok(config)
    }

    fn check_forced_bets(&self, table: &[Position]) -> Result<(), ConfigError> {
        if !self
            .forced_bets
            .iter()
            .any(|bet| bet.kind == ForcedBetKind::Blind)
        {
            return Err(invalid(
                "forced_bets",
                "has no blind; someone must post a live bet",
            ));
        }
        for &position in table {
            if self.blind(position) + self.ante(position) >= self.stack {
                let reason = format!("does not cover the forced bets that {position} posts");
                return Err(invalid("stack_bb", reason));
            }
        }
        // The showdown assumes every seat can put the same amount in play (DECISIONS P7).
        let first_ante = self.ante(table[0]);
        if table
            .iter()
            .any(|&position| self.ante(position) != first_ante)
        {
            let reason = "leave unequal live stacks: every seat must post the same ante";
            return Err(invalid("forced_bets", reason));
        }
        Ok(())
    }

    fn check_sizing(&self) -> Result<(), ConfigError> {
        let preflop = &self.preflop;
        check_multiple("preflop.open", preflop.open)?;
        check_multiple("preflop.open_sb", preflop.open_sb)?;
        check_multiple("preflop.threebet_ip", preflop.threebet_ip)?;
        check_multiple("preflop.threebet_oop", preflop.threebet_oop)?;
        check_multiple("preflop.fourbet", preflop.fourbet)?;
        let threshold = preflop.allin_threshold;
        if !(threshold > 0.0 && threshold <= 1.0) {
            let reason = format!("is {threshold}; it must be more than 0 and at most 1");
            return Err(invalid("preflop.allin_threshold", reason));
        }
        if preflop.max_raises == 0 {
            return Err(invalid("preflop.max_raises", "is 0; it must be at least 1"));
        }

        self.postflop.check("postflop")?;
        check_street("abstract_postflop", &self.abstract_postflop)?;
        Ok(())
    }

    fn forced(&self, position: Position, kind: ForcedBetKind) -> Chips {
        self.forced_bets
            .iter()
            .find(|bet| bet.position == position && bet.kind == kind)
            .map_or(0, |bet| bet.amount)
    }

    /// The live blind a seat posts, or 0.
    pub fn blind(&self, position: Position) -> Chips {
        self.forced(position, ForcedBetKind::Blind)
    }

    /// The ante a seat posts, or 0.
    pub fn ante(&self, position: Position) -> Chips {
        self.forced(position, ForcedBetKind::Ante)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG_C: &str = include_str!("../../../configs/c.toml");

    fn load(name: &str) -> GameConfig {
        let path = format!("{}/../../configs/{name}.toml", env!("CARGO_MANIFEST_DIR"));
        GameConfig::load(&path).unwrap_or_else(|err| panic!("{path} should load: {err}"))
    }

    /// Replaces the first `from` in config C with `to` and returns the field the loader rejects.
    fn rejected_field(from: &str, to: &str) -> String {
        assert!(
            CONFIG_C.contains(from),
            "test setup: {from:?} is not in c.toml"
        );
        match GameConfig::from_toml(&CONFIG_C.replacen(from, to, 1)) {
            Err(ConfigError::Invalid { field, reason }) => {
                println!("`{field}` {reason}");
                field
            }
            other => panic!("expected an invalid-config error for {to:?}, got {other:?}"),
        }
    }

    #[test]
    fn all_four_configs_load() {
        for (name, stack_bb, cap_bb, forced) in [
            ("a", 100, 6, 2),
            ("b", 200, 6, 2),
            ("c", 100, 6, 11),
            ("d", 200, 6, 11),
        ] {
            let config = load(name);
            assert_eq!(config.name, name.to_uppercase());
            assert_eq!(config.players, 8);
            assert_eq!(config.stack, stack_bb * CHIPS_PER_BB);
            assert_eq!(config.forced_bets.len(), forced, "config {name}");
            assert_eq!(config.rake.percent, 5.0);
            assert_eq!(config.rake.cap, cap_bb * CHIPS_PER_BB);
            assert!(config.rake.no_flop_no_drop);
            assert_eq!(config.preflop.open, 2.5);
            assert_eq!(config.postflop.river.bets, [0.33, 0.75]);
            assert!(config.postflop.river.allin && !config.postflop.flop.allin);
            assert_eq!(config.postflop.max_raises, 3);
            assert_eq!(config.postflop.allin_threshold, 0.67);
            assert_eq!(config.abstract_postflop.bets, [0.75]);
        }
    }

    #[test]
    fn forced_bets_per_seat() {
        let a = load("a");
        assert_eq!(a.blind(Position::Sb), 50);
        assert_eq!(a.blind(Position::Bb), 100);
        assert_eq!(a.blind(Position::Utg), 0);
        assert_eq!(a.ante(Position::Bb), 0);

        let c = load("c");
        assert_eq!(c.blind(Position::Sb), 50);
        assert_eq!(c.blind(Position::Bb), 100);
        assert_eq!(c.blind(Position::Utg), 200);
        assert_eq!(c.blind(Position::Btn), 0);
        for &position in Position::table(8).unwrap() {
            assert_eq!(c.ante(position), 50, "{position}");
        }
    }

    #[test]
    fn players_must_be_two_to_nine() {
        assert_eq!(rejected_field("players = 8", "players = 10"), "players");
        assert_eq!(rejected_field("players = 8", "players = 1"), "players");
    }

    #[test]
    fn positions_must_exist_at_the_table() {
        assert_eq!(
            rejected_field("position = \"UTG\"", "position = \"MP\""),
            "forced_bets[2].position"
        );
        // There is no UTG seat at a 5-handed table.
        assert_eq!(
            rejected_field("players = 8", "players = 5"),
            "forced_bets[2].position"
        );
    }

    #[test]
    fn amounts_too_large_to_add_up_are_rejected() {
        // 42,949,672.5bb fits in `Chips` by itself, but not once the 0.5bb ante is added.
        assert_eq!(
            rejected_field("amount_bb = 1.0", "amount_bb = 42949672.5"),
            "forced_bets[1].amount_bb"
        );
        assert_eq!(
            rejected_field("stack_bb = 100.0", "stack_bb = 1000000.01"),
            "stack_bb"
        );
        assert_eq!(
            rejected_field("stack_bb = 100.0", "stack_bb = inf"),
            "stack_bb"
        );
        assert_eq!(
            rejected_field("stack_bb = 100.0", "stack_bb = nan"),
            "stack_bb"
        );

        // The largest stack loads, and its totals stay in range.
        let text = CONFIG_C.replacen("stack_bb = 100.0", "stack_bb = 1000000.0", 1);
        let config = GameConfig::from_toml(&text).unwrap();
        assert_eq!(config.stack, 100_000_000);
        assert_eq!(config.stack_behind(Position::Utg), 100_000_000 - 250);
    }

    #[test]
    fn amounts_must_be_whole_chips() {
        assert_eq!(
            rejected_field("amount_bb = 0.5", "amount_bb = 0.505"),
            "forced_bets[0].amount_bb"
        );
        assert_eq!(
            rejected_field("amount_bb = 0.5", "amount_bb = -1.0"),
            "forced_bets[0].amount_bb"
        );
        assert_eq!(
            rejected_field("amount_bb = 0.5", "amount_bb = 0.0"),
            "forced_bets[0].amount_bb"
        );
        assert_eq!(
            rejected_field("stack_bb = 100.0", "stack_bb = 100.005"),
            "stack_bb"
        );
        assert_eq!(
            rejected_field("cap_bb = 6.0", "cap_bb = 6.001"),
            "rake.cap_bb"
        );
    }

    #[test]
    fn the_stack_must_cover_the_forced_bets() {
        // UTG posts a 2bb straddle and a 0.5bb ante.
        assert_eq!(
            rejected_field("stack_bb = 100.0", "stack_bb = 2.5"),
            "stack_bb"
        );
        assert_eq!(
            rejected_field("stack_bb = 100.0", "stack_bb = 0.0"),
            "stack_bb"
        );
    }

    #[test]
    fn forced_bets_must_leave_equal_live_stacks() {
        // A big blind ante would leave the BB with a shorter live stack than the rest.
        assert_eq!(
            rejected_field("position = \"ALL\"", "position = \"BB\""),
            "forced_bets"
        );
    }

    #[test]
    fn forced_bets_cannot_repeat_or_lack_a_blind() {
        let second_small_blind = "[rake]";
        let with_repeat =
            "[[forced_bets]]\nposition = \"SB\"\namount_bb = 1.0\nkind = \"blind\"\n\n[rake]";
        assert_eq!(
            rejected_field(second_small_blind, with_repeat),
            "forced_bets[4].position"
        );

        let antes_only = CONFIG_C.replace("kind = \"blind\"", "kind = \"ante\"");
        match GameConfig::from_toml(&antes_only) {
            Err(ConfigError::Invalid { field, .. }) => assert_eq!(field, "forced_bets[3].position"),
            other => panic!("expected an invalid-config error, got {other:?}"),
        }
        let no_blind = "name = \"X\"\nplayers = 2\nstack_bb = 10.0\nforced_bets = []\n";
        let rest = &CONFIG_C[CONFIG_C.find("[rake]").unwrap()..];
        match GameConfig::from_toml(&format!("{no_blind}\n{rest}")) {
            Err(ConfigError::Invalid { field, .. }) => assert_eq!(field, "forced_bets"),
            other => panic!("expected an invalid-config error, got {other:?}"),
        }
    }

    #[test]
    fn sizing_lists_and_values_are_checked() {
        assert_eq!(
            rejected_field("bets = [0.75]", "bets = []"),
            "abstract_postflop.bets"
        );
        let flop = "flop = { bets = [0.33, 0.75]";
        assert_eq!(
            rejected_field(flop, "flop = { bets = []"),
            "postflop.flop.bets"
        );
        assert_eq!(
            rejected_field(flop, "flop = { bets = [0.33, -0.5]"),
            "postflop.flop.bets"
        );
        assert_eq!(
            rejected_field(
                "raises = [3.0], allin = true",
                "raises = [0.9], allin = true"
            ),
            "postflop.river.raises"
        );
        assert_eq!(rejected_field("open = 2.5", "open = 0.5"), "preflop.open");
        // Percents typed where a fraction or a multiple belongs.
        assert_eq!(
            rejected_field(flop, "flop = { bets = [33.0, 75.0]"),
            "postflop.flop.bets"
        );
        assert_eq!(
            rejected_field(
                "raises = [3.0], allin = true",
                "raises = [250.0], allin = true"
            ),
            "postflop.river.raises"
        );
        assert_eq!(rejected_field("open = 2.5", "open = 250.0"), "preflop.open");
        assert_eq!(
            rejected_field("fourbet = 2.3", "fourbet = 1.0"),
            "preflop.fourbet"
        );
        assert_eq!(
            rejected_field("allin_threshold = 0.40", "allin_threshold = 1.5"),
            "preflop.allin_threshold"
        );
        assert_eq!(
            rejected_field("max_raises = 4", "max_raises = 0"),
            "preflop.max_raises"
        );
        assert_eq!(
            rejected_field("percent = 5.0", "percent = 150.0"),
            "rake.percent"
        );
    }

    #[test]
    fn the_postflop_threshold_is_more_than_zero_and_at_most_one() {
        for bad in ["0.0", "-0.5", "1.01", "nan"] {
            assert_eq!(
                rejected_field(
                    "allin_threshold = 0.67",
                    &format!("allin_threshold = {bad}")
                ),
                "postflop.allin_threshold"
            );
        }
        let never = CONFIG_C.replacen("allin_threshold = 0.67", "allin_threshold = 1.0", 1);
        assert_eq!(
            GameConfig::from_toml(&never)
                .unwrap()
                .postflop
                .allin_threshold,
            1.0
        );
    }

    #[test]
    fn the_postflop_raise_cap_may_be_zero_but_not_negative() {
        let no_raises = CONFIG_C.replacen("max_raises = 3", "max_raises = 0", 1);
        assert_ne!(no_raises, CONFIG_C);
        assert_eq!(
            GameConfig::from_toml(&no_raises)
                .unwrap()
                .postflop
                .max_raises,
            0
        );

        let negative = CONFIG_C.replacen("max_raises = 3", "max_raises = -1", 1);
        match GameConfig::from_toml(&negative) {
            Err(ConfigError::Toml(error)) => {
                let message = error.to_string();
                println!("{message}");
                assert!(message.contains("max_raises = -1"), "{message}");
            }
            other => panic!("expected a TOML error, got {other:?}"),
        }
    }

    #[test]
    fn the_postflop_rules_are_required() {
        for line in ["max_raises = 3", "allin_threshold = 0.67"] {
            let missing = CONFIG_C.replacen(line, "", 1);
            assert_ne!(missing, CONFIG_C);
            assert!(matches!(
                GameConfig::from_toml(&missing),
                Err(ConfigError::Toml(_))
            ));
        }
    }

    #[test]
    fn a_street_may_have_no_raises() {
        let no_raises = CONFIG_C.replacen(
            "bets = [0.75]\nraises = [3.0]",
            "bets = [0.75]\nraises = []",
            1,
        );
        assert_ne!(no_raises, CONFIG_C);
        let config = GameConfig::from_toml(&no_raises).unwrap();
        assert!(config.abstract_postflop.raises.is_empty());
    }

    #[test]
    fn malformed_toml_is_a_toml_error() {
        let unknown_field = CONFIG_C.replacen("players = 8", "players = 8\nseats = 8", 1);
        assert!(matches!(
            GameConfig::from_toml(&unknown_field),
            Err(ConfigError::Toml(_))
        ));
        let missing_field = CONFIG_C.replacen("players = 8\n", "", 1);
        assert!(matches!(
            GameConfig::from_toml(&missing_field),
            Err(ConfigError::Toml(_))
        ));
        let bad_kind = CONFIG_C.replacen("kind = \"ante\"", "kind = \"dead\"", 1);
        assert!(matches!(
            GameConfig::from_toml(&bad_kind),
            Err(ConfigError::Toml(_))
        ));
        assert!(matches!(
            GameConfig::from_toml("name = "),
            Err(ConfigError::Toml(_))
        ));
    }

    #[test]
    fn a_missing_file_is_an_io_error() {
        let error = GameConfig::load("configs/does-not-exist.toml").unwrap_err();
        assert!(matches!(error, ConfigError::Io { .. }));
        assert!(error.to_string().contains("does-not-exist.toml"));
    }
}
