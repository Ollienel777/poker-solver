//! The cross-solver comparison of spots that start on the turn or the flop, shown to tell
//! right from wrong on a machine with no reference solver installed, as `compare.rs` shows it
//! for the river:
//!
//! - a turn and a flop solution of ours, written as version 2 dumps of every street and of the
//!   first one or two, read back and compared with ours, are identical where they hold the
//!   strategy, and a partial one's exploitability is reported as not measured;
//! - each kind of wrong version 2 input is caught by the check meant for it: a deal in the
//!   wrong place, a missing card, a row for a hand that holds a dealt card, amounts that are
//!   totals over all streets instead of street totals, a partial dump with a street half
//!   listed, EVs counted from the node, and a strategy that is right on the first street and
//!   wrong below a card;
//! - the subtree after a card that cannot fall is in our tree and in no dump, and that is not a
//!   mismatch; an EV written `null` is no EV, never zero.
//!
//! The spots are those of `tests/data`. `flop_tiny`, whose only bet is all-in, has a public
//! tree small enough for the fast suite. Two slow tests measure: `honest_solvers_table_by_street`
//! compares two honest solves of a turn and a flop spot, street by street, which is what
//! [`Tolerances::frequency_below_deals`] is set from; and, with `PK_REFERENCE_SOLVER` set,
//! `the_configured_reference_solver_matches_ours_on_turn_and_flop_spots` runs the reference on
//! every spot of `reference/spots/turn` and `reference/spots/flop`, and
//! `the_configured_reference_solver_on_the_test_spots` on the spots here.
//!
//! The last tests compare a spot with itself, its suits relabelled, as the gate compares ours
//! with the reference: on two small spots in the fast suite, and in the slow test
//! `every_gate_spot_relabelled_passes_the_comparison_with_the_spot` on the spots of
//! `reference/spots/turn` and `reference/spots/flop`, which needs no reference solver.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Instant;

use pk_cfr::{Dcfr, PublicGame, Schedule};
use pk_core::{Card, Combo, SuitPermutation};
use pk_game::{Player, Street};
use pk_postflop::dump::PathText;
use pk_postflop::{
    Check, Comparison, DumpAction, DumpError, ExternalSolver, Failure, ImportError, MappedSolution,
    Mismatch, Solution, SolutionDump, SolveOptions, Spot, Tolerances, Unchecked, check_solution,
    compare, compare_mapped, solve,
};

fn data_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data")
        .join(format!("{name}.toml"))
}

fn load(name: &str) -> Spot {
    Spot::load(data_path(name)).unwrap()
}

fn card(text: &str) -> Card {
    text.parse().unwrap()
}

fn deal(text: &str) -> DumpAction {
    DumpAction::Deal(card(text))
}

const X: DumpAction = DumpAction::Check;

/// The number of streets a spot plays.
fn streets_of(spot: &Spot) -> usize {
    6 - spot.board.len()
}

/// A solve capped at `iterations`, checked against its invariants.
fn capped(spot: &Spot, iterations: u64) -> Solution {
    let options = SolveOptions {
        target: None,
        max_iterations: iterations,
        check_every: iterations.max(1),
        ..SolveOptions::default()
    };
    let solution = solve(spot, &options, |_| {}).unwrap();
    let report = check_solution(&solution);
    assert!(report.is_valid(), "{}: {report}", spot.name);
    solution
}

/// A solve under `schedule` to `target` percent of the pot, measured every `check_every`
/// iterations, checked against its invariants.
fn solve_to(spot: &Spot, schedule: Schedule, target: f64, check_every: u64) -> Solution {
    let options = SolveOptions {
        schedule,
        target: Some(target),
        max_iterations: 100_000,
        check_every,
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

/// The turn spot solved to 0.3% of the pot, once for every test that needs it.
fn turn_solution() -> &'static Solution {
    static SOLUTION: OnceLock<Solution> = OnceLock::new();
    SOLUTION.get_or_init(|| solve_to(&load("turn_small"), dcfr(), 0.3, 10))
}

/// The tiny flop spot solved to 0.3% of the pot, once for every test that needs it.
fn flop_solution() -> &'static Solution {
    static SOLUTION: OnceLock<Solution> = OnceLock::new();
    SOLUTION.get_or_init(|| solve_to(&load("flop_tiny"), dcfr(), 0.3, 10))
}

/// The dump written as JSON and read back.
fn through_json(dump: &SolutionDump) -> SolutionDump {
    SolutionDump::from_json(&dump.to_json()).unwrap()
}

/// The field and reason of the error reading the dump's text, which there must be.
fn text_error(dump: &SolutionDump) -> (String, String) {
    match SolutionDump::from_json(&dump.to_json()) {
        Err(DumpError::Invalid { field, reason }) => (field, reason),
        Err(other) => panic!("{other}"),
        Ok(_) => panic!("the dump reads"),
    }
}

fn mismatches(dump: &SolutionDump, spot: &Spot) -> Vec<Mismatch> {
    match MappedSolution::from_dump(dump, spot) {
        Err(ImportError::Mismatch(list)) => list,
        Err(other) => panic!("{other}"),
        Ok(_) => panic!("the dump maps"),
    }
}

fn index_of(dump: &SolutionDump, path: &[DumpAction]) -> usize {
    dump.nodes
        .iter()
        .position(|node| node.path == path)
        .unwrap_or_else(|| panic!("no node at {}", PathText(path)))
}

fn checks(comparison: &Comparison) -> Vec<Check> {
    let mut checks: Vec<Check> = comparison.failures.iter().map(Failure::check).collect();
    checks.sort_by_key(|c| *c as u8);
    checks.dedup();
    checks
}

/// What each player has put in at the end of `path`: on the streets before the last, and in
/// all.
fn put_in(path: &[DumpAction]) -> ([u32; 2], [u32; 2]) {
    let (mut before, mut now, mut actor) = ([0u32; 2], [0u32; 2], 0);
    for step in path {
        if step.is_deal() {
            before = [before[0] + now[0], before[1] + now[1]];
            now = [0, 0];
            actor = 0;
        } else {
            if let Some(amount) = step.amount() {
                now[actor] = amount;
            }
            actor = 1 - actor;
        }
    }
    (before, [before[0] + now[0], before[1] + now[1]])
}

// ---------------------------------------------------------------------------------------------
// The round trip

#[test]
fn every_turn_and_flop_dump_round_trips_through_json_and_compares_identical_to_itself() {
    // JSON loses one thing, as on the river: a probability, an f32, reads back as the f64
    // nearest its shortest decimal, which rounds to the same f32. So at every node a dump
    // holds, the strategy mapped back onto our tree is ours bit for bit, and every difference
    // is zero, on every street and in a partial dump as in a complete one.
    let cases = [
        ("turn_small", turn_solution()),
        ("flop_tiny", flop_solution()),
    ];
    for (name, solution) in cases {
        let spot = load(name);
        let game = solution.game();
        let total = streets_of(&spot);
        let ours = MappedSolution::from_solution(solution);
        assert!(ours.is_complete() && ours.streets() == total);
        for streets in 1..=total {
            let what = format!("{name}, {streets} of {total} streets");
            let complete = streets == total;
            let dump = SolutionDump::from_solution_streets(solution, streets);
            assert_eq!(dump.version(), 2, "{what}");
            assert_eq!(dump.complete, complete, "{what}");
            assert_eq!(dump.streets(), streets, "{what}");
            let text = dump.to_json();
            assert!(
                text.starts_with("{\n  \"format\": \"pk-solution-dump\",\n  \"version\": 2,\n"),
                "{what}"
            );
            assert!(
                text.contains(&format!("\n  \"complete\": {complete},\n  \"values\"")),
                "{what}"
            );
            let back = SolutionDump::from_json(&text).unwrap();
            assert_eq!(back.to_json(), text, "{what}");
            assert_eq!(
                (&back.solver, &back.spot, back.complete, back.values),
                (&dump.solver, &dump.spot, dump.complete, dump.values),
                "{what}"
            );
            assert_eq!(back.nodes.len(), dump.nodes.len(), "{what}");
            // An EV, an f64, that happens to be an f32 value is written as that f32's shortest
            // decimal, as a probability is, and reads back as the nearest f64: the same f32.
            let narrow = |ev: &Option<Vec<(Combo, Option<f64>)>>| -> Vec<Option<u32>> {
                let evs = ev.as_ref().unwrap();
                evs.iter()
                    .map(|(_, ev)| ev.map(|ev| (ev as f32).to_bits()))
                    .collect()
            };
            for (read, written) in back.nodes.iter().zip(&dump.nodes) {
                assert_eq!(
                    (&read.path, read.player, &read.actions),
                    (&written.path, written.player, &written.actions),
                    "{what}"
                );
                assert_eq!(narrow(&read.ev), narrow(&written.ev), "{what}");
                for ((combo, a), (other, b)) in
                    read.ev.iter().flatten().zip(written.ev.iter().flatten())
                {
                    assert_eq!(combo, other);
                    if let (Some(a), Some(b)) = (a, b) {
                        assert!(a == b || f64::from(*b as f32) == *b, "{what}: {a} {b}");
                    }
                }
                for ((combo, a), (other, b)) in read.strategy.iter().zip(&written.strategy) {
                    assert_eq!(combo, other);
                    for (&x, &y) in a.iter().zip(b) {
                        assert_eq!((x as f32).to_bits(), (y as f32).to_bits(), "{what}");
                    }
                }
            }

            let mapped = MappedSolution::from_dump(&back, &spot).unwrap();
            assert_eq!(
                (mapped.streets(), mapped.is_complete()),
                (streets, complete)
            );
            // It holds exactly the decision nodes of its streets, where its strategy is ours.
            let mut held = 0;
            for node in 0..game.num_nodes() as u32 {
                if game.actor(node).is_none() {
                    assert!(!mapped.holds(node));
                    continue;
                }
                let deals = game
                    .path(node)
                    .iter()
                    .filter(|&&(at, _)| !game.outcomes(at).is_empty())
                    .count();
                assert_eq!(mapped.holds(node), deals < streets, "{what}: node {node}");
                if deals < streets {
                    held += 1;
                    assert_eq!(
                        mapped.strategy().get(node as usize),
                        solution.strategy().get(node as usize),
                        "{what}: node {node}"
                    );
                }
            }
            assert_eq!(held, dump.nodes.len(), "{what}");
            if complete {
                assert_eq!(mapped.strategy(), solution.strategy(), "{what}");
                assert_eq!(
                    mapped.exploitability(),
                    Some(solution.exploitability()),
                    "{what}"
                );
                // Its EVs are the EVs its strategy gives here, to the rounding of an EV that
                // happens to be an f32.
                let reported = mapped.reported_evs().unwrap();
                for node in 0..game.num_nodes() as u32 {
                    let (Some(r), Some(m)) = (reported.get(node), mapped.measured_evs().get(node))
                    else {
                        assert!(!mapped.holds(node), "{what}: node {node}");
                        continue;
                    };
                    for (r, m) in r.iter().zip(m) {
                        assert_eq!(r.is_some(), m.is_some(), "{what}: node {node}");
                        if let (Some(r), Some(m)) = (r, m) {
                            assert!((r - m).abs() <= 1e-7 * m.abs().max(1.0), "{what}: {r} {m}");
                        }
                    }
                }
            } else {
                assert_eq!(mapped.exploitability(), None, "{what}");
            }

            let comparison = compare_mapped(&ours, &mapped, &Tolerances::default());
            assert!(comparison.passes(), "{what}: {comparison}");
            assert_eq!(comparison.nodes.len(), dump.nodes.len(), "{what}");
            assert_eq!(comparison.streets.len(), streets, "{what}");
            for node in &comparison.nodes {
                assert_eq!(node.reach[0], node.reach[1], "{what}");
                assert_eq!(node.reach_given_cards[0], node.reach_given_cards[1]);
                assert_eq!(node.frequencies[0], node.frequencies[1], "{what}");
                match node.ev {
                    [Some(a), Some(b)] => assert!((a - b).abs() <= 1e-7 * a.abs().max(1.0)),
                    [a, b] => assert_eq!(a, b, "{what}"),
                }
                assert_eq!(node.deals, node.path.iter().filter(|s| s.is_deal()).count());
            }
            for street in &comparison.streets {
                assert_eq!(street.frequencies.weighted_mean, 0.0, "{what}");
                let largest = street.frequencies.largest.map(|l| l.difference);
                assert!(largest.is_none_or(|l| l == 0.0), "{what}");
            }
            assert!(comparison.streets[0].frequencies.checked > 0, "{what}");
            // Its values are as its solver reported them when it is partial: ours, exactly.
            assert_eq!(comparison.values()[0], comparison.values()[1], "{what}");
            if complete {
                assert!(comparison.unchecked.is_empty(), "{what}");
            } else {
                assert_eq!(
                    comparison.unchecked,
                    [
                        Unchecked::Exploitability {
                            side: 1,
                            reported: Some(solution.exploitability().chips),
                        },
                        Unchecked::Values {
                            side: 1,
                            reported: true,
                        },
                    ],
                    "{what}"
                );
                let text = comparison.to_string();
                for expected in [
                    "B: NOT MEASURED HERE. B holds the first",
                    "which could not be checked here",
                    "(as reported, not measured)",
                    "not compared, since one solution does not hold it",
                ] {
                    assert!(text.contains(expected), "{what}: {expected}\n{text}");
                }
            }
            // And the comparison through the dumps alone gives the same.
            let again = compare(&spot, &dump, &back, &Tolerances::default()).unwrap();
            assert!(again.passes(), "{what}: {again}");
        }
    }
}

#[test]
fn our_dump_names_deals_and_street_totals_as_the_format_does() {
    // On the turn spot, with a raise and all-in on both streets: after the turn's bet of 500
    // and its call, the river starts from zero, and its all-in is the 1,000 chips each player
    // has left, not the whole stack of 1,500.
    let (_, _, dump) = turn();
    let after_call = [DumpAction::Bet(500), DumpAction::Call(500), deal("3c")];
    let river = dump.node(&after_call).unwrap();
    assert_eq!(river.player, Player::Oop);
    assert_eq!(river.actions, [X, DumpAction::Bet(1000)]);
    let checked_through = dump.node(&[X, X, deal("3c")]).unwrap();
    assert_eq!(
        checked_through.actions,
        [X, DumpAction::Bet(750), DumpAction::Bet(1500)]
    );
    // A hand that holds the card dealt has no row: OOP's 9s9h after the 9s.
    let nine = dump.node(&[X, X, deal("9s")]).unwrap();
    let combo: Combo = "9s9h".parse().unwrap();
    assert!(nine.strategy.iter().all(|(c, _)| *c != combo));
    assert!(river.strategy.iter().any(|(c, _)| *c == combo));
    // No node follows a called all-in: the river is dealt with no decision.
    let allin = [X, DumpAction::Bet(500), DumpAction::Raise(1500)];
    let called = allin.iter().copied().chain([DumpAction::Call(1500)]);
    let called: Vec<DumpAction> = called.collect();
    assert!(dump.nodes.iter().all(|n| !n.path.starts_with(&called)));
    assert_eq!(
        dump.node(&allin).unwrap().actions,
        [DumpAction::Fold, DumpAction::Call(1500)]
    );
}

// ---------------------------------------------------------------------------------------------
// Wrong dumps

/// The turn spot and the complete dump of its solution.
fn turn() -> (Spot, &'static Solution, SolutionDump) {
    let solution = turn_solution();
    (
        load("turn_small"),
        solution,
        SolutionDump::from_solution(solution),
    )
}

#[test]
fn a_deal_in_the_wrong_place_is_an_invalid_dump() {
    let (_, _, ours) = turn();
    // The deal before IP's check behind, which is what closes the turn.
    let mut early = ours.clone();
    let at = index_of(&early, &[X, X, deal("3c")]);
    early.nodes[at].path = vec![X, deal("3c"), X];
    let (field, reason) = text_error(&early);
    assert_eq!(field, format!("nodes[{at}].path[1]"));
    assert!(
        reason.contains("is the deal of the 3c, but the turn is not over"),
        "{reason}"
    );
    // No deal at all between the turn and the river.
    let mut none = ours.clone();
    let at = index_of(&none, &[X, X, deal("3c"), X]);
    none.nodes[at].path = vec![X, X, X];
    let (field, reason) = text_error(&none);
    assert_eq!(field, format!("nodes[{at}].path[2]"));
    assert!(
        reason.contains(
            "is check, but the turn closed at `path[1]` with the check behind, so the next step \
             is the deal of the river card"
        ),
        "{reason}"
    );
    // A node at the deal itself, which is a chance node.
    let mut chance = ours.clone();
    let at = index_of(&chance, &[X, X, deal("3c")]);
    chance.nodes[at].path = vec![X, X];
    let (field, reason) = text_error(&chance);
    assert_eq!(field, format!("nodes[{at}].path"));
    assert!(
        reason.contains("ends with the check behind, which closes the turn"),
        "{reason}"
    );
}

#[test]
fn a_missing_card_is_reported_once_at_the_deal() {
    // The dump has no river after the 3c when both check the turn. The river nodes after the
    // other 47 cards, and after the 3c on the other lines, are all there.
    let (spot, _, ours) = turn();
    let mut dump = ours.clone();
    let line = [X, X, deal("3c")];
    dump.nodes.retain(|node| !node.path.starts_with(&line));
    assert!(dump.validate().is_ok());
    let found = mismatches(&dump, &spot);
    assert_eq!(
        found,
        [Mismatch::MissingCards {
            path: vec![X, X],
            street: Street::River,
            cards: vec![card("3c")],
            of: 48,
        }]
    );
    assert!(
        found[0].to_string().contains(
            "after [check, check], our tree deals the river and has a decision after each of the \
             48 cards that can fall there; the dump has no node after 1 of them: 3c"
        ),
        "{}",
        found[0]
    );
}

#[test]
fn a_row_for_a_hand_that_holds_a_dealt_card_is_an_invalid_dump() {
    let (_, _, ours) = turn();
    let path = [X, X, deal("9s")];
    let combo: Combo = "9s9h".parse().unwrap();
    let mut dump = ours.clone();
    let at = index_of(&dump, &path);
    let width = dump.nodes[at].actions.len();
    dump.nodes[at]
        .strategy
        .push((combo, vec![1.0 / width as f64; width]));
    let (field, reason) = text_error(&dump);
    assert_eq!(field, format!("nodes[{at}].strategy.9s9h"));
    assert!(
        reason.contains("holds the 9s, dealt at `path[2]` (at [check, check, deal 9s])"),
        "{reason}"
    );
    // And the other way round: no row for a hand that holds no card dealt.
    let mut dump = ours.clone();
    let at = index_of(&dump, &[X, X, deal("3c")]);
    dump.nodes[at].strategy.retain(|(c, _)| *c != combo);
    let (field, reason) = text_error(&dump);
    assert_eq!(field, format!("nodes[{at}].strategy"));
    assert!(reason.contains("has no row for 9s9h"), "{reason}");
}

/// `action` with `by` chips added to its amount.
fn shifted(action: DumpAction, by: u32) -> DumpAction {
    match action {
        DumpAction::Call(amount) => DumpAction::Call(amount + by),
        DumpAction::Bet(amount) => DumpAction::Bet(amount + by),
        DumpAction::Raise(amount) => DumpAction::Raise(amount + by),
        other => other,
    }
}

/// `dump` with every amount written as the acting player's total over all streets so far,
/// instead of its total for the street.
fn cumulative(dump: &SolutionDump) -> SolutionDump {
    let mut out = dump.clone();
    for node in &mut out.nodes {
        let original = node.path.clone();
        for (k, step) in node.path.iter_mut().enumerate() {
            let (before, _) = put_in(&original[..k]);
            let actor = original[..k]
                .iter()
                .rev()
                .take_while(|s| !s.is_deal())
                .count()
                % 2;
            *step = shifted(*step, before[actor]);
        }
        let (before, _) = put_in(&original);
        let earlier = before[node.player.index()];
        for action in &mut node.actions {
            *action = shifted(*action, earlier);
        }
    }
    out
}

#[test]
fn amounts_that_are_totals_over_all_streets_are_caught() {
    // Where the river's all-in becomes more than a player has left, the dump alone shows it.
    let (_, _, ours) = turn();
    let (field, reason) = text_error(&cumulative(&ours));
    assert!(field.starts_with("nodes["), "{field}");
    assert!(
        reason.contains("an amount is the acting player's total for the street"),
        "{field}: {reason}"
    );
    assert!(reason.contains("more than the 1000 chips"), "{reason}");
    // Where every amount stays within the stack, the dump is valid, and the tree stage
    // reports the first river node after each line with betting on the turn: our tree offers
    // a bet of 1,500 into the pot of 2,000, the dump one of 2,000.
    let spot = load("turn_wide");
    let ours = SolutionDump::from_solution(&capped(&spot, 3));
    let dump = cumulative(&ours);
    assert!(dump.validate().is_ok());
    // Each river after a turn bet and call is reported at OOP's first node and at IP's after
    // OOP checks, both reached by an action both trees have: two lines, 48 cards, two nodes.
    let found = mismatches(&dump, &spot);
    assert_eq!(found.len(), 2 * 48 * 2, "{}", found.len());
    for mismatch in &found {
        let Mismatch::Actions {
            path, ours, theirs, ..
        } = mismatch
        else {
            panic!("{mismatch}");
        };
        let deal_at = path.iter().position(|s| s.is_deal()).unwrap();
        assert!(path.len() - deal_at <= 2, "{mismatch}");
        assert!(path[..deal_at].ends_with(&[DumpAction::Bet(500), DumpAction::Call(500)]));
        assert_eq!(ours, &[X, DumpAction::Bet(1500)], "{mismatch}");
        assert_eq!(theirs, &[X, DumpAction::Bet(2000)], "{mismatch}");
    }
}

#[test]
fn a_partial_dump_with_a_street_half_listed_is_caught() {
    let spot = load("flop_tiny");
    let solution = flop_solution();
    let partial = SolutionDump::from_solution_streets(solution, 2);
    // The turn listed after half the cards only: one report, at the deal.
    let mut half = partial.clone();
    let falls: Vec<Card> = half
        .nodes
        .iter()
        .filter(|n| n.path.len() == 3 && n.path[..2] == [X, X])
        .map(|n| n.path[2].card().unwrap())
        .collect();
    assert_eq!(falls.len(), 49);
    let dropped = &falls[25..];
    half.nodes.retain(|n| {
        !(n.path.len() >= 3
            && n.path[..2] == [X, X]
            && dropped.contains(&n.path[2].card().unwrap()))
    });
    let found = mismatches(&half, &spot);
    assert_eq!(
        found,
        [Mismatch::MissingCards {
            path: vec![X, X],
            street: Street::Turn,
            cards: dropped.to_vec(),
            of: 49,
        }]
    );
    // The turn listed after every card, but not every node of it: IP's node after OOP checks
    // the turn after the Ah, with the nodes below it, is reported once, at that node.
    let path = [X, X, deal("Ah"), X];
    let mut gap = partial.clone();
    gap.nodes.retain(|n| !n.path.starts_with(&path));
    assert_eq!(
        mismatches(&gap, &spot),
        [Mismatch::MissingNode {
            path: path.to_vec(),
            player: Player::Ip,
        }]
    );
    // Without that node alone, the nodes below it lead through a node that is not listed: the
    // dump is not closed under prefixes, which the dump stage sees.
    let mut hole = partial.clone();
    let at = index_of(&hole, &path);
    hole.nodes.remove(at);
    let (field, reason) = text_error(&hole);
    assert!(field.ends_with("].path"), "{field}");
    assert!(
        reason.contains("leads through [check, check, deal Ah, check], the decision node"),
        "{reason}"
    );
    // A dump of two streets that says it is complete, or of every street that says it is not.
    let mut claims = partial.clone();
    claims.complete = true;
    let (field, reason) = text_error(&claims);
    assert_eq!(field, "complete");
    assert!(
        reason.contains("is true, but no path deals more than 1 card, so the dump holds the first 2 of the 3 streets"),
        "{reason}"
    );
    let mut denies = SolutionDump::from_solution(solution);
    denies.complete = false;
    let (field, reason) = text_error(&denies);
    assert_eq!(field, "complete");
    assert!(
        reason.contains("so the dump holds every street"),
        "{reason}"
    );
}

/// `dump` with each EV, and each value, moved by `shift` of the node's path and player.
fn rebased(dump: &SolutionDump, shift: impl Fn(&[DumpAction], Player) -> f64) -> SolutionDump {
    let mut moved = dump.clone();
    for node in &mut moved.nodes {
        let by = shift(&node.path, node.player);
        for (_, ev) in node.ev.as_mut().unwrap() {
            *ev = ev.map(|ev| ev + by);
        }
    }
    moved
}

#[test]
fn evs_counted_from_the_node_fail_on_reported_evs_alone() {
    // Two ways to count from the node: with every chip the acting player has put in sunk, and
    // with only those of the street sunk, as a wrapper would that converted with the street
    // totals of the amounts. The strategy is the same and compares the same; its EVs do not
    // agree with what it gives here, at the nodes where the chips sunk show.
    let (spot, _, ours) = turn();
    let cases: [(&str, SolutionDump, bool); 2] = [
        (
            "every street's chips sunk",
            rebased(&ours, |path, player| {
                f64::from(put_in(path).1[player.index()])
            }),
            false,
        ),
        (
            "only the street's chips sunk, the earlier streets' forgotten",
            rebased(&ours, |path, player| {
                f64::from(put_in(path).0[player.index()])
            }),
            true,
        ),
    ];
    for (what, theirs, later_only) in cases {
        let comparison =
            compare(&spot, &ours, &through_json(&theirs), &Tolerances::default()).unwrap();
        assert_eq!(
            checks(&comparison),
            [Check::ReportedEv],
            "{what}: {comparison}"
        );
        for failure in &comparison.failures {
            let Failure::ReportedNodeEv {
                side, path, player, ..
            } = failure
            else {
                panic!("{what}: {failure}");
            };
            assert_eq!(*side, 1, "{what}");
            let (earlier, all) = put_in(path);
            assert!(all[player.index()] > 0, "{what}: {failure}");
            if later_only {
                assert!(earlier[player.index()] > 0, "{what}: {failure}");
            }
        }
    }
}

#[test]
fn a_strategy_right_on_the_first_street_and_wrong_below_a_card_fails_there() {
    // A flop solution whose strategy is ours on the flop and on every turn but the 8h, and
    // below the 8h checks or folds every hand. Below a deal a node is reached at most 1/45 of
    // the time, so its raw reach would check none of them; given its cards each is reached as
    // often as its line is, and the frequencies fail there, and nowhere else. The complete
    // dump fails on exploitability too; the partial one, of the flop and the turn, cannot be
    // measured, and is caught by its frequencies alone.
    let spot = load("flop_tiny");
    let solution = flop_solution();
    let wrong = deal("8h");
    let ours = MappedSolution::from_solution(solution);
    for streets in [3, 2] {
        let mut dump = SolutionDump::from_solution_streets(solution, streets);
        let mut changed = 0;
        for node in dump.nodes.iter_mut().filter(|n| n.path.contains(&wrong)) {
            changed += 1;
            for (_, row) in &mut node.strategy {
                for (i, p) in row.iter_mut().enumerate() {
                    *p = if i == 0 { 1.0 } else { 0.0 };
                }
            }
        }
        assert!(changed > 0);
        let theirs = MappedSolution::from_dump(&through_json(&dump), &spot).unwrap();
        let comparison = compare_mapped(&ours, &theirs, &Tolerances::default());
        let frequency: Vec<&Failure> = comparison.failures_of(Check::Frequency).collect();
        assert!(!frequency.is_empty(), "{streets}: {comparison}");
        for failure in &frequency {
            let Failure::Frequency {
                path,
                reach,
                reach_given_cards,
                ..
            } = failure
            else {
                unreachable!();
            };
            assert!(path.contains(&wrong), "{streets}: {failure}");
            assert!(reach[0] < 1.0 / 45.0, "{streets}: {failure}");
            assert!(reach_given_cards[0] > 0.01, "{streets}: {failure}");
        }
        if streets == 3 {
            assert!(
                matches!(
                    comparison
                        .failures_of(Check::Exploitability)
                        .collect::<Vec<_>>()[..],
                    [Failure::Exploitability { side: 1, .. }]
                ),
                "{comparison}"
            );
        } else {
            assert_eq!(
                comparison.failures_of(Check::Exploitability).count(),
                0,
                "{comparison}"
            );
            assert!(theirs.exploitability().is_none());
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Cards that cannot fall, and EVs that are not given

#[test]
fn the_subtree_after_a_card_that_cannot_fall_is_expected_in_no_dump() {
    // OOP holds AsAh only, and meets only IP's KsKh: every pair of hands holds the As, the Ah,
    // the Ks and the Kh, which cannot fall on the river. Our tree has a subtree after each;
    // our dump leaves them out, as the format says, and maps back without a mismatch.
    let spot = load("turn_dead_cards");
    let solution = capped(&spot, 20);
    let dump = SolutionDump::from_solution(&solution);
    let dead = ["As", "Ah", "Ks", "Kh"].map(card);
    for node in &dump.nodes {
        assert!(
            node.path
                .iter()
                .all(|s| !s.card().is_some_and(|c| dead.contains(&c))),
            "{}",
            PathText(&node.path)
        );
    }
    let game = solution.game();
    let after_dead = game.find("x x As").unwrap();
    assert!(game.actor(after_dead).is_some(), "our tree has the subtree");
    let mapped = MappedSolution::from_dump(&through_json(&dump), &spot).unwrap();
    assert!(!mapped.holds(after_dead));
    assert!(mapped.is_complete());
    assert_eq!(mapped.exploitability(), Some(solution.exploitability()));
    let comparison = compare_mapped(
        &MappedSolution::from_solution(&solution),
        &mapped,
        &Tolerances::default(),
    );
    assert!(comparison.passes(), "{comparison}");
    assert!(comparison.nodes.iter().all(|n| n.node != after_dead));
    // A dump with a node after a card that cannot fall is invalid.
    let mut extra = dump.clone();
    let mut node = extra.nodes[index_of(&extra, &[X, X, deal("3c")])].clone();
    node.path[2] = deal("Ks");
    node.strategy
        .retain(|(combo, _)| !combo.intersects(pk_core::CardSet::single(card("Ks"))));
    if let Some(ev) = &mut node.ev {
        ev.retain(|(combo, _)| !combo.intersects(pk_core::CardSet::single(card("Ks"))));
    }
    extra.nodes.push(node);
    let (field, reason) = text_error(&extra);
    assert_eq!(field, format!("nodes[{}].path[2]", extra.nodes.len() - 1));
    assert!(
        reason.contains("deals the Ks, which cannot fall"),
        "{reason}"
    );
}

#[test]
fn an_ev_written_null_is_no_ev_and_never_zero() {
    // A reference solver writes null where it has no EV. At every river node, every EV null:
    // nothing to compare there, and nothing fails. Zero in their place is an EV, which its
    // strategy does not give.
    let (spot, _, ours) = turn();
    let blank = |value: Option<f64>| {
        let mut dump = ours.clone();
        for node in dump.nodes.iter_mut().filter(|n| n.street_index() > 0) {
            for (_, ev) in node.ev.as_mut().unwrap() {
                *ev = value;
            }
        }
        dump
    };
    let comparison = compare(
        &spot,
        &ours,
        &through_json(&blank(None)),
        &Tolerances::default(),
    )
    .unwrap();
    assert!(comparison.passes(), "{comparison}");
    for node in comparison.nodes.iter().filter(|n| n.deals > 0) {
        assert_eq!(node.ev[1], None);
        assert!(node.measured_ev[1].is_some() || node.frequencies[1].is_none());
    }
    let comparison = compare(
        &spot,
        &ours,
        &through_json(&blank(Some(0.0))),
        &Tolerances::default(),
    )
    .unwrap();
    assert_eq!(checks(&comparison), [Check::ReportedEv], "{comparison}");
}

// ---------------------------------------------------------------------------------------------
// Two honest solvers, street by street

/// What a comparison of two honest solves measures on one street.
struct StreetMeasure {
    checked: usize,
    nodes: usize,
    largest: f64,
    largest_at: String,
    mean: f64,
}

fn by_street(comparison: &Comparison) -> Vec<StreetMeasure> {
    comparison
        .streets
        .iter()
        .map(|street| {
            let (largest, largest_at) = match street.frequencies.largest {
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
            StreetMeasure {
                checked: street.frequencies.checked,
                nodes: street.nodes,
                largest,
                largest_at,
                mean: 100.0 * street.frequencies.weighted_mean,
            }
        })
        .collect()
}

/// The largest frequency difference at the checked nodes of one street whose reach given
/// their cards is above `reach`, in points.
fn largest_above(comparison: &Comparison, deals: usize, reach: f64) -> f64 {
    comparison
        .nodes
        .iter()
        .filter(|n| n.deals == deals && n.is_checked(comparison.tolerances.min_reach))
        .filter(|n| n.reach_given_cards.iter().any(|&r| r > reach))
        .map(|n| 100.0 * n.largest_difference())
        .fold(0.0, f64::max)
}

/// The percentage of the checked nodes of one street whose largest frequency difference is at
/// most each of `limits`, in points.
fn shares_within(comparison: &Comparison, deals: usize, limits: [f64; 3]) -> [f64; 3] {
    let differences: Vec<f64> = comparison
        .nodes
        .iter()
        .filter(|n| n.deals == deals && n.is_checked(comparison.tolerances.min_reach))
        .map(|n| 100.0 * n.largest_difference())
        .collect();
    limits.map(|limit| {
        let within = differences.iter().filter(|&&d| d <= limit).count();
        100.0 * within as f64 / differences.len().max(1) as f64
    })
}

#[test]
#[ignore = "slow"]
fn honest_solvers_table_by_street() {
    // Two pairs of honest solves, DCFR against CFR+ to the same target, of three turn spots,
    // with about 300, 100 and 10 combos a side, and of a small flop spot with about 100,
    // compared under the default tolerances and summed up street by street: the nodes checked
    // (reached more than 1% of the time given their cards), the largest frequency difference
    // among them and among those reached more than 10% of the time, the share of them within
    // 2, 5 and 10 points, the reach-weighted mean difference, and the values and class EVs at
    // the root. Prints the tables with --nocapture; `Tolerances::frequency_below_deals` is set
    // from them (DECISIONS P36). What it asserts is what holds for every pair: no solution's
    // EVs disagree with its strategy, the exploitabilities and values are within their limits,
    // and the first street's frequencies at 0.03% are within the M3 gate's 2 points. The
    // frequencies below a deal are what it measures, and on the narrow spot they exceed any
    // useful limit. It takes about eight minutes in release.
    let spots = [
        ("turn_wide", 1),
        ("turn_medium", 1),
        ("turn_small", 1),
        ("flop_wide", 10),
    ];
    let pairs: [(&str, f64); 2] = [
        ("DCFR to 0.1% against CFR+ to 0.1%", 0.1),
        ("DCFR to 0.03% against CFR+ to 0.03%", 0.03),
    ];
    for (title, target) in pairs {
        println!("\n{title}\n");
        println!(
            "| Spot | Iterations | Exploitability (% pot) | Street | Nodes checked | Largest \
             frequency difference | Above 10% reach | Checked within 2 / 5 / 10 points | \
             Reach-weighted mean | Values apart | Class EVs apart | Verdict |"
        );
        println!("|---|---:|---:|---|---:|---|---:|---:|---:|---:|---:|---|");
        for (name, check_every) in spots {
            let spot = load(name);
            let start = Instant::now();
            let a = solve_to(&spot, dcfr(), target, check_every);
            let b = solve_to(&spot, Schedule::CfrPlus, target, check_every);
            let seconds = start.elapsed().as_secs_f64();
            let comparison = compare_mapped(
                &MappedSolution::from_solution(&a),
                &MappedSolution::from_solution(&b),
                &Tolerances::default(),
            );
            let values = comparison.values().map(Option::unwrap);
            let value = (0..2)
                .map(|p| comparison.percent((values[0][p] - values[1][p]).abs()))
                .fold(0.0, f64::max);
            let class_ev = comparison
                .classes
                .iter()
                .filter(|c| c.share > comparison.tolerances.min_reach)
                .filter_map(|c| match c.ev {
                    [Some(x), Some(y)] => Some(comparison.percent((x - y).abs())),
                    _ => None,
                })
                .fold(0.0, f64::max);
            let verdict = if comparison.passes() {
                "passes".to_string()
            } else {
                let fails: Vec<String> = checks(&comparison).iter().map(Check::to_string).collect();
                format!(
                    "fails ({} on {} items)",
                    fails.join(", "),
                    comparison.failures.len()
                )
            };
            for (deals, street) in by_street(&comparison).iter().enumerate() {
                let head = if deals == 0 {
                    format!(
                        "| `{name}` | {} / {} | {:.3} / {:.3} |",
                        a.iterations(),
                        b.iterations(),
                        a.exploitability().percent_of_pot,
                        b.exploitability().percent_of_pot
                    )
                } else {
                    "| | | |".to_string()
                };
                let tail = if deals == 0 {
                    format!("{value:.4} | {class_ev:.3} | {verdict} ({seconds:.0} s) |")
                } else {
                    " | | |".to_string()
                };
                let [two, five, ten] = shares_within(&comparison, deals, [2.0, 5.0, 10.0]);
                println!(
                    "{head} {} | {} of {} | {:.2}, {} | {:.2} | {two:.1}% / {five:.1}% / \
                     {ten:.1}% | {:.3} | {tail}",
                    comparison.streets[deals].street,
                    street.checked,
                    street.nodes,
                    street.largest,
                    street.largest_at,
                    largest_above(&comparison, deals, 0.1),
                    street.mean,
                );
            }
            for check in [Check::Exploitability, Check::ReportedEv, Check::Value] {
                assert!(
                    !checks(&comparison).contains(&check),
                    "{title}, {name}: {comparison}"
                );
            }
            if target <= 0.03 {
                assert!(
                    comparison
                        .failures
                        .iter()
                        .all(|f| !matches!(f, Failure::Frequency { deals: 0, .. })),
                    "{title}, {name}: {comparison}"
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// The reference solver

/// The spot files in `folder`, sorted, or none if it does not exist.
fn spot_files(folder: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(folder) else {
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = entries
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|e| e == "toml"))
        .collect();
    paths.sort();
    paths
}

fn out_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("compare-streets-{name}.json"))
}

/// Runs the reference on the spot file at `path`, solves it ourselves to 0.03% of the pot and
/// compares the two under the default tolerances; prints the comparison and returns it, or
/// the error that stopped it.
fn against_reference(reference: &ExternalSolver, path: &Path) -> Result<Comparison, String> {
    let spot = Spot::load(path).map_err(|e| e.to_string())?;
    let start = Instant::now();
    let dump = reference
        .run(path, &out_path(&spot.name))
        .map_err(|e| e.to_string())?;
    let ran = start.elapsed();
    let theirs = MappedSolution::from_dump(&dump, &spot).map_err(|e| e.to_string())?;
    let check_every = if spot.board.len() == 3 { 10 } else { 1 };
    let start = Instant::now();
    let ours = solve_to(&spot, dcfr(), 0.03, check_every);
    let solved = start.elapsed();
    let comparison = compare_mapped(
        &MappedSolution::from_solution(&ours),
        &theirs,
        &Tolerances::default(),
    );
    println!(
        "{} ({} decision nodes in the dump; the reference took {ran:.1?}, our solve {solved:.1?}):\n\
         {comparison}",
        spot.name,
        dump.nodes.len()
    );
    Ok(comparison)
}

#[test]
#[ignore = "slow"]
fn the_configured_reference_solver_matches_ours_on_turn_and_flop_spots() {
    // The comparison of the M4 gate, when `PK_REFERENCE_SOLVER` names a reference: each spot
    // of `reference/spots/turn` and `reference/spots/flop` solved by it and by us to 0.03% of
    // the pot, and compared under the default tolerances. Without a reference, or without
    // spots, it does nothing.
    let Some(reference) = ExternalSolver::from_env().unwrap() else {
        println!("no reference solver: `PK_REFERENCE_SOLVER` is not set, so nothing to compare");
        return;
    };
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../reference/spots");
    let mut failed = Vec::new();
    for street in ["turn", "flop"] {
        let paths = spot_files(&root.join(street));
        if paths.is_empty() {
            println!("no {street} spots in reference/spots/{street}, so none to compare");
        }
        for path in paths {
            match against_reference(&reference, &path) {
                Ok(comparison) if comparison.passes() => {}
                Ok(_) => failed.push(path.display().to_string()),
                Err(error) => {
                    println!("{}: {error}", path.display());
                    failed.push(path.display().to_string());
                }
            }
        }
    }
    assert!(failed.is_empty(), "the reference differs on {failed:?}");
}

#[test]
#[ignore = "slow"]
fn the_configured_reference_solver_on_the_test_spots() {
    // The same comparison on the small spots of `tests/data`, whose ranges are made up and
    // narrow: each dump must answer its spot, and the strategies must agree on exploitability,
    // values and EVs, and on the first street's frequencies. Below a deal, where a single hand
    // is a large share of these ranges, the frequencies are printed and not asserted.
    let Some(reference) = ExternalSolver::from_env().unwrap() else {
        println!("no reference solver: `PK_REFERENCE_SOLVER` is not set, so nothing to compare");
        return;
    };
    let mut failed = Vec::new();
    for name in ["turn_small", "turn_medium", "turn_wide", "flop_small"] {
        match against_reference(&reference, &data_path(name)) {
            Ok(comparison) => {
                let wrong: Vec<&Failure> = comparison
                    .failures
                    .iter()
                    .filter(|f| !matches!(f, Failure::Frequency { deals, .. } if *deals > 0))
                    .collect();
                if !wrong.is_empty() {
                    failed.push(name);
                }
            }
            Err(error) => {
                println!("{name}: {error}");
                failed.push(name);
            }
        }
    }
    assert!(failed.is_empty(), "the reference differs on {failed:?}");
}

// A spot against itself with its suits relabelled

/// `spot` with its suits relabelled by `sigma`: the board and both ranges.
fn relabelled(spot: &Spot, sigma: SuitPermutation) -> Spot {
    let mut image = spot.clone();
    image.name = format!("{} relabelled by {sigma}", spot.name);
    image.board = sigma.apply_set(spot.board);
    image.oop_range = sigma.apply_range(&spot.oop_range);
    image.ip_range = sigma.apply_range(&spot.ip_range);
    image
}

/// A dump of a solution of a spot relabelled by `sigma`, with its suits relabelled back, which
/// makes it a dump of the spot itself: the board, every hand and every card dealt on a path go
/// through the inverse of `sigma`, and nothing else changes.
fn relabelled_back(dump: &SolutionDump, sigma: SuitPermutation) -> SolutionDump {
    let back = sigma.inverse();
    let mut image = dump.clone();
    image.spot.board = back.apply_set(dump.spot.board);
    for (combo, _) in image.spot.hands.iter_mut().flatten() {
        *combo = back.apply_combo(*combo);
    }
    for node in &mut image.nodes {
        for step in &mut node.path {
            if let DumpAction::Deal(card) = step {
                *card = back.apply_card(*card);
            }
        }
        for (combo, _) in &mut node.strategy {
            *combo = back.apply_combo(*combo);
        }
        for (combo, _) in node.ev.iter_mut().flatten() {
            *combo = back.apply_combo(*combo);
        }
    }
    image
}

/// The relabelling these tests use: clubs with spades and diamonds with hearts, which moves
/// the board of every spot they solve.
const RELABELLING: SuitPermutation = SuitPermutation::new([3, 2, 1, 0]);

/// Solves `spot` and `spot` relabelled by [`RELABELLING`] to `target` percent of the pot, and
/// compares the two as the gate compares our solution with the reference's: ours on every
/// street against the dump of the relabelled solve's first two streets, relabelled back, under
/// the default tolerances. Prints the comparison.
fn against_relabelled(spot: &Spot, target: f64) -> Comparison {
    let image = relabelled(spot, RELABELLING);
    assert_ne!(image.board, spot.board, "{}", spot.name);
    let check_every = if spot.board.len() == 3 { 10 } else { 1 };
    let ours = solve_to(spot, dcfr(), target, check_every);
    let theirs = solve_to(&image, dcfr(), target, check_every);
    let streets = streets_of(spot).min(2);
    let dump = SolutionDump::from_solution_streets(&theirs, streets);
    // As it stands the dump is of another board, and does not answer the spot.
    assert!(MappedSolution::from_dump(&dump, spot).is_err());
    let dump = relabelled_back(&dump, RELABELLING);
    let comparison = compare_mapped(
        &MappedSolution::from_solution(&ours),
        &MappedSolution::from_dump(&dump, spot).unwrap(),
        &Tolerances::default(),
    );
    println!(
        "{} against itself relabelled by {RELABELLING} ({} and {} iterations):\n{comparison}",
        spot.name,
        ours.iterations(),
        theirs.iterations()
    );
    comparison
}

#[test]
fn a_spot_relabelled_passes_the_comparison_with_the_spot() {
    // The gate's "suit-isomorphic boards give identical results", read as the gate reads
    // agreement with the reference: a turn spot and a flop spot, each solved to 0.03% of the
    // pot as it is and with its suits relabelled, pass the comparison under the default
    // tolerances, with nodes checked on both streets compared. The two solves are of different
    // boards, so their sums run in different orders and they part within rounding (DECISIONS
    // P61); `tests/merged.rs` holds them to each other bit for bit while they have not.
    for name in ["turn_small", "flop_tiny"] {
        let comparison = against_relabelled(&load(name), 0.03);
        assert!(comparison.passes(), "{name}: {comparison}");
        let streets = by_street(&comparison);
        assert_eq!(streets.len(), 2, "{name}");
        assert!(streets.iter().all(|street| street.checked > 0), "{name}");
    }
}

#[test]
#[ignore = "slow"]
fn every_gate_spot_relabelled_passes_the_comparison_with_the_spot() {
    // The same on every spot the gate compares with the reference, those of
    // `reference/spots/turn` and `reference/spots/flop`, and needing no reference: each
    // against itself relabelled, both solved to 0.03% of the pot, under the tolerances the
    // reference is held to. The river of a flop spot is not compared, as it is not with the
    // reference, whose dumps of a flop spot hold two streets; DECISIONS P61 records what was
    // measured there once, where one rarely reached river node of `paired_raise` is far apart.
    // Measured, the largest differences are 0.03 points on the first street and 1.23 below a
    // card, and the values are within 0.0002% of the pot; the test takes about 11 minutes.
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../reference/spots");
    let mut failed = Vec::new();
    for street in ["turn", "flop"] {
        let paths = spot_files(&root.join(street));
        assert!(!paths.is_empty(), "no spots in reference/spots/{street}");
        for path in paths {
            let spot = Spot::load(&path).unwrap();
            if !against_relabelled(&spot, 0.03).passes() {
                failed.push(spot.name);
            }
        }
    }
    assert!(failed.is_empty(), "relabelled, these differ: {failed:?}");
}
