//! The river solve: vector-form discounted CFR on a [`RiverGame`], run until its average
//! strategy is close enough to an equilibrium or a cap is reached.
//!
//! # The loop
//!
//! [`solve`] builds the spot's [`RiverGame`] and runs [`VectorCfr`] on it under
//! [`SolveOptions::schedule`], DCFR(1.5, 0, 2) by default. Each iteration updates OOP, then IP
//! against OOP's updated strategy (alternating updates), as [`pk_cfr::vector`] specifies.
//! Regrets and strategy sums are stored as `f32`, one entry per action and in-range combo of
//! the acting player at each player node, and the strategy the solve reports is the average
//! strategy.
//!
//! After every [`check_every`](SolveOptions::check_every) iterations, the solve measures the
//! [exploitability](mod@crate::exploitability) of the average strategy and calls the progress
//! callback with it. The solve stops at the first of these rules that holds, checked in this
//! order:
//!
//! 1. **Target.** A measurement, made after an iteration, is at or below
//!    [`target`](SolveOptions::target), a percent of the pot.
//! 2. **Iteration cap.** [`max_iterations`](SolveOptions::max_iterations) iterations are done.
//! 3. **Time cap.** [`max_time`](SolveOptions::max_time) has passed since `solve` was called.
//!
//! The target is checked after each measurement, and the caps before each iteration, the first
//! one included: a cap of zero iterations, or of no time, returns the uniform strategy that
//! the solver starts from. The time includes building the game and every measurement. If the
//! solve stops after an iteration it did not measure, it measures once more, so the
//! [`Solution`] always carries the exploitability of its own strategy. That last measurement
//! does not change the rule reported: a solve stopped by a cap reports the cap, even if the
//! measurement turns out to be at or below the target.
//!
//! In a build with debug assertions, which the tests are, the solve then checks the solution
//! against its [invariants](crate::invariants) and panics with the report if it breaks any.
//!
//! # Progress
//!
//! The callback is called once per measurement, in iteration order, with a [`Progress`]: every
//! `check_every` iterations, and once more at the end if the last iteration was not one of
//! those. Its last call carries the solution's iterations and exploitability. The library never
//! prints; a caller that wants a progress line writes it from the callback.
//!
//! # Determinism
//!
//! An iteration depends only on the spot, the schedule and the iterations before it, and
//! measuring exploitability reads the average strategy without changing anything. So the
//! strategy after `t` iterations is the same bit for bit in every run, whatever `check_every`,
//! the target and the caps. A time cap makes the number of iterations depend on the machine,
//! but not the strategy reached at any given number.
//!
//! # References
//!
//! - Brown and Sandholm, *Solving Imperfect-Information Games via Discounted Regret
//!   Minimization*, AAAI (2019): discounted CFR, and DCFR(1.5, 0, 2).
//! - Johanson, Bard, Lanctot, Gibson and Bowling, *Efficient Nash Equilibrium Approximation
//!   through Monte Carlo Counterfactual Regret Minimization*, AAMAS (2012): CFR over the public
//!   tree, with one regret per (public node, private hand).

use std::time::{Duration, Instant};

use pk_cfr::{Dcfr, PublicProfile, Schedule, VectorCfr};
use pk_game::{Player, PostflopTree, TreeError};
use thiserror::Error;

use crate::exploitability::{Exploitability, exploitability};
use crate::hands::HandList;
use crate::invariants::debug_assert_valid;
use crate::river::RiverGame;
use crate::spot::{Spot, SpotError};

/// Why a solve could not start.
#[derive(Debug, Error)]
pub enum SolveError {
    /// The spot does not start on the river. Flop and turn spots come in milestone M4.
    #[error(
        "the board has {cards} cards; the river solver needs a river board of 5, and flop and \
         turn spots are not supported yet"
    )]
    NotRiver { cards: usize },
    /// The spot fails [`Spot::validate`].
    #[error("invalid spot: {0}")]
    Spot(#[from] SpotError),
    /// The spot's tree has more nodes than the builder's limit.
    #[error("{0}")]
    Tree(#[from] TreeError),
    /// An option is out of range.
    #[error("`{field}` {reason}")]
    Options { field: &'static str, reason: String },
}

/// How a solve runs and when it stops. [`SolveOptions::default`] gives the defaults listed on
/// each field.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SolveOptions {
    /// The regret and averaging schedule. Defaults to DCFR(1.5, 0, 2).
    pub schedule: Schedule,
    /// Stop once the average strategy's exploitability is at or below this percent of the pot.
    /// `None` runs until a cap. Defaults to 0.3. Must be finite and not negative.
    pub target: Option<f64>,
    /// Stop after this many iterations. Defaults to 10,000.
    pub max_iterations: u64,
    /// Stop once this much time has passed. `None`, the default, sets no limit.
    pub max_time: Option<Duration>,
    /// Measure exploitability after every this many iterations. Defaults to 10. Must be at
    /// least 1.
    pub check_every: u64,
}

impl Default for SolveOptions {
    fn default() -> SolveOptions {
        SolveOptions {
            schedule: Schedule::Discounted(Dcfr::default()),
            target: Some(0.3),
            max_iterations: 10_000,
            max_time: None,
            check_every: 10,
        }
    }
}

impl SolveOptions {
    fn check(&self) -> Result<(), SolveError> {
        if let Some(target) = self.target
            && !(target.is_finite() && target >= 0.0)
        {
            return Err(SolveError::Options {
                field: "target",
                reason: format!("is {target}; it must be a percent of the pot from 0 up"),
            });
        }
        if self.check_every == 0 {
            return Err(SolveError::Options {
                field: "check_every",
                reason: "is 0; exploitability is measured every 1 or more iterations".into(),
            });
        }
        if let Schedule::Discounted(Dcfr { alpha, beta, gamma }) = self.schedule
            && !(alpha.is_finite() && beta.is_finite() && gamma.is_finite())
        {
            return Err(SolveError::Options {
                field: "schedule",
                reason: format!("is DCFR({alpha}, {beta}, {gamma}); its parameters must be finite"),
            });
        }
        Ok(())
    }
}

/// Which stopping rule ended a solve.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    /// The exploitability reached the target.
    Target,
    /// The iteration cap was reached.
    IterationCap,
    /// The time cap was reached.
    TimeCap,
}

/// One exploitability measurement during a solve, as the progress callback receives it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Progress {
    /// The iterations done when the measurement was made.
    pub iterations: u64,
    /// The time since `solve` was called, this measurement included.
    pub elapsed: Duration,
    /// The exploitability of the average strategy after `iterations` iterations.
    pub exploitability: Exploitability,
}

/// The result of a solve: how it ended, and the average strategy with read access for later
/// code. It is held in memory; the solution file comes in a later milestone.
///
/// Node indices are those of the [`PostflopTree`] ([`Solution::tree`]), and hand indices those
/// of each player's [`HandList`] ([`Solution::hands`]).
#[derive(Debug, Clone)]
pub struct Solution {
    game: RiverGame,
    strategy: PublicProfile,
    stop: StopReason,
    iterations: u64,
    elapsed: Duration,
    exploitability: Exploitability,
}

impl Solution {
    /// The game that was solved.
    pub fn game(&self) -> &RiverGame {
        &self.game
    }

    /// The betting tree.
    pub fn tree(&self) -> &PostflopTree {
        self.game.tree()
    }

    /// One player's hands, in hand-index order.
    pub fn hands(&self, player: Player) -> &HandList {
        self.game.hands(player)
    }

    /// The stopping rule that ended the solve.
    pub fn stop(&self) -> StopReason {
        self.stop
    }

    /// The iterations run.
    pub fn iterations(&self) -> u64 {
        self.iterations
    }

    /// The time the solve took, from the call until it returned, the last measurement included.
    /// The check of the solution that a build with debug assertions makes is not counted.
    pub fn elapsed(&self) -> Duration {
        self.elapsed
    }

    /// The exploitability of the average strategy.
    pub fn exploitability(&self) -> &Exploitability {
        &self.exploitability
    }

    /// Each player's expected value under the average strategy, in chips per hand dealt and
    /// counted from the start of the river, indexed by [`Player::index`].
    pub fn values(&self) -> [f64; 2] {
        self.exploitability.values
    }

    /// The average strategy at every player node, laid out `[actions × hands]` as
    /// [`PublicProfile`] describes.
    pub fn strategy(&self) -> &PublicProfile {
        &self.strategy
    }

    /// The probability of each action at the player node `node` for the acting player's hand
    /// `hand`, or `None` if `node` is not a player node. Panics if the acting player has no
    /// hand `hand`.
    pub fn hand_strategy(&self, node: u32, hand: usize) -> Option<Vec<f32>> {
        self.strategy.hand(node as usize, hand)
    }

    /// Each player's own reach of `node` with each of its hands under the average strategy:
    /// see [`RiverGame::reach`]. Panics if `node` is out of range.
    pub fn reach(&self, node: u32) -> [Vec<f64>; 2] {
        self.game.reach(&self.strategy, node)
    }

    /// The probability that the acting player holds each of its hands once `node` is reached
    /// under the average strategy: see [`RiverGame::hand_weights`].
    pub fn hand_weights(&self, node: u32) -> Option<Vec<f64>> {
        self.game.hand_weights(&self.strategy, node)
    }

    /// How often the acting player takes each action at `node`, over its whole range, under
    /// the average strategy: see [`RiverGame::action_frequencies`].
    pub fn action_frequencies(&self, node: u32) -> Option<Vec<f64>> {
        self.game.action_frequencies(&self.strategy, node)
    }
}

/// Solves a river spot, as the [module documentation](self) describes, calling `progress`
/// after each exploitability measurement.
///
/// Fails if the spot is not a valid river spot (see [`RiverGame::new`]) or an option is out of
/// range: a target that is negative or not finite, a `check_every` of 0, or DCFR parameters
/// that are not finite.
pub fn solve(
    spot: &Spot,
    options: &SolveOptions,
    mut progress: impl FnMut(&Progress),
) -> Result<Solution, SolveError> {
    let start = Instant::now();
    options.check()?;
    let game = RiverGame::new(spot)?;
    let mut solver = VectorCfr::new(game, options.schedule);
    let mut measure = |solver: &VectorCfr<RiverGame>| -> (Progress, PublicProfile) {
        let strategy = solver.average_strategy();
        let exploitability = exploitability(solver.game(), &strategy);
        let measured = Progress {
            iterations: solver.iterations(),
            elapsed: start.elapsed(),
            exploitability,
        };
        progress(&measured);
        (measured, strategy)
    };
    let mut last: Option<(Progress, PublicProfile)> = None;
    let stop = loop {
        if solver.iterations() >= options.max_iterations {
            break StopReason::IterationCap;
        }
        if options.max_time.is_some_and(|cap| start.elapsed() >= cap) {
            break StopReason::TimeCap;
        }
        solver.iterate();
        if solver.iterations().is_multiple_of(options.check_every) {
            let measured = measure(&solver);
            let reached = options
                .target
                .is_some_and(|target| measured.0.exploitability.percent_of_pot <= target);
            last = Some(measured);
            if reached {
                break StopReason::Target;
            }
        }
    };
    let (measured, strategy) = match last {
        Some((measured, strategy)) if measured.iterations == solver.iterations() => {
            (measured, strategy)
        }
        _ => measure(&solver),
    };
    let solution = Solution {
        game: solver.game().clone(),
        strategy,
        stop,
        iterations: measured.iterations,
        elapsed: start.elapsed(),
        exploitability: measured.exploitability,
    };
    // A debug assertion, which the elapsed time does not count.
    debug_assert_valid(&solution);
    Ok(solution)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load(name: &str) -> Spot {
        let path = format!(
            "{}/../../reference/spots/river/{name}.toml",
            env!("CARGO_MANIFEST_DIR")
        );
        Spot::load(path).unwrap()
    }

    /// Runs a solve, recording every progress call.
    fn run(spot: &Spot, options: SolveOptions) -> (Solution, Vec<Progress>) {
        let mut calls = Vec::new();
        let solution = solve(spot, &options, |p| calls.push(*p)).unwrap();
        (solution, calls)
    }

    fn capped(iterations: u64, check_every: u64) -> SolveOptions {
        SolveOptions {
            target: None,
            max_iterations: iterations,
            check_every,
            ..SolveOptions::default()
        }
    }

    fn bits(profile: &PublicProfile) -> Vec<Vec<u32>> {
        profile
            .nodes()
            .map(|node| {
                profile
                    .get(node)
                    .unwrap()
                    .iter()
                    .map(|p| p.to_bits())
                    .collect()
            })
            .collect()
    }

    #[test]
    fn the_defaults_are_dcfr_and_three_tenths_of_a_percent() {
        let options = SolveOptions::default();
        assert_eq!(options.schedule, Schedule::Discounted(Dcfr::default()));
        assert_eq!(options.target, Some(0.3));
        assert_eq!(options.max_iterations, 10_000);
        assert_eq!(options.max_time, None);
        assert_eq!(options.check_every, 10);
    }

    #[test]
    fn the_target_stops_the_solve_at_the_first_measurement_that_reaches_it() {
        let spot = load("toy_polarized");
        let options = SolveOptions {
            target: Some(0.3),
            check_every: 5,
            ..SolveOptions::default()
        };
        let (solution, calls) = run(&spot, options);
        assert_eq!(solution.stop(), StopReason::Target);
        let last = calls.last().unwrap();
        assert!(last.exploitability.percent_of_pot <= 0.3, "{last:?}");
        for earlier in &calls[..calls.len() - 1] {
            assert!(earlier.exploitability.percent_of_pot > 0.3, "{earlier:?}");
        }
        assert_eq!(solution.iterations(), last.iterations);
        assert_eq!(solution.iterations() % 5, 0);
        assert_eq!(solution.exploitability(), &last.exploitability);
        assert!(solution.iterations() < options.max_iterations);
    }

    #[test]
    fn the_iteration_cap_stops_the_solve_and_the_last_iteration_is_measured() {
        // 37 iterations measured every 10: calls after 10, 20 and 30, and after 37 at the end.
        let spot = load("toy_polarized");
        let (solution, calls) = run(&spot, capped(37, 10));
        assert_eq!(solution.stop(), StopReason::IterationCap);
        assert_eq!(solution.iterations(), 37);
        let counts: Vec<u64> = calls.iter().map(|p| p.iterations).collect();
        assert_eq!(counts, [10, 20, 30, 37]);
        assert_eq!(solution.exploitability(), &calls[3].exploitability);
        // When the cap falls on a measurement, it is not measured twice.
        let (solution, calls) = run(&spot, capped(30, 10));
        let counts: Vec<u64> = calls.iter().map(|p| p.iterations).collect();
        assert_eq!(counts, [10, 20, 30]);
        assert_eq!(solution.iterations(), 30);
        // The calls come in order of time as well.
        assert!(calls.windows(2).all(|w| w[0].elapsed <= w[1].elapsed));
        assert!(calls[2].elapsed <= solution.elapsed());
    }

    #[test]
    fn a_target_met_at_the_cap_is_reported_as_the_target() {
        // The target comes first among the rules. A target of 100% is met by any strategy.
        let spot = load("toy_polarized");
        let options = SolveOptions {
            target: Some(100.0),
            max_iterations: 10,
            check_every: 10,
            ..SolveOptions::default()
        };
        let (solution, calls) = run(&spot, options);
        assert_eq!(solution.stop(), StopReason::Target);
        assert_eq!((solution.iterations(), calls.len()), (10, 1));
    }

    #[test]
    fn a_cap_of_zero_returns_the_uniform_starting_strategy() {
        let spot = load("toy_polarized");
        let (solution, calls) = run(&spot, capped(0, 10));
        assert_eq!(solution.stop(), StopReason::IterationCap);
        assert_eq!(solution.iterations(), 0);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].iterations, 0);
        for node in solution.strategy().nodes() {
            assert!(
                solution
                    .strategy()
                    .get(node)
                    .unwrap()
                    .iter()
                    .all(|&p| p == 0.5)
            );
        }
        assert!(solution.exploitability().percent_of_pot > 1.0);
    }

    #[test]
    fn the_time_cap_stops_the_solve() {
        let spot = load("dry_wide_single_bet");
        // No time at all: the cap is checked before the first iteration.
        let options = SolveOptions {
            max_time: Some(Duration::ZERO),
            ..capped(1_000_000, 10)
        };
        let (solution, calls) = run(&spot, options);
        assert_eq!(solution.stop(), StopReason::TimeCap);
        assert_eq!(solution.iterations(), 0);
        assert_eq!(calls.len(), 1);
        // A short cap: the solve runs some iterations, then stops once the cap has passed.
        // Building the game takes about a millisecond, well inside the cap.
        let cap = Duration::from_millis(100);
        let options = SolveOptions {
            max_time: Some(cap),
            ..capped(1_000_000, 10)
        };
        let (solution, _) = run(&spot, options);
        assert_eq!(solution.stop(), StopReason::TimeCap);
        assert!(solution.iterations() > 0);
        assert!(solution.elapsed() >= cap);
    }

    #[test]
    fn the_same_iterations_give_the_same_strategy_bit_for_bit() {
        // Two runs, with different measurement intervals, and a run stopped by its target,
        // all agree with a run capped at the same count.
        let spot = load("four_flush");
        let (first, _) = run(&spot, capped(60, 1));
        let (second, _) = run(&spot, capped(60, 60));
        assert_eq!(bits(first.strategy()), bits(second.strategy()));
        assert_eq!(first.exploitability(), second.exploitability());
        let (targeted, _) = run(
            &spot,
            SolveOptions {
                target: Some(1.0),
                check_every: 7,
                ..SolveOptions::default()
            },
        );
        assert_eq!(targeted.stop(), StopReason::Target);
        let (same, _) = run(&spot, capped(targeted.iterations(), 1000));
        assert_eq!(bits(targeted.strategy()), bits(same.strategy()));
        // And the strategy is the solver's own average after that many iterations.
        let mut solver = VectorCfr::new(
            RiverGame::new(&spot).unwrap(),
            Schedule::Discounted(Dcfr::default()),
        );
        solver.run(60, |_| {});
        assert_eq!(bits(&solver.average_strategy()), bits(first.strategy()));
    }

    #[test]
    fn options_out_of_range_are_refused() {
        let spot = load("toy_polarized");
        let cases = [
            (
                SolveOptions {
                    target: Some(-0.1),
                    ..SolveOptions::default()
                },
                "target",
            ),
            (
                SolveOptions {
                    target: Some(f64::NAN),
                    ..SolveOptions::default()
                },
                "target",
            ),
            (
                SolveOptions {
                    check_every: 0,
                    ..SolveOptions::default()
                },
                "check_every",
            ),
            (
                SolveOptions {
                    schedule: Schedule::Discounted(Dcfr {
                        alpha: f64::INFINITY,
                        beta: 0.0,
                        gamma: 2.0,
                    }),
                    ..SolveOptions::default()
                },
                "schedule",
            ),
        ];
        for (options, expected) in cases {
            match solve(&spot, &options, |_| panic!("the solve should not start")) {
                Err(SolveError::Options { field, .. }) => assert_eq!(field, expected),
                other => panic!("{options:?}: {other:?}"),
            }
        }
    }

    #[test]
    fn a_flop_spot_is_refused_with_an_error() {
        let mut spot = load("toy_polarized");
        spot.board = "Ks9h6d".parse().unwrap();
        spot.tree.flop = spot.tree.river.clone();
        spot.tree.turn = spot.tree.river.clone();
        let error = solve(&spot, &SolveOptions::default(), |_| {}).unwrap_err();
        assert!(
            matches!(error, SolveError::NotRiver { cards: 3 }),
            "{error}"
        );
        assert!(error.to_string().contains("river board of 5"), "{error}");
    }
}
