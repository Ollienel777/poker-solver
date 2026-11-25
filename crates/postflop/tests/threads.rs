//! The solver on several threads against the solver on one, on the turn and flop spots of
//! `tests/data`: every stored regret and strategy sum, the average strategy, both best
//! responses with their strategies, both expected values and the exploitability are the same
//! bit for bit, and so is a solve with every measurement it makes (DECISIONS P5). The same holds
//! for the solver of a merged game, on spots with symmetries to merge.
//!
//! The pools have more threads than the CI's machines have cores, so that the walks of a chance
//! node's children interleave there too. A race would show only sometimes, so these tests are
//! the ones to run in a loop after a change to the parallel walks.

use pk_cfr::{
    Dcfr, PublicGame, PublicProfile, Schedule, VectorCfr, vector_best_response,
    vector_expected_value,
};
use pk_postflop::{
    Exploitability, MergedGame, PostflopGame, Progress, SolveOptions, Spot, exploitability, solve,
};
use rayon::ThreadPoolBuilder;

fn load(name: &str) -> Spot {
    let path = format!("{}/tests/data/{name}.toml", env!("CARGO_MANIFEST_DIR"));
    Spot::load(path).unwrap()
}

fn f32_bits(values: &[f32]) -> Vec<u32> {
    values.iter().map(|v| v.to_bits()).collect()
}

fn f64_bits(values: &[f64]) -> Vec<u64> {
    values.iter().map(|v| v.to_bits()).collect()
}

/// A profile's probabilities as bits, node by node.
type ProfileBits = Vec<(usize, Vec<u32>)>;

/// The bits of every probability of a profile, node by node.
fn profile_bits(profile: &PublicProfile) -> ProfileBits {
    let nodes = profile.nodes();
    nodes
        .map(|node| (node, f32_bits(profile.get(node).unwrap())))
        .collect()
}

/// The bits of an exploitability measurement: the values, the best responses, the chips and
/// the percent of the pot.
fn exploitability_bits(measured: &Exploitability) -> Vec<u64> {
    let mut bits = f64_bits(&measured.values);
    bits.extend(f64_bits(&measured.best_responses));
    bits.extend(f64_bits(&[measured.chips, measured.percent_of_pot]));
    bits
}

/// Everything a run of DCFR computes, as bits, for comparing runs on different threads.
#[derive(Debug, PartialEq)]
struct Computed {
    /// Each player node's regrets and strategy sum.
    stored: Vec<(usize, Vec<u32>, Vec<u32>)>,
    average: ProfileBits,
    /// Each player's best response: its value, its hand values and its pure strategy.
    responses: Vec<(u64, Vec<u64>, ProfileBits)>,
    /// Each player's expected value and hand values.
    values: Vec<(u64, Vec<u64>)>,
    exploitability: Vec<u64>,
}

/// Runs `iterations` iterations of DCFR(1.5, 0, 2) on `game` and measures the average strategy,
/// on the calling thread or on the threads of the pool it is called in.
fn compute(game: &PostflopGame, iterations: u64) -> Computed {
    let mut solver = VectorCfr::new(game.clone(), Schedule::Discounted(Dcfr::default()));
    solver.run(iterations, |_| {});
    let stored = (0..game.num_nodes())
        .filter_map(|node| {
            let regrets = solver.regrets(node)?;
            let sums = solver.strategy_sum(node)?;
            Some((node, f32_bits(regrets), f32_bits(sums)))
        })
        .collect();
    let average = solver.average_strategy();
    let responses = (0..2)
        .map(|player| {
            let response = vector_best_response(game, &average, player);
            let strategy = profile_bits(&response.strategy);
            (
                response.value.to_bits(),
                f64_bits(&response.hand_values),
                strategy,
            )
        })
        .collect();
    let values = (0..2)
        .map(|player| {
            let value = vector_expected_value(game, &average, player);
            (value.value.to_bits(), f64_bits(&value.hand_values))
        })
        .collect();
    Computed {
        stored,
        average: profile_bits(&average),
        responses,
        values,
        exploitability: exploitability_bits(&exploitability(game, &average)),
    }
}

/// `compute` on the calling thread, and in pools of several threads, gives the same bits.
fn same_on_threads(name: &str, iterations: u64) {
    let game = PostflopGame::from_spot(&load(name)).unwrap();
    let alone = compute(&game, iterations);
    assert!(alone.stored.len() > 100, "{name}");
    for threads in [2, 7, 16] {
        let pool = ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .unwrap();
        let pooled = pool.install(|| compute(&game, iterations));
        let at = format!("{name}, {threads} threads");
        assert!(
            pooled.stored == alone.stored,
            "{at}: the stored values differ"
        );
        assert!(
            pooled.average == alone.average,
            "{at}: the average strategy differs"
        );
        assert!(
            pooled.responses == alone.responses,
            "{at}: a best response differs"
        );
        assert_eq!(pooled.values, alone.values, "{at}");
        assert_eq!(pooled.exploitability, alone.exploitability, "{at}");
    }
}

#[test]
fn on_several_threads_the_turn_spot_iterates_and_measures_as_on_one() {
    same_on_threads("turn_small", 30);
}

#[test]
fn on_several_threads_the_flop_spot_iterates_and_measures_as_on_one() {
    same_on_threads("flop_small", 6);
}

#[test]
fn a_solve_on_several_threads_is_the_solve_on_one() {
    // The turn spot to its target, and the flop spot capped, measured every 5 and 4 iterations:
    // the same iterations, the same stopping rule, the same strategy and the same measurement
    // at every progress call.
    let turn = SolveOptions {
        check_every: 5,
        ..SolveOptions::default()
    };
    let flop = SolveOptions {
        target: None,
        max_iterations: 10,
        check_every: 4,
        ..SolveOptions::default()
    };
    for (name, options) in [("turn_small", turn), ("flop_small", flop)] {
        let spot = load(name);
        let run = |threads: usize| {
            let options = SolveOptions { threads, ..options };
            let mut calls: Vec<Progress> = Vec::new();
            let solution = solve(&spot, &options, |progress| calls.push(*progress)).unwrap();
            let measured: Vec<(u64, Vec<u64>)> = calls
                .iter()
                .map(|call| (call.iterations, exploitability_bits(&call.exploitability)))
                .collect();
            (solution, measured)
        };
        let (alone, alone_calls) = run(1);
        for threads in [3, 16] {
            let (pooled, pooled_calls) = run(threads);
            let at = format!("{name}, {threads} threads");
            assert_eq!(pooled.stop(), alone.stop(), "{at}");
            assert_eq!(pooled.iterations(), alone.iterations(), "{at}");
            assert!(
                profile_bits(pooled.strategy()) == profile_bits(alone.strategy()),
                "{at}: the strategy differs"
            );
            assert_eq!(pooled_calls, alone_calls, "{at}");
        }
    }
}

#[test]
fn a_solve_refuses_zero_threads() {
    let options = SolveOptions {
        threads: 0,
        ..SolveOptions::default()
    };
    match solve(&load("turn_small"), &options, |_| {}) {
        Err(pk_postflop::SolveError::Options { field, .. }) => assert_eq!(field, "threads"),
        other => panic!("{other:?}"),
    }
}

/// Each player node's regrets and strategy sum, as bits.
type Stored = Vec<(usize, Vec<u32>, Vec<u32>)>;

/// Everything a run of DCFR on the merged game of `game` computes, as bits: the merged solver's
/// stored values, its average strategy expanded onto the full tree, and that strategy's
/// exploitability on the full game, on the calling thread or on the threads of the pool it is
/// called in.
fn compute_merged(game: &PostflopGame, iterations: u64) -> (Stored, ProfileBits, Vec<u64>) {
    let merged = MergedGame::new(game).unwrap();
    let mut solver = VectorCfr::new(merged, Schedule::Discounted(Dcfr::default()));
    solver.run(iterations, |_| {});
    let stored = (0..solver.game().num_nodes())
        .filter_map(|node| {
            let regrets = solver.regrets(node)?;
            let sums = solver.strategy_sum(node)?;
            Some((node, f32_bits(regrets), f32_bits(sums)))
        })
        .collect();
    let average = MergedGame::average_strategy(&solver);
    let measured = exploitability_bits(&exploitability(game, &average));
    (stored, profile_bits(&average), measured)
}

#[test]
fn on_several_threads_merged_spots_iterate_and_measure_as_on_one() {
    // A monotone flop, merged under the six relabellings of its three other suits, and a turn
    // of four clubs, merged likewise: the split chance nodes have 23 and 22 outcomes, each with
    // its images.
    for (name, iterations) in [("flop_monotone", 12), ("turn_four_flush", 30)] {
        let game = PostflopGame::from_spot(&load(name)).unwrap();
        let alone = compute_merged(&game, iterations);
        assert!(alone.0.len() > 100, "{name}");
        for threads in [2, 7, 16] {
            let pool = ThreadPoolBuilder::new()
                .num_threads(threads)
                .build()
                .unwrap();
            let pooled = pool.install(|| compute_merged(&game, iterations));
            let at = format!("{name}, {threads} threads");
            assert!(pooled.0 == alone.0, "{at}: the stored values differ");
            assert!(pooled.1 == alone.1, "{at}: the average strategy differs");
            assert_eq!(pooled.2, alone.2, "{at}");
        }
    }
}

#[test]
fn a_merged_solve_on_several_threads_is_the_solve_on_one() {
    // A two-tone flop and a paired flop, merged by default, capped and measured every 4
    // iterations: the same strategy and the same measurement at every progress call.
    let options = SolveOptions {
        target: None,
        max_iterations: 10,
        check_every: 4,
        ..SolveOptions::default()
    };
    for name in ["flop_two_tone", "flop_paired"] {
        let spot = load(name);
        let run = |threads: usize| {
            let options = SolveOptions { threads, ..options };
            let mut calls: Vec<Progress> = Vec::new();
            let solution = solve(&spot, &options, |progress| calls.push(*progress)).unwrap();
            let measured: Vec<(u64, Vec<u64>)> = calls
                .iter()
                .map(|call| (call.iterations, exploitability_bits(&call.exploitability)))
                .collect();
            (profile_bits(solution.strategy()), measured)
        };
        let alone = run(1);
        for threads in [3, 16] {
            assert!(run(threads) == alone, "{name}, {threads} threads");
        }
    }
}
