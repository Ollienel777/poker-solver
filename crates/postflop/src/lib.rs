//! Heads-up flop-to-river solver: discounted CFR over exact cards.
//!
//! Built in milestones M3 to M5. It solves spots that start on the flop, the turn or the
//! river. The input is a [`Spot`] read from a spot file ([`spot`]), with the per-board hand
//! lists ([`hands`]), the terminal payoffs ([`payoff`]) and the O(n) range-against-range
//! terminal evaluation ([`terminal`]). A spot becomes a public game for vector-form CFR
//! ([`game`]), whose chance nodes deal the turn and the river, and which the river code still
//! calls [`RiverGame`] ([`river`]). [`solve`](mod@solve) runs the solve loop, and
//! [`exploitability`](mod@exploitability) measures how far a strategy is from an equilibrium.
//! [`expected_rake`](mod@expected_rake) works out the rake a strategy pays, apart from the
//! payoffs, and so what the players' values must sum to.
//!
//! [`invariants`] checks a solution against the invariants every solve must keep, and in a
//! build with debug assertions every solve checks the solution it returns.
//!
//! [`memory`] estimates what a solve will need before anything is allocated, and the solve
//! refuses one over its limit. [`resource_log`] records what each solve took, for a caller to
//! append to a log.
//!
//! For checking against another solver, which is only ever run as a separate program:
//! [`dump`] is a solver-neutral JSON form of a solution, version 1 for a river spot and
//! version 2 for a flop or turn spot, complete or of the first streets only, with the per-hand
//! EVs of [`node_values`](mod@node_values); [`import`] maps a dump onto our tree for its spot,
//! checking first that it answers that spot; [`compare`](mod@compare) compares two solutions
//! and gives a verdict against tolerances; and [`external`] runs a program that writes a dump.

pub mod compare;
pub mod dump;
pub mod expected_rake;
pub mod exploitability;
pub mod external;
pub mod game;
pub mod hands;
pub mod import;
pub mod invariants;
pub mod memory;
pub mod node_values;
pub mod payoff;
pub mod resource_log;
pub mod river;
pub mod solve;
pub mod spot;
pub mod terminal;

pub use compare::{
    Check, Comparison, Failure, StreetSummary, Tolerances, Unchecked, compare, compare_mapped,
};
pub use dump::{DumpAction, DumpError, SolutionDump};
pub use expected_rake::{ExpectedRake, TerminalOutcome, expected_rake};
pub use exploitability::{Exploitability, exploitability};
pub use external::{ExternalSolver, RunError};
pub use game::{ClassView, NodePathError, NodePathErrorKind, PostflopGame};
pub use hands::HandList;
pub use import::{ImportError, MappedSolution, Mismatch};
pub use invariants::{Invariant, Report, Violation, check_solution};
pub use memory::{MemoryEstimate, estimate_memory};
pub use node_values::{NodeValues, node_values};
pub use payoff::{Payoff, ShowdownPayoff, fold_payoffs, rake_on, showdown_payoff};
pub use resource_log::{SolveRecord, append_record, peak_resident_bytes};
pub use river::RiverGame;
pub use solve::{Progress, Solution, SolveError, SolveOptions, StopReason, solve};
pub use spot::{SPOT_FORMAT, Spot, SpotError, TreeSizing};
pub use terminal::BoardHands;
