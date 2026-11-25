//! The river game: the name the river solver gave the game of a spot, kept for river code.
//!
//! [`RiverGame`] is [`PostflopGame`], the game of a spot on any street, which
//! [`crate::game`] describes. M3 built it for river spots under this name, and the river's
//! tests still call it that (DECISIONS P27). Its constructor [`RiverGame::new`] still takes
//! river spots only, and fails with [`SolveError::NotRiver`](crate::SolveError::NotRiver) on a
//! flop or turn spot, so that code that needs a river game cannot be handed a flop game by
//! accident. [`PostflopGame::from_spot`] takes a spot on any street, as the
//! [solution dump](crate::dump), its [import](crate::import) and the
//! [comparison](mod@crate::compare) do (DECISIONS P30).
//!
//! On a river spot nothing changed: the public tree is the betting tree node for node, every
//! hand is possible at every node, and a solve is the same bit for bit as M3's.

pub use crate::game::{ClassView, PostflopGame};

/// The game of a spot, by the name M3 gave it: see the [module documentation](self).
pub type RiverGame = PostflopGame;

// The river tests below were written in this module, against the names it then imported.
#[cfg(test)]
use crate::game::player_of;
#[cfg(test)]
use crate::payoff::Payoff;
#[cfg(test)]
use crate::solve::SolveError;
#[cfg(test)]
use crate::spot::Spot;
#[cfg(test)]
use pk_cfr::{PublicGame, PublicNode, PublicProfile};
#[cfg(test)]
use pk_core::HandClass;
#[cfg(test)]
use pk_game::{Node, Player};

#[cfg(test)]
mod tests {
    use pk_cfr::{Schedule, VectorCfr};
    use pk_core::evaluate;
    use pk_game::Outcome;

    use super::*;
    use crate::spot::SpotError;

    /// A small raked spot whose ranges share combos and cards.
    const SPOT: &str = r#"format = 1
name = "small"
description = "Illustrative ranges, for the river game's tests."
board = "Qh9c7d4s2h"
pot_bb = 10.0
effective_stack_bb = 25.0
oop_range = "AhAd,AcAs:0.5,KhQd,KcQc:0.75,9s8s"
ip_range = "AhAd:0.5,KsQs,KhQd:0.25,QcJc,9d8d:0.75"

[rake]
percent = 5.0
cap_bb = 1.0

[tree]
max_raises = 1
allin_threshold = 1.0
river = { bets = [0.5], raises = [3.0], allin = false }
"#;

    fn spot() -> Spot {
        Spot::from_toml(SPOT).unwrap()
    }

    fn game() -> RiverGame {
        RiverGame::new(&spot()).unwrap()
    }

    #[test]
    fn the_public_tree_is_the_betting_tree() {
        let game = game();
        let tree = spot().build_tree().unwrap();
        assert_eq!(game.tree(), &tree);
        assert_eq!(game.num_nodes(), tree.nodes().len());
        assert_eq!(game.root(), 0);
        for (index, node) in tree.nodes().iter().enumerate() {
            match node {
                Node::Player {
                    actor,
                    actions,
                    children,
                    ..
                } => {
                    assert_eq!(
                        game.node(index),
                        PublicNode::Player {
                            player: actor.index(),
                            num_actions: actions.len()
                        }
                    );
                    for (action, &child) in children.iter().enumerate() {
                        assert_eq!(game.child(index, action), child as usize);
                    }
                    assert_eq!(game.payoff(index as u32), None);
                }
                Node::Terminal { .. } => {
                    assert_eq!(game.node(index), PublicNode::Terminal);
                    assert!(game.payoff(index as u32).is_some());
                }
                Node::Chance { .. } => panic!("a river tree has no chance node"),
            }
        }
        // The solver accepts the tree and stores the acting player's hands at every node.
        let solver = VectorCfr::new(game.clone(), Schedule::Vanilla);
        for node in 0..game.num_nodes() {
            if let PublicNode::Player { player, .. } = game.node(node) {
                let possible = solver.possible_hands(node).unwrap();
                assert_eq!(possible.len(), game.num_hands(player));
                assert!(possible.iter().all(|&p| p));
            }
        }
    }

    #[test]
    fn the_hands_are_the_hand_lists_and_the_deal_sums_to_one() {
        let game = game();
        let spot = spot();
        for player in [Player::Oop, Player::Ip] {
            let list = game.hands(player);
            assert_eq!(list, spot.board_hands().hands(player));
            assert_eq!(game.num_hands(player.index()), list.len());
        }
        // OOP's five combos and IP's five; none is blocked by the board.
        assert_eq!((game.num_hands(0), game.num_hands(1)), (5, 5));
        let mut total = 0.0;
        let mut pairs = 0.0;
        let (oop, ip) = (game.hands(Player::Oop), game.hands(Player::Ip));
        for (h, a) in oop.combos().iter().enumerate() {
            for (o, b) in ip.combos().iter().enumerate() {
                if !a.cards().intersects(b.cards()) {
                    total += game.deal(0)[h] * game.deal(1)[o];
                    pairs += oop.weights()[h] * ip.weights()[o];
                }
            }
        }
        assert!((total - 1.0).abs() < 1e-15, "{total}");
        assert_eq!(game.pair_weight(), pairs);
        for player in 0..2 {
            let weights = game.hands(player_of(player)).weights();
            for (d, w) in game.deal(player).iter().zip(weights) {
                assert!((d * pairs.sqrt() - w).abs() < 1e-15);
            }
        }
    }

    #[test]
    fn terminal_values_are_the_payoffs_for_each_pair_of_hands() {
        // With all of the opponent's reach on one hand, the value of each of the player's
        // hands is its payoff against that hand, or 0 if they share a card.
        let game = game();
        let board = spot().board;
        for node in 0..game.num_nodes() {
            let Some(payoff) = game.payoff(node as u32) else {
                continue;
            };
            let Node::Terminal { outcome, .. } = game.tree().node(node as u32) else {
                unreachable!();
            };
            for player in 0..2 {
                let (mine, theirs) = (
                    game.hands(player_of(player)),
                    game.hands(player_of(1 - player)),
                );
                for (o, other) in theirs.combos().iter().enumerate() {
                    let mut reach = vec![0.0; theirs.len()];
                    reach[o] = 1.0;
                    let mut values = vec![f64::NAN; mine.len()];
                    game.terminal_values(node, player, &reach, &mut values);
                    for (h, combo) in mine.combos().iter().enumerate() {
                        let expected = if combo.cards().intersects(other.cards()) {
                            0.0
                        } else {
                            match (payoff, outcome) {
                                (Payoff::Fold(payoffs), Outcome::Fold { .. }) => payoffs[player],
                                (Payoff::Showdown(s), Outcome::Showdown) => {
                                    let a = evaluate(board | combo.cards());
                                    let b = evaluate(board | other.cards());
                                    match a.cmp(&b) {
                                        std::cmp::Ordering::Greater => s.win,
                                        std::cmp::Ordering::Equal => s.tie,
                                        std::cmp::Ordering::Less => s.lose,
                                    }
                                }
                                other => panic!("{other:?}"),
                            }
                        };
                        assert_eq!(values[h], expected, "node {node}, {combo} against {other}");
                    }
                }
            }
        }
    }

    #[test]
    fn paths_lead_from_the_root_to_every_node() {
        let game = game();
        assert!(game.path(0).is_empty());
        for node in 0..game.num_nodes() as u32 {
            let mut at = 0;
            for (step, action) in game.path(node) {
                assert_eq!(step, at);
                at = game.tree().node(at).children()[action];
            }
            assert_eq!(at, node);
        }
    }

    #[test]
    fn hand_weights_and_frequencies_follow_the_profile_with_card_removal() {
        let game = game();
        // OOP always checks (action 0) at the root and bets (action 1) facing nothing after
        // a check; IP plays (1/4, 3/4) at its first node with every hand but the last.
        let profile = PublicProfile::from_fn(&game, |node, hand| {
            let PublicNode::Player {
                player,
                num_actions,
            } = game.node(node)
            else {
                unreachable!();
            };
            let mut row = vec![0.0; num_actions];
            if player == 1 && hand + 1 < game.num_hands(1) {
                row[0] = 0.25;
                row[1..].fill(0.75 / (num_actions - 1) as f32);
            } else {
                row[0] = 1.0;
            }
            row
        });
        let after_check = game.tree().node(0).children()[0];
        let reach = game.reach(&profile, after_check);
        assert!(reach[0].iter().all(|&r| r == 1.0));
        assert!(reach[1].iter().all(|&r| r == 1.0));
        // At IP's node, IP's hands are weighted by their range weights and by the OOP weight
        // they can be dealt with.
        let weights = game.hand_weights(&profile, after_check).unwrap();
        let (oop, ip) = (game.hands(Player::Oop), game.hands(Player::Ip));
        let mut expected: Vec<f64> = ip
            .combos()
            .iter()
            .zip(ip.weights())
            .map(|(combo, w)| {
                let met: f64 = oop
                    .combos()
                    .iter()
                    .zip(oop.weights())
                    .filter(|(other, _)| !other.cards().intersects(combo.cards()))
                    .map(|(_, w)| w)
                    .sum();
                w * met
            })
            .collect();
        let total: f64 = expected.iter().sum();
        expected.iter_mut().for_each(|w| *w /= total);
        for (a, b) in weights.iter().zip(&expected) {
            assert!((a - b).abs() < 1e-15, "{weights:?} against {expected:?}");
        }
        let frequencies = game.action_frequencies(&profile, after_check).unwrap();
        let last = *weights.last().unwrap();
        let checks = 0.25 * (1.0 - last) + last;
        assert!((frequencies[0] - checks).abs() < 1e-12, "{frequencies:?}");
        assert!((frequencies.iter().sum::<f64>() - 1.0).abs() < 1e-12);
        // OOP never bets at the root, so the node after its bet is never reached.
        let after_bet = game.tree().node(0).children()[1];
        assert_eq!(game.hand_weights(&profile, after_bet), None);
        assert_eq!(game.action_frequencies(&profile, after_bet), None);
        // A terminal has no acting player.
        let showdown = game.tree().node(after_check).children()[0];
        assert_eq!(game.tree().node(showdown).children(), &[] as &[u32]);
        assert_eq!(game.hand_weights(&profile, showdown), None);
    }

    #[test]
    fn a_board_that_is_not_a_river_is_refused() {
        for (board, cards) in [("Qh9c7d", 3), ("Qh9c7d4s", 4)] {
            // A valid flop or turn spot, with a sizing for every street it plays.
            let mut spot = spot();
            spot.board = board.parse().unwrap();
            let river = spot.tree.river.clone();
            spot.tree.turn = river.clone();
            if cards == 3 {
                spot.tree.flop = river;
            }
            assert!(spot.validate().is_ok(), "{board}");
            match RiverGame::new(&spot) {
                Err(SolveError::NotRiver { cards: found }) => assert_eq!(found, cards),
                other => panic!("{board}: {other:?}"),
            }
        }
    }

    #[test]
    fn the_two_read_outs_of_a_range_are_different_quantities() {
        // They must stay two: the probability of each action once a node is reached, with card
        // removal by the opponent's range, and a range view's frequency over the player's own
        // combos. On `four_flush`, solved to 0.05% of the pot, at IP's node facing OOP's
        // check-raise, they are 10.1% and 18.7% for IP's re-raise. Each is checked against
        // its definition, summed here pair by pair.
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../reference/spots/river/four_flush.toml"
        );
        let options = crate::solve::SolveOptions {
            target: Some(0.05),
            check_every: 1,
            ..crate::solve::SolveOptions::default()
        };
        let solution = crate::solve::solve(&Spot::load(path).unwrap(), &options, |_| {}).unwrap();
        let (game, profile) = (solution.game(), solution.strategy());
        // Check, bet 8bb, raise to 20bb.
        let tree = game.tree();
        let node = [0, 1, 2]
            .iter()
            .fold(0, |at, &action| tree.node(at).children()[action]);
        let Node::Player { actor, actions, .. } = tree.node(node) else {
            panic!("IP decides facing the check-raise");
        };
        assert_eq!(*actor, Player::Ip);
        assert_eq!(actions.len(), 3, "fold, call, re-raise");
        let reach = game.reach(profile, node);
        let (ip, oop) = (game.hands(Player::Ip), game.hands(Player::Oop));
        let strategy = profile.get(node as usize).unwrap();
        let n = ip.len();
        let (mut removal, mut own) = ([0.0; 3], [0.0; 3]);
        let (mut removal_total, mut own_total) = (0.0, 0.0);
        let mut reached = 0.0;
        for (h, combo) in ip.combos().iter().enumerate() {
            let w = ip.weights()[h] * reach[1][h];
            let met: f64 = oop
                .combos()
                .iter()
                .enumerate()
                .filter(|(_, other)| !other.cards().intersects(combo.cards()))
                .map(|(o, _)| oop.weights()[o] * reach[0][o])
                .sum();
            for a in 0..3 {
                let p = f64::from(strategy[a * n + h]);
                removal[a] += w * met * p;
                own[a] += w * p;
            }
            removal_total += w * met;
            own_total += w;
            reached += w * met;
        }
        let removal = removal.map(|x| x / removal_total);
        let own = own.map(|x| x / own_total);
        let measured = game.action_frequencies(profile, node).unwrap();
        let shown = game.range_view_frequencies(profile, node).unwrap();
        for a in 0..3 {
            assert!(
                (measured[a] - removal[a]).abs() < 1e-12,
                "{measured:?} {removal:?}"
            );
            assert!((shown[a] - own[a]).abs() < 1e-12, "{shown:?} {own:?}");
        }
        assert_eq!(solution.action_frequencies(node).unwrap(), measured);
        assert_eq!(solution.range_view_frequencies(node).unwrap(), shown);
        // The node is reached about 4% of the time: the pairs that play to it over all pairs.
        let reached = reached / game.pair_weight();
        assert!((0.03..0.05).contains(&reached), "{reached}");
        // The re-raise: 10.06% and 18.72%, measured.
        assert!((measured[2] - 0.1006).abs() < 5e-4, "{measured:?}");
        assert!((shown[2] - 0.1872).abs() < 5e-4, "{shown:?}");
        assert!(shown[2] - measured[2] > 0.08);
        // The weights behind them: a range view's sum to 1 and ignore the opponent.
        let weights = game.range_view_weights(profile, node).unwrap();
        assert!((weights.iter().sum::<f64>() - 1.0).abs() < 1e-12);
        assert_eq!(solution.range_view_weights(node).unwrap(), weights);
        assert_ne!(Some(weights), game.hand_weights(profile, node));
        // At a terminal there is neither.
        let terminal = tree.node(node).children()[0];
        assert_eq!(game.range_view_frequencies(profile, terminal), None);
    }

    /// A profile on the small spot: at the root OOP checks with AhAd alone and bets with every
    /// other hand, after a check IP always checks, and every other node plays uniformly.
    fn checks_with_aces(game: &RiverGame) -> PublicProfile {
        let aces: pk_core::Combo = "AhAd".parse().unwrap();
        let after_check = game.tree().find("x").unwrap() as usize;
        PublicProfile::from_fn(game, |node, hand| {
            let PublicNode::Player { num_actions, .. } = game.node(node) else {
                unreachable!();
            };
            match node {
                0 if game.hands(Player::Oop).combos()[hand] == aces => vec![1.0, 0.0],
                0 => vec![0.0, 1.0],
                n if n == after_check => vec![1.0, 0.0],
                _ => vec![1.0 / num_actions as f32; num_actions],
            }
        })
    }

    #[test]
    fn the_reach_probability_is_the_sum_over_the_pairs_that_play_to_the_node() {
        let game = game();
        let profile = checks_with_aces(&game);
        let (oop, ip) = (game.hands(Player::Oop), game.hands(Player::Ip));
        for node in 0..game.num_nodes() as u32 {
            let reach = game.reach(&profile, node);
            let mut pairs = 0.0;
            for (h, a) in oop.combos().iter().enumerate() {
                for (o, b) in ip.combos().iter().enumerate() {
                    if !a.cards().intersects(b.cards()) {
                        pairs += oop.weights()[h] * reach[0][h] * ip.weights()[o] * reach[1][o];
                    }
                }
            }
            let expected = pairs / game.pair_weight();
            let found = game.reach_probability(&profile, node);
            assert!(
                (found - expected).abs() < 1e-12,
                "node {node}: {found} {expected}"
            );
            // A player node's reach is split among its children.
            let children = game.tree().node(node).children();
            if !children.is_empty() {
                let split: f64 = children
                    .iter()
                    .map(|&child| game.reach_probability(&profile, child))
                    .sum();
                assert!((split - found).abs() < 1e-6, "node {node}: {split} {found}");
            }
        }
        assert!((game.reach_probability(&profile, 0) - 1.0).abs() < 1e-12);
        // IP never bets after a check, so its check-raise line is never reached.
        let never = game.tree().find("x b50 r3").unwrap();
        assert_eq!(game.reach_probability(&profile, never), 0.0);
    }

    #[test]
    fn range_view_classes_sum_the_range_view_over_each_class() {
        // Every player node of a solve, each class checked against the definition summed hand
        // by hand from the reach and the strategy.
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../reference/spots/river/dry_ip_polarized.toml"
        );
        let options = crate::solve::SolveOptions {
            target: None,
            max_iterations: 60,
            ..crate::solve::SolveOptions::default()
        };
        let solution = crate::solve::solve(&Spot::load(path).unwrap(), &options, |_| {}).unwrap();
        let (game, profile) = (solution.game(), solution.strategy());
        let mut nodes = 0;
        for (node, tree_node) in game.tree().nodes().iter().enumerate() {
            let node = node as u32;
            let Node::Player { actor, actions, .. } = tree_node else {
                assert_eq!(game.range_view_classes(profile, node), None);
                continue;
            };
            nodes += 1;
            let classes = game.range_view_classes(profile, node).unwrap();
            assert_eq!(solution.range_view_classes(node).unwrap(), classes);
            let hands = game.hands(*actor);
            let reach = &game.reach(profile, node)[actor.index()];
            let strategy = profile.get(node as usize).unwrap();
            let n = hands.len();
            let total: f64 = (0..n).map(|h| hands.weights()[h] * reach[h]).sum();
            // In grid order, and every hand in its own class: after a solve every hand plays to
            // every node with some probability.
            assert!(classes.windows(2).all(|w| w[0].class < w[1].class));
            let mut seen = vec![false; n];
            for class in &classes {
                let mut share = 0.0;
                let mut frequencies = vec![0.0; actions.len()];
                for &h in &class.hands {
                    assert_eq!(HandClass::of(hands.combos()[h]), class.class);
                    assert!(!std::mem::replace(&mut seen[h], true));
                    let w = hands.weights()[h] * reach[h] / total;
                    share += w;
                    for (a, f) in frequencies.iter_mut().enumerate() {
                        *f += w * f64::from(strategy[a * n + h]);
                    }
                }
                assert!(
                    (class.share - share).abs() < 1e-12,
                    "node {node}: {class:?}"
                );
                for (found, expected) in class.frequencies.iter().zip(&frequencies) {
                    let expected = expected / share;
                    assert!((found - expected).abs() < 1e-12, "node {node}: {class:?}");
                }
            }
            assert!(seen.iter().all(|&s| s), "node {node}");
            // The shares sum to 1, and weighted by them the classes give the range view.
            let shares: f64 = classes.iter().map(|c| c.share).sum();
            assert!((shares - 1.0).abs() < 1e-12, "node {node}: {shares}");
            let range = game.range_view_frequencies(profile, node).unwrap();
            for (a, &f) in range.iter().enumerate() {
                let summed: f64 = classes.iter().map(|c| c.share * c.frequencies[a]).sum();
                assert!((summed - f).abs() < 1e-12, "node {node}: {summed} {f}");
            }
        }
        assert_eq!(nodes, 10);
    }

    #[test]
    fn a_class_with_no_hand_at_the_node_is_left_out() {
        let game = game();
        let profile = checks_with_aces(&game);
        // OOP checked with AhAd alone, so facing IP's bet its range is that one hand: the class
        // AA, with its other hand, AcAs, at a weight of 0.
        let facing = game.tree().find("x b50").unwrap();
        let classes = game.range_view_classes(&profile, facing).unwrap();
        assert_eq!(classes.len(), 1);
        let aa = &classes[0];
        assert_eq!(aa.class, "AA".parse::<HandClass>().unwrap());
        assert_eq!(aa.hands.len(), 2);
        assert_eq!(aa.share, 1.0);
        let third = f64::from(1.0f32 / 3.0);
        assert_eq!(aa.frequencies, [third; 3]);
        // At the root, every class of the range: AA, KQs, KQo and 98s, by range weight.
        let root = game.range_view_classes(&profile, 0).unwrap();
        let names: Vec<String> = root.iter().map(|c| c.class.to_string()).collect();
        assert_eq!(names, ["AA", "KQs", "KQo", "98s"]);
        let shares: Vec<f64> = root.iter().map(|c| c.share * 4.25).collect();
        for (found, expected) in shares.iter().zip([1.5, 0.75, 1.0, 1.0]) {
            assert!((found - expected).abs() < 1e-12, "{shares:?}");
        }
        // AA checks with AhAd, at weight 1, and bets with AcAs, at weight 0.5.
        let aa = &root[0].frequencies;
        assert!((aa[0] - 2.0 / 3.0).abs() < 1e-15 && (aa[1] - 1.0 / 3.0).abs() < 1e-15);
        // IP never plays to its node facing the check-raise, and a terminal has no range.
        let never = game.tree().find("x b50 r3").unwrap();
        assert_eq!(game.range_view_classes(&profile, never), None);
        let showdown = game.tree().find("x x").unwrap();
        assert_eq!(game.range_view_classes(&profile, showdown), None);
    }

    #[test]
    fn the_toy_games_classes_at_the_root_are_its_equilibrium() {
        // OOP's range at the root: JTs and JTo (air), 99 and 66 (nuts), in grid order, by
        // their combos on the board, 4, 12, 3 and 3 of 22. The nuts always bet, and the air
        // bets 1/8 of the time: 2 of its 16 combos.
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../reference/spots/river/toy_polarized.toml"
        );
        let options = crate::solve::SolveOptions {
            target: None,
            max_iterations: 1000,
            check_every: 1000,
            ..crate::solve::SolveOptions::default()
        };
        let solution = crate::solve::solve(&Spot::load(path).unwrap(), &options, |_| {}).unwrap();
        let classes = solution.range_view_classes(0).unwrap();
        let names: Vec<String> = classes.iter().map(|c| c.class.to_string()).collect();
        assert_eq!(names, ["JTs", "JTo", "99", "66"]);
        for (class, combos) in classes.iter().zip([4.0, 12.0, 3.0, 3.0]) {
            assert!((class.share - combos / 22.0).abs() < 1e-14, "{class:?}");
        }
        for nuts in &classes[2..] {
            assert!(nuts.frequencies[1] > 1.0 - 1e-5, "{nuts:?}");
        }
        let air = (4.0 * classes[0].frequencies[1] + 12.0 * classes[1].frequencies[1]) / 16.0;
        assert!((air - 0.125).abs() < 2e-3, "{air}");
        assert!((solution.reach_probability(0) - 1.0).abs() < 1e-12);
    }

    #[test]
    fn an_invalid_spot_is_refused() {
        let mut spot = spot();
        spot.rake_percent = 150.0;
        match RiverGame::new(&spot) {
            Err(SolveError::Spot(SpotError::Invalid { field, .. })) => {
                assert_eq!(field, "rake.percent")
            }
            other => panic!("{other:?}"),
        }
    }
}
