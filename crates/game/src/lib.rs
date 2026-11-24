//! GameConfig, positions, action order and betting-tree builders.
//!
//! The heads-up postflop tree is in [`tree`], and the betting rules it shares with the preflop
//! builder (milestone M7) are in [`rules`].

pub mod config;
mod order;
pub mod position;
pub mod rules;
pub mod tree;

pub use config::{
    CHIPS_PER_BB, Chips, ConfigError, ForcedBet, ForcedBetKind, GameConfig, PostflopSizing,
    PreflopSizing, Rake, StreetSizing,
};
pub use position::{ParsePositionError, Position};
pub use tree::{
    Action, Node, NodeCounts, Outcome, Player, PostflopTree, Street, StreetCounts, TreeError,
    TreeSpec,
};
