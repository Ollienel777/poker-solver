//! The solve: vector-form discounted CFR on a [`PostflopGame`], run until its average strategy
//! is close enough to an equilibrium or a cap is reached.
//!
//! # The loop
//!
//! [`solve`] builds the spot's [`PostflopGame`], on the flop, the turn or the river, and runs
//! [`VectorCfr`] on it under [`SolveOptions::schedule`], DCFR(1.5, 0, 2) by default. Each
//! iteration updates OOP, then IP against OOP's updated strategy (alternating updates), as
//! [`pk_cfr::vector`] specifies. Regrets and strategy sums are stored as `f32`, one entry per
//! action and in-range combo of the acting player at each player node of the public tree, and
//! the strategy the solve reports is the average strategy. The public tree is refused before
//! anything is allocated for it if it has more than [`SolveOptions::max_nodes`] nodes, and so
//! is a solve whose [memory estimate](crate::memory) is over [`SolveOptions::max_memory`].
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
//! # Threads
//!
//! A solve on more than one thread ([`SolveOptions::threads`]) of a spot that has a chance node
//! makes a rayon pool of its own of that many threads, and runs each iteration and each
//! measurement on it, so that the walks run the children of chance nodes in parallel, as
//! [`pk_cfr::vector`'s parallel walk](pk_cfr::vector#the-parallel-walk) describes. It does not
//! touch rayon's global pool, so solves in one process, or a caller's own threads, do not share
//! threads with it. Everything else, the progress callback included, runs on the calling
//! thread. A river spot has no chance node, so it makes no pool and runs on the calling thread,
//! as does a solve on one thread.
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
//! the target and the caps. The walks give the same results bit for bit on any number of
//! threads (DECISIONS P5), so the strategy and every measurement are the same whatever
//! `threads` is too. A time cap makes the number of iterations depend on the machine, but not
//! the strategy reached at any given number.
//!
//! # References
//!
//! - Brown and Sandholm, *Solving Imperfect-Information Games via Discounted Regret
//!   Minimization*, AAAI (2019): discounted CFR, and DCFR(1.5, 0, 2).
//! - Johanson, Bard, Lanctot, Gibson and Bowling, *Efficient Nash Equilibrium Approximation
//!   through Monte Carlo Counterfactual Regret Minimization*, AAMAS (2012): CFR over the public
//!   tree, with one regret per (public node, private hand).

use std::num::NonZero;
use std::thread;
use std::time::{Duration, Instant};

use pk_cfr::{Dcfr, PublicProfile, Schedule, VectorCfr};
use pk_game::{Player, PostflopTree, TreeError};
use rayon::{ThreadPool, ThreadPoolBuilder};
use thiserror::Error;

use crate::exploitability::{Exploitability, exploitability};
use crate::game::{ClassView, PostflopGame};
use crate::hands::HandList;
use crate::invariants::debug_assert_valid;
use crate::memory::{estimate_memory, format_bytes};
use crate::spot::{Spot, SpotError};

// The tests below build the river game by the name they were written with.
#[cfg(test)]
use crate::river::RiverGame;

/// Why a game could not be built, or a solve could not start.
#[derive(Debug, Error)]
pub enum SolveError {
    /// The spot does not start on the river, and a river game was asked for: see
    /// [`PostflopGame::new`]. [`solve`] takes spots on every street.
    #[error("the board has {cards} cards; a river game needs a river board of 5")]
    NotRiver { cards: usize },
    /// The spot fails [`Spot::validate`].
    #[error("invalid spot: {0}")]
    Spot(#[from] SpotError),
    /// The spot's betting tree has more nodes than the builder's limit.
    #[error("{0}")]
    Tree(#[from] TreeError),
    /// The public tree, with a child for every card a chance node can deal, would have more
    /// nodes than the limit: see [`PostflopGame::with_node_limit`].
    #[error(
        "the public tree, with a child for every card that can fall, would have {nodes} nodes, \
         more than the limit of {limit}"
    )]
    PublicTreeTooLarge { nodes: u64, limit: u64 },
    /// The solve would need more memory than [`SolveOptions::max_memory`], by the
    /// [memory estimate](crate::memory): both are in bytes.
    #[error(
        "the solve would need about {} of memory by its estimate, more than the limit of {}",
        format_bytes(*.estimate),
        format_bytes(*.limit)
    )]
    MemoryTooLarge { estimate: u64, limit: u64 },
    /// An option is out of range.
    #[error("`{field}` {reason}")]
    Options { field: &'static str, reason: String },
    /// The solve's pool of [`SolveOptions::threads`] threads could not be started.
    #[error("could not start a pool of {threads} threads: {reason}")]
    Threads { threads: usize, reason: String },
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
    /// The most nodes the public tree may have: see [`PostflopGame::with_node_limit`]. Defaults
    /// to [`PostflopGame::DEFAULT_MAX_NODES`], one million. Must be at least 1.
    pub max_nodes: u64,
    /// The most memory the solve may need, in bytes: a spot whose
    /// [estimate](crate::memory::estimate_memory) is larger is refused before anything is
    /// allocated for it. Defaults to [`SolveOptions::DEFAULT_MAX_MEMORY`], 10 GB.
    pub max_memory: u64,
    /// The threads the solve runs its walks on, in a pool of its own when there is more than one
    /// (see [the module documentation](self#threads)). The results are the same for every
    /// count. Defaults to [`SolveOptions::default_threads`], all the machine offers. Must be at
    /// least 1.
    pub threads: usize,
}

impl Default for SolveOptions {
    fn default() -> SolveOptions {
        SolveOptions {
            schedule: Schedule::Discounted(Dcfr::default()),
            target: Some(0.3),
            max_iterations: 10_000,
            max_time: None,
            check_every: 10,
            max_nodes: PostflopGame::DEFAULT_MAX_NODES,
            max_memory: SolveOptions::DEFAULT_MAX_MEMORY,
            threads: SolveOptions::default_threads(),
        }
    }
}

impl SolveOptions {
    /// The default memory limit: 10 GB, 10^10 bytes, the most a local solve may use
    /// (DECISIONS D17).
    pub const DEFAULT_MAX_MEMORY: u64 = 10_000_000_000;

    /// The default number of threads: all the machine offers, as
    /// [`std::thread::available_parallelism`] gives it, or 1 if it cannot say.
    pub fn default_threads() -> usize {
        thread::available_parallelism().map_or(1, NonZero::get)
    }

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
        if self.max_nodes == 0 {
            return Err(SolveError::Options {
                field: "max_nodes",
                reason: "is 0; a public tree has at least its root".into(),
            });
        }
        if self.threads == 0 {
            return Err(SolveError::Options {
                field: "threads",
                reason: "is 0; a solve runs on 1 or more threads".into(),
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
/// Node indices are those of the game's public tree ([`Solution::game`]), which on a river spot
/// are those of the [`PostflopTree`] ([`Solution::tree`]); [`PostflopGame::find`] gives the
/// node of a path. Hand indices are those of each player's [`HandList`] ([`Solution::hands`]).
#[derive(Debug, Clone)]
pub struct Solution {
    game: PostflopGame,
    strategy: PublicProfile,
    stop: StopReason,
    iterations: u64,
    elapsed: Duration,
    exploitability: Exploitability,
}

impl Solution {
    /// The game that was solved.
    pub fn game(&self) -> &PostflopGame {
        &self.game
    }

    /// The betting tree. Its node indices are the public tree's on a river spot only: see
    /// [`PostflopGame::tree_node`].
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
    /// counted from the start of the spot, indexed by [`Player::index`].
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
    /// see [`PostflopGame::reach`]. Panics if `node` is out of range.
    pub fn reach(&self, node: u32) -> [Vec<f64>; 2] {
        self.game.reach(&self.strategy, node)
    }

    /// The probability that the acting player holds each of its hands once `node` is reached
    /// under the average strategy: see [`PostflopGame::hand_weights`].
    pub fn hand_weights(&self, node: u32) -> Option<Vec<f64>> {
        self.game.hand_weights(&self.strategy, node)
    }

    /// The probability of each action once `node` is reached under the average strategy, card
    /// removal by the opponent's range included: see [`PostflopGame::action_frequencies`]. This
    /// is how often each action is actually taken there, not what a range view shows.
    pub fn action_frequencies(&self, node: u32) -> Option<Vec<f64>> {
        self.game.action_frequencies(&self.strategy, node)
    }

    /// Each hand's share of the acting player's range at `node` by its own combos alone, range
    /// weight times own reach, under the average strategy: see
    /// [`PostflopGame::range_view_weights`].
    pub fn range_view_weights(&self, node: u32) -> Option<Vec<f64>> {
        self.game.range_view_weights(&self.strategy, node)
    }

    /// Each action's frequency at `node` as a solver's range view shows it, over the acting
    /// player's own combos with no card removal by the opponent's range, under the average
    /// strategy: see [`PostflopGame::range_view_frequencies`]. Use it to compare with another
    /// solver; use [`Solution::action_frequencies`] for how often an action is taken.
    pub fn range_view_frequencies(&self, node: u32) -> Option<Vec<f64>> {
        self.game.range_view_frequencies(&self.strategy, node)
    }

    /// Each hand class in the acting player's range at `node`, with its share of the range and
    /// its frequency for each action, as a range view shows them, under the average strategy:
    /// see [`PostflopGame::range_view_classes`].
    pub fn range_view_classes(&self, node: u32) -> Option<Vec<ClassView>> {
        self.game.range_view_classes(&self.strategy, node)
    }

    /// The probability that a hand dealt plays to `node` under the average strategy: see
    /// [`PostflopGame::reach_probability`]. Panics if `node` is out of range.
    pub fn reach_probability(&self, node: u32) -> f64 {
        self.game.reach_probability(&self.strategy, node)
    }
}

/// Solves a spot on the flop, the turn or the river, as the [module documentation](self)
/// describes, calling `progress` after each exploitability measurement.
///
/// Fails if the game cannot be built (see [`PostflopGame::with_node_limit`]): a spot that does
/// not pass [`Spot::validate`], or a betting or public tree over `options.max_nodes`. Fails as
/// well if an option is out of range: a target that is negative or not finite, a `check_every`,
/// `max_nodes` or `threads` of 0, or DCFR parameters that are not finite. And fails with
/// [`SolveError::MemoryTooLarge`], before the game is built, if the
/// [memory estimate](crate::memory::estimate_memory) is over `options.max_memory`, and with
/// [`SolveError::Threads`] if its threads cannot be started.
pub fn solve(
    spot: &Spot,
    options: &SolveOptions,
    mut progress: impl FnMut(&Progress),
) -> Result<Solution, SolveError> {
    let start = Instant::now();
    options.check()?;
    let estimate = estimate_memory(spot, options)?;
    if estimate.total > options.max_memory {
        return Err(SolveError::MemoryTooLarge {
            estimate: estimate.total,
            limit: options.max_memory,
        });
    }
    let game = PostflopGame::with_node_limit(spot, options.max_nodes)?;
    let mut solver = VectorCfr::new(game, options.schedule);
    // The solve's own pool, when it has threads to share out and chance nodes to split.
    let pool = if estimate.threads > 1 {
        let pool = ThreadPoolBuilder::new()
            .num_threads(options.threads)
            .build()
            .map_err(|error| SolveError::Threads {
                threads: options.threads,
                reason: error.to_string(),
            })?;
        Some(pool)
    } else {
        None
    };
    let mut measure = |solver: &VectorCfr<PostflopGame>| -> (Progress, PublicProfile) {
        let (strategy, exploitability) = on_pool(pool.as_ref(), || {
            let strategy = solver.average_strategy();
            let exploitability = exploitability(solver.game(), &strategy);
            (strategy, exploitability)
        });
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
        on_pool(pool.as_ref(), || solver.iterate());
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

/// Runs `walk` on the solve's pool, if it has one, and otherwise on the calling thread.
fn on_pool<R: Send>(pool: Option<&ThreadPool>, walk: impl FnOnce() -> R + Send) -> R {
    match pool {
        Some(pool) => pool.install(walk),
        None => walk(),
    }
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
    fn a_flop_spot_is_solved_and_only_the_river_game_refuses_it() {
        // Until M4 the solve refused this spot. It now builds the flop game and measures it,
        // while the river game, which the dump and the comparison build, still refuses it.
        let mut spot = load("toy_polarized");
        spot.board = "Ks9h6d".parse().unwrap();
        spot.tree.flop = spot.tree.river.clone();
        spot.tree.turn = spot.tree.river.clone();
        let (solution, calls) = run(&spot, capped(0, 10));
        assert_eq!(calls.len(), 1);
        assert_eq!(solution.tree().spec().street, pk_game::Street::Flop);
        assert!(solution.game().board(0) == spot.board);
        assert!(solution.exploitability().percent_of_pot > 1.0);
        let error = RiverGame::new(&spot).unwrap_err();
        assert!(
            matches!(error, SolveError::NotRiver { cards: 3 }),
            "{error}"
        );
        assert!(error.to_string().contains("river board of 5"), "{error}");
    }
}
