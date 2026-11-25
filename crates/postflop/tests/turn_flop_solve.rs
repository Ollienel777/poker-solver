//! Solves of spots that start on the turn and the flop: they reach their target, keep every
//! invariant, and read out by node path at any decision node on any street.
//!
//! The spots are the two of `tests/data`, which `chance_nodes.rs` checks the game of. The turn
//! spot solves in the fast suite. The flop spot's solve takes several seconds and is a slow
//! test, `the_flop_spot_solves_to_its_target_and_keeps_every_invariant`, which also prints the
//! size of both trees, the iterations, the time and the exploitability.

use std::time::Instant;

use pk_cfr::{PublicGame, PublicNode};
use pk_core::CardSet;
use pk_game::{Player, Street};
use pk_postflop::{
    NodePathErrorKind, PostflopGame, Solution, SolveError, SolveOptions, Spot, StopReason,
    check_solution, expected_rake, exploitability, node_values, solve,
};

fn load(name: &str) -> Spot {
    let path = format!("{}/tests/data/{name}.toml", env!("CARGO_MANIFEST_DIR"));
    Spot::load(path).unwrap()
}

/// The checks every solve here must pass: its invariants, measured again in release too, and
/// the exploitability it reports, measured again.
fn assert_sound(solution: &Solution, name: &str) {
    let report = check_solution(solution);
    assert!(report.is_valid(), "{name}: {report}");
    let value_sum = report.value_sum.expect("the value sum is checked");
    assert!(value_sum.rake > 0.0, "{name}: the spot is raked");
    assert_eq!(
        &exploitability(solution.game(), solution.strategy()),
        solution.exploitability(),
        "{name}"
    );
    for gain in solution.exploitability().gains() {
        assert!(gain > -1e-9, "{name}: {:?}", solution.exploitability());
    }
}

/// The public tree's size: nodes, player nodes, and the `f32` entries the solver stores for
/// them, regrets and strategy sums, one per action and hand of the acting player.
fn size(game: &PostflopGame) -> (usize, usize, usize) {
    let mut player = 0;
    let mut entries = 0;
    for node in 0..game.num_nodes() {
        if let PublicNode::Player {
            player: actor,
            num_actions,
        } = game.node(node)
        {
            player += 1;
            entries += 2 * num_actions * game.num_hands(actor);
        }
    }
    (game.num_nodes(), player, entries)
}

#[test]
fn the_turn_spot_solves_to_its_target_and_keeps_every_invariant() {
    let spot = load("turn_small");
    let solution = solve(&spot, &SolveOptions::default(), |_| {}).unwrap();
    assert_eq!(solution.stop(), StopReason::Target);
    let measured = solution.exploitability();
    assert!(measured.percent_of_pot <= 0.3, "{measured:?}");
    assert_sound(&solution, "turn_small");
    // The uniform strategy the solve starts from is far from it: both players gain by
    // deviating, by more than 4% of the pot between them.
    let start = SolveOptions {
        max_iterations: 0,
        ..SolveOptions::default()
    };
    let uniform = solve(&spot, &start, |_| {}).unwrap();
    assert_sound(&uniform, "turn_small, uniform");
    let gains = uniform.exploitability().gains();
    assert!(gains.iter().all(|&g| g > 10.0), "{gains:?}");
    assert!(uniform.exploitability().percent_of_pot > 4.0, "{gains:?}");
    println!(
        "turn_small, uniform: gains {gains:?}, {:.2}% of the pot",
        uniform.exploitability().percent_of_pot
    );
    let (nodes, player, entries) = size(solution.game());
    println!(
        "turn_small: {nodes} public nodes, {player} player nodes, {entries} stored values; \
         {} iterations to {:.4}%",
        solution.iterations(),
        measured.percent_of_pot
    );
}

#[test]
fn the_root_evs_weighted_by_the_deal_are_the_players_values_on_the_turn() {
    // The EVs at every player node walk the chance nodes too: at the root, weighted by the
    // probability of each hand, they give each player's value.
    let spot = load("turn_small");
    let options = SolveOptions {
        target: None,
        max_iterations: 100,
        ..SolveOptions::default()
    };
    let solution = solve(&spot, &options, |_| {}).unwrap();
    let game = solution.game();
    let values = node_values(game, solution.strategy());
    let (oop, ip) = (game.hands(Player::Oop), game.hands(Player::Ip));
    let root = values.get(0).unwrap();
    let total: f64 = oop
        .combos()
        .iter()
        .enumerate()
        .map(|(h, a)| {
            let met: f64 = ip
                .combos()
                .iter()
                .zip(game.deal(1))
                .filter(|(b, _)| !a.cards().intersects(b.cards()))
                .map(|(_, d)| d)
                .sum();
            game.deal(0)[h] * met * root[h].unwrap()
        })
        .sum();
    assert!(
        (total - solution.values()[0]).abs() < 1e-9,
        "{total} against {:?}",
        solution.values()
    );
    // Below a card, the hands that hold it have no EV.
    let river = game.find("x x Qs").unwrap();
    let evs = values.get(river).unwrap();
    for (combo, ev) in game.hands(Player::Oop).combos().iter().zip(evs) {
        let holds = combo.cards().contains("Qs".parse().unwrap());
        assert_eq!(ev.is_none(), holds, "{combo}");
    }
}

#[test]
fn read_outs_below_a_card_follow_their_definitions() {
    // At OOP's river node after a check-check on the turn and the queen of spades, every
    // read-out against its definition summed hand by hand here, from the strategy rows alone.
    let spot = load("turn_small");
    let options = SolveOptions {
        target: None,
        max_iterations: 150,
        ..SolveOptions::default()
    };
    let solution = solve(&spot, &options, |_| {}).unwrap();
    let game = solution.game();
    let queen: CardSet = "Qs".parse().unwrap();
    let after_check = game.find("x").unwrap();
    let chance = game.find("x x").unwrap();
    let node = game.find("x x Qs").unwrap();
    assert_eq!(game.street(node), Street::River);
    assert_eq!(game.actor(node), Some(Player::Oop));
    assert_eq!(game.board(node), spot.board | queen);
    // Own reaches: OOP checked at the root, IP checked behind, and the queen fell.
    let reach_of = |player: Player, at: u32| -> Vec<f64> {
        game.hands(player)
            .combos()
            .iter()
            .enumerate()
            .map(|(h, combo)| {
                if combo.cards().intersects(queen) {
                    0.0
                } else {
                    f64::from(solution.hand_strategy(at, h).unwrap()[0])
                }
            })
            .collect()
    };
    let own = [reach_of(Player::Oop, 0), reach_of(Player::Ip, after_check)];
    assert_eq!(solution.reach(node), own);
    assert!(own[0].contains(&0.0) && own[1].iter().all(|&r| r > 0.0));
    let (oop, ip) = (game.hands(Player::Oop), game.hands(Player::Ip));
    // The probability of reaching the node: the pairs that play to it, times the 1/44 chance
    // of the queen among the cards neither holds.
    let mut pairs = 0.0;
    let mut total = 0.0;
    let mut met = vec![0.0; oop.len()];
    for (h, a) in oop.combos().iter().enumerate() {
        for (o, b) in ip.combos().iter().enumerate() {
            if !a.cards().intersects(b.cards()) {
                let w = oop.weights()[h] * ip.weights()[o];
                total += w;
                pairs += w * own[0][h] * own[1][o];
                met[h] += ip.weights()[o] * own[1][o];
            }
        }
    }
    let expected = pairs / total / 44.0;
    let found = solution.reach_probability(node);
    assert!(
        (found - expected).abs() < 1e-14,
        "{found} against {expected}"
    );
    // Over every river card, the chance node's reach.
    let split: f64 = game
        .children(chance)
        .iter()
        .map(|&child| solution.reach_probability(child))
        .sum();
    let whole = solution.reach_probability(chance);
    assert!((split - whole).abs() < 1e-12, "{split} against {whole}");
    // The range view and the probability of each action, by their definitions.
    let strategy = solution.strategy().get(node as usize).unwrap();
    let n = oop.len();
    let actions = game.children(node).len();
    let (mut view, mut played) = (vec![0.0; actions], vec![0.0; actions]);
    let (mut view_total, mut played_total) = (0.0, 0.0);
    for h in 0..n {
        let w = oop.weights()[h] * own[0][h];
        view_total += w;
        played_total += w * met[h];
        for a in 0..actions {
            let p = f64::from(strategy[a * n + h]);
            view[a] += w * p;
            played[a] += w * met[h] * p;
        }
    }
    let shown = solution.range_view_frequencies(node).unwrap();
    let taken = solution.action_frequencies(node).unwrap();
    for a in 0..actions {
        assert!((shown[a] - view[a] / view_total).abs() < 1e-12, "{shown:?}");
        assert!(
            (taken[a] - played[a] / played_total).abs() < 1e-12,
            "{taken:?}"
        );
    }
    let weights = solution.range_view_weights(node).unwrap();
    for (combo, w) in oop.combos().iter().zip(&weights) {
        if combo.cards().intersects(queen) {
            assert_eq!(*w, 0.0, "{combo}");
        }
    }
    let classes = solution.range_view_classes(node).unwrap();
    let shares: f64 = classes.iter().map(|c| c.share).sum();
    assert!((shares - 1.0).abs() < 1e-12);
    // A player node's reach is split among its children, at every node of the tree.
    for at in 0..game.num_nodes() as u32 {
        let children = game.children(at);
        if children.is_empty() {
            continue;
        }
        let whole = solution.reach_probability(at);
        let split: f64 = children
            .iter()
            .map(|&child| solution.reach_probability(child))
            .sum();
        assert!(
            (split - whole).abs() < 1e-6,
            "node {at}: {split} against {whole}"
        );
    }
    assert!((solution.reach_probability(0) - 1.0).abs() < 1e-12);
}

#[test]
fn node_paths_name_every_node_with_its_cards() {
    for name in ["turn_small", "flop_small"] {
        let game = PostflopGame::from_spot(&load(name)).unwrap();
        for node in 0..game.num_nodes() as u32 {
            let path = game.node_path(node);
            assert_eq!(game.find(&path), Ok(node), "{name}: `{path}`");
        }
    }
    let game = PostflopGame::from_spot(&load("flop_small")).unwrap();
    let node = game.find("x b50 c 8h x").unwrap();
    assert_eq!(game.node_path(node), "x b50 c 8h x");
    assert_eq!(game.find("  x b50  c 8H x "), Ok(node));
    assert_eq!(game.street(node), Street::Turn);
    assert_eq!(game.actor(node), Some(Player::Ip));
    assert_eq!(game.board(node), "Ks7d2c8h".parse().unwrap());
    let river = game.find("x x 8h x x 3d").unwrap();
    assert_eq!(game.board(river), "Ks7d2c8h3d".parse().unwrap());
    assert_eq!(game.find(""), Ok(0));
    // What can go wrong, and what the error says.
    let error = game.find("x x 7d").unwrap_err();
    assert_eq!(
        error.kind,
        NodePathErrorKind::OnBoard {
            card: "7d".parse().unwrap(),
            board: "Ks7d2c".parse().unwrap()
        }
    );
    assert_eq!(
        error.to_string(),
        "`7d` cannot fall after `x x`: the 7d is on the board, Ks7d2c"
    );
    let error = game.find("x x 8h x x 8h").unwrap_err();
    assert_eq!(error.before, ["x", "x", "8h", "x", "x"]);
    let error = game.find("x x b50").unwrap_err();
    assert_eq!(
        error.kind,
        NodePathErrorKind::NotACard {
            street: Street::Turn
        }
    );
    assert_eq!(
        error.to_string(),
        "`b50` is not a card; `x x` deals the turn, so the next token is the card that falls, \
         such as `7s`"
    );
    let error = game.find("x x 8h b75").unwrap_err();
    assert_eq!(
        error.to_string(),
        "`b75` is not an action after `x x 8h`; the actions there are `x` and `b50`"
    );
    let error = game.find("b50 f x").unwrap_err();
    assert_eq!(
        error.to_string(),
        "`x` comes after the end of the hand, at `b50 f`, where IP folds"
    );
}

#[test]
fn the_public_tree_is_refused_over_its_node_limit_before_it_is_built() {
    let spot = load("flop_small");
    let size = 121_388;
    assert_eq!(
        PostflopGame::with_node_limit(&spot, size)
            .unwrap()
            .num_nodes(),
        size as usize
    );
    match PostflopGame::with_node_limit(&spot, size - 1) {
        Err(SolveError::PublicTreeTooLarge { nodes, limit }) => {
            assert_eq!((nodes, limit), (size, size - 1));
        }
        other => panic!("{other:?}"),
    }
    // A flop tree with three sizes, two raises and all-in, 100bb deep: a betting tree well
    // inside its limit, and a public tree far over the default one, refused from the count
    // alone, in no time and memory.
    let mut deep = spot.clone();
    deep.effective_stack = 10_000;
    for sizing in [
        &mut deep.tree.flop,
        &mut deep.tree.turn,
        &mut deep.tree.river,
    ] {
        let sizing = sizing.as_mut().unwrap();
        sizing.bets = vec![0.33, 0.66, 1.0];
        sizing.raises = vec![2.5, 4.0];
        sizing.allin = true;
    }
    deep.tree.max_raises = 2;
    let start = Instant::now();
    let error = solve(&deep, &SolveOptions::default(), |_| {
        panic!("the solve should not start")
    })
    .unwrap_err();
    let SolveError::PublicTreeTooLarge { nodes, limit } = error else {
        panic!("{error}");
    };
    assert_eq!(limit, PostflopGame::DEFAULT_MAX_NODES);
    assert!(nodes > 10 * limit, "{nodes}");
    assert!(start.elapsed().as_secs_f64() < 1.0);
    assert!(error.to_string().contains("more than the limit of 1000000"));
    let betting = deep.build_tree().unwrap().nodes().len();
    println!("a betting tree of {betting} nodes has a public tree of {nodes}");
    // And a limit of 0 is not an option.
    let options = SolveOptions {
        max_nodes: 0,
        ..SolveOptions::default()
    };
    match solve(&spot, &options, |_| {}) {
        Err(SolveError::Options { field, .. }) => assert_eq!(field, "max_nodes"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn the_expected_rake_sums_over_every_runout() {
    // The probability of each terminal of the public tree, by the expected rake's card
    // counting, against the sum over every pair of hands that can reach it, each card dealt
    // on the way at 1/45 on the turn and 1/44 on the river.
    let spot = load("turn_small");
    let options = SolveOptions {
        target: None,
        max_iterations: 30,
        ..SolveOptions::default()
    };
    let solution = solve(&spot, &options, |_| {}).unwrap();
    let game = solution.game();
    let expected = expected_rake(game, solution.strategy());
    let (oop, ip) = (game.hands(Player::Oop), game.hands(Player::Ip));
    let mut total = 0.0;
    for (h, a) in oop.combos().iter().enumerate() {
        for (o, b) in ip.combos().iter().enumerate() {
            if !a.cards().intersects(b.cards()) {
                total += oop.weights()[h] * ip.weights()[o];
            }
        }
    }
    let mut terminals = 0;
    for terminal in &expected.terminals {
        let reach = solution.reach(terminal.node);
        let mut sum = 0.0;
        for (h, a) in oop.combos().iter().enumerate() {
            for (o, b) in ip.combos().iter().enumerate() {
                if !a.cards().intersects(b.cards()) {
                    sum += oop.weights()[h] * reach[0][h] * ip.weights()[o] * reach[1][o];
                }
            }
        }
        let dealt = game.board(terminal.node).len() - spot.board.len();
        let chance = if dealt == 1 { 1.0 / 44.0 } else { 1.0 };
        let probability = sum / total * chance;
        assert!(
            (terminal.probability - probability).abs() < 1e-15,
            "node {}: {} against {probability}",
            terminal.node,
            terminal.probability
        );
        terminals += 1;
    }
    assert_eq!(
        terminals,
        (0..game.num_nodes())
            .filter(|&n| game.node(n) == PublicNode::Terminal)
            .count()
    );
    assert!((expected.probability - 1.0).abs() < 1e-6, "{expected:?}");
    let [v0, v1] = solution.values();
    let off = (v0 + v1 - expected.value_sum()).abs();
    assert!(off < 1e-10, "{off}");
}

#[test]
#[ignore = "slow"]
fn the_flop_spot_solves_to_its_target_and_keeps_every_invariant() {
    // Run with `--nocapture` to see the sizes and the solve.
    for name in ["turn_small", "flop_small"] {
        let spot = load(name);
        let game = PostflopGame::from_spot(&spot).unwrap();
        let (nodes, player, entries) = size(&game);
        let hands = [game.num_hands(0), game.num_hands(1)];
        drop(game);
        let start = Instant::now();
        let mut calls = Vec::new();
        let solution = solve(&spot, &SolveOptions::default(), |p| calls.push(*p)).unwrap();
        let elapsed = start.elapsed().as_secs_f64();
        assert_eq!(solution.stop(), StopReason::Target, "{name}");
        assert!(solution.exploitability().percent_of_pot <= 0.3, "{name}");
        let check = Instant::now();
        assert_sound(&solution, name);
        let checked = check.elapsed().as_secs_f64();
        let report = check_solution(&solution);
        println!(
            "{name}: {nodes} public nodes, {player} player nodes, {entries} stored f32 values \
             ({:.1} MB), hands {hands:?}; {} iterations in {elapsed:.2}s ({:.2} ms each, \
             measurements included), exploitability {:.4}% of the pot; check {checked:.2}s, \
             worst {:?}",
            entries as f64 * 4.0 / 1e6,
            solution.iterations(),
            1000.0 * elapsed / solution.iterations() as f64,
            solution.exploitability().percent_of_pot,
            report.worst
        );
        for progress in &calls {
            println!(
                "  {:>5} iterations {:>8.3}s {:>9.4}%",
                progress.iterations,
                progress.elapsed.as_secs_f64(),
                progress.exploitability.percent_of_pot
            );
        }
    }
}
