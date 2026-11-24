//! Heads-up flop-to-river solver: discounted CFR over exact cards.
//!
//! Built in milestones M3 to M5. So far it holds the per-board hand lists ([`hands`]), the
//! terminal payoffs ([`payoff`]) and the O(n) range-against-range terminal evaluation
//! ([`terminal`]).

pub mod hands;
pub mod payoff;
pub mod terminal;

pub use hands::HandList;
pub use payoff::{Payoff, ShowdownPayoff, fold_payoffs, rake_on, showdown_payoff};
pub use terminal::BoardHands;
