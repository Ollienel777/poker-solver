//! Solving the river test spots under `reference/spots/river/`: the polarized toy game against
//! its analytical equilibrium, every spot to below the target exploitability, and the rake:
//! on every spot the two players' values sum to the root pot less the expected rake.
//!
//! The expected rake comes from [`expected_rake`], which works out the probability of ending at
//! each terminal and the rake taken there from the chips and the spot's rake settings, sharing
//! none of the code of the payoffs and the expected values it is compared with. Here it is
//! also checked against the sum over every pair of hands, and against the same solution's
//! values in the rake-free game.
//!
//! Every solution here is checked against the invariants of `pk_postflop::invariants`, in a
//! release build too, where the solve does not check them itself.

use std::cell::Cell;
use std::fs;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use pk_cfr::{Dcfr, PublicGame, PublicNode, Schedule, VectorCfr, vector_expected_values};
use pk_game::{Node, Player};
use pk_postflop::{
    RiverGame, Solution, SolveOptions, Spot, StopReason, check_solution, expected_rake,
    exploitability, solve,
};

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

/// Every river spot, sorted by name, solved by the default solve once for all the tests here.
fn solved() -> &'static [(Spot, Solution)] {
    static SOLVED: OnceLock<Vec<(Spot, Solution)>> = OnceLock::new();
    SOLVED.get_or_init(|| {
        all_spots()
            .into_iter()
            .map(|spot| {
                let solution = solve(&spot, &SolveOptions::default(), |_| {}).unwrap();
                assert_sound(&solution, &spot.name);
                (spot, solution)
            })
            .collect()
    })
}

/// Asserts that a solution keeps every invariant. The solve checks it too in a build with debug
/// assertions, but not in a release build, where the slow tests run.
fn assert_sound(solution: &Solution, what: &str) {
    let report = check_solution(solution);
    assert!(report.is_valid(), "{what}: {report}");
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
    assert_sound(&solution, "the toy game");
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
    // Measured, the nine spots take 30 to 350 iterations and 0.35 s in all in a release build, about 0.9 s in a test build (see
    // `river_spot_table`).
    for (spot, solution) in solved() {
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
        let uniform = solve(spot, &start, |_| {}).unwrap();
        assert_sound(&uniform, name);
        assert!(uniform.exploitability().percent_of_pot > 3.0, "{name}");
    }
}

// ---------------------------------------------------------------------------------------------
// Rake at terminal nodes

fn is_raked(spot: &Spot) -> bool {
    spot.rake_percent > 0.0
}

/// Neumaier's compensated sum.
#[derive(Debug, Default)]
struct Compensated {
    sum: f64,
    compensation: f64,
}

impl Compensated {
    fn add(&mut self, x: f64) {
        let sum = self.sum + x;
        self.compensation += if self.sum.abs() >= x.abs() {
            (self.sum - sum) + x
        } else {
            (x - sum) + self.sum
        };
        self.sum = sum;
    }

    fn total(&self) -> f64 {
        self.sum + self.compensation
    }
}

#[test]
fn the_values_sum_to_the_pot_less_the_expected_rake() {
    // Measured on the nine spots solved to 0.3% of the pot, with `--nocapture` printing the
    // table: `v_0 + v_1` is within 4.2e-5 chips of `pot − E[rake]`, at most 1.9e-8 of the pot.
    // That difference is the f32 rounding of the strategies, whose rows sum to 1 only to a few
    // f32 ulps, so that the terminal probabilities sum to `M`, within 1.9e-8 of 1, and it is
    // `pot · (M − 1)`. Against `pot · M − E[rake]` the difference is at most 6.9e-13 chips, or
    // 6.7e-17 of the pot plus the stack, the size of the largest payoff: the rounding of the
    // f64 sums alone. The bounds are about 5 and 150 times those.
    println!(
        "| Spot | Rake | Pot | v_0 + v_1 | E[rake] | v_0 + v_1 − (pot − E[rake]) | M − 1 | \
         v_0 + v_1 − (pot · M − E[rake]) |"
    );
    println!("|---|---|---:|---:|---:|---:|---:|---:|");
    for (spot, solution) in solved() {
        let name = &spot.name;
        let expected = expected_rake(solution.game(), solution.strategy());
        let [v0, v1] = solution.values();
        let pot = f64::from(spot.pot);
        let sum = v0 + v1;
        let difference = sum - (pot - expected.rake);
        let tight = sum - expected.value_sum();
        println!(
            "| `{name}` | {} | {pot} | {sum:.9} | {:.9} | {difference:+.2e} | {:+.2e} | \
             {tight:+.2e} |",
            if is_raked(spot) {
                "5%, cap 3bb"
            } else {
                "none"
            },
            expected.rake,
            expected.probability - 1.0,
        );

        // Every terminal of the tree is listed once, and the probabilities sum to 1.
        let terminals = solution
            .tree()
            .nodes()
            .iter()
            .filter(|node| matches!(node, Node::Terminal { .. }))
            .count();
        assert_eq!(expected.terminals.len(), terminals, "{name}");
        assert!(
            (expected.probability - 1.0).abs() < 1e-7,
            "{name}: the terminal probabilities sum to {}",
            expected.probability
        );
        assert!(
            expected
                .terminals
                .iter()
                .all(|t| (0.0..=1.0).contains(&t.probability)),
            "{name}"
        );

        // The rake: none on the rake-free spots; on the raked ones, some at every terminal, and
        // in all no more than the cap.
        if is_raked(spot) {
            assert!(expected.terminals.iter().all(|t| t.rake > 0.0), "{name}");
            assert!(expected.rake > 0.0, "{name}");
            assert!(
                expected.rake <= f64::from(spot.rake_cap) * expected.probability,
                "{name}"
            );
        } else {
            assert!(expected.terminals.iter().all(|t| t.rake == 0.0), "{name}");
            assert_eq!(expected.rake, 0.0, "{name}");
        }

        // The issue's check, to the f32 rounding of the strategies, and the same at f64
        // precision with the pot weighted by the probability of ending at all.
        assert!(
            difference.abs() < 1e-7 * pot,
            "{name}: {sum} against {pot} − {}",
            expected.rake
        );
        let scale = pot + f64::from(spot.effective_stack);
        assert!(
            tight.abs() < 1e-14 * scale,
            "{name}: {sum} against {}",
            expected.value_sum()
        );
    }
}

#[test]
fn terminal_probabilities_are_the_sum_over_every_pair_of_hands() {
    // The O(n²) double loop over the pairs of hands that share no card, with the reaches from
    // `RiverGame::reach`, against the expected rake's card counting and its own walk. Measured,
    // they agree within 4.5e-16 at every terminal of the nine solved spots.
    let mut largest = 0.0f64;
    for (spot, solution) in solved() {
        let name = &spot.name;
        let game = solution.game();
        let expected = expected_rake(game, solution.strategy());
        let (oop, ip) = (game.hands(Player::Oop), game.hands(Player::Ip));
        let compatible: Vec<Vec<bool>> = oop
            .combos()
            .iter()
            .map(|a| {
                ip.combos()
                    .iter()
                    .map(|b| !a.cards().intersects(b.cards()))
                    .collect()
            })
            .collect();
        // Tens of thousands of terms: summed with compensation, so that the reference is
        // accurate to a few ulps.
        let pair_sum = |a: &[f64], b: &[f64]| -> f64 {
            let mut sum = Compensated::default();
            for (h, row) in compatible.iter().enumerate() {
                for (o, &ok) in row.iter().enumerate() {
                    if ok {
                        sum.add(a[h] * b[o]);
                    }
                }
            }
            sum.total()
        };
        let total = pair_sum(oop.weights(), ip.weights());
        let mut checked = 0;
        for terminal in &expected.terminals {
            assert_eq!(game.node(terminal.node as usize), PublicNode::Terminal);
            let reach = solution.reach(terminal.node);
            let weighted = [(oop, &reach[0]), (ip, &reach[1])].map(|(list, reach)| -> Vec<f64> {
                list.weights()
                    .iter()
                    .zip(reach)
                    .map(|(w, r)| w * r)
                    .collect()
            });
            let probability = pair_sum(&weighted[0], &weighted[1]) / total;
            let difference = (terminal.probability - probability).abs();
            largest = largest.max(difference);
            assert!(
                difference < 5e-15,
                "{name}, node {}: {} against {probability}",
                terminal.node,
                terminal.probability
            );
            checked += 1;
        }
        assert!(checked >= 4, "{name}");
    }
    println!("largest difference: {largest:e}");
}

#[test]
fn raked_values_are_lower_than_rake_free_values_by_the_expected_rake() {
    // The solved strategy of each spot, played in the same spot without rake: the tree and the
    // hands are the same, and only the payoffs change. Each player's value can only go up, and
    // together they go up by the expected rake. Measured, the gain differs from the expected
    // rake by at most 1.2e-12 chips, and the rake-free values sum to `pot · M` within 1.9e-12:
    // about 2.6e-16 of the pot plus the stack, the size of the largest payoff.
    println!(
        "| Spot | Values with the rake | Values without | Gained | Gained − E[rake] | \
         Sum without − pot · M |"
    );
    println!("|---|---:|---:|---:|---:|---:|");
    let mut raked = 0;
    for (spot, solution) in solved() {
        let name = &spot.name;
        let mut free_spot = spot.clone();
        free_spot.rake_percent = 0.0;
        free_spot.rake_cap = 0;
        let free = RiverGame::new(&free_spot).unwrap();
        assert_eq!(free.tree(), solution.tree(), "{name}");
        for player in [Player::Oop, Player::Ip] {
            assert_eq!(free.hands(player), solution.hands(player), "{name}");
        }
        let without = vector_expected_values(&free, solution.strategy());
        let with = solution.values();
        let expected = expected_rake(solution.game(), solution.strategy());
        let gained = (without[0] + without[1]) - (with[0] + with[1]);
        let pot = f64::from(spot.pot);
        let scale = pot + f64::from(spot.effective_stack);
        let free_sum = without[0] + without[1];
        println!(
            "| `{name}` | {:.6}, {:.6} | {:.6}, {:.6} | {gained:.9} | {:+.2e} | {:+.2e} |",
            with[0],
            with[1],
            without[0],
            without[1],
            gained - expected.rake,
            free_sum - pot * expected.probability
        );
        assert!(
            (gained - expected.rake).abs() < 1e-14 * scale,
            "{name}: {gained} against {}",
            expected.rake
        );
        // Without rake the values sum to the pot, weighted by the probability of ending.
        assert!(
            (free_sum - pot * expected.probability).abs() < 1e-14 * scale,
            "{name}: {free_sum}"
        );
        if is_raked(spot) {
            raked += 1;
            for player in 0..2 {
                assert!(without[player] > with[player], "{name}, player {player}");
            }
        } else {
            assert_eq!(without, with, "{name}");
        }
    }
    assert_eq!(raked, 4);
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
    assert_sound(&curve, &spot.name);
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
        assert_sound(&solution, &spot.name);
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
