//! The cross-solver comparison, shown to tell right from wrong on a machine with no reference
//! solver installed:
//!
//! - every river spot's solution, written as a dump, read back and compared with itself, is
//!   identical;
//! - two honest solves that differ, by schedule or by iterations, are measured against each
//!   other on every spot (the slow test `honest_solvers_table`);
//! - each kind of wrong input is caught by the check meant for it: a tree with another bet
//!   size, the players' hands the wrong way round, poor play, one node's actions swapped, and
//!   EVs on another baseline;
//! - a hand-written dump of the toy game's analytical equilibrium, which plays every combo
//!   purely where our solve mixes, passes against our solve;
//! - at the one node the toy game's solution leaves free, a solution is held to the analytical
//!   bound and not to our frequencies, and a difference at any other node still fails;
//! - an external program, played here by this test binary itself, is run on a spot file and
//!   its dump read and compared, and each way it can fail is reported.
//!
//! With `PK_REFERENCE_SOLVER` set, the slow test `the_configured_reference_solver_matches_ours`
//! runs the reference on every spot, which is the M3 gate's comparison (DECISIONS D20); without
//! it, it does nothing.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use pk_cfr::{Dcfr, Schedule};
use pk_game::{Player, StreetSizing};
use pk_postflop::dump::PathText;
use pk_postflop::{
    Check, Comparison, DumpAction, DumpError, ExternalSolver, Failure, ImportError, MappedSolution,
    Mismatch, RunError, Solution, SolutionDump, SolveOptions, Spot, Tolerances, check_solution,
    compare, compare_mapped, solve,
};

fn repo_path(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
}

fn spot_path(name: &str) -> PathBuf {
    repo_path(&format!("reference/spots/river/{name}.toml"))
}

fn load(name: &str) -> Spot {
    Spot::load(spot_path(name)).unwrap()
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

/// A solve under `schedule` to `target` percent of the pot, measured after every iteration.
fn solve_to(spot: &Spot, schedule: Schedule, target: f64) -> Solution {
    let options = SolveOptions {
        schedule,
        target: Some(target),
        max_iterations: 100_000,
        check_every: 1,
        ..SolveOptions::default()
    };
    let solution = solve(spot, &options, |_| {}).unwrap();
    let report = check_solution(&solution);
    assert!(report.is_valid(), "{}: {report}", spot.name);
    solution
}

fn dcfr() -> Schedule {
    Schedule::Discounted(Dcfr::default())
}

/// A solve capped at `iterations`.
fn capped(spot: &Spot, iterations: u64) -> Solution {
    let options = SolveOptions {
        target: None,
        max_iterations: iterations,
        check_every: iterations.max(1),
        ..SolveOptions::default()
    };
    solve(spot, &options, |_| {}).unwrap()
}

/// The dump of a solution, written as JSON and read back.
fn through_json(dump: &SolutionDump) -> SolutionDump {
    SolutionDump::from_json(&dump.to_json()).unwrap()
}

fn checks(comparison: &Comparison) -> Vec<Check> {
    let mut checks: Vec<Check> = comparison.failures.iter().map(Failure::check).collect();
    checks.dedup();
    checks
}

/// The node of the comparison at `path`.
fn node_at(comparison: &Comparison, path: &[DumpAction]) -> u32 {
    comparison
        .nodes
        .iter()
        .find(|node| node.path == path)
        .unwrap_or_else(|| panic!("no node at {}", PathText(path)))
        .node
}

// ---------------------------------------------------------------------------------------------
// The round trip

#[test]
fn every_spot_round_trips_through_json_and_compares_identical_to_itself() {
    // JSON loses one thing: a probability, an f32, is written as the shortest decimal that
    // reads back as the same f32, so it reads back as an f64 a little off the f32's value,
    // and rounds to the same f32. Every other number reads back exactly. So the strategy
    // mapped back onto our tree is ours bit for bit, and every difference is zero.
    for spot in all_spots() {
        let name = &spot.name;
        let solution = solve(&spot, &SolveOptions::default(), |_| {}).unwrap();
        let dump = SolutionDump::from_solution(&solution);
        let back = through_json(&dump);
        assert_eq!(back.solver, dump.solver, "{name}");
        assert_eq!(back.spot, dump.spot, "{name}");
        assert_eq!(back.values, dump.values, "{name}");
        assert_eq!(back.nodes.len(), dump.nodes.len(), "{name}");
        let mut largest: f64 = 0.0;
        for (read, written) in back.nodes.iter().zip(&dump.nodes) {
            assert_eq!(
                (&read.path, read.player, &read.actions, &read.ev),
                (&written.path, written.player, &written.actions, &written.ev),
                "{name}"
            );
            for ((combo, a), (other, b)) in read.strategy.iter().zip(&written.strategy) {
                assert_eq!(combo, other);
                for (&x, &y) in a.iter().zip(b) {
                    assert_eq!((x as f32).to_bits(), (y as f32).to_bits(), "{name} {combo}");
                    largest = largest.max((x - y).abs());
                }
            }
        }
        // Half an f32 ulp of 1 at most.
        assert!(largest <= 3e-8, "{name}: {largest}");

        let mapped = MappedSolution::from_dump(&back, &spot).unwrap();
        assert_eq!(mapped.strategy(), solution.strategy(), "{name}");
        assert_eq!(
            mapped.exploitability(),
            Some(solution.exploitability()),
            "{name}"
        );
        assert_eq!(mapped.reported_evs(), Some(mapped.measured_evs()), "{name}");
        let ours = MappedSolution::from_solution(&solution);
        let comparison = compare_mapped(&ours, &mapped, &Tolerances::default());
        assert!(comparison.passes(), "{name}: {comparison}");
        assert_eq!(comparison.values()[0], comparison.values()[1], "{name}");
        for node in &comparison.nodes {
            assert_eq!(node.reach[0], node.reach[1], "{name}");
            assert_eq!(node.frequencies[0], node.frequencies[1], "{name}");
            assert_eq!(node.ev[0], node.ev[1], "{name}");
            assert_eq!(node.ev, node.measured_ev, "{name}");
        }
        for class in &comparison.classes {
            assert_eq!(class.frequencies[0], class.frequencies[1], "{name}");
            assert_eq!(class.ev[0], class.ev[1], "{name}");
        }
        assert_eq!(comparison.frequencies.weighted_mean, 0.0, "{name}");
        // And the comparison through the dumps alone gives the same.
        let again = compare(&spot, &dump, &back, &Tolerances::default()).unwrap();
        assert!(again.passes(), "{name}");
    }
}

// ---------------------------------------------------------------------------------------------
// Wrong solvers

#[test]
fn a_dump_of_a_tree_with_one_other_bet_size_fails_the_tree_check_first() {
    // The reference was set up with the larger bet at 110% of the pot instead of 100%: 22bb
    // where our tree bets 20bb, for OOP at the root and for IP after a check. Each place the
    // trees part is reported once, and nothing below the bet that only one side has.
    let spot = load("dry_ip_polarized");
    let mut other = spot.clone();
    other.tree.river = Some(StreetSizing {
        bets: vec![0.5, 1.1],
        ..spot.tree.river.clone().unwrap()
    });
    let ours = SolutionDump::from_solution(&solve_to(&spot, dcfr(), 0.1));
    let theirs = SolutionDump::from_solution(&solve_to(&other, dcfr(), 0.1));
    let error = compare(&spot, &ours, &theirs, &Tolerances::default()).unwrap_err();
    let expected_actions = |amount| {
        vec![
            DumpAction::Check,
            DumpAction::Bet(1000),
            DumpAction::Bet(amount),
        ]
    };
    assert_eq!(
        error.mismatches(),
        &[
            Mismatch::Actions {
                path: vec![],
                player: Player::Oop,
                ours: expected_actions(2000),
                theirs: expected_actions(2200),
            },
            Mismatch::Actions {
                path: vec![DumpAction::Check],
                player: Player::Ip,
                ours: expected_actions(2000),
                theirs: expected_actions(2200),
            },
        ],
        "{error}"
    );
    let text = error.to_string();
    assert!(
        text.contains(
            "at [] (OOP to act), our tree offers check, bet 1000, bet 2000 and the dump offers \
             check, bet 1000, bet 2200; only ours has bet 2000; only the dump has bet 2200"
        ),
        "{text}"
    );
    // The same dump against the spot it was made for is fine.
    assert!(MappedSolution::from_dump(&theirs, &other).is_ok());
}

#[test]
fn a_dump_with_the_players_the_wrong_way_round_is_caught_before_its_strategy_is_read() {
    // The wrapper gave each player the other's range: its dump is valid, of a spot whose OOP
    // holds our IP range.
    let spot = load("dry_ip_polarized");
    let mut swapped = spot.clone();
    std::mem::swap(&mut swapped.oop_range, &mut swapped.ip_range);
    let theirs = SolutionDump::from_solution(&solve_to(&swapped, dcfr(), 0.3));
    let error = MappedSolution::from_dump(&theirs, &spot).unwrap_err();
    assert_eq!(error.mismatches(), &[Mismatch::SwappedPlayers], "{error}");
    assert!(error.to_string().contains("wrong way round"), "{error}");
}

#[test]
fn a_strategy_for_the_other_players_hands_is_an_invalid_dump() {
    // At IP's node facing OOP's bet, the rows are keyed by OOP's hands.
    let spot = load("dry_ip_polarized");
    let ours = SolutionDump::from_solution(&solve_to(&spot, dcfr(), 0.3));
    let mut theirs = ours.clone();
    let index = theirs
        .nodes
        .iter()
        .position(|node| node.path == [DumpAction::Bet(1000)])
        .unwrap();
    let width = theirs.nodes[index].actions.len();
    // The ranges share some combos: the first reported is the first that IP does not hold.
    let ip: Vec<pk_core::Combo> = theirs.spot.hands[1].iter().map(|h| h.0).collect();
    let first_oop = theirs.spot.hands[0]
        .iter()
        .map(|h| h.0)
        .find(|combo| !ip.contains(combo))
        .unwrap();
    theirs.nodes[index].strategy = theirs.spot.hands[0]
        .iter()
        .map(|&(combo, _)| (combo, vec![1.0 / width as f64; width]))
        .collect();
    theirs.nodes[index].ev = Some(
        theirs.spot.hands[0]
            .iter()
            .map(|&(combo, _)| (combo, None))
            .collect(),
    );
    let text = theirs.to_json();
    let field = match SolutionDump::from_json(&text).unwrap_err() {
        DumpError::Invalid { field, reason } => {
            assert!(
                reason.contains("is not one of IP's hands in `spot.hands.ip`"),
                "{reason}"
            );
            assert!(reason.contains("(at [bet 1000])"), "{reason}");
            field
        }
        other => panic!("{other}"),
    };
    assert_eq!(field, format!("nodes[{index}].strategy.{first_oop}"));
    // A dump built in code is checked the same way when it is mapped.
    match compare(&spot, &ours, &theirs, &Tolerances::default()) {
        Err(ImportError::Dump(DumpError::Invalid { field: f, .. })) => assert_eq!(f, field),
        other => panic!("{other:?}"),
    }
}

#[test]
fn uniform_play_and_a_solve_stopped_after_two_iterations_fail_on_exploitability() {
    // Both are valid strategies on the right tree; only measuring them shows what they are.
    let spot = load("dry_ip_polarized");
    let ours = SolutionDump::from_solution(&solve_to(&spot, dcfr(), 0.03));
    for iterations in [0, 2] {
        let theirs = through_json(&SolutionDump::from_solution(&capped(&spot, iterations)));
        let comparison = compare(&spot, &ours, &theirs, &Tolerances::default()).unwrap();
        let failures: Vec<&Failure> = comparison.failures_of(Check::Exploitability).collect();
        assert_eq!(failures.len(), 1, "{iterations}: {comparison}");
        let Failure::Exploitability {
            side,
            percent_of_pot,
        } = failures[0]
        else {
            unreachable!();
        };
        assert_eq!(*side, 1, "{iterations}");
        assert!(*percent_of_pot > 1.0, "{iterations}: {percent_of_pot}");
        assert!(comparison.failures_of(Check::Frequency).count() > 0);
    }
}

#[test]
fn one_nodes_actions_swapped_fails_on_frequencies_there_and_on_exploitability() {
    // The wrapper listed OOP's fold and call, facing IP's 20bb bet after a check, the wrong
    // way round, so its probabilities go to the other action. The tree is the same, since
    // actions are matched by kind and amount in any order; the strategy is not.
    let spot = load("dry_ip_polarized");
    let ours = SolutionDump::from_solution(&solve_to(&spot, dcfr(), 0.03));
    let mut theirs = ours.clone();
    let path = [DumpAction::Check, DumpAction::Bet(2000)];
    let node = theirs
        .nodes
        .iter_mut()
        .find(|node| node.path == path)
        .unwrap();
    assert_eq!(
        node.actions,
        [
            DumpAction::Fold,
            DumpAction::Call(2000),
            DumpAction::Raise(6000)
        ]
    );
    node.actions.swap(0, 1);
    let theirs = through_json(&theirs);
    let comparison = compare(&spot, &ours, &theirs, &Tolerances::default()).unwrap();
    let at = node_at(&comparison, &path);
    let compared = comparison.nodes.iter().find(|n| n.node == at).unwrap();
    assert!(compared.reach[0] > 0.01, "{:?}", compared.reach);
    let frequency: Vec<&Failure> = comparison.failures_of(Check::Frequency).collect();
    assert!(!frequency.is_empty(), "{comparison}");
    // Only there: a range view weighs each hand by its own player's reach, and OOP does not
    // act again below the node.
    assert!(
        frequency.iter().all(|f| f.node() == Some(at)),
        "{comparison}"
    );
    let exploitable: Vec<&Failure> = comparison.failures_of(Check::Exploitability).collect();
    assert!(
        matches!(exploitable[..], [Failure::Exploitability { side: 1, .. }]),
        "{comparison}"
    );
}

/// The chips `player` has put in at the end of `path`, which starts with OOP's action.
fn put_in(path: &[DumpAction], player: Player) -> u32 {
    let mut chips = [0; 2];
    for (i, action) in path.iter().enumerate() {
        if let Some(amount) = action.amount() {
            chips[i % 2] = amount;
        }
    }
    chips[player.index()]
}

/// `dump` with every EV and value moved to another baseline.
fn rebased(dump: &SolutionDump, shift: impl Fn(&[DumpAction], Player, f64) -> f64) -> SolutionDump {
    let mut moved = dump.clone();
    for node in &mut moved.nodes {
        for (_, ev) in node.ev.as_mut().unwrap() {
            *ev = ev.map(|ev| shift(&node.path, node.player, ev));
        }
    }
    moved.values = dump
        .values
        .map(|values| [Player::Oop, Player::Ip].map(|p| shift(&[], p, values[p.index()])));
    moved
}

#[test]
fn evs_on_another_baseline_fail_on_evs_alone() {
    // The same strategy with its EVs written three wrong ways. Its frequencies, its values as
    // our code measures them, and its exploitability agree with ours; its reported EVs do
    // not agree with what its own strategy gives.
    let spot = load("dry_ip_polarized");
    let pot = f64::from(spot.pot);
    let ours = SolutionDump::from_solution(&solve_to(&spot, dcfr(), 0.03));
    let cases: [(&str, SolutionDump, bool); 3] = [
        (
            "zero-sum, crediting each player half the pot",
            rebased(&ours, |_, _, ev| ev - pot / 2.0),
            true,
        ),
        ("in big blinds", rebased(&ours, |_, _, ev| ev / 100.0), true),
        (
            "counted from the node, so that the chips put in are sunk",
            rebased(&ours, |path, player, ev| {
                ev + f64::from(put_in(path, player))
            }),
            false,
        ),
    ];
    for (what, theirs, shows_at_the_root) in cases {
        let comparison =
            compare(&spot, &ours, &through_json(&theirs), &Tolerances::default()).unwrap();
        let checks = checks(&comparison);
        assert!(
            checks
                .iter()
                .all(|c| matches!(c, Check::Ev | Check::ReportedEv)),
            "{what}: {comparison}"
        );
        assert!(checks.contains(&Check::ReportedEv), "{what}: {comparison}");
        // The class EVs at the root compare one strategy's EVs with the other's: a baseline
        // that moves the root shows there too, and one that only moves later nodes does not.
        assert_eq!(
            checks.contains(&Check::Ev),
            shows_at_the_root,
            "{what}: {comparison}"
        );
        for failure in &comparison.failures {
            match failure {
                Failure::ReportedNodeEv { side, .. }
                | Failure::ReportedClassEv { side, .. }
                | Failure::ReportedValue { side, .. } => assert_eq!(*side, 1, "{what}"),
                _ => {}
            }
        }
        if !shows_at_the_root {
            // Each node it is caught at is one where the acting player has chips in.
            for failure in comparison.failures_of(Check::ReportedEv) {
                let Failure::ReportedNodeEv { path, player, .. } = failure else {
                    panic!("{what}: {failure}");
                };
                assert!(put_in(path, *player) > 0, "{what}: {failure}");
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// The toy game

#[test]
fn the_toy_games_hand_written_equilibrium_passes_against_our_solve() {
    // `tests/data/toy_equilibrium.json` is the analytical equilibrium of `toy_polarized`,
    // written by hand from the spot's description with a pure strategy for every combo: OOP
    // bets its 6 nut combos and 2 chosen air combos, JhTh and JcTc; IP calls with 8 of its 12
    // combos, those with the Kh or the Kd, and after a check bets with 6, as our solve's 50%
    // there. Its EVs are worked out by hand too. Our solve mixes instead: every air combo
    // bluffs an eighth of the time, every KQ calls two thirds of the time.
    let spot = load("toy_polarized");
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/data/toy_equilibrium.json"
    );
    let theirs = SolutionDump::load(path).unwrap();
    let mapped = MappedSolution::from_dump(&theirs, &spot).unwrap();
    // It is an equilibrium: nobody gains by deviating, to rounding.
    assert!(
        mapped.exploitability().unwrap().chips.abs() < 1e-9,
        "{:?}",
        mapped.exploitability()
    );
    let ours = solve_to(&spot, dcfr(), 0.03);
    let comparison = compare_mapped(
        &MappedSolution::from_solution(&ours),
        &mapped,
        &Tolerances::default(),
    );
    assert!(comparison.passes(), "{comparison}");
    // Its EVs, by hand, are those its strategy gives.
    for node in &comparison.nodes {
        if let (Some(reported), Some(measured)) = (node.ev[1], node.measured_ev[1]) {
            assert!(
                (reported - measured).abs() < 1e-9,
                "{}",
                PathText(&node.path)
            );
        }
    }
    // And the two play different strategies combo by combo.
    let combo = |s: &str| s.parse::<pk_core::Combo>().unwrap();
    let hand = |player: Player, s: &str| ours.hands(player).position(combo(s)).unwrap();
    let root = ours.hand_strategy(0, hand(Player::Oop, "JsTs")).unwrap();
    assert!((root[1] - 0.125).abs() < 0.01, "{root:?}");
    let written = theirs.node(&[]).unwrap();
    assert_eq!(
        written
            .strategy
            .iter()
            .find(|r| r.0 == combo("JsTs"))
            .unwrap()
            .1,
        [1.0, 0.0]
    );
    let after_bet = node_at(&comparison, &[DumpAction::Bet(500)]);
    let call = ours
        .hand_strategy(after_bet, hand(Player::Ip, "KcQs"))
        .unwrap();
    assert!((call[1] - 2.0 / 3.0).abs() < 0.01, "{call:?}");
    let written = theirs.node(&[DumpAction::Bet(500)]).unwrap();
    assert_eq!(
        written
            .strategy
            .iter()
            .find(|r| r.0 == combo("KcQs"))
            .unwrap()
            .1,
        [1.0, 0.0]
    );
}

/// The path of the toy game's free node: IP's, after OOP checks.
///
/// IP's bluff-catchers win the pot there whether they check or bet, because OOP's nuts never
/// check, and any betting frequency up to 2/3 is an equilibrium (`reference/README.md`). Two
/// correct solvers need not agree on it, so the M3 gate (DECISIONS D20) holds each solver to
/// that bound there in place of the frequency check.
const TOY_FREE_NODE: [DumpAction; 1] = [DumpAction::Check];

/// The most IP may bet at the toy game's free node at an equilibrium.
const TOY_FREE_BET_LIMIT: f64 = 2.0 / 3.0;

/// What a comparison of two solutions of `toy_polarized` fails once its free node is held to
/// its bound: every failure but those of the frequency check at that node, and each solution
/// that bets there more than 2/3 of the time, give or take the frequency tolerance.
fn toy_game_failures(comparison: &Comparison) -> Vec<String> {
    let free = comparison
        .nodes
        .iter()
        .find(|node| node.path == TOY_FREE_NODE)
        .expect("the toy game has a node after a check");
    let mut failures: Vec<String> = comparison
        .failures
        .iter()
        .filter(|f| !(f.check() == Check::Frequency && f.node() == Some(free.node)))
        .map(Failure::to_string)
        .collect();
    let limit = TOY_FREE_BET_LIMIT + comparison.tolerances.frequency;
    for (side, frequencies) in free.frequencies.iter().enumerate() {
        let Some(frequencies) = frequencies else {
            continue;
        };
        let bet: f64 = free
            .actions
            .iter()
            .zip(frequencies)
            .filter(|(action, _)| matches!(action, DumpAction::Bet(_)))
            .map(|(_, &frequency)| frequency)
            .sum();
        if bet > limit {
            failures.push(format!(
                "frequency: at {} {} bets {:.2}% of the time, more than the 2/3 an equilibrium \
                 allows",
                PathText(&free.path),
                ["A", "B"][side],
                100.0 * bet
            ));
        }
    }
    failures
}

/// The hand-written equilibrium of the toy game without its EVs and values, which hold only
/// for the strategy as written.
fn toy_equilibrium() -> SolutionDump {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/data/toy_equilibrium.json"
    );
    let mut dump = SolutionDump::load(path).unwrap();
    dump.values = None;
    for node in &mut dump.nodes {
        node.ev = None;
    }
    dump
}

/// Makes every hand at the dump's node at `path` play one action: the one `pick` gives for the
/// hand's place in the node's list and its cards.
fn play_purely(
    dump: &mut SolutionDump,
    path: &[DumpAction],
    pick: impl Fn(usize, &str) -> DumpAction,
) {
    let node = dump
        .nodes
        .iter_mut()
        .find(|node| node.path == path)
        .unwrap_or_else(|| panic!("no node at {}", PathText(path)));
    for (place, (combo, row)) in node.strategy.iter_mut().enumerate() {
        let action = pick(place, &combo.to_string());
        let chosen = node.actions.iter().position(|&a| a == action).unwrap();
        for (index, probability) in row.iter_mut().enumerate() {
            *probability = if index == chosen { 1.0 } else { 0.0 };
        }
    }
}

#[test]
fn the_toy_games_free_node_is_held_to_its_bound_and_nothing_else_is_excused() {
    let spot = load("toy_polarized");
    let ours = SolutionDump::from_solution(&solve_to(&spot, dcfr(), 0.03));
    let against =
        |theirs: &SolutionDump| compare(&spot, &ours, theirs, &Tolerances::default()).unwrap();
    // IP bets after a check with the first `bets` of its 12 combos.
    let betting = |bets: usize| {
        let mut dump = toy_equilibrium();
        play_purely(&mut dump, &TOY_FREE_NODE, |place, _| {
            if place < bets {
                DumpAction::Bet(500)
            } else {
                DumpAction::Check
            }
        });
        dump
    };

    // Never betting there is an equilibrium, as is betting 2/3 of the time. Each is far from
    // our solve's 50%, which is all the comparison finds, and the gate excuses it.
    for bets in [0, 8] {
        let comparison = against(&betting(bets));
        assert_eq!(checks(&comparison), [Check::Frequency], "{comparison}");
        let at_free_node = node_at(&comparison, &TOY_FREE_NODE);
        assert!(
            comparison
                .failures
                .iter()
                .all(|f| f.node() == Some(at_free_node)),
            "{comparison}"
        );
        let failures = toy_game_failures(&comparison);
        assert!(failures.is_empty(), "{bets}: {failures:?}");
    }

    // Betting with 9 of the 12 is past the bound: OOP's nuts would now rather check and call.
    let failures = toy_game_failures(&against(&betting(9)));
    assert!(
        failures.iter().any(|f| f.contains("B bets 75.00%")),
        "{failures:?}"
    );

    // A difference at any other node is a failure as before: here OOP never bluffs.
    let mut no_bluffs = betting(0);
    play_purely(&mut no_bluffs, &[], |_, combo| {
        let cards = combo.as_bytes();
        if cards[0] == cards[2] {
            DumpAction::Bet(500)
        } else {
            DumpAction::Check
        }
    });
    let failures = toy_game_failures(&against(&no_bluffs));
    assert!(
        failures.iter().any(|f| f.starts_with("frequency: at []")),
        "{failures:?}"
    );
}

// ---------------------------------------------------------------------------------------------
// Two honest solvers

/// What a comparison of two honest solves measures, for the table.
struct Measured {
    iterations: [u64; 2],
    exploitability: [f64; 2],
    largest: f64,
    largest_at: String,
    mean: f64,
    value: f64,
    class_ev: f64,
    node_ev: f64,
    comparison: Comparison,
}

fn measure(a: &Solution, b: &Solution) -> Measured {
    let comparison = compare_mapped(
        &MappedSolution::from_solution(a),
        &MappedSolution::from_solution(b),
        &Tolerances::default(),
    );
    let t = comparison.tolerances;
    let (largest, largest_at) = match comparison.frequencies.largest {
        Some(largest) => {
            let node = comparison
                .nodes
                .iter()
                .find(|n| n.node == largest.node)
                .unwrap();
            (
                100.0 * largest.difference,
                format!(
                    "{} at {}",
                    node.actions[largest.action],
                    PathText(&node.path)
                ),
            )
        }
        None => (0.0, String::new()),
    };
    let values = comparison.values().map(Option::unwrap);
    let value = (0..2)
        .map(|p| comparison.percent((values[0][p] - values[1][p]).abs()))
        .fold(0.0, f64::max);
    let class_ev = comparison
        .classes
        .iter()
        .filter(|c| c.share > t.min_reach)
        .filter_map(|c| match c.ev {
            [Some(x), Some(y)] => Some(comparison.percent((x - y).abs())),
            _ => None,
        })
        .fold(0.0, f64::max);
    let node_ev = comparison
        .nodes
        .iter()
        .filter(|n| n.is_checked(t.min_reach))
        .filter_map(|n| match n.ev {
            [Some(x), Some(y)] => Some(comparison.percent((x - y).abs())),
            _ => None,
        })
        .fold(0.0, f64::max);
    Measured {
        iterations: [a.iterations(), b.iterations()],
        exploitability: [
            a.exploitability().percent_of_pot,
            b.exploitability().percent_of_pot,
        ],
        largest,
        largest_at,
        mean: 100.0 * comparison.frequencies.weighted_mean,
        value,
        class_ev,
        node_ev,
        comparison,
    }
}

#[test]
#[ignore = "slow"]
fn honest_solvers_table() {
    // Three pairs of honest solves of every spot, compared under the default tolerances,
    // those proposed on #34. Prints the table with --nocapture. Frequencies in points, EVs and
    // values in percent of the pot; the class EVs are those of classes holding more than 1% of
    // the range, and the node EVs over the range at the nodes checked, which the verdict
    // reports but does not check.
    let pairs: [(&str, Schedule, f64, Schedule, f64); 3] = [
        (
            "DCFR to 0.1% against CFR+ to 0.1%",
            dcfr(),
            0.1,
            Schedule::CfrPlus,
            0.1,
        ),
        (
            "DCFR to 0.1% against DCFR to 0.02%",
            dcfr(),
            0.1,
            dcfr(),
            0.02,
        ),
        (
            "DCFR to 0.03% against CFR+ to 0.03%",
            dcfr(),
            0.03,
            Schedule::CfrPlus,
            0.03,
        ),
    ];
    let spots = all_spots();
    let mut results = Vec::new();
    for (title, first, first_target, second, second_target) in pairs {
        println!("\n{title}\n");
        println!(
            "| Spot | Iterations | Exploitability (% pot) | Largest frequency difference | \
             Reach-weighted mean | Values apart | Class EVs apart | Node EVs apart | #34 \
             tolerance |"
        );
        println!("|---|---:|---:|---|---:|---:|---:|---:|---|");
        let mut rows = Vec::new();
        for spot in &spots {
            let a = solve_to(spot, first, first_target);
            let b = solve_to(spot, second, second_target);
            let m = measure(&a, &b);
            let verdict = if m.comparison.passes() {
                "passes".to_string()
            } else {
                let fails: Vec<String> =
                    checks(&m.comparison).iter().map(Check::to_string).collect();
                format!(
                    "fails ({} on {} items)",
                    fails.join(", "),
                    m.comparison.failures.len()
                )
            };
            println!(
                "| `{}` | {} / {} | {:.3} / {:.3} | {:.2}, {} | {:.2} | {:.3} | {:.2} | {:.2} | {} |",
                spot.name,
                m.iterations[0],
                m.iterations[1],
                m.exploitability[0],
                m.exploitability[1],
                m.largest,
                m.largest_at,
                m.mean,
                m.value,
                m.class_ev,
                m.node_ev,
                verdict
            );
            rows.push((spot.name.clone(), m));
        }
        results.push((title, rows));
    }
    // What holds for every pair: the exploitability, values and class EVs pass, and no
    // solution's EVs disagree with its strategy. The frequencies at 0.1% do not always pass,
    // which is the finding; at 0.03% they do on every spot.
    for (title, rows) in &results {
        for (name, m) in rows {
            let checks = checks(&m.comparison);
            for check in [
                Check::Exploitability,
                Check::Value,
                Check::Ev,
                Check::ReportedEv,
            ] {
                assert!(
                    !checks.contains(&check),
                    "{title}, {name}: {}",
                    m.comparison
                );
            }
        }
    }
    for (name, m) in &results[2].1 {
        assert!(m.comparison.passes(), "{name}: {}", m.comparison);
    }
}

// ---------------------------------------------------------------------------------------------
// An external program

/// The marker after which a stand-in run's arguments are its mode, the spot and the output.
const STAND_IN: &str = "pk-stand-in";

/// The stand-in reference solver: this test binary, run again by the tests below as an
/// external program, with this test as its filter. In a normal test run its marker is not
/// among the arguments and it does nothing. Its modes:
///
/// - `solve`: solves the spot with CFR+ to 0.03% of the pot and writes the dump;
/// - `other-tree`: the same for the spot with its first bet at 60% of the pot;
/// - `fail`: writes a message to standard error and exits with code 3;
/// - `nothing`: exits successfully without writing;
/// - `garbage`: writes a file that is not JSON;
/// - `hang`: sleeps for a minute.
#[test]
fn stand_in_reference_solver() {
    let args: Vec<String> = std::env::args().collect();
    let Some(at) = args.iter().position(|arg| arg == STAND_IN) else {
        return;
    };
    let [mode, spot, out] = [1, 2, 3].map(|i| args[at + i].as_str());
    let solved = |spot: &Spot| {
        let mut dump = SolutionDump::from_solution(&solve_to(spot, Schedule::CfrPlus, 0.03));
        dump.solver.name = "stand-in (CFR+)".to_string();
        dump.spot.name = Some(spot.name.clone());
        fs::write(out, dump.to_json()).unwrap();
    };
    match mode {
        "solve" => solved(&Spot::load(spot).unwrap()),
        "other-tree" => {
            let mut other = Spot::load(spot).unwrap();
            other.tree.river.as_mut().unwrap().bets[0] = 0.6;
            solved(&other);
        }
        "fail" => {
            eprintln!("stand-in: cannot solve {spot}");
            std::process::exit(3);
        }
        "nothing" => {}
        "garbage" => fs::write(out, "this is not a dump").unwrap(),
        "hang" => std::thread::sleep(Duration::from_secs(60)),
        other => panic!("unknown stand-in mode {other}"),
    }
}

/// The stand-in in one of its modes, as a command template.
fn stand_in(mode: &str) -> ExternalSolver {
    let exe = std::env::current_exe().unwrap();
    let command: Vec<String> = [
        exe.to_str().unwrap(),
        "stand_in_reference_solver",
        "--exact",
        "--nocapture",
        "--test-threads=1",
        STAND_IN,
        mode,
        "{spot}",
        "{out}",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    ExternalSolver::new(&command).unwrap()
}

fn out_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("compare-{name}.json"))
}

#[test]
fn an_external_programs_dump_is_read_and_compared() {
    let spot = load("toy_polarized");
    let ours = MappedSolution::from_solution(&solve_to(&spot, dcfr(), 0.03));
    let out = out_path("solve");
    let dump = stand_in("solve")
        .run(&spot_path("toy_polarized"), &out)
        .unwrap();
    assert_eq!(dump.solver.name, "stand-in (CFR+)");
    let theirs = MappedSolution::from_dump(&dump, &spot).unwrap();
    let comparison = compare_mapped(&ours, &theirs, &Tolerances::default());
    assert!(comparison.passes(), "{comparison}");
    assert!(comparison.to_string().contains("stand-in (CFR+)"));
    // A program that solves another tree runs fine, and the comparison stops at the tree.
    let other = stand_in("other-tree")
        .run(&spot_path("toy_polarized"), &out_path("other-tree"))
        .unwrap();
    let error = MappedSolution::from_dump(&other, &spot).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("only ours has bet 500; only the dump has bet 600"),
        "{error}"
    );
}

#[test]
fn each_way_an_external_program_can_fail_is_reported() {
    let spot = spot_path("toy_polarized");
    let out = out_path("fail");
    match stand_in("fail").run(&spot, &out) {
        Err(RunError::Failed { status, stderr, .. }) => {
            assert_eq!(status.code(), Some(3));
            assert!(stderr.contains("stand-in: cannot solve"), "{stderr}");
        }
        other => panic!("{other:?}"),
    }
    // A dump left from an earlier run is removed first, never read.
    let out = out_path("nothing");
    fs::write(
        &out,
        SolutionDump::from_solution(&capped(&load("toy_polarized"), 1)).to_json(),
    )
    .unwrap();
    let error = stand_in("nothing").run(&spot, &out).unwrap_err();
    assert!(matches!(error, RunError::NoOutput { .. }), "{error}");
    assert!(!out.exists());
    let error = stand_in("garbage")
        .run(&spot, &out_path("garbage"))
        .unwrap_err();
    assert!(
        matches!(
            error,
            RunError::Dump {
                source: DumpError::Json(_),
                ..
            }
        ),
        "{error}"
    );
    let mut hang = stand_in("hang");
    hang.timeout = Duration::from_millis(500);
    let start = Instant::now();
    let error = hang.run(&spot, &out_path("hang")).unwrap_err();
    assert!(matches!(error, RunError::Timeout { .. }), "{error}");
    assert!(
        start.elapsed() < Duration::from_secs(20),
        "{:?}",
        start.elapsed()
    );
}

#[test]
#[ignore = "slow"]
fn the_configured_reference_solver_matches_ours() {
    // The M3 gate's comparison (DECISIONS D20), when `PK_REFERENCE_SOLVER` names a reference:
    // each spot solved by it and by us to 0.03% of the pot, compared under the default
    // tolerances, with the toy game's free node held to its bound. Without a reference it
    // does nothing.
    let Some(reference) = ExternalSolver::from_env().unwrap() else {
        println!("no reference solver: `PK_REFERENCE_SOLVER` is not set, so nothing to compare");
        return;
    };
    let mut failed = Vec::new();
    for spot in all_spots() {
        let out = out_path(&format!("reference-{}", spot.name));
        let dump = reference.run(&spot_path(&spot.name), &out).unwrap();
        let ours = SolutionDump::from_solution(&solve_to(&spot, dcfr(), 0.03));
        match compare(&spot, &ours, &dump, &Tolerances::default()) {
            Ok(comparison) => {
                println!("{}:\n{comparison}", spot.name);
                let passes = if spot.name == "toy_polarized" {
                    let failures = toy_game_failures(&comparison);
                    println!("With the free node held to its bound: {failures:?}");
                    failures.is_empty()
                } else {
                    comparison.passes()
                };
                if !passes {
                    failed.push(spot.name.clone());
                }
            }
            Err(error) => {
                println!("{}: {error}", spot.name);
                failed.push(spot.name.clone());
            }
        }
    }
    assert!(failed.is_empty(), "the reference differs on {failed:?}");
}
