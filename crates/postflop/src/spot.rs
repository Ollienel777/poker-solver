//! Spots: the whole input of a heads-up postflop solve.
//!
//! A [`Spot`] holds the board, each player's range, the pot and the effective stack, the sizing
//! of the betting tree and the rake. It is plain data with public fields, and it is the contract
//! between the test spots under `reference/spots/` and the solve loop.
//!
//! # The file
//!
//! A spot file is TOML. It is meant to be typed by hand, so that the same spot can be set up in
//! a reference solver from what the file says:
//!
//! ```toml
//! format = 1
//! name = "example"
//! description = "Illustrative ranges, not from a reference."
//! board = "Ks9h7d4c2s"
//! pot_bb = 20.0
//! effective_stack_bb = 80.0
//! oop_range = "TT-QQ,AKs,KQs,K9s,AKo:0.5"
//! ip_range = "KK,99,AKs,KJs,QJs,T8s:0.5"
//!
//! [rake]
//! percent = 5.0
//! cap_bb = 3.0
//!
//! [tree]
//! max_raises = 1
//! allin_threshold = 1.0
//! river = { bets = [0.5, 1.0], raises = [3.0], allin = false }
//! ```
//!
//! # Fields
//!
//! Every key is required except the street sizings, and no other key is allowed.
//!
//! - `format` is the integer 1.
//! - `name` names the spot, and `description` says what it is for and where its ranges came
//!   from. Neither may be blank.
//! - `board` is 3, 4 or 5 distinct cards, written one after another as in `Ks9h7d4c2s`. Its
//!   length sets the street the spot starts on: the flop, the turn or the river.
//! - `pot_bb` is the pot when the spot starts and `effective_stack_bb` what each player has left
//!   to bet, both in big blinds. Each must be a positive whole number of chips (1bb = 100 chips),
//!   at most 1,000,000bb. They may be written as integers or decimals.
//! - `oop_range` is the range of the player who acts first and `ip_range` the range of the
//!   player who acts last, in the range-string syntax of [`pk_core::range`].
//! - `[rake]` has `percent`, at least 0 and less than 100, and `cap_bb`, a whole number of chips
//!   from 0 to 1,000,000bb. A rake-free spot writes `percent = 0.0`.
//! - `[tree]` has the two rules of the config's `[postflop]` table, `max_raises` and
//!   `allin_threshold`, and a sizing for each street the spot plays: `river` alone for a river
//!   spot, `turn` and `river` for a turn spot, all three for a flop spot. Each street sizing is
//!   `{ bets, raises, allin }`, as in the config. [`pk_game::tree`] states how they become
//!   actions.
//!
//! # Checks
//!
//! [`Spot::from_toml`], [`Spot::load`] and [`Spot::validate`] reject a spot for:
//!
//! - a format other than 1, a missing or unknown key, or a value of the wrong type
//! - a blank `name` or `description`
//! - a board that does not parse, repeats a card, or has other than 3, 4 or 5 cards
//! - a pot or stack that is not a positive whole number of chips, at most 1,000,000bb
//! - a range that does not parse, or holds no hand once the board's cards are removed
//! - two ranges with no pair of hands, one from each, that can be dealt together
//! - a rake percent outside 0 to 100, a cap that is not a whole number of chips, or a cap of 0
//!   under a positive percent, which would take no rake at all
//! - an all-in threshold that is not more than 0 and at most 1, a bet size that is not a positive
//!   fraction of the pot, a raise size that is not more than 1, or an empty list of bets: what
//!   the tree builder rejects
//! - a missing sizing for a street the spot plays, or a sizing for a street before its board
//!
//! Each error names the field, such as `ip_range` or `tree.river.bets`, and says what is wrong.
//! Errors come in the order of the layout, except that the format version is checked first and
//! unknown keys before missing ones. A range string that does not parse passes on the range
//! parser's message.
//!
//! # Writing
//!
//! [`Spot::to_toml`] writes the layout above: the keys in that order, the board from its highest
//! card to its lowest, each range in its canonical form (`Display` for [`Range`]), amounts in big
//! blinds as decimals, and sizes as the shortest decimals that read back exactly. Reading what
//! was written gives back an equal spot, and a spot read from a file in this layout writes back
//! the same text.

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use pk_core::{CardSet, Range};
use pk_game::{
    CHIPS_PER_BB, Chips, MAX_BET_FRACTION, MAX_RAISE_MULTIPLE, Player, PostflopSizing,
    PostflopTree, Rake, Street, StreetSizing, TreeError, TreeSpec,
};
use thiserror::Error;
use toml::{Table, Value};

use crate::terminal::BoardHands;

/// The version of the spot-file format that this module reads and writes.
pub const SPOT_FORMAT: i64 = 1;

/// The keys of a spot file, in the order they are written.
const KEYS: [&str; 10] = [
    "format",
    "name",
    "description",
    "board",
    "pot_bb",
    "effective_stack_bb",
    "oop_range",
    "ip_range",
    "rake",
    "tree",
];

const RAKE_KEYS: [&str; 2] = ["percent", "cap_bb"];
const TREE_KEYS: [&str; 5] = ["max_raises", "allin_threshold", "flop", "turn", "river"];
const STREET_KEYS: [&str; 3] = ["bets", "raises", "allin"];

/// The largest amount a spot may hold, in big blinds: the same as a config's or a range file's.
const MAX_BB: Chips = 1_000_000;
const MAX_CHIPS: Chips = MAX_BB * CHIPS_PER_BB;

// Amounts are written with two decimals at most, and the largest pot and stacks add up to well
// under `Chips::MAX`, so the tree builder's arithmetic cannot overflow.
const _: () = assert!(CHIPS_PER_BB == 100);
const _: () = assert!(3 * (MAX_CHIPS as u64) < Chips::MAX as u64);

#[derive(Debug, Error)]
pub enum SpotError {
    #[error("could not read {}: {source}", .path.display())]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    /// The text is not valid TOML.
    #[error("invalid TOML: {0}")]
    Toml(#[from] toml::de::Error),
    /// A field is missing, unknown, of the wrong type, or holds a value that is not allowed.
    #[error("`{field}` {reason}")]
    Invalid { field: String, reason: String },
}

fn invalid(field: impl Into<String>, reason: impl Into<String>) -> SpotError {
    SpotError::Invalid {
        field: field.into(),
        reason: reason.into(),
    }
}

/// The sizing of a spot's betting tree: the two rules of the config's `[postflop]` table, and a
/// [`StreetSizing`] for each street the spot plays.
///
/// A river spot sizes only the river, a turn spot the turn and the river, a flop spot all three.
/// The streets before the spot's board are `None`.
#[derive(Debug, Clone, PartialEq)]
pub struct TreeSizing {
    /// The most raises a street may have after its opening bet, all-in raises included.
    pub max_raises: u32,
    /// A bet or raise that would leave a player with more than this fraction of the effective
    /// stack put in becomes all-in. 1 converts nothing.
    pub allin_threshold: f64,
    pub flop: Option<StreetSizing>,
    pub turn: Option<StreetSizing>,
    pub river: Option<StreetSizing>,
}

impl TreeSizing {
    /// The sizing for one street, if the spot has one.
    pub fn street(&self, street: Street) -> Option<&StreetSizing> {
        match street {
            Street::Flop => self.flop.as_ref(),
            Street::Turn => self.turn.as_ref(),
            Street::River => self.river.as_ref(),
        }
    }

    /// The sizing in the form the tree builder takes, for a tree whose root is on `start`.
    /// `None` if a street from `start` on has no sizing.
    ///
    /// A [`PostflopSizing`] has a field for every street, but a tree never reaches the streets
    /// before its root. Those fields hold a copy of the root street's sizing.
    pub fn postflop_sizing(&self, start: Street) -> Option<PostflopSizing> {
        let first = self.street(start)?;
        let sizing = |street: Street| {
            if street < start {
                Some(first.clone())
            } else {
                self.street(street).cloned()
            }
        };
        Some(PostflopSizing {
            flop: sizing(Street::Flop)?,
            turn: sizing(Street::Turn)?,
            river: sizing(Street::River)?,
            max_raises: self.max_raises,
            allin_threshold: self.allin_threshold,
        })
    }
}

/// Everything a heads-up postflop solve starts from. The [module documentation](self)
/// describes the file.
#[derive(Debug, Clone, PartialEq)]
pub struct Spot {
    pub name: String,
    /// What the spot is for, and where its ranges came from.
    pub description: String,
    /// The 3, 4 or 5 cards dealt so far. Their number sets the street the spot starts on.
    pub board: CardSet,
    /// The pot when the spot starts.
    pub pot: Chips,
    /// What each player has left to bet when the spot starts.
    pub effective_stack: Chips,
    /// The range of the player who acts first on every street.
    pub oop_range: Range,
    /// The range of the player who acts last on every street.
    pub ip_range: Range,
    /// The rake as a percent of the pot, at least 0 and less than 100.
    pub rake_percent: f64,
    /// The most rake taken from one pot.
    pub rake_cap: Chips,
    pub tree: TreeSizing,
}

impl Spot {
    pub fn load(path: impl AsRef<Path>) -> Result<Spot, SpotError> {
        let path = path.as_ref();
        let text = fs::read_to_string(path).map_err(|source| SpotError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        Spot::from_toml(&text)
    }

    /// Reads a spot file and makes every check of [`Spot::validate`], in the order of the
    /// layout. The format version is checked first, so that a file from a later format is
    /// reported as one, and unknown keys before missing ones.
    pub fn from_toml(text: &str) -> Result<Spot, SpotError> {
        let table: Table = toml::from_str(text)?;
        check_format(&table)?;
        let top = Fields::new(&table, None, &KEYS)?;

        let name = top.text("name")?;
        let description = top.text("description")?;
        let board = top.board("board")?;
        let street = check_board(board)?;
        let pot = top.amount("pot_bb")?;
        check_amount("pot_bb", pot)?;
        let effective_stack = top.amount("effective_stack_bb")?;
        check_amount("effective_stack_bb", effective_stack)?;
        let oop_range = top.range("oop_range")?;
        check_range("oop_range", &oop_range, board)?;
        let ip_range = top.range("ip_range")?;
        check_range("ip_range", &ip_range, board)?;
        check_pair(board, &oop_range, &ip_range)?;

        let rake = top.table("rake", &RAKE_KEYS)?;
        let rake_percent = rake.number("percent")?;
        let rake_cap = rake.amount("cap_bb")?;
        check_rake(rake_percent, rake_cap)?;

        let tree = top.table("tree", &TREE_KEYS)?;
        let sizing = TreeSizing {
            max_raises: tree.count("max_raises")?,
            allin_threshold: tree.number("allin_threshold")?,
            flop: tree.street("flop")?,
            turn: tree.street("turn")?,
            river: tree.street("river")?,
        };
        check_tree(&sizing, street)?;

        let spot = Spot {
            name,
            description,
            board,
            pot,
            effective_stack,
            oop_range,
            ip_range,
            rake_percent,
            rake_cap,
            tree: sizing,
        };
        debug_assert!(spot.validate().is_ok());
        Ok(spot)
    }

    /// The spot as TOML, in the layout the [module documentation](self) shows. Reading the text
    /// back gives an equal spot, for any spot that passes [`Spot::validate`].
    pub fn to_toml(&self) -> String {
        self.to_string()
    }

    /// Makes the checks the [module documentation](self) lists on a spot built in code, in the
    /// order of the layout, and names the first field that fails as the file would.
    pub fn validate(&self) -> Result<(), SpotError> {
        check_text("name", &self.name)?;
        check_text("description", &self.description)?;
        let street = check_board(self.board)?;
        check_amount("pot_bb", self.pot)?;
        check_amount("effective_stack_bb", self.effective_stack)?;
        check_range("oop_range", &self.oop_range, self.board)?;
        check_range("ip_range", &self.ip_range, self.board)?;
        check_pair(self.board, &self.oop_range, &self.ip_range)?;
        check_rake(self.rake_percent, self.rake_cap)?;
        check_tree(&self.tree, street)
    }

    /// The street the spot starts on: the flop, turn or river for a board of 3, 4 or 5 cards.
    ///
    /// Panics if the board has another number of cards. [`Spot::validate`] checks it.
    pub fn street(&self) -> Street {
        street_of(self.board).unwrap_or_else(|| {
            panic!(
                "a spot's board has 3, 4 or 5 cards, not {}",
                self.board.len()
            )
        })
    }

    /// The range of one player: [`Player::Oop`] for `oop_range` and [`Player::Ip`] for
    /// `ip_range`.
    pub fn range(&self, player: Player) -> &Range {
        match player {
            Player::Oop => &self.oop_range,
            Player::Ip => &self.ip_range,
        }
    }

    /// The rake, for [`Payoff::new`](crate::Payoff::new) and the other payoff functions.
    ///
    /// `no_flop_no_drop` is set, as in every config. It makes no difference here: a spot starts
    /// after the flop, so every pot it reaches is raked.
    pub fn rake(&self) -> Rake {
        Rake {
            percent: self.rake_percent,
            cap: self.rake_cap,
            no_flop_no_drop: true,
        }
    }

    /// What the spot's betting tree is built from, with the default node limit.
    ///
    /// Panics if the board does not have 3, 4 or 5 cards, or if a street the spot plays has no
    /// sizing. [`Spot::validate`] checks both.
    pub fn tree_spec(&self) -> TreeSpec {
        let street = self.street();
        let sizing = self.tree.postflop_sizing(street).unwrap_or_else(|| {
            panic!("a spot that starts on the {street} needs a sizing for every street it plays")
        });
        TreeSpec::new(street, self.pot, self.effective_stack, sizing)
    }

    /// Builds the spot's betting tree from [`Spot::tree_spec`]. For a valid spot, the only error
    /// is a tree with more nodes than the default limit.
    pub fn build_tree(&self) -> Result<PostflopTree, TreeError> {
        PostflopTree::build(self.tree_spec())
    }

    /// Both players' hands on the board, for the terminal evaluations.
    ///
    /// Panics if the board does not have 3, 4 or 5 cards.
    pub fn board_hands(&self) -> BoardHands {
        BoardHands::new(self.board, &self.oop_range, &self.ip_range)
    }
}

/// The canonical TOML text of the spot. [`Spot::to_toml`] returns it.
impl fmt::Display for Spot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "format = {SPOT_FORMAT}")?;
        writeln!(f, "name = {}", quote(&self.name))?;
        writeln!(f, "description = {}", quote(&self.description))?;
        writeln!(f, "board = {}", quote(&self.board.to_string()))?;
        writeln!(f, "pot_bb = {}", decimal_bb(self.pot))?;
        writeln!(
            f,
            "effective_stack_bb = {}",
            decimal_bb(self.effective_stack)
        )?;
        writeln!(f, "oop_range = {}", quote(&self.oop_range.to_string()))?;
        writeln!(f, "ip_range = {}", quote(&self.ip_range.to_string()))?;

        writeln!(f)?;
        writeln!(f, "[rake]")?;
        writeln!(f, "percent = {}", float(self.rake_percent))?;
        writeln!(f, "cap_bb = {}", decimal_bb(self.rake_cap))?;

        writeln!(f)?;
        writeln!(f, "[tree]")?;
        writeln!(f, "max_raises = {}", self.tree.max_raises)?;
        writeln!(f, "allin_threshold = {}", float(self.tree.allin_threshold))?;
        for street in Street::ALL {
            if let Some(sizing) = self.tree.street(street) {
                writeln!(
                    f,
                    "{street} = {{ bets = {}, raises = {}, allin = {} }}",
                    floats(&sizing.bets),
                    floats(&sizing.raises),
                    sizing.allin
                )?;
            }
        }
        Ok(())
    }
}

/// The street a board of 3, 4 or 5 cards starts on.
fn street_of(board: CardSet) -> Option<Street> {
    match board.len() {
        3 => Some(Street::Flop),
        4 => Some(Street::Turn),
        5 => Some(Street::River),
        _ => None,
    }
}

// ---------------------------------------------------------------------------------------------
// Checks on values, shared by `from_toml` and `validate`

fn check_format(table: &Table) -> Result<(), SpotError> {
    let reason = match table.get("format") {
        Some(Value::Integer(SPOT_FORMAT)) => return Ok(()),
        Some(Value::Integer(version)) => {
            format!("is {version}; this solver reads format {SPOT_FORMAT} only")
        }
        Some(other) => format!("is {}; it must be the integer {SPOT_FORMAT}", kind(other)),
        None => format!("is missing; a spot file starts with `format = {SPOT_FORMAT}`"),
    };
    Err(invalid("format", reason))
}

fn check_text(field: &str, text: &str) -> Result<(), SpotError> {
    if text.trim().is_empty() {
        return Err(invalid(field, "is blank"));
    }
    Ok(())
}

/// Checks the board's length and returns the street it starts on.
fn check_board(board: CardSet) -> Result<Street, SpotError> {
    street_of(board).ok_or_else(|| {
        let reason = format!(
            "has {} cards; a board has 3 (a flop), 4 (a turn) or 5 (a river)",
            board.len()
        );
        invalid("board", reason)
    })
}

fn check_amount(field: &str, chips: Chips) -> Result<(), SpotError> {
    if chips == 0 {
        return Err(invalid(field, "is 0; it must be more than 0"));
    }
    check_at_most_max(field, chips)
}

fn check_at_most_max(field: &str, chips: Chips) -> Result<(), SpotError> {
    if chips > MAX_CHIPS {
        let reason = format!(
            "is {}bb; an amount must be at most {MAX_BB}bb",
            bb(chips.into())
        );
        return Err(invalid(field, reason));
    }
    Ok(())
}

fn check_range(field: &str, range: &Range, board: CardSet) -> Result<(), SpotError> {
    if range.is_empty() {
        return Err(invalid(field, "holds no hands"));
    }
    if range.iter().all(|(combo, _)| combo.intersects(board)) {
        let reason = format!(
            "holds no hand that can be dealt on this board: every combo in it holds a card of \
             {board}"
        );
        return Err(invalid(field, reason));
    }
    Ok(())
}

/// Checks that some hand of OOP's and some hand of IP's can be dealt together: neither holds a
/// board card, and they share no card.
fn check_pair(board: CardSet, oop: &Range, ip: &Range) -> Result<(), SpotError> {
    let dealt = |range: &Range| -> Vec<CardSet> {
        range
            .iter()
            .map(|(combo, _)| combo.cards())
            .filter(|cards| !cards.intersects(board))
            .collect()
    };
    let (oop, ip) = (dealt(oop), dealt(ip));
    if oop.iter().any(|a| ip.iter().any(|b| !a.intersects(*b))) {
        return Ok(());
    }
    Err(invalid(
        "ip_range",
        "has no hand that can be dealt together with a hand of `oop_range`: every pair of \
         hands, one from each range, shares a card",
    ))
}

fn check_rake(percent: f64, cap: Chips) -> Result<(), SpotError> {
    if !(percent.is_finite() && (0.0..100.0).contains(&percent)) {
        let reason = format!("is {percent}; it must be at least 0 and less than 100");
        return Err(invalid("rake.percent", reason));
    }
    check_at_most_max("rake.cap_bb", cap)?;
    if percent > 0.0 && cap == 0 {
        let reason = format!(
            "is 0, so the {percent}% rake would take nothing; a rake-free spot sets `percent = \
             0.0` instead"
        );
        return Err(invalid("rake.cap_bb", reason));
    }
    Ok(())
}

/// Checks the tree's rules and sizes as the tree builder does, and that the spot sizes exactly
/// the streets it plays from `start` on.
fn check_tree(tree: &TreeSizing, start: Street) -> Result<(), SpotError> {
    let threshold = tree.allin_threshold;
    if !(threshold > 0.0 && threshold <= 1.0) {
        let reason = format!("is {threshold}; it must be more than 0 and at most 1");
        return Err(invalid("tree.allin_threshold", reason));
    }
    // `max_raises` needs no check: every count from 0 up is a rule the builder can apply.
    for street in Street::ALL {
        let field = format!("tree.{street}");
        match (tree.street(street), street < start) {
            (Some(_), true) => {
                let reason = format!(
                    "is given, but this spot starts on the {start} and never plays the {street}; \
                     leave it out"
                );
                return Err(invalid(field, reason));
            }
            (None, false) => {
                let reason = format!(
                    "is missing; a spot that starts on the {start} plays the {street} and needs \
                     its sizing"
                );
                return Err(invalid(field, reason));
            }
            (Some(sizing), false) => check_street(&field, sizing)?,
            (None, true) => {}
        }
    }
    Ok(())
}

/// The rules the config loader and the tree builder apply to a street's sizes.
fn check_street(field: &str, sizing: &StreetSizing) -> Result<(), SpotError> {
    if sizing.bets.is_empty() {
        return Err(invalid(
            format!("{field}.bets"),
            "is empty; a street needs at least one bet size",
        ));
    }
    if let Some(bet) = sizing
        .bets
        .iter()
        .find(|bet| !(bet.is_finite() && **bet > 0.0))
    {
        let reason = format!("has {bet}; a bet size must be a positive fraction of the pot");
        return Err(invalid(format!("{field}.bets"), reason));
    }
    if let Some(bet) = sizing.bets.iter().find(|bet| **bet > MAX_BET_FRACTION) {
        let reason = format!(
            "has {bet}; a bet size is a fraction of the pot, at most {MAX_BET_FRACTION} (write 0.33 for 33%)"
        );
        return Err(invalid(format!("{field}.bets"), reason));
    }
    if let Some(raise) = sizing
        .raises
        .iter()
        .find(|raise| **raise > MAX_RAISE_MULTIPLE)
    {
        let reason = format!(
            "has {raise}; a raise size is a multiple, at most {MAX_RAISE_MULTIPLE} (write 2.5, not 250)"
        );
        return Err(invalid(format!("{field}.raises"), reason));
    }
    if let Some(raise) = sizing
        .raises
        .iter()
        .find(|raise| !(raise.is_finite() && **raise > 1.0))
    {
        let reason = format!("has {raise}; a raise size must be more than 1");
        return Err(invalid(format!("{field}.raises"), reason));
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Reading values of the right type

/// The fields of one table: the top level, `[rake]`, `[tree]` or a street sizing. Each is named
/// by its path, such as `pot_bb`, `rake.cap_bb` or `tree.river.bets`.
struct Fields<'a> {
    table: &'a Table,
    /// `""` at the top level, `"tree."` inside `[tree]`.
    prefix: String,
}

impl<'a> Fields<'a> {
    /// The fields of `table`, which has the path `path`, or is the top level for `None`.
    /// Rejects any key not in `keys`.
    fn new(table: &'a Table, path: Option<&str>, keys: &[&str]) -> Result<Fields<'a>, SpotError> {
        let prefix = path.map_or_else(String::new, |path| format!("{path}."));
        if let Some(key) = table.keys().find(|key| !keys.contains(&key.as_str())) {
            let reason = match path {
                None => format!("is not a spot-file key; the keys are {}", list(keys)),
                Some(path) if path.contains('.') => {
                    format!("is not a key of `{path}`; its keys are {}", list(keys))
                }
                Some(path) => format!("is not a key of `[{path}]`; its keys are {}", list(keys)),
            };
            return Err(invalid(format!("{prefix}{key}"), reason));
        }
        Ok(Fields { table, prefix })
    }

    fn name(&self, key: &str) -> String {
        format!("{}{key}", self.prefix)
    }

    fn get(&self, key: &str) -> Result<&'a Value, SpotError> {
        self.table
            .get(key)
            .ok_or_else(|| invalid(self.name(key), "is missing"))
    }

    fn wrong_type(&self, key: &str, value: &Value, wanted: &str) -> SpotError {
        invalid(
            self.name(key),
            format!("is {}; it must be {wanted}", kind(value)),
        )
    }

    fn string(&self, key: &str) -> Result<&'a str, SpotError> {
        match self.get(key)? {
            Value::String(text) => Ok(text),
            other => Err(self.wrong_type(key, other, "a string in quotes")),
        }
    }

    fn text(&self, key: &str) -> Result<String, SpotError> {
        let text = self.string(key)?;
        check_text(&self.name(key), text)?;
        Ok(text.to_string())
    }

    fn board(&self, key: &str) -> Result<CardSet, SpotError> {
        let text = self.string(key)?;
        text.parse()
            .map_err(|err| invalid(self.name(key), format!("is not a valid board: {err}")))
    }

    fn range(&self, key: &str) -> Result<Range, SpotError> {
        let text = self.string(key)?;
        text.parse()
            .map_err(|err| invalid(self.name(key), format!("is not a valid range: {err}")))
    }

    fn number(&self, key: &str) -> Result<f64, SpotError> {
        match self.get(key)? {
            // Adding 0 turns -0.0 into 0.0, so that it is written back as `0.0`.
            Value::Float(number) => Ok(number + 0.0),
            Value::Integer(number) => Ok(*number as f64),
            other => Err(self.wrong_type(key, other, "a number")),
        }
    }

    /// A non-negative amount in big blinds, as whole chips.
    fn amount(&self, key: &str) -> Result<Chips, SpotError> {
        let bb = match self.get(key)? {
            Value::Float(bb) => *bb,
            Value::Integer(bb) => *bb as f64,
            other => return Err(self.wrong_type(key, other, "a number of big blinds")),
        };
        let field = self.name(key);
        if !(bb.is_finite() && bb >= 0.0) {
            let reason = format!("is {bb}; an amount must be a number of big blinds from 0 up");
            return Err(invalid(field, reason));
        }
        if bb > f64::from(MAX_BB) {
            let reason = format!("is {bb}bb; an amount must be at most {MAX_BB}bb");
            return Err(invalid(field, reason));
        }
        let chips = bb * f64::from(CHIPS_PER_BB);
        if (chips - chips.round()).abs() > 1e-6 {
            let reason =
                format!("is {bb}bb, which is not a whole number of chips (1bb = 100 chips)");
            return Err(invalid(field, reason));
        }
        let chips = chips.round() as Chips;
        if chips == 0 && bb > 0.0 {
            let reason = format!("is {bb}bb, less than one chip; the smallest amount is 0.01bb");
            return Err(invalid(field, reason));
        }
        Ok(chips)
    }

    /// A count from 0 to `u32::MAX`.
    fn count(&self, key: &str) -> Result<u32, SpotError> {
        match self.get(key)? {
            Value::Integer(count) => u32::try_from(*count).map_err(|_| {
                let reason = format!(
                    "is {count}; it must be a whole number from 0 to {}",
                    u32::MAX
                );
                invalid(self.name(key), reason)
            }),
            other => Err(self.wrong_type(key, other, "a whole number")),
        }
    }

    fn boolean(&self, key: &str) -> Result<bool, SpotError> {
        match self.get(key)? {
            Value::Boolean(flag) => Ok(*flag),
            other => Err(self.wrong_type(key, other, "`true` or `false`")),
        }
    }

    fn numbers(&self, key: &str) -> Result<Vec<f64>, SpotError> {
        let items = match self.get(key)? {
            Value::Array(items) => items,
            other => return Err(self.wrong_type(key, other, "an array of numbers")),
        };
        items
            .iter()
            .map(|item| match item {
                Value::Float(number) => Ok(number + 0.0),
                Value::Integer(number) => Ok(*number as f64),
                other => {
                    let reason = format!("has {}; it must hold numbers only", kind(other));
                    Err(invalid(self.name(key), reason))
                }
            })
            .collect()
    }

    fn table(&self, key: &str, keys: &[&str]) -> Result<Fields<'a>, SpotError> {
        match self.get(key)? {
            Value::Table(table) => Fields::new(table, Some(&self.name(key)), keys),
            other => {
                let wanted = format!("a table, `[{key}]`, holding {}", list(keys));
                Err(self.wrong_type(key, other, &wanted))
            }
        }
    }

    /// A street sizing inside `[tree]`, which may be left out.
    fn street(&self, key: &str) -> Result<Option<StreetSizing>, SpotError> {
        let table = match self.table.get(key) {
            None => return Ok(None),
            Some(Value::Table(table)) => table,
            Some(other) => {
                let wanted = format!(
                    "a table such as `{key} = {{ bets = [0.5], raises = [], allin = false }}`"
                );
                return Err(self.wrong_type(key, other, &wanted));
            }
        };
        let fields = Fields::new(table, Some(&self.name(key)), &STREET_KEYS)?;
        Ok(Some(StreetSizing {
            bets: fields.numbers("bets")?,
            raises: fields.numbers("raises")?,
            allin: fields.boolean("allin")?,
        }))
    }
}

// ---------------------------------------------------------------------------------------------
// Writing values

/// What kind of TOML value `value` is, for error messages.
fn kind(value: &Value) -> &'static str {
    match value {
        Value::String(_) => "a string",
        Value::Integer(_) => "an integer",
        Value::Float(_) => "a decimal number",
        Value::Boolean(_) => "a boolean",
        Value::Datetime(_) => "a date",
        Value::Array(_) => "an array",
        Value::Table(_) => "a table",
    }
}

/// `a`, `b` and `c`.
fn list(keys: &[&str]) -> String {
    let quoted: Vec<String> = keys.iter().map(|key| format!("`{key}`")).collect();
    match quoted.split_last() {
        Some((last, rest)) if !rest.is_empty() => format!("{} and {last}", rest.join(", ")),
        _ => quoted.concat(),
    }
}

/// An amount in big blinds, exactly and without trailing zeros: `5.5`, `97.25` or `100`.
fn bb(chips: u64) -> String {
    let per_bb = u64::from(CHIPS_PER_BB);
    let (whole, cents) = (chips / per_bb, chips % per_bb);
    match cents {
        0 => whole.to_string(),
        tenths if tenths % 10 == 0 => format!("{whole}.{}", tenths / 10),
        cents => format!("{whole}.{cents:02}"),
    }
}

/// An amount in big blinds as a TOML decimal, which always has a decimal point: `5.5`, `97.25`
/// or `100.0`.
fn decimal_bb(chips: Chips) -> String {
    let text = bb(chips.into());
    if text.contains('.') {
        text
    } else {
        text + ".0"
    }
}

/// A number as a TOML decimal: the shortest text that reads back as the same `f64`, with a
/// decimal point or an exponent, such as `0.75`, `3.0` or `1e-7`. -0.0 is written as `0.0`.
fn float(number: f64) -> String {
    format!("{:?}", number + 0.0)
}

/// `[0.5, 1.0]`, or `[]`.
fn floats(numbers: &[f64]) -> String {
    let items: Vec<String> = numbers.iter().map(|&number| float(number)).collect();
    format!("[{}]", items.join(", "))
}

/// `text` as a one-line TOML basic string, in double quotes with escapes.
fn quote(text: &str) -> String {
    let mut quoted = String::with_capacity(text.len() + 2);
    quoted.push('"');
    for c in text.chars() {
        match c {
            '"' => quoted.push_str("\\\""),
            '\\' => quoted.push_str("\\\\"),
            '\n' => quoted.push_str("\\n"),
            '\r' => quoted.push_str("\\r"),
            '\t' => quoted.push_str("\\t"),
            c if c.is_control() => quoted.push_str(&format!("\\u{:04X}", u32::from(c))),
            c => quoted.push(c),
        }
    }
    quoted.push('"');
    quoted
}

#[cfg(test)]
mod tests;
