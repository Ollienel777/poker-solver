use super::*;
use crate::solve::{SolveOptions, solve};

fn load(name: &str) -> Spot {
    let path = format!(
        "{}/../../reference/spots/river/{name}.toml",
        env!("CARGO_MANIFEST_DIR")
    );
    Spot::load(path).unwrap()
}

fn solved(spot: &Spot) -> Solution {
    let options = SolveOptions {
        target: None,
        max_iterations: 20,
        ..SolveOptions::default()
    };
    solve(spot, &options, |_| {}).unwrap()
}

/// The toy game and the dump of a short solve of it.
fn toy() -> (Spot, Solution, SolutionDump) {
    let spot = load("toy_polarized");
    let solution = solved(&spot);
    let dump = SolutionDump::from_solution(&solution);
    (spot, solution, dump)
}

fn mismatches(dump: &SolutionDump, spot: &Spot) -> Vec<Mismatch> {
    match MappedSolution::from_dump(dump, spot) {
        Err(ImportError::Mismatch(list)) => list,
        Err(other) => panic!("{other}"),
        Ok(_) => panic!("the dump maps"),
    }
}

fn index(dump: &SolutionDump, path: &[DumpAction]) -> usize {
    dump.nodes.iter().position(|n| n.path == path).unwrap()
}

#[test]
fn a_solutions_own_dump_maps_back_to_its_strategy() {
    let (spot, solution, dump) = toy();
    let mapped = MappedSolution::from_dump(&dump, &spot).unwrap();
    assert_eq!(mapped.strategy(), solution.strategy());
    assert_eq!(mapped.exploitability(), Some(solution.exploitability()));
    assert_eq!(mapped.reported_values(), Some(solution.values()));
    assert_eq!(mapped.solver().name, SOLVER_NAME);
    assert_eq!(mapped.reported_evs(), Some(mapped.measured_evs()));
}

#[test]
fn actions_and_hands_in_another_order_map_to_the_same_strategy() {
    // Another solver may list the actions and the hands in its own order: each is matched by
    // what it is, so only the order of the dump changes.
    let spot = load("dry_ip_polarized");
    let solution = solved(&spot);
    let mut dump = SolutionDump::from_solution(&solution);
    for node in &mut dump.nodes {
        node.actions.reverse();
        for (_, row) in &mut node.strategy {
            row.reverse();
        }
        node.strategy.reverse();
        node.ev.as_mut().unwrap().reverse();
    }
    dump.nodes.reverse();
    dump.spot.hands[0].reverse();
    let mapped = MappedSolution::from_dump(&dump, &spot).unwrap();
    assert_eq!(mapped.strategy(), solution.strategy());
    assert_eq!(mapped.reported_evs(), Some(mapped.measured_evs()));
}

#[test]
fn the_spot_is_checked_before_the_hands() {
    // Amounts in big blinds instead of chips, the rake as a fraction, and a weight that the
    // hands check would report: only the spot is reported.
    let spot = load("dry_ip_polarized");
    let mut dump = SolutionDump::from_solution(&solved(&spot));
    dump.spot.pot = 20;
    dump.spot.rake_percent = 0.05;
    dump.spot.hands[0][0].1 = 0.5;
    let found = mismatches(&dump, &spot);
    assert_eq!(
        found,
        [
            Mismatch::Spot {
                field: "spot.pot",
                ours: "2000 chips".into(),
                theirs: "20 chips; a dump's amounts are in chips, with 1bb = 100 chips".into(),
            },
            Mismatch::Spot {
                field: "spot.rake.percent",
                ours: "5".into(),
                theirs: "0.05; it is a percent, 5 for 5%".into(),
            },
        ]
    );
    let text = ImportError::Mismatch(found).to_string();
    assert!(
        text.starts_with("the dump does not answer this spot: 2 differences\n- `spot.pot` is 20"),
        "{text}"
    );
    // Another board, which no hand of the toy game blocks, and another stack: each named.
    let (spot, _, mut dump) = toy();
    dump.spot.board = "Ks9h6d3c2h".parse().unwrap();
    dump.spot.effective_stack += 100;
    let fields: Vec<&str> = mismatches(&dump, &spot)
        .iter()
        .map(|m| match m {
            Mismatch::Spot { field, .. } => *field,
            other => panic!("{other}"),
        })
        .collect();
    assert_eq!(fields, ["spot.board", "spot.effective_stack"]);
}

#[test]
fn without_rake_the_cap_does_not_matter() {
    let (spot, _, mut dump) = toy();
    dump.spot.rake_cap = 300;
    assert!(MappedSolution::from_dump(&dump, &spot).is_ok());
}

#[test]
fn missing_extra_and_reweighted_hands_are_listed_by_player() {
    let (spot, _, dump) = toy();
    // OOP lacks a hand, which then has no rows, and IP has one that is not ours.
    let mut changed = dump.clone();
    let dropped = changed.spot.hands[0].remove(0);
    let extra: Combo = "AsAh".parse().unwrap();
    changed.spot.hands[1].push((extra, 0.5));
    for node in &mut changed.nodes {
        let width = node.actions.len();
        match node.player {
            Player::Oop => {
                node.strategy.retain(|(c, _)| *c != dropped.0);
                node.ev.as_mut().unwrap().retain(|(c, _)| *c != dropped.0);
            }
            Player::Ip => {
                node.strategy.push((extra, vec![1.0 / width as f64; width]));
                node.ev.as_mut().unwrap().push((extra, None));
            }
        }
    }
    assert_eq!(
        mismatches(&changed, &spot),
        [
            Mismatch::Hands {
                player: Player::Oop,
                missing: vec![dropped],
                extra: vec![],
            },
            Mismatch::Hands {
                player: Player::Ip,
                missing: vec![],
                extra: vec![(extra, 0.5)],
            },
        ]
    );
    // A weight read as another.
    let mut changed = dump.clone();
    changed.spot.hands[1][3].1 = 0.25;
    let combo = changed.spot.hands[1][3].0;
    let found = mismatches(&changed, &spot);
    assert_eq!(
        found,
        [Mismatch::Weights {
            player: Player::Ip,
            hands: vec![(combo, 1.0, 0.25)],
        }]
    );
    assert!(
        found[0]
            .to_string()
            .contains(&format!("{combo} 0.25 (ours 1)"))
    );
}

#[test]
fn a_missing_node_is_reported_once_without_the_nodes_below_it() {
    // The dump lacks IP's node after a check, and so OOP's below it is not reached.
    let (spot, _, mut dump) = toy();
    let at = index(&dump, &[DumpAction::Check]);
    dump.nodes.remove(at);
    assert_eq!(
        mismatches(&dump, &spot),
        [Mismatch::MissingNode {
            path: vec![DumpAction::Check],
            player: Player::Ip,
        }]
    );
}

#[test]
fn a_node_where_our_hand_is_over_or_where_we_have_none_is_reported() {
    let (spot, _, dump) = toy();
    let root = dump.nodes[index(&dump, &[])].clone();
    // A decision after IP folds to the bet, where our hand is over.
    let mut after_fold = root.clone();
    after_fold.path = vec![DumpAction::Bet(500), DumpAction::Fold];
    // And one after a bet our tree does not have, with one more below it.
    let mut unknown = root.clone();
    unknown.path = vec![DumpAction::Bet(700)];
    let mut below = root.clone();
    below.path = vec![DumpAction::Bet(700), DumpAction::Check];
    let mut changed = dump.clone();
    changed.nodes.extend([after_fold, unknown, below]);
    let found = mismatches(&changed, &spot);
    assert_eq!(
        found,
        [
            Mismatch::ExtraNode {
                path: vec![DumpAction::Bet(500), DumpAction::Fold],
                player: Player::Oop,
                ends_hand: true,
            },
            Mismatch::ExtraNode {
                path: vec![DumpAction::Bet(700)],
                player: Player::Oop,
                ends_hand: false,
            },
        ]
    );
    assert!(
        found[0]
            .to_string()
            .ends_with("ends the hand there, with a fold or a showdown")
    );
}

#[test]
fn another_player_to_act_is_reported_without_the_nodes_below() {
    let (spot, _, mut dump) = toy();
    let at = index(&dump, &[DumpAction::Check]);
    let width = dump.nodes[at].actions.len();
    let oop: Vec<Combo> = dump.spot.hands[0].iter().map(|h| h.0).collect();
    let node = &mut dump.nodes[at];
    node.player = Player::Oop;
    node.strategy = oop
        .iter()
        .map(|&c| (c, vec![1.0 / width as f64; width]))
        .collect();
    node.ev = Some(oop.iter().map(|&c| (c, None)).collect());
    assert_eq!(
        mismatches(&dump, &spot),
        [Mismatch::Player {
            path: vec![DumpAction::Check],
            ours: Player::Ip,
            theirs: Player::Oop,
        }]
    );
}

#[test]
fn an_invalid_dump_or_a_spot_whose_game_cannot_be_built_is_its_own_error() {
    let (spot, _, mut dump) = toy();
    dump.nodes[0].strategy[0].1 = vec![0.5, 0.25];
    assert!(matches!(
        MappedSolution::from_dump(&dump, &spot),
        Err(ImportError::Dump(DumpError::Invalid { .. }))
    ));
    // A spot that fails its own checks: a pot of 0.
    let (mut broken, _, dump) = toy();
    broken.pot = 0;
    assert!(matches!(
        MappedSolution::from_dump(&dump, &broken),
        Err(ImportError::Spot(SolveError::Spot(_)))
    ));
    // A river dump mapped onto a flop spot builds the flop game, and stops at the board.
    let (mut flop, _, dump) = toy();
    flop.board = "Ks9h6d".parse().unwrap();
    flop.tree.flop = flop.tree.river.clone();
    flop.tree.turn = flop.tree.river.clone();
    let found = mismatches(&dump, &flop);
    assert!(
        matches!(
            &found[..],
            [Mismatch::Spot {
                field: "spot.board",
                ..
            }]
        ),
        "{found:?}"
    );
}

// ---------------------------------------------------------------------------------------------
// Version 2

fn turn_spot() -> Spot {
    let path = format!("{}/tests/data/turn_small.toml", env!("CARGO_MANIFEST_DIR"));
    Spot::load(path).unwrap()
}

#[test]
fn a_partial_dump_holds_its_first_street_and_is_not_measured() {
    // Of the turn spot, the turn alone: every turn node is held, no river node is, nothing is
    // measured but where the whole subtree is held, and below the turn the strategy is uniform.
    let spot = turn_spot();
    let solution = solved(&spot);
    let dump = SolutionDump::from_solution_streets(&solution, 1);
    let mapped = MappedSolution::from_dump(&dump, &spot).unwrap();
    assert!(!mapped.is_complete());
    assert_eq!(mapped.streets(), 1);
    assert_eq!(mapped.exploitability(), None);
    assert_eq!(mapped.reported_values(), Some(solution.values()));
    let game = mapped.game();
    let mut facing_allin = 0;
    for decision in decisions(game) {
        assert_eq!(mapped.holds(decision.node), decision.street() == 0);
        let measured = mapped.measured_evs().get(decision.node);
        if decision.street() > 0 {
            let width = decision.actions.len() as f32;
            let row = mapped.strategy().hand(decision.node as usize, 0).unwrap();
            assert!(row.iter().all(|&p| p == 1.0 / width));
            assert!(measured.is_none());
            continue;
        }
        // On the turn, a node is measured when no river decision lies below it: facing an
        // all-in, whose call runs the river out.
        let all_in = decision
            .path
            .last()
            .and_then(|a| a.amount())
            .is_some_and(|amount| amount == spot.effective_stack);
        assert_eq!(measured.is_some(), all_in, "{}", PathText(&decision.path));
        facing_allin += usize::from(all_in);
    }
    assert!(facing_allin > 0);
    assert!(!mapped.holds(game.num_nodes() as u32));
}

#[test]
fn a_missing_card_is_named_with_the_cards_that_can_fall() {
    let spot = turn_spot();
    let mut dump = SolutionDump::from_solution(&solved(&spot));
    let line = [DumpAction::Check, DumpAction::Check];
    let dropped: Vec<Card> = ["3c", "4c", "5c"]
        .iter()
        .map(|c| c.parse().unwrap())
        .collect();
    dump.nodes.retain(|node| {
        !(node.path.starts_with(&line)
            && node.path.len() > 2
            && dropped.contains(&node.path[2].card().unwrap()))
    });
    let found = mismatches(&dump, &spot);
    assert_eq!(
        found,
        [Mismatch::MissingCards {
            path: line.to_vec(),
            street: Street::River,
            cards: dropped,
            of: 48,
        }]
    );
    assert_eq!(
        found[0].to_string(),
        "after [check, check], our tree deals the river and has a decision after each of the 48 \
         cards that can fall there; the dump has no node after 3 of them: 3c, 4c, 5c; a dump \
         covers each street it holds in full, after every card that can fall"
    );
    // Without any river after the line, it says so.
    dump.nodes
        .retain(|node| !(node.path.starts_with(&line) && node.path.len() > 2));
    let found = mismatches(&dump, &spot);
    assert!(
        found[0]
            .to_string()
            .contains("the dump has no node after any of them"),
        "{}",
        found[0]
    );
}
