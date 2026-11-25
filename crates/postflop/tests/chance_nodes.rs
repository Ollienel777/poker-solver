//! The game of a flop or turn spot, its chance nodes and its terminals, checked against
//! definitions that share nothing with the library but the evaluator, the card types and the
//! public tree's shape.
//!
//! Two small raked spots in `tests/data` are the cases: `flop_small`, whose all-in is offered on
//! the flop and whose later bets reach the stack, and `turn_small`, with all-in on the turn and
//! the river. Their ranges share combos and cards.
//!
//! - **The public tree.** Every node follows its betting node, and every chance node deals each
//!   card off its board once, in card order, ruling out exactly the hands that hold it.
//! - **Equity.** When both players only check, each hand's value at the root is the pot less
//!   its rake times the hand's exact equity against the other range, from `pk_core`'s
//!   enumeration over every runout.
//! - **Chance.** At every chance node, for every pair of hands that can be held together there,
//!   the outcomes that rule out neither have probabilities that sum to 1.
//! - **Terminals.** Folds on the flop and the turn, and all-ins called on the flop and the turn
//!   and run out to the river, against double loops over the hands and the runouts, with
//!   payoffs worked out here from the chips and the rake.
//! - **A scalar game.** Both players' values under a random profile, against the expected
//!   payoff of every pair of hands, walked line by line and card by card.

use std::cmp::Ordering;

use pk_cfr::{
    PublicGame, PublicNode, PublicProfile, vector_expected_value, vector_expected_values,
};
use pk_core::{Card, CardSet, Combo, Range, evaluate, range_vs_range};
use pk_game::{Node, Outcome, Player, PostflopTree, Street};
use pk_postflop::{PostflopGame, Spot, node_values};
use rand_chacha::ChaCha8Rng;
use rand_chacha::rand_core::{Rng, SeedableRng};

fn load(name: &str) -> Spot {
    let path = format!("{}/tests/data/{name}.toml", env!("CARGO_MANIFEST_DIR"));
    Spot::load(path).unwrap()
}

fn game(spot: &Spot) -> PostflopGame {
    PostflopGame::from_spot(spot).unwrap()
}

/// Every public node, as an index.
fn nodes(game: &PostflopGame) -> std::ops::Range<u32> {
    0..game.num_nodes() as u32
}

/// A uniform number in `[0, 1)`.
fn unit(rng: &mut ChaCha8Rng) -> f64 {
    (rng.next_u64() >> 11) as f64 / (1u64 << 53) as f64
}

/// A profile that plays action 0 with every hand at every node: check where there is no bet to
/// face, so that no bet is ever made and every hand is checked down.
fn always_first(game: &PostflopGame) -> PublicProfile {
    PublicProfile::from_fn(game, |node, _| {
        let PublicNode::Player { num_actions, .. } = game.node(node) else {
            unreachable!();
        };
        let mut row = vec![0.0; num_actions];
        row[0] = 1.0;
        row
    })
}

/// A random profile, every probability positive.
fn random_profile(game: &PostflopGame, seed: u64) -> PublicProfile {
    let mut rng = ChaCha8Rng::seed_from_u64(seed);
    PublicProfile::from_fn(game, |node, _| {
        let PublicNode::Player { num_actions, .. } = game.node(node) else {
            unreachable!();
        };
        let raw: Vec<f64> = (0..num_actions).map(|_| 0.05 + unit(&mut rng)).collect();
        let total: f64 = raw.iter().sum();
        raw.iter().map(|r| (r / total) as f32).collect()
    })
}

/// A random reach vector with no zeros.
fn random_reach(rng: &mut ChaCha8Rng, len: usize) -> Vec<f64> {
    (0..len).map(|_| 0.1 + unit(rng)).collect()
}

/// The rake on a pot of `pot` chips, by the spot's settings, worked out here.
fn rake_on(spot: &Spot, pot: f64) -> f64 {
    if spot.rake_percent == 0.0 {
        0.0
    } else {
        (spot.rake_percent * pot / 100.0).min(f64::from(spot.rake_cap))
    }
}

/// The payoff of `me`, holding `mine` against `theirs`, at the betting terminal `terminal`,
/// with the five-card `board` for a showdown: worked out from the chips and the rake. The
/// uncalled part of a bet goes back unraked.
fn payoff(
    spot: &Spot,
    tree: &PostflopTree,
    terminal: u32,
    me: Player,
    mine: Combo,
    theirs: Combo,
    board: CardSet,
) -> f64 {
    let Node::Terminal {
        outcome,
        contributions,
        ..
    } = tree.node(terminal)
    else {
        panic!("node {terminal} is not a terminal");
    };
    let pot = f64::from(spot.pot);
    let called = f64::from(contributions[0].min(contributions[1]));
    let rake = rake_on(spot, pot + 2.0 * called);
    let win = pot + called - rake;
    match outcome {
        Outcome::Fold { winner } if *winner == me => win,
        Outcome::Fold { .. } => -called,
        Outcome::Showdown => {
            assert_eq!(board.len(), 5, "a showdown is on the river");
            let strength = |combo: Combo| evaluate(board | combo.cards());
            match strength(mine).cmp(&strength(theirs)) {
                Ordering::Greater => win,
                Ordering::Equal => (pot - rake) / 2.0,
                Ordering::Less => -called,
            }
        }
    }
}

/// The expected payoff of `me` at the betting terminal `terminal` when the cards still to come
/// are dealt to `board` from those that neither hand holds, each card equally likely.
fn runout_payoff(
    spot: &Spot,
    tree: &PostflopTree,
    terminal: u32,
    me: Player,
    hands: [Combo; 2],
    board: CardSet,
) -> f64 {
    if board.len() == 5 {
        return payoff(spot, tree, terminal, me, hands[0], hands[1], board);
    }
    let live = CardSet::FULL - board - hands[0].cards() - hands[1].cards();
    let total: f64 = live
        .iter()
        .map(|card| {
            runout_payoff(
                spot,
                tree,
                terminal,
                me,
                hands,
                board | CardSet::single(card),
            )
        })
        .sum();
    total / live.len() as f64
}

/// `Σ reach[o] · u(h, o)` for each hand `h` of `me` at the public node `node`, over the
/// opponent's hands that share no card with `h` or the board there, where `u(h, o)` is `h`'s
/// expected payoff at the betting terminal `terminal` over the runouts still to come, and 0 for
/// a hand that holds a board card: the double loop.
fn double_loop(
    spot: &Spot,
    game: &PostflopGame,
    node: u32,
    terminal: u32,
    me: Player,
    reach: &[f64],
) -> Vec<f64> {
    let board = game.board(node);
    let (mine, theirs) = (game.hands(me), game.hands(me.opponent()));
    mine.combos()
        .iter()
        .map(|&a| {
            if a.cards().intersects(board) {
                return 0.0;
            }
            theirs
                .combos()
                .iter()
                .zip(reach)
                .filter(|(b, _)| !b.cards().intersects(a.cards() | board))
                .map(|(&b, r)| r * runout_payoff(spot, game.tree(), terminal, me, [a, b], board))
                .sum()
        })
        .collect()
}

/// The values of `player` at the public node `node`, below which no player acts, for the
/// opponent's reach `reach`: the terminal's values, or at a chance node the sum over its
/// outcomes, each with the opponent's reach times the outcome's probability and zero for the
/// hands it rules out, as the solver walks it.
fn values_below(
    game: &PostflopGame,
    node: usize,
    player: usize,
    reach: &[f64],
    possible: &[bool],
) -> Vec<f64> {
    match game.node(node) {
        PublicNode::Terminal => {
            let mut values = vec![f64::NAN; possible.len()];
            game.terminal_values(node, player, reach, &mut values);
            for (value, &p) in values.iter_mut().zip(possible) {
                if !p {
                    *value = 0.0;
                }
            }
            values
        }
        PublicNode::Chance { num_outcomes } => {
            let mut values = vec![0.0; possible.len()];
            for outcome in 0..num_outcomes {
                let probability = game.outcome_probability(node, outcome);
                let after: Vec<bool> = (0..possible.len())
                    .map(|h| possible[h] && !game.rules_out(node, outcome, player, h))
                    .collect();
                let other: Vec<f64> = (0..reach.len())
                    .map(|o| {
                        if game.rules_out(node, outcome, 1 - player, o) {
                            0.0
                        } else {
                            reach[o] * probability
                        }
                    })
                    .collect();
                let below = values_below(game, game.child(node, outcome), player, &other, &after);
                for (value, v) in values.iter_mut().zip(below) {
                    *value += v;
                }
            }
            values
        }
        PublicNode::Player { .. } => panic!("node {node}: a player acts after an all-in call"),
    }
}

#[test]
fn the_public_tree_deals_every_card_that_can_fall_and_follows_the_betting_tree() {
    // The sizes, counted by hand from the tree rules. On `turn_small`, the turn has 8 player
    // nodes, 6 folds and 7 chance nodes. Three lead to a river with betting: 21 nodes after a
    // check-check, 9 after a bet and a call, where the river bet is all-in. Four follow an
    // all-in called, and lead straight to a showdown. So 8 + 6 + (1 + 48 · 21) + 2 · (1 + 48 · 9)
    // + 4 · (1 + 48) = 2,085. On `flop_small` the flop has 6 player nodes, 4 folds and 5 chance
    // nodes; a turn after a check-check has 1,305 nodes below its card, one after a bet and a
    // call 537, and an all-in runs out through 1 + 49 · 49 = 2,402. So 10 + (1 + 49 · 1,305) +
    // 2 · (1 + 49 · 537) + 2 · 2,402 = 121,388.
    for (name, size) in [("flop_small", 121_388), ("turn_small", 2_085)] {
        let spot = load(name);
        let game = game(&spot);
        let tree = game.tree();
        assert_eq!(game.board(0), spot.board, "{name}");
        assert_eq!(game.tree_node(0), PostflopTree::ROOT);
        let mut chance = 0;
        for node in nodes(&game) {
            let board = game.board(node);
            let children = game.children(node);
            // Children come after their parent, in order, the first one next.
            assert!(children.windows(2).all(|pair| pair[0] < pair[1]));
            if let Some(&first) = children.first() {
                assert_eq!(first, node + 1, "{name}: node {node}");
            }
            match tree.node(game.tree_node(node)) {
                Node::Player {
                    actor,
                    actions,
                    children: betting,
                    ..
                } => {
                    assert_eq!(game.actor(node), Some(*actor));
                    assert_eq!(game.actions(node), actions.as_slice());
                    assert_eq!(
                        game.node(node as usize),
                        PublicNode::Player {
                            player: actor.index(),
                            num_actions: actions.len()
                        }
                    );
                    assert_eq!(children.len(), betting.len());
                    for (&child, &next) in children.iter().zip(betting) {
                        assert_eq!(game.tree_node(child), next);
                        assert_eq!(game.board(child), board);
                    }
                }
                Node::Chance { street, child } => {
                    chance += 1;
                    let before = match street {
                        Street::Turn => 3,
                        Street::River => 4,
                        Street::Flop => unreachable!("no chance node deals the flop"),
                    };
                    assert_eq!(board.len(), before, "{name}: node {node}");
                    // The outcomes are the cards off the board, in card order.
                    let cards: Vec<Card> = Card::all().filter(|&c| !board.contains(c)).collect();
                    assert_eq!(game.outcomes(node), cards.as_slice(), "{name}: node {node}");
                    assert_eq!(children.len(), cards.len());
                    for (outcome, (&card, &next)) in cards.iter().zip(children).enumerate() {
                        assert_eq!(game.tree_node(next), *child);
                        assert_eq!(game.board(next), board | CardSet::single(card));
                        assert_eq!(game.card_child(node, card), Some(next));
                        for player in [Player::Oop, Player::Ip] {
                            for (hand, combo) in game.hands(player).combos().iter().enumerate() {
                                let out =
                                    game.rules_out(node as usize, outcome, player.index(), hand);
                                assert_eq!(out, combo.cards().contains(card));
                            }
                        }
                    }
                    for card in board {
                        assert_eq!(game.card_child(node, card), None);
                    }
                }
                Node::Terminal { .. } => {
                    assert!(children.is_empty());
                    assert_eq!(game.node(node as usize), PublicNode::Terminal);
                    assert!(game.payoff(node).is_some());
                }
            }
        }
        println!(
            "{name}: {} betting nodes, {} public nodes, {chance} chance nodes",
            tree.nodes().len(),
            game.num_nodes()
        );
        assert_eq!(game.num_nodes(), size, "{name}");
    }
}

#[test]
fn a_river_spot_has_the_betting_tree_for_its_public_tree() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../reference/spots/river/deep_high_spr.toml"
    );
    let spot = Spot::load(path).unwrap();
    let game = game(&spot);
    assert_eq!(game.num_nodes(), game.tree().nodes().len());
    for node in nodes(&game) {
        assert_eq!(game.tree_node(node), node);
        assert_eq!(game.board(node), spot.board);
        assert_eq!(game.children(node), game.tree().node(node).children());
    }
}

#[test]
fn chance_is_a_distribution_over_the_cards_that_can_fall() {
    // For every pair of hands that can be held together at a chance node, the outcomes that
    // rule out neither sum to 1, and every outcome has the same probability.
    for name in ["flop_small", "turn_small"] {
        let spot = load(name);
        let game = game(&spot);
        let (oop, ip) = (game.hands(Player::Oop), game.hands(Player::Ip));
        let (mut pairs, mut chance_nodes) = (0u64, 0);
        for node in nodes(&game) {
            let PublicNode::Chance { num_outcomes } = game.node(node as usize) else {
                continue;
            };
            chance_nodes += 1;
            let board = game.board(node);
            let first = game.outcome_probability(node as usize, 0);
            for outcome in 0..num_outcomes {
                assert_eq!(game.outcome_probability(node as usize, outcome), first);
            }
            for (h, a) in oop.combos().iter().enumerate() {
                for (o, b) in ip.combos().iter().enumerate() {
                    if a.cards().intersects(b.cards() | board) || b.cards().intersects(board) {
                        continue;
                    }
                    let total: f64 = (0..num_outcomes)
                        .filter(|&outcome| {
                            !game.rules_out(node as usize, outcome, 0, h)
                                && !game.rules_out(node as usize, outcome, 1, o)
                        })
                        .map(|outcome| game.outcome_probability(node as usize, outcome))
                        .sum();
                    assert!(
                        (total - 1.0).abs() < 1e-12,
                        "{name}: node {node}, {a} and {b}: {total}"
                    );
                    pairs += 1;
                }
            }
        }
        assert!(chance_nodes > 0 && pairs > 0, "{name}");
        println!("{name}: {pairs} pairs of hands over {chance_nodes} chance nodes");
    }
}

#[test]
fn a_check_down_is_worth_the_pot_times_the_exact_equity() {
    // The issue's own check. With every bet left out, each hand wins the pot less its rake
    // with its equity against the other range, a tie splitting it, as pk_core's exact
    // enumeration over every runout gives it, card removal included.
    for name in ["flop_small", "turn_small"] {
        let spot = load(name);
        let game = game(&spot);
        let profile = always_first(&game);
        let pot = f64::from(spot.pot);
        let net = pot - rake_on(&spot, pot);
        let evs = node_values(&game, &profile);
        for player in [Player::Oop, Player::Ip] {
            let (mine, theirs) = (game.hands(player), game.hands(player.opponent()));
            let deal = game.deal(player.opponent().index());
            let q = vector_expected_value(&game, &profile, player.index()).hand_values;
            for (h, &combo) in mine.combos().iter().enumerate() {
                // The hand's expected payoff: its counterfactual value over the opponent's
                // deal that it can be dealt with.
                let met: f64 = theirs
                    .combos()
                    .iter()
                    .zip(deal)
                    .filter(|(b, _)| !combo.cards().intersects(b.cards()))
                    .map(|(_, d)| d)
                    .sum();
                let value = q[h] / met;
                let mut hand = Range::empty();
                hand.set(combo, 1.0);
                let equity =
                    range_vs_range(&hand, spot.range(player.opponent()), spot.board).unwrap();
                let expected = net * equity.share();
                assert!(
                    (value - expected).abs() < 1e-9 * net,
                    "{name}: {player} {combo}: {value} against {expected}"
                );
                if player == Player::Oop {
                    let ev = evs.get(0).unwrap()[h].unwrap();
                    assert!((ev - expected).abs() < 1e-9 * net, "{name}: {combo}: {ev}");
                }
            }
        }
    }
}

#[test]
fn folds_on_the_flop_and_the_turn_equal_the_double_loop() {
    // Raked folds on the flop and the turn of the flop spot, and on the turn and the river of
    // the turn spot. Below a card, the opponent's reach on the hands that hold it is not zero
    // here, and must be left out.
    let cases = [
        ("flop_small", "b50 f"),
        ("flop_small", "x a f"),
        ("flop_small", "x x 7h b50 f"),
        ("flop_small", "b50 c 8h x a f"),
        ("turn_small", "b50 a f"),
        ("turn_small", "x x Qs b75 f"),
    ];
    let mut rng = ChaCha8Rng::seed_from_u64(36);
    for (name, path) in cases {
        let spot = load(name);
        let game = game(&spot);
        let node = game.find(path).unwrap();
        let terminal = game.tree_node(node);
        assert!(
            matches!(
                game.tree().node(terminal),
                Node::Terminal {
                    outcome: Outcome::Fold { .. },
                    ..
                }
            ),
            "{name}: `{path}`"
        );
        for me in [Player::Oop, Player::Ip] {
            let reach = random_reach(&mut rng, game.hands(me.opponent()).len());
            let mut values = vec![f64::NAN; game.hands(me).len()];
            game.terminal_values(node as usize, me.index(), &reach, &mut values);
            let expected = double_loop(&spot, &game, node, terminal, me, &reach);
            for (h, (got, want)) in values.iter().zip(&expected).enumerate() {
                assert!(
                    (got - want).abs() < 1e-9,
                    "{name}: `{path}`, {me} hand {h}: {got} against {want}"
                );
            }
        }
    }
}

#[test]
fn all_ins_called_on_the_flop_and_the_turn_equal_the_double_loop_over_the_runouts() {
    // An all-in called before the river runs out through the chance nodes to a showdown on
    // every river. Its value at the chance node after the call, for every hand, against the
    // double loop over the opponent's hands and every runout of the cards neither holds.
    let cases = [
        ("flop_small", "a c"),
        ("flop_small", "x a c"),
        ("flop_small", "b50 c 8h a c"),
        ("turn_small", "a c"),
        ("turn_small", "b50 a c"),
    ];
    let mut rng = ChaCha8Rng::seed_from_u64(37);
    for (name, path) in cases {
        let spot = load(name);
        let game = game(&spot);
        let node = game.find(path).unwrap();
        assert!(matches!(
            game.node(node as usize),
            PublicNode::Chance { .. }
        ));
        // The showdown every runout ends at.
        let mut end = node;
        while let Some(&child) = game.children(end).first() {
            end = child;
        }
        let terminal = game.tree_node(end);
        let Node::Terminal {
            outcome: Outcome::Showdown,
            contributions,
            ..
        } = game.tree().node(terminal)
        else {
            panic!("{name}: `{path}` does not run out to a showdown");
        };
        assert_eq!(contributions[0], spot.effective_stack, "{name}: `{path}`");
        for me in [Player::Oop, Player::Ip] {
            let reach = random_reach(&mut rng, game.hands(me.opponent()).len());
            let board = game.board(node);
            let possible: Vec<bool> = game
                .hands(me)
                .combos()
                .iter()
                .map(|c| !c.cards().intersects(board))
                .collect();
            let blocked: Vec<f64> = game
                .hands(me.opponent())
                .combos()
                .iter()
                .zip(&reach)
                .map(|(c, &r)| if c.cards().intersects(board) { 0.0 } else { r })
                .collect();
            let values = values_below(&game, node as usize, me.index(), &blocked, &possible);
            let expected = double_loop(&spot, &game, node, terminal, me, &reach);
            for (h, (got, want)) in values.iter().zip(&expected).enumerate() {
                assert!(
                    (got - want).abs() < 1e-9 * want.abs().max(1.0),
                    "{name}: `{path}`, {me} hand {h}: {got} against {want}"
                );
            }
        }
    }
}

/// The payoff of `me` from the public node `node` on when OOP holds `hands[0]` and IP
/// `hands[1]`, by hand indices `index`, both following `profile`: every line and every card,
/// one at a time.
fn pair_value(
    spot: &Spot,
    game: &PostflopGame,
    profile: &PublicProfile,
    node: u32,
    me: Player,
    hands: [Combo; 2],
    index: [usize; 2],
) -> f64 {
    match game.node(node as usize) {
        PublicNode::Player { player, .. } => {
            let row = profile.hand(node as usize, index[player]).unwrap();
            game.children(node)
                .iter()
                .zip(row)
                .map(|(&child, p)| {
                    f64::from(p) * pair_value(spot, game, profile, child, me, hands, index)
                })
                .sum()
        }
        PublicNode::Chance { .. } => {
            let board = game.board(node);
            let live = CardSet::FULL - board - hands[0].cards() - hands[1].cards();
            let total: f64 = live
                .iter()
                .map(|card| {
                    let child = game.card_child(node, card).unwrap();
                    pair_value(spot, game, profile, child, me, hands, index)
                })
                .sum();
            total / live.len() as f64
        }
        PublicNode::Terminal => {
            let (mine, theirs) = (hands[me.index()], hands[me.opponent().index()]);
            let terminal = game.tree_node(node);
            payoff(
                spot,
                game.tree(),
                terminal,
                me,
                mine,
                theirs,
                game.board(node),
            )
        }
    }
}

#[test]
fn values_under_a_random_profile_equal_the_scalar_game() {
    // Each player's value, as the vector walk of the public tree gives it, against the
    // average over every pair of hands that can be dealt, weighted by the product of the range
    // weights, of the pair's expected payoff from the root.
    for (name, seed) in [("turn_small", 1), ("flop_small", 2)] {
        let spot = load(name);
        let game = game(&spot);
        let profile = random_profile(&game, seed);
        let (oop, ip) = (game.hands(Player::Oop), game.hands(Player::Ip));
        let (mut totals, mut weight) = ([0.0; 2], 0.0);
        for (h, &a) in oop.combos().iter().enumerate() {
            for (o, &b) in ip.combos().iter().enumerate() {
                if a.cards().intersects(b.cards()) {
                    continue;
                }
                let w = oop.weights()[h] * ip.weights()[o];
                weight += w;
                for me in [Player::Oop, Player::Ip] {
                    totals[me.index()] +=
                        w * pair_value(&spot, &game, &profile, 0, me, [a, b], [h, o]);
                }
            }
        }
        let scalar = totals.map(|total| total / weight);
        let vector = vector_expected_values(&game, &profile);
        for player in 0..2 {
            assert!(
                (scalar[player] - vector[player]).abs() < 1e-9 * f64::from(spot.pot),
                "{name}: player {player}: {} against {}",
                vector[player],
                scalar[player]
            );
        }
        println!("{name}: values {vector:?}, scalar {scalar:?}");
    }
}
