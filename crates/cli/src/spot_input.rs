//! The options that name a spot, which every command that solves or estimates takes, and the
//! [`Spot`] they name.
//!
//! A spot is named one of two ways:
//!
//! - `--ranges <file> --board <cards> [--tree <file>] [--config <file>]`. The range file gives
//!   the pot, the stack, both ranges and their positions. The config it names gives the rake
//!   and, without `--tree`, the tree's sizing, its `[postflop]` table. The config is read from
//!   `--config`, or else from `configs/<name>.toml` under the current directory, `<name>` being
//!   the range file's `config` in lower case, and the range file is checked against it. A tree
//!   file is read by [`TreeSizing::load`], the parser of a spot file's `[tree]` table.
//! - `--spot <file>`: a spot file as it is.
//!
//! Every error names the file or the option it is about and says what to do.

use std::fmt;
use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow, bail};
use clap::{ArgGroup, Args};
use pk_core::{CardSet, ParseCardError};
use pk_game::{
    ConfigError, GameConfig, Player, Position, PostflopSizing, RangeFile, RangeFileError, Street,
};
use pk_postflop::{Spot, SpotError, TreeSizing};

use crate::format::list;

/// The folder, under the current directory, where a range file's config is looked up.
pub const CONFIG_FOLDER: &str = "configs";

/// The options that name a spot.
#[derive(Debug, Clone, Default, Args)]
#[command(group(ArgGroup::new("input").required(true).args(["ranges", "spot"])))]
pub struct SpotArgs {
    /// The range file: both players' positions and ranges, the pot and the stack at the flop
    #[arg(long, value_name = "FILE", requires = "board", help_heading = "Spot")]
    pub ranges: Option<PathBuf>,

    /// The board: 3, 4 or 5 cards, such as Ks7d2c
    ///
    /// A turn or river board starts the spot there, with the range file's pot, stack and ranges
    /// as they are at the flop, as if the streets before it were checked through.
    #[arg(long, value_name = "CARDS", requires = "ranges", help_heading = "Spot")]
    pub board: Option<String>,

    /// A tree file to size the betting tree with, in place of the config's [postflop] table
    ///
    /// It holds the keys of a spot file's [tree] table at its top level: max_raises,
    /// allin_threshold, and a sizing { bets, raises, allin } for the river alone, for the turn
    /// and the river, or for all three streets.
    #[arg(long, value_name = "FILE", requires = "ranges", help_heading = "Spot")]
    pub tree: Option<PathBuf>,

    /// The game config [default: configs/<name>.toml, <name> being the range file's config]
    #[arg(long, value_name = "FILE", requires = "ranges", help_heading = "Spot")]
    pub config: Option<PathBuf>,

    /// A spot file, which holds the whole spot, in place of --ranges and --board
    #[arg(
        long,
        value_name = "FILE",
        conflicts_with_all = ["ranges", "board", "tree", "config"],
        help_heading = "Spot"
    )]
    pub spot: Option<PathBuf>,
}

/// A spot, with where it and its tree came from, as the commands print them.
#[derive(Debug, Clone)]
pub struct SpotInput {
    pub spot: Spot,
    pub source: Source,
    pub tree: TreeSource,
}

impl SpotInput {
    /// Each player's position, OOP's first, when the spot came from a range file.
    pub fn positions(&self) -> Option<[Position; 2]> {
        match &self.source {
            Source::RangeFile { positions, .. } => Some(*positions),
            Source::SpotFile(_) => None,
        }
    }

    /// A player as the commands name it: `OOP (BB)` from a range file, `OOP` from a spot file.
    pub fn player(&self, player: Player) -> String {
        match self.positions() {
            Some(positions) => format!("{player} ({})", positions[player.index()]),
            None => player.to_string(),
        }
    }
}

/// What a spot was read from.
#[derive(Debug, Clone, PartialEq)]
pub enum Source {
    SpotFile(PathBuf),
    RangeFile {
        path: PathBuf,
        /// The config's name, such as `A`, and the file it was read from.
        config: String,
        config_path: PathBuf,
        /// The position of the player who acts first after the flop and of the one who acts
        /// last, indexed by [`Player::index`].
        positions: [Position; 2],
    },
}

impl fmt::Display for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Source::SpotFile(path) => write!(f, "the spot file `{}`", path.display()),
            Source::RangeFile {
                path,
                config,
                config_path,
                ..
            } => write!(
                f,
                "the range file `{}`, under config {config} from `{}`",
                path.display(),
                config_path.display()
            ),
        }
    }
}

/// Where a spot's tree sizing came from.
#[derive(Debug, Clone, PartialEq)]
pub enum TreeSource {
    /// The spot file's `[tree]` table.
    SpotFile,
    /// A config's `[postflop]` table: the config's name and its file.
    Config { name: String, path: PathBuf },
    /// A tree file.
    TreeFile(PathBuf),
}

impl fmt::Display for TreeSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TreeSource::SpotFile => f.write_str("the spot file's [tree] table"),
            TreeSource::Config { name, path } => {
                write!(
                    f,
                    "config {name}'s [postflop] table, in `{}`",
                    path.display()
                )
            }
            TreeSource::TreeFile(path) => write!(f, "the tree file `{}`", path.display()),
        }
    }
}

/// Reads the spot that `args` name, checking it as the [module documentation](self) says.
pub fn load(args: &SpotArgs) -> Result<SpotInput> {
    if let Some(path) = &args.spot {
        let spot = Spot::load(path).map_err(|error| match error {
            SpotError::Io { source, .. } => anyhow!(
                "could not read the spot file `{}`: {source}; check the path",
                path.display()
            ),
            other => anyhow!("the spot file `{}` is not valid: {other}", path.display()),
        })?;
        return Ok(SpotInput {
            spot,
            source: Source::SpotFile(path.clone()),
            tree: TreeSource::SpotFile,
        });
    }
    let (Some(ranges), Some(board)) = (&args.ranges, &args.board) else {
        bail!("name the spot with --ranges <file> and --board <cards>, or with --spot <file>");
    };
    let board = parse_board(board)?;
    let file = RangeFile::load(ranges).map_err(|error| match error {
        RangeFileError::Io { source, .. } => anyhow!(
            "could not read the range file `{}`: {source}; check the path",
            ranges.display()
        ),
        other => anyhow!(
            "the range file `{}` is not valid: {other}",
            ranges.display()
        ),
    })?;
    let (config, config_path) = read_config(&file, ranges, args.config.as_deref())?;
    file.check(&config).map_err(|error| {
        anyhow!(
            "the range file `{}` does not fit config {} from `{}`: {error}; correct the range \
             file, or give the config it was made for with --config",
            ranges.display(),
            config.name,
            config_path.display()
        )
    })?;
    let (tree, tree_source) = match &args.tree {
        Some(path) => {
            let tree = TreeSizing::load(path).map_err(|error| match error {
                SpotError::Io { source, .. } => anyhow!(
                    "could not read the tree file `{}`: {source}; check the path",
                    path.display()
                ),
                other => anyhow!("the tree file `{}` is not valid: {other}", path.display()),
            })?;
            (tree, TreeSource::TreeFile(path.clone()))
        }
        None => {
            let source = TreeSource::Config {
                name: config.name.clone(),
                path: config_path.clone(),
            };
            (config_tree(&config.postflop), source)
        }
    };
    let spot = spot_from_range_file(&file, ranges, &config, board, &tree, &tree_source)?;
    let source = Source::RangeFile {
        path: ranges.clone(),
        config: config.name.clone(),
        config_path,
        positions: [file.oop.position, file.ip.position],
    };
    Ok(SpotInput {
        spot,
        source,
        tree: tree_source,
    })
}

/// Where a range file's config is read from when `--config` is not given:
/// `configs/<name>.toml`, the name in lower case.
pub fn default_config_path(name: &str) -> PathBuf {
    PathBuf::from(format!("{CONFIG_FOLDER}/{}.toml", name.to_lowercase()))
}

/// The config of the range file at `ranges`: the one `--config` names, or the default.
fn read_config(
    file: &RangeFile,
    ranges: &Path,
    given: Option<&Path>,
) -> Result<(GameConfig, PathBuf)> {
    let path = given.map_or_else(|| default_config_path(&file.config), Path::to_path_buf);
    match GameConfig::load(&path) {
        Ok(config) => Ok((config, path)),
        Err(ConfigError::Io { source, .. }) if given.is_none() => {
            let name = format!("{}.toml", file.config.to_lowercase());
            let looked = std::env::current_dir()
                .map_or_else(|_| path.clone(), |dir| dir.join(CONFIG_FOLDER).join(name));
            bail!(
                "the range file `{}` names config `{}`, which is read from `{}` under the current \
                 directory, but {} could not be read: {source}; run solver from the folder that \
                 holds `{CONFIG_FOLDER}`, or give the config with --config <file>",
                ranges.display(),
                file.config,
                path.display(),
                looked.display()
            )
        }
        Err(ConfigError::Io { source, .. }) => bail!(
            "could not read the config `{}` that --config names: {source}; check the path",
            path.display()
        ),
        Err(other) => bail!("the config `{}` is not valid: {other}", path.display()),
    }
}

/// Reads `--board`: 3, 4 or 5 different cards, in any order and case.
pub fn parse_board(text: &str) -> Result<CardSet> {
    let board: CardSet = text.parse().map_err(|error| match error {
        ParseCardError::Duplicate(card) => {
            anyhow!("--board `{text}` holds {card} twice; give each card once, as in Ks7d2c")
        }
        ParseCardError::BadCard(card) => anyhow!(
            "--board `{text}`: `{card}` is not a card; write each card as a rank \
             (23456789TJQKA) and a suit (cdhs), as in Ks7d2c"
        ),
        ParseCardError::BadLength(_) => anyhow!(
            "--board `{text}` is not a list of cards; write each card as a rank (23456789TJQKA) \
             and a suit (cdhs), as in Ks7d2c"
        ),
    })?;
    match board.len() {
        3..=5 => Ok(board),
        cards => bail!(
            "--board `{text}` has {cards} cards; a board has 3 (a flop), 4 (a turn) or 5 (a \
             river), as in Ks7d2c"
        ),
    }
}

/// A config's `[postflop]` table as a tree sizing of all three streets.
pub fn config_tree(postflop: &PostflopSizing) -> TreeSizing {
    TreeSizing {
        max_raises: postflop.max_raises,
        allin_threshold: postflop.allin_threshold,
        flop: Some(postflop.flop.clone()),
        turn: Some(postflop.turn.clone()),
        river: Some(postflop.river.clone()),
    }
}

/// The spot that a range file, read from `path`, its config, a board and a tree sizing
/// describe. The pot, the stack and the ranges are the range file's, the rake the config's, and
/// the sizing that of the streets the board plays. The spot is named after the formation and
/// the board, as in `BTN_vs_BB_SRP_Ks7d2c`.
///
/// The range file is taken to have been checked against `config` already. Fails, saying what
/// to do, if the tree does not size the board's street or if the board leaves a player no hand.
pub fn spot_from_range_file(
    file: &RangeFile,
    path: &Path,
    config: &GameConfig,
    board: CardSet,
    tree: &TreeSizing,
    tree_source: &TreeSource,
) -> Result<Spot> {
    let start = match board.len() {
        3 => Street::Flop,
        4 => Street::Turn,
        5 => Street::River,
        cards => bail!("a board has 3, 4 or 5 cards, not {cards}"),
    };
    let tree = starting_on(tree, start, tree_source, board)?;
    for player in [Player::Oop, Player::Ip] {
        let side = file.player(player);
        if side.range.iter().all(|(combo, _)| combo.intersects(board)) {
            bail!(
                "the board {board} leaves {} no hand: every combo of its range (`{}.range` in \
                 `{}`) holds a card of the board; check --board, or give a range that holds a \
                 hand this board leaves possible",
                side.position,
                key(player),
                path.display()
            );
        }
    }
    let spot = Spot {
        name: format!("{}_{board}", file.formation),
        description: format!(
            "{} on {board}, under config {}: {}. Ranges: {}.",
            file.formation, config.name, file.line, file.source
        ),
        board,
        pot: file.pot,
        effective_stack: file.effective_stack,
        oop_range: file.oop.range.clone(),
        ip_range: file.ip.range.clone(),
        rake_percent: config.rake.percent,
        rake_cap: config.rake.cap,
        tree,
    };
    spot.validate().map_err(|error| match error {
        SpotError::Invalid { field, reason } => {
            if field == "ip_range" {
                anyhow!(
                    "on the board {board}, no hand of {}'s range can be dealt together with a \
                     hand of {}'s: every pair of them shares a card (`oop.range` and `ip.range` \
                     in `{}`); check the ranges",
                    file.oop.position,
                    file.ip.position,
                    path.display()
                )
            } else if let Some(key) = field.strip_prefix("tree.") {
                anyhow!("{tree_source}: `{key}` {reason}")
            } else if let Some(key) = field.strip_prefix("rake.") {
                anyhow!("config {}'s rake: `{key}` {reason}", config.name)
            } else {
                anyhow!(
                    "the spot made from the range file `{}` is not valid: `{field}` {reason}",
                    path.display()
                )
            }
        }
        other => anyhow!(
            "the spot made from the range file `{}` is not valid: {other}",
            path.display()
        ),
    })?;
    Ok(spot)
}

/// The sizing of the streets a spot on `start` plays, from a sizing of `start` or of an earlier
/// street on.
fn starting_on(
    tree: &TreeSizing,
    start: Street,
    source: &TreeSource,
    board: CardSet,
) -> Result<TreeSizing> {
    if tree.street(start).is_none() {
        let sized: Vec<String> = Street::ALL
            .into_iter()
            .filter(|&street| tree.street(street).is_some())
            .map(|street| format!("the {street}"))
            .collect();
        let missing: Vec<String> = Street::ALL
            .into_iter()
            .filter(|&street| street >= start && tree.street(street).is_none())
            .map(|street| format!("`{street}`"))
            .collect();
        let first = Street::ALL
            .into_iter()
            .find(|&street| tree.street(street).is_some())
            .unwrap_or(Street::River);
        bail!(
            "{source} sizes {} only, but the board {board} starts on the {start}; add {} to it, \
             or give a board that starts on the {first}",
            list(&sized),
            match missing.as_slice() {
                [street] => format!("a {street} sizing"),
                streets => format!("{} sizings", list(streets)),
            }
        );
    }
    let keep = |street: Street| {
        if street < start {
            None
        } else {
            tree.street(street).cloned()
        }
    };
    Ok(TreeSizing {
        max_raises: tree.max_raises,
        allin_threshold: tree.allin_threshold,
        flop: keep(Street::Flop),
        turn: keep(Street::Turn),
        river: keep(Street::River),
    })
}

/// The range file's table for a player: `oop` or `ip`.
fn key(player: Player) -> &'static str {
    match player {
        Player::Oop => "oop",
        Player::Ip => "ip",
    }
}

#[cfg(test)]
mod tests {
    use pk_core::Range;
    use pk_game::{PlayerRange, StreetSizing};

    use super::*;

    fn repo(path: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(path)
    }

    const RANGES: &str = "reference/ranges/a/BTN_vs_BB_SRP.toml";

    fn range_file() -> RangeFile {
        RangeFile::load(repo(RANGES)).unwrap()
    }

    fn config(name: &str) -> GameConfig {
        GameConfig::load(repo(&format!("configs/{name}.toml"))).unwrap()
    }

    fn board(text: &str) -> CardSet {
        text.parse().unwrap()
    }

    fn config_source() -> TreeSource {
        TreeSource::Config {
            name: "A".into(),
            path: "configs/a.toml".into(),
        }
    }

    /// The spot of the range file under config A on `board`, sized by config A's tree.
    fn spot_on(board_text: &str) -> Spot {
        let config = config("a");
        let tree = config_tree(&config.postflop);
        spot_from_range_file(
            &range_file(),
            Path::new(RANGES),
            &config,
            board(board_text),
            &tree,
            &config_source(),
        )
        .unwrap()
    }

    fn error_of(result: Result<impl fmt::Debug>) -> String {
        let message = format!("{:#}", result.unwrap_err());
        println!("{message}");
        message
    }

    /// A tree file of one half-pot bet a street and no raises.
    const ONE_BET: &str = "max_raises = 0\nallin_threshold = 1.0\n\
        flop = { bets = [0.5], raises = [], allin = false }\n\
        turn = { bets = [0.5], raises = [], allin = false }\n\
        river = { bets = [0.5], raises = [], allin = false }\n";

    fn half_pot() -> StreetSizing {
        StreetSizing {
            bets: vec![0.5],
            raises: vec![],
            allin: false,
        }
    }

    #[test]
    fn a_range_file_and_its_config_give_the_pot_stack_ranges_rake_and_tree() {
        let file = range_file();
        let config = config("a");
        let spot = spot_on("Ks7d2c");
        assert_eq!(spot.name, "BTN_vs_BB_SRP_Ks7d2c");
        assert_eq!(spot.street(), Street::Flop);
        assert_eq!(spot.board, board("Ks7d2c"));
        assert_eq!((spot.pot, spot.effective_stack), (550, 9750));
        assert_eq!(spot.oop_range, file.oop.range);
        assert_eq!(spot.ip_range, file.ip.range);
        assert_eq!((spot.rake_percent, spot.rake_cap), (5.0, 600));
        assert_eq!(spot.tree, config_tree(&config.postflop));
        assert_eq!(spot.tree.flop.as_ref(), Some(&config.postflop.flop));
        assert_eq!(spot.tree.river.as_ref(), Some(&config.postflop.river));
        assert_eq!((spot.tree.max_raises, spot.tree.allin_threshold), (3, 0.67));
        assert!(
            spot.description.contains("BTN r2.5"),
            "{}",
            spot.description
        );
        spot.validate().unwrap();
    }

    #[test]
    fn loading_from_files_gives_the_spot_its_positions_and_its_sources() {
        let args = SpotArgs {
            ranges: Some(repo(RANGES)),
            board: Some("2c7dKs".into()),
            config: Some(repo("configs/a.toml")),
            ..SpotArgs::default()
        };
        let input = load(&args).unwrap();
        assert_eq!(input.spot, spot_on("Ks7d2c"));
        assert_eq!(input.positions(), Some([Position::Bb, Position::Btn]));
        assert_eq!(input.player(Player::Oop), "OOP (BB)");
        assert_eq!(input.player(Player::Ip), "IP (BTN)");
        assert!(matches!(input.tree, TreeSource::Config { ref name, .. } if name == "A"));

        // A spot file comes as it is, with no positions.
        let path = repo("reference/spots/river/toy_polarized.toml");
        let args = SpotArgs {
            spot: Some(path.clone()),
            ..SpotArgs::default()
        };
        let input = load(&args).unwrap();
        assert_eq!(input.spot, Spot::load(&path).unwrap());
        assert_eq!(input.positions(), None);
        assert_eq!(input.player(Player::Ip), "IP");
        assert_eq!(input.tree, TreeSource::SpotFile);
    }

    #[test]
    fn a_turn_or_river_board_keeps_the_streets_it_plays() {
        let config = config("a");
        let turn = spot_on("Ks7d2c9h");
        assert_eq!(turn.street(), Street::Turn);
        assert_eq!(turn.tree.flop, None);
        assert_eq!(turn.tree.turn.as_ref(), Some(&config.postflop.turn));
        assert_eq!(turn.tree.river.as_ref(), Some(&config.postflop.river));
        let river = spot_on("Ks7d2c9h3s");
        assert_eq!(
            (river.tree.flop.as_ref(), river.tree.turn.as_ref()),
            (None, None)
        );
        assert_eq!(river.tree.river.as_ref(), Some(&config.postflop.river));
        // The pot and the stack are the range file's on every street.
        assert_eq!((river.pot, river.effective_stack), (550, 9750));
        assert_eq!(river.name, "BTN_vs_BB_SRP_Ks9h7d3s2c");
    }

    #[test]
    fn a_tree_file_sizes_the_tree_in_place_of_the_config() {
        let config = config("a");
        let tree = TreeSizing::from_toml(ONE_BET).unwrap();
        let source = TreeSource::TreeFile("one_bet.toml".into());
        let flop = spot_from_range_file(
            &range_file(),
            Path::new(RANGES),
            &config,
            board("Ks7d2c"),
            &tree,
            &source,
        )
        .unwrap();
        assert_eq!(flop.tree, tree);
        assert_eq!((flop.tree.max_raises, flop.tree.allin_threshold), (0, 1.0));
        // The rake is still the config's.
        assert_eq!((flop.rake_percent, flop.rake_cap), (5.0, 600));

        let river = spot_from_range_file(
            &range_file(),
            Path::new(RANGES),
            &config,
            board("Ks7d2c9h3s"),
            &tree,
            &source,
        )
        .unwrap();
        assert_eq!(
            river.tree,
            TreeSizing {
                max_raises: 0,
                allin_threshold: 1.0,
                flop: None,
                turn: None,
                river: Some(half_pot()),
            }
        );
    }

    #[test]
    fn a_tree_file_must_size_the_street_the_board_starts_on() {
        let river_only = "max_raises = 0\nallin_threshold = 1.0\n\
            river = { bets = [0.5], raises = [], allin = false }\n";
        let tree = TreeSizing::from_toml(river_only).unwrap();
        let message = error_of(spot_from_range_file(
            &range_file(),
            Path::new(RANGES),
            &config("a"),
            board("Ks7d2c9h"),
            &tree,
            &TreeSource::TreeFile("river.toml".into()),
        ));
        assert_eq!(
            message,
            "the tree file `river.toml` sizes the river only, but the board Ks9h7d2c starts on \
             the turn; add a `turn` sizing to it, or give a board that starts on the river"
        );
        let message = error_of(spot_from_range_file(
            &range_file(),
            Path::new(RANGES),
            &config("a"),
            board("Ks7d2c"),
            &tree,
            &TreeSource::TreeFile("river.toml".into()),
        ));
        assert!(message.contains("starts on the flop; add `flop` and `turn` sizings to it"));
    }

    #[test]
    fn the_board_is_three_to_five_different_cards() {
        assert_eq!(parse_board("ks7D2c").unwrap(), board("Ks7d2c"));
        assert_eq!(parse_board("Ks 7d 2c 9h 3s").unwrap().len(), 5);
        let message = error_of(parse_board("Ks7dKs"));
        assert!(message.contains("holds Ks twice; give each card once"));
        let message = error_of(parse_board("Ks7d"));
        assert!(message.contains("has 2 cards; a board has 3 (a flop)"));
        let message = error_of(parse_board("Ks7d2c9h3s4s"));
        assert!(message.contains("has 6 cards"));
        let message = error_of(parse_board("Ks7d2x"));
        assert!(message.contains("`2x` is not a card"));
        let message = error_of(parse_board("Ks7d2"));
        assert!(message.contains("is not a list of cards"));
    }

    #[test]
    fn a_board_that_leaves_a_player_no_hand_is_refused() {
        let mut file = range_file();
        file.oop = PlayerRange {
            position: Position::Bb,
            range: "AA".parse::<Range>().unwrap(),
        };
        let config = config("a");
        let tree = config_tree(&config.postflop);
        let message = error_of(spot_from_range_file(
            &file,
            Path::new("narrow.toml"),
            &config,
            board("AsAhAd"),
            &tree,
            &config_source(),
        ));
        assert!(message.contains("the board AsAhAd leaves BB no hand"));
        assert!(message.contains("`oop.range` in `narrow.toml`"));
        assert!(message.contains("check --board"));

        // Each range holds a hand, but none of BB's can be dealt with one of BTN's.
        file.oop.range = "AcKc".parse().unwrap();
        file.ip.range = "AcQc,KcQd".parse().unwrap();
        let message = error_of(spot_from_range_file(
            &file,
            Path::new("narrow.toml"),
            &config,
            board("2s3h4d"),
            &tree,
            &config_source(),
        ));
        assert!(
            message.contains("no hand of BB's range can be dealt together with a hand of BTN's")
        );
    }

    #[test]
    fn the_range_file_is_checked_against_its_config() {
        let args = SpotArgs {
            ranges: Some(repo(RANGES)),
            board: Some("Ks7d2c".into()),
            config: Some(repo("configs/c.toml")),
            ..SpotArgs::default()
        };
        let message = error_of(load(&args));
        assert!(message.contains("does not fit config C"));
        assert!(message.contains("`config` is `a`"));
        assert!(message.contains("--config"));
    }

    #[test]
    fn the_config_is_looked_up_under_the_current_directory_by_default() {
        assert_eq!(default_config_path("A"), PathBuf::from("configs/a.toml"));
        // Tests run in the crate's folder, which holds no `configs`.
        let args = SpotArgs {
            ranges: Some(repo(RANGES)),
            board: Some("Ks7d2c".into()),
            ..SpotArgs::default()
        };
        let message = error_of(load(&args));
        assert!(message.contains("names config `a`, which is read from `configs/a.toml`"));
        assert!(message.contains("give the config with --config <file>"));
    }

    #[test]
    fn a_missing_file_says_which_file_and_to_check_the_path() {
        for (args, file) in [
            (
                SpotArgs {
                    spot: Some("no/such/spot.toml".into()),
                    ..SpotArgs::default()
                },
                "the spot file `no/such/spot.toml`",
            ),
            (
                SpotArgs {
                    ranges: Some("no/such/ranges.toml".into()),
                    board: Some("Ks7d2c".into()),
                    ..SpotArgs::default()
                },
                "the range file `no/such/ranges.toml`",
            ),
            (
                SpotArgs {
                    ranges: Some(repo(RANGES)),
                    board: Some("Ks7d2c".into()),
                    config: Some(repo("configs/a.toml")),
                    tree: Some("no/such/tree.toml".into()),
                    ..SpotArgs::default()
                },
                "the tree file `no/such/tree.toml`",
            ),
        ] {
            let message = error_of(load(&args));
            assert!(
                message.starts_with(&format!("could not read {file}")),
                "{message}"
            );
            assert!(message.ends_with("check the path"), "{message}");
        }
    }
}
