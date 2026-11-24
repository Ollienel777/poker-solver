//! GameConfig, positions, action order, betting-tree builders and range files.
//!
//! The heads-up postflop tree is in [`tree`], and the betting rules it shares with the preflop
//! builder (milestone M7) are in [`rules`]. Range files, which join the preflop and postflop
//! solvers, are in [`range_file`].

pub mod config;
mod order;
pub mod position;
pub mod range_file;
pub mod rules;
pub mod tree;

pub use config::{
    CHIPS_PER_BB, Chips, ConfigError, ForcedBet, ForcedBetKind, GameConfig, MAX_BET_FRACTION,
    MAX_RAISE_MULTIPLE, PostflopSizing, PreflopSizing, Rake, StreetSizing,
};
pub use position::{ParsePositionError, Position};
pub use range_file::{PlayerRange, RANGE_FILE_FORMAT, RangeFile, RangeFileError};
pub use tree::{
    Action, Node, NodeCounts, Outcome, PathError, PathErrorKind, Player, PostflopTree, Street,
    StreetCounts, TreeError, TreeSpec,
};
