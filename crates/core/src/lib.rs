//! Cards, ranges, hand evaluation and equity.

pub mod card;
pub mod combo;
pub mod equity;
pub mod eval;
pub mod isomorphism;
pub mod range;

pub use card::{Card, CardSet, NUM_CARDS, NUM_RANKS, NUM_SUITS, ParseCardError};
pub use combo::{Combo, HandClass, NUM_CLASSES, NUM_COMBOS, ParseHandError};
pub use equity::{Equity, EquityError, Estimate, hand_vs_hand, monte_carlo, range_vs_range};
pub use eval::{Category, HandRank, evaluate};
pub use isomorphism::{
    BoardError, CardClass, DealtBoard, NUM_SUIT_PERMUTATIONS, NextCards, SuitGroup,
    SuitPermutation, canonical,
};
pub use range::{ParseRangeError, Range};
