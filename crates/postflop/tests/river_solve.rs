//! Solving the river test spots under `reference/spots/river/`: the polarized toy game against
//! its analytical equilibrium, and every spot to below the target exploitability.

use std::cell::Cell;
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use pk_cfr::{Dcfr, PublicGame, PublicNode, Schedule, VectorCfr};
use pk_game::{Node, Player};
use pk_postflop::{RiverGame, SolveOptions, Spot, StopReason, exploitability, solve};

fn repo_path(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
}

fn load(name: &str) -> Spot {
    Spot::load(repo_path(&format!("reference/spots/river/{name}.toml"))).unwrap()
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

// ---------------------------------------------------------------------------------------------
// The toy game

/// The toy game's numbers in a solution: the exploitability, the smallest probability with
/// which a nut combo bets, the share of bluffs in OOP's bets, IP's calling frequency, and
/// each player's value in chips.
#[derive(Debug, Clone, Copy)]
struct Toy {
    iterations: u64,
    exploitability: f64,
    nuts_bet: f64,
    bluff_share: f64,
    calls: f64,
    values: [f64; 2],
}

fn toy(iterations: u64) -> Toy {
    let spot = load("toy_polarized");
    let options = SolveOptions {
        target: None,
        max_iterations: iterations,
        check_every: iterations.max(1),
        ..SolveOptions::default()
    };
    let solution = solve(&spot, &options, |_| {}).unwrap();
    let root = 0;
    let Node::Player {
        actions, children, ..
    } = solution.tree().node(root)
    else {
        panic!("the root is OOP's decision");
    };
    assert_eq!(actions.len(), 2, "check or bet");
    let after_bet = children[1];
    let oop = solution.hands(Player::Oop);
    let is_nuts = |hand: usize| oop.combos()[hand].lo().rank() == oop.combos()[hand].hi().rank();
    let bets: Vec<f64> = (0..oop.len())
        .map(|hand| f64::from(solution.hand_strategy(root, hand).unwrap()[1]))
        .collect();
    let nuts_bet = (0..oop.len())
        .filter(|&h| is_nuts(h))
        .map(|h| bets[h])
        .fold(1.0, f64::min);
    let weights = solution.hand_weights(root).unwrap();
    let betting: f64 = weights.iter().zip(&bets).map(|(w, b)| w * b).sum();
    let bluffing: f64 = (0..oop.len())
        .filter(|&h| !is_nuts(h))
        .map(|h| weights[h] * bets[h])
        .sum();
    let frequencies = solution.action_frequencies(after_bet).unwrap();
    Toy {
        iterations,
        exploitability: solution.exploitability().percent_of_pot,
        nuts_bet,
        bluff_share: bluffing / betting,
        calls: frequencies[1],
        values: solution.values(),
    }
}

#[test]
fn the_toy_game_reaches_its_analytical_equilibrium() {
    // The README's equilibrium, with b = 0.5: OOP bets every nut combo, bluffs are
    // b / (1 + 2b) = 1/4 of its bets, IP calls 1 / (1 + b) = 2/3 of them, and the values are
    // 40/11 and 70/11 bb. Only the totals over each range are pinned down. Measured with
    // DCFR(1.5, 0, 2), as distances from the analytical values (exploitability as a percent
    // of the pot, values in chips):
    //
    // | Iterations | Exploitability | Bluff share | Calls    | OOP value | IP value |
    // |-----------:|---------------:|------------:|---------:|----------:|---------:|
    // | 100        | 4.8e-2         | -1.1e-3     | +2.9e-3  | +2.1e-3   | -2.1e-3  |
    // | 1,000      | 5.3e-3         | +3.7e-4     | +1.3e-4  | -1.2e-5   | +2.1e-6  |
    // | 10,000     | 3.5e-4         | +4.8e-6     | +4.3e-5  | +2.2e-5   | +1.1e-5  |
    // | 30,000     | 8.1e-5         | +1.3e-6     | +9.5e-6  | -9.3e-9   | +9.3e-9  |
    //
    // Every nut combo bets with probability 1 to six decimals at each count measured from 300
    // on. The test runs 10,000 iterations, about 0.1 s, with bounds a few times the
    // measurements.
    let toy = toy(10_000);
    assert_eq!(toy.iterations, 10_000);
    assert!(toy.exploitability < 1e-3, "{toy:?}");
    assert!(toy.nuts_bet > 1.0 - 1e-5, "{toy:?}");
    assert!((toy.bluff_share - 0.25).abs() < 5e-5, "{toy:?}");
    assert!((toy.calls - 2.0 / 3.0).abs() < 2e-4, "{toy:?}");
    // 40/11 bb and 70/11 bb, which add up to the 10bb pot: the game is not zero-sum.
    assert!((toy.values[0] - 4000.0 / 11.0).abs() < 2e-4, "{toy:?}");
    assert!((toy.values[1] - 7000.0 / 11.0).abs() < 2e-4, "{toy:?}");
}

#[test]
fn every_river_spot_solves_below_the_target() {
    // The default solve: DCFR(1.5, 0, 2) to 0.3% of the pot, measured every 10 iterations.
    // Measured, the nine spots take 30 to 350 iterations and 0.35 s in all (see
    // `river_spot_table`).
    for spot in all_spots() {
        let solution = solve(&spot, &SolveOptions::default(), |_| {}).unwrap();
        let name = &spot.name;
        assert_eq!(solution.stop(), StopReason::Target, "{name}");
        let measured = solution.exploitability();
        assert!(measured.percent_of_pot <= 0.3, "{name}: {measured:?}");
        assert!(
            solution.iterations() <= 1000,
            "{name}: {}",
            solution.iterations()
        );
        // The exploitability is the strategy's own, and each player's gain from deviating
        // is not negative beyond rounding.
        assert_eq!(
            &exploitability(solution.game(), solution.strategy()),
            measured,
            "{name}"
        );
        for gain in measured.gains() {
            assert!(gain > -1e-9, "{name}: {measured:?}");
        }
        // The uniform strategy the solve started from was far from it.
        let start = SolveOptions {
            max_iterations: 0,
            ..SolveOptions::default()
        };
        let uniform = solve(&spot, &start, |_| {}).unwrap();
        assert!(uniform.exploitability().percent_of_pot > 3.0, "{name}");
    }
}

// ---------------------------------------------------------------------------------------------
// Where the time goes

/// A [`RiverGame`] that times its terminal evaluations, to see how much of an iteration they
/// take.
struct Timed<'a> {
    game: &'a RiverGame,
    terminal: Cell<Duration>,
}

impl PublicGame for Timed<'_> {
    fn num_hands(&self, player: usize) -> usize {
        self.game.num_hands(player)
    }

    fn deal(&self, player: usize) -> &[f64] {
        self.game.deal(player)
    }

    fn num_nodes(&self) -> usize {
        self.game.num_nodes()
    }

    fn root(&self) -> usize {
        self.game.root()
    }

    fn node(&self, node: usize) -> PublicNode {
        self.game.node(node)
    }

    fn child(&self, node: usize, index: usize) -> usize {
        self.game.child(node, index)
    }

    fn outcome_probability(&self, node: usize, outcome: usize) -> f64 {
        self.game.outcome_probability(node, outcome)
    }

    fn rules_out(&self, node: usize, outcome: usize, player: usize, hand: usize) -> bool {
        self.game.rules_out(node, outcome, player, hand)
    }

    fn terminal_values(&self, node: usize, player: usize, reach: &[f64], values: &mut [f64]) {
        let start = Instant::now();
        self.game.terminal_values(node, player, reach, values);
        self.terminal.set(self.terminal.get() + start.elapsed());
    }
}

/// What one spot measured.
struct Row {
    name: String,
    nodes: usize,
    hands: [usize; 2],
    /// The first iteration at which the exploitability was at or below 0.3%, 0.1% and 0.03%
    /// of the pot, measured after every iteration, if it was within the cap.
    reached: [Option<u64>; 3],
    /// The iteration from which it stayed at or below each threshold up to the cap.
    stays: [Option<u64>; 3],
    /// The exploitability after the last iteration of that run.
    last: f64,
    /// Milliseconds per iteration, and the share of it spent in terminal evaluation.
    iteration_ms: f64,
    terminal_share: f64,
    /// Milliseconds per exploitability measurement.
    measure_ms: f64,
    /// The default solve, to 0.3% measured every 10 iterations: iterations and seconds.
    default: (u64, f64),
    /// The same to 0.03%.
    fine: (u64, f64),
}

const THRESHOLDS: [f64; 3] = [0.3, 0.1, 0.03];

fn measure(spot: &Spot, cap: u64) -> Row {
    let game = RiverGame::new(spot).unwrap();
    let schedule = Schedule::Discounted(Dcfr::default());

    // The curve, measured after every iteration.
    let mut reached = [None; 3];
    let mut last_above = [0; 3];
    let options = SolveOptions {
        target: None,
        max_iterations: cap,
        check_every: 1,
        ..SolveOptions::default()
    };
    let curve = solve(spot, &options, |progress| {
        for k in 0..3 {
            if progress.exploitability.percent_of_pot <= THRESHOLDS[k] {
                reached[k].get_or_insert(progress.iterations);
            } else {
                last_above[k] = progress.iterations;
            }
        }
    })
    .unwrap();
    let stays = last_above.map(|t| (t < cap).then_some(t + 1));

    // Iterations alone, with the terminal evaluations timed.
    let timed = Timed {
        game: &game,
        terminal: Cell::new(Duration::ZERO),
    };
    let mut solver = VectorCfr::new(timed, schedule);
    let iterations = 400;
    let start = Instant::now();
    solver.run(iterations, |_| {});
    let total = start.elapsed().as_secs_f64();
    let terminal = solver.game().terminal.get().as_secs_f64();
    let average = solver.average_strategy();
    let start = Instant::now();
    let repeats = 50;
    for _ in 0..repeats {
        std::hint::black_box(exploitability(&game, &average));
    }
    let measure_ms = start.elapsed().as_secs_f64() * 1e3 / f64::from(repeats);

    let timed_solve = |target: f64| {
        let options = SolveOptions {
            target: Some(target),
            max_iterations: cap,
            ..SolveOptions::default()
        };
        let solution = solve(spot, &options, |_| {}).unwrap();
        (solution.iterations(), solution.elapsed().as_secs_f64())
    };
    Row {
        name: spot.name.clone(),
        nodes: game.num_nodes(),
        hands: [game.num_hands(0), game.num_hands(1)],
        reached,
        stays,
        last: curve.exploitability().percent_of_pot,
        iteration_ms: total * 1e3 / iterations as f64,
        terminal_share: terminal / total,
        measure_ms,
        default: timed_solve(0.3),
        fine: timed_solve(0.03),
    }
}

#[test]
#[ignore = "slow"]
fn river_spot_table() {
    // Every spot, measured after every iteration up to 5,000, under DCFR(1.5, 0, 2). Prints
    // the table with --nocapture. A threshold's cell is the first iteration at or below it,
    // then the iteration from which it stays there up to the cap: the exploitability of the
    // average strategy is not monotone. Every spot must stay below 0.3% of the pot well
    // within the cap, and every default solve must stop at its target.
    let cap = 5_000;
    let rows: Vec<Row> = all_spots().iter().map(|spot| measure(spot, cap)).collect();
    println!(
        "| Spot | Nodes | Hands (OOP, IP) | To 0.3% | To 0.1% | To 0.03% | After {cap} | \
         ms per iteration | In terminals | ms per measurement | Default solve (0.3%) | \
         Solve to 0.03% |"
    );
    println!("|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|");
    let shown = |row: &Row, k: usize| match (row.reached[k], row.stays[k]) {
        (Some(first), Some(stays)) => format!("{first} / {stays}"),
        (Some(first), None) => format!("{first} / > {cap}"),
        (None, _) => format!("> {cap}"),
    };
    for row in &rows {
        println!(
            "| `{}` | {} | {}, {} | {} | {} | {} | {:.1e}% | {:.3} | {:.0}% | {:.3} | {} in {:.3} s | {} in {:.3} s |",
            row.name,
            row.nodes,
            row.hands[0],
            row.hands[1],
            shown(row, 0),
            shown(row, 1),
            shown(row, 2),
            row.last,
            row.iteration_ms,
            100.0 * row.terminal_share,
            row.measure_ms,
            row.default.0,
            row.default.1,
            row.fine.0,
            row.fine.1,
        );
    }
    for row in &rows {
        assert!(
            row.stays[0].is_some_and(|t| t <= 1000),
            "{} did not settle below 0.3%",
            row.name
        );
        assert!(row.default.0 <= 1000 && row.fine.0 < cap, "{}", row.name);
    }
}
