//! Heads-up flop-to-river solver: discounted CFR over exact cards.
//!
//! Built in milestones M3 to M5. So far it solves river spots. The input is a [`Spot`] read
//! from a spot file ([`spot`]), with the per-board hand lists ([`hands`]), the terminal payoffs
//! ([`payoff`]) and the O(n) range-against-range terminal evaluation ([`terminal`]). A river
//! spot becomes a public game for vector-form CFR ([`river`]), [`solve`](mod@solve) runs the
//! solve loop, and [`exploitability`](mod@exploitability) measures how far a strategy is from
//! an equilibrium. [`expected_rake`](mod@expected_rake) works out the rake a strategy pays,
//! apart from the payoffs, and so what the players' values must sum to.
//!
//! [`invariants`] checks a solution against the invariants every solve must keep, and in a
//! build with debug assertions every solve checks the solution it returns.
//!
//! For checking against another solver, which is only ever run as a separate program:
//! [`dump`] is a solver-neutral JSON form of a solution, with the per-hand EVs of
//! [`node_values`](mod@node_values); [`import`] maps a dump onto our tree for its spot,
//! checking first that it answers that spot; [`compare`](mod@compare) compares two solutions
//! and gives a verdict against tolerances; and [`external`] runs a program that writes a dump.

pub mod compare;
pub mod dump;
pub mod expected_rake;
pub mod exploitability;
pub mod external;
pub mod hands;
pub mod import;
pub mod invariants;
pub mod node_values;
pub mod payoff;
pub mod river;
pub mod solve;
pub mod spot;
pub mod terminal;

pub use compare::{Check, Comparison, Failure, Tolerances, compare, compare_mapped};
pub use dump::{DumpAction, DumpError, SolutionDump};
pub use expected_rake::{ExpectedRake, TerminalOutcome, expected_rake};
pub use exploitability::{Exploitability, exploitability};
pub use external::{ExternalSolver, RunError};
pub use hands::HandList;
pub use import::{ImportError, MappedSolution, Mismatch};
pub use invariants::{Invariant, Report, Violation, check_solution};
pub use node_values::{NodeValues, node_values};
pub use payoff::{Payoff, ShowdownPayoff, fold_payoffs, rake_on, showdown_payoff};
pub use river::{ClassView, RiverGame};
pub use solve::{Progress, Solution, SolveError, SolveOptions, StopReason, solve};
pub use spot::{SPOT_FORMAT, Spot, SpotError, TreeSizing};
pub use terminal::BoardHands;
