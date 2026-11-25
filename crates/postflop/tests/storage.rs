//! Solves with compressed storage (`SolveOptions::compressed_storage`, `pk_cfr::compressed`)
//! against the same solves with f32 storage, on small spots of every street: a river spot, a
//! turn spot and a monotone flop, which the solve merges.
//!
//! The compressed solver rounds what it stores at every update, so its iterations part from the
//! f32 solver's, and the two solves are held to each other by what they measure: the
//! exploitability and the values of their average strategies, and the iteration at which each
//! reaches the default target. The tolerances come from the measurements in `BENCHMARKS.md`,
//! which follow both storages to 10,000 and 20,000 iterations: there the compressed solves stop
//! improving between 0.0002% and 0.003% of the pot, where the f32 solves go on.

use pk_postflop::{SolveOptions, Spot, estimate_memory, solve};

fn load(path: &str) -> Spot {
    Spot::load(format!("{}/{path}.toml", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

/// The spots, with each one's iterations to the default target under both storages, measured
/// on 16 threads, which every thread count gives.
const SPOTS: [(&str, u64); 3] = [
    ("../../reference/spots/river/four_flush", 50),
    ("tests/data/turn_small", 60),
    ("tests/data/flop_monotone", 50),
];

#[test]
fn a_compressed_solve_reaches_the_exploitability_and_values_of_the_f32_solve() {
    // After 500 iterations, the compressed solve's exploitability is within 0.002% of the pot of
    // the f32 solve's, and each player's value within 0.001% of the pot. Measured, the
    // exploitabilities are 0.005500% and 0.005466% on the river spot, 0.004180% and 0.004531%
    // on the turn spot, and 0.002202% and 0.002952% on the flop, and the values at most 0.0003%
    // of the pot apart.
    for (path, _) in SPOTS {
        let capped = |compressed_storage| SolveOptions {
            target: None,
            max_iterations: 500,
            check_every: 500,
            compressed_storage,
            ..SolveOptions::default()
        };
        let full = solve(&load(path), &capped(false), |_| {}).unwrap();
        let half = solve(&load(path), &capped(true), |_| {}).unwrap();
        let pot = f64::from(full.game().pot());
        let (ours, theirs) = (half.exploitability(), full.exploitability());
        println!(
            "{path}: {:.6}% against {:.6}% of the pot",
            ours.percent_of_pot, theirs.percent_of_pot
        );
        assert!(
            (ours.percent_of_pot - theirs.percent_of_pot).abs() < 0.002,
            "{path}: {ours:?} against {theirs:?}"
        );
        for (a, b) in ours.values.iter().zip(theirs.values) {
            assert!(
                100.0 * (a - b).abs() / pot < 0.001,
                "{path}: values {:?} against {:?}",
                ours.values,
                theirs.values
            );
        }
        // The storage is what differs: the strategies are not the same.
        assert!(half.strategy() != full.strategy(), "{path}");
    }
}

#[test]
fn a_compressed_solve_stops_at_the_default_target_with_the_f32_solve() {
    // Each reaches 0.3% of the pot at the same measurement as the f32 solve, and holds its
    // regrets and strategy sums in half the bytes by the estimate.
    for (path, iterations) in SPOTS {
        let spot = load(path);
        for compressed_storage in [false, true] {
            let options = SolveOptions {
                compressed_storage,
                ..SolveOptions::default()
            };
            let solution = solve(&spot, &options, |_| {}).unwrap();
            assert_eq!(solution.iterations(), iterations, "{path}, {options:?}");
            assert!(solution.exploitability().percent_of_pot <= 0.3, "{path}");
        }
        let f32 = estimate_memory(&spot, &SolveOptions::default()).unwrap();
        let compressed = SolveOptions {
            compressed_storage: true,
            ..SolveOptions::default()
        };
        let half = estimate_memory(&spot, &compressed).unwrap();
        assert_eq!(2 * half.regrets_and_sums, f32.regrets_and_sums, "{path}");
    }
}
