//! Heads-up flop-to-river solver: discounted CFR over exact cards.
//!
//! Built in milestones M3 to M5. So far it solves river spots. The input is a [`Spot`] read
//! from a spot file ([`spot`]), with the per-board hand lists ([`hands`]), the terminal payoffs
//! ([`payoff`]) and the O(n) range-against-range terminal evaluation ([`terminal`]). A river
//! spot becomes a public game for vector-form CFR ([`river`]), [`solve`](mod@solve) runs the
//! solve loop, and [`exploitability`](mod@exploitability) measures how far a strategy is from
//! an equilibrium. [`expected_rake`](mod@expected_rake) works out the rake a strategy pays,
//! apart from the payoffs, and so what the players' values must sum to.

pub mod expected_rake;
pub mod exploitability;
pub mod hands;
pub mod payoff;
pub mod river;
pub mod solve;
pub mod spot;
pub mod terminal;

pub use expected_rake::{ExpectedRake, TerminalOutcome, expected_rake};
pub use exploitability::{Exploitability, exploitability};
pub use hands::HandList;
pub use payoff::{Payoff, ShowdownPayoff, fold_payoffs, rake_on, showdown_payoff};
pub use river::RiverGame;
pub use solve::{Progress, Solution, SolveError, SolveOptions, StopReason, solve};
pub use spot::{SPOT_FORMAT, Spot, SpotError, TreeSizing};
pub use terminal::BoardHands;
