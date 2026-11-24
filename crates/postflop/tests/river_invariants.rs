//! The margins of the solution invariants on real solves, from which the tolerances of
//! `pk_postflop::invariants` were set, and what checking a solution costs. Every solution here
//! must break no invariant.

use std::fs;
use std::path::PathBuf;
use std::time::Instant;

use pk_cfr::{Dcfr, Schedule};
use pk_core::Range;
use pk_game::Player;
use pk_postflop::invariants::{REACH_TOLERANCE, STRATEGY_TOLERANCE, VALUE_TOLERANCE, Worst};
use pk_postflop::{SolveOptions, Spot, check_solution, exploitability, solve};

fn repo_path(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
}

/// Every river spot, sorted by name.
fn all_spots() -> Vec<Spot> {
    let root = repo_path("reference/spots/river");
    let mut paths: Vec<PathBuf> = fs::read_dir(&root)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    paths.sort();
    paths.iter().map(|path| Spot::load(path).unwrap()).collect()
}

/// Every combo in both ranges, on a board with a flush and a straight possible, with three bet
/// sizes, raises and all-in, deep and raked: the most hands a river spot can have, and many
/// terminals with large pots.
fn full_ranges() -> Spot {
    let mut spot = Spot::from_toml(
        r#"format = 1
name = "full_ranges"
description = "Illustrative ranges: every combo for both players, for the invariant margins."
board = "Th9h6d3c2h"
pot_bb = 10.0
effective_stack_bb = 150.0
oop_range = "AA"
ip_range = "AA"

[rake]
percent = 5.0
cap_bb = 3.0

[tree]
max_raises = 2
allin_threshold = 1.0
river = { bets = [0.33, 0.75, 1.5], raises = [2.5], allin = true }
"#,
    )
    .unwrap();
    spot.oop_range = Range::full();
    spot.ip_range = Range::full();
    spot
}

/// The solves measured on each spot: the default solve to its target, DCFR capped at counts
/// from 0 to 1,000 iterations, and the other schedules at two counts.
fn runs() -> Vec<(String, SolveOptions)> {
    let capped = |schedule: Schedule, iterations: u64| SolveOptions {
        schedule,
        target: None,
        max_iterations: iterations,
        check_every: iterations.max(1),
        ..SolveOptions::default()
    };
    let mut runs = vec![("default".to_string(), SolveOptions::default())];
    let dcfr = Schedule::Discounted(Dcfr::default());
    for iterations in [0, 1, 10, 100, 1000] {
        runs.push((format!("DCFR, {iterations}"), capped(dcfr, iterations)));
    }
    for schedule in [Schedule::Vanilla, Schedule::CfrPlus, Schedule::Linear] {
        for iterations in [10, 300] {
            runs.push((
                format!("{schedule:?}, {iterations}"),
                capped(schedule, iterations),
            ));
        }
    }
    runs
}

fn widen(worst: &mut Worst, other: &Worst) {
    worst.strategy = worst.strategy.max(other.strategy);
    worst.reach = worst.reach.max(other.reach);
    worst.value_sum = worst.value_sum.max(other.value_sum);
}

#[test]
#[ignore = "slow"]
fn invariant_margins() {
    // Prints, with --nocapture, the largest deviation each check found on each spot over all
    // its solves, and the time a check takes against one exploitability measurement and the
    // default solve. Run in release, as the slow tests are, the check is not part of the solve.
    // Measured: strategy sums within 2.1e-7 of 1, reaches exact, and value sums within 2.3e-15
    // of the largest pot (6.1e-16 on the river test spots); a check takes 0.02 to 1.3 ms on
    // the test spots and 5.3 ms with every combo in both ranges, 1.1 to 1.4 times one
    // measurement.
    println!(
        "| Spot | Hands | Strategy sums | Reach | Value sum, of the largest pot | Check ms | \
         Measurement ms | Default solve ms |"
    );
    println!("|---|---:|---:|---:|---:|---:|---:|---:|");
    let mut overall = Worst::default();
    let mut spots = all_spots();
    spots.push(full_ranges());
    for spot in &spots {
        let name = &spot.name;
        let mut worst = Worst::default();
        for (run, options) in runs() {
            let solution = solve(spot, &options, |_| {}).unwrap();
            let report = check_solution(&solution);
            assert!(report.is_valid(), "{name}, {run}: {report}");
            assert!(report.value_sum.is_some(), "{name}, {run}");
            widen(&mut worst, &report.worst);
        }
        // The costs, on the default solve.
        let solution = solve(spot, &SolveOptions::default(), |_| {}).unwrap();
        let repeats = 20;
        let start = Instant::now();
        for _ in 0..repeats {
            std::hint::black_box(check_solution(&solution));
        }
        let check_ms = start.elapsed().as_secs_f64() * 1e3 / f64::from(repeats);
        let start = Instant::now();
        for _ in 0..repeats {
            std::hint::black_box(exploitability(solution.game(), solution.strategy()));
        }
        let measure_ms = start.elapsed().as_secs_f64() * 1e3 / f64::from(repeats);
        let hands = [Player::Oop, Player::Ip].map(|player| solution.hands(player).len());
        println!(
            "| `{name}` | {}, {} | {:.2e} | {:.2e} | {:.2e} | {check_ms:.2} | {measure_ms:.2} | \
             {:.1} |",
            hands[0],
            hands[1],
            worst.strategy,
            worst.reach,
            worst.value_sum,
            solution.elapsed().as_secs_f64() * 1e3
        );
        widen(&mut overall, &worst);
    }
    println!(
        "| all | | {:.2e} | {:.2e} | {:.2e} | | | |",
        overall.strategy, overall.reach, overall.value_sum
    );
    println!(
        "tolerances: strategy sums {STRATEGY_TOLERANCE:e}, reach {REACH_TOLERANCE:e}, value sum \
         {VALUE_TOLERANCE:e}"
    );
    assert!(overall.strategy <= STRATEGY_TOLERANCE);
    assert!(overall.reach <= REACH_TOLERANCE);
    assert!(overall.value_sum <= VALUE_TOLERANCE);
}
