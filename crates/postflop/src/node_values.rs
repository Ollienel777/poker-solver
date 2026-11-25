//! Each acting player's expected value with each of its hands at every player node of a game,
//! under a profile: the EVs of a range view, and of a [solution dump](crate::dump).
//!
//! # Definition
//!
//! At a player node `n` where player `i` acts, with opponent `j`, the EV of `i`'s hand `h` is
//! its expected payoff given that play has reached `n` and that `i` holds `h`, when both players
//! follow the profile from `n` on:
//!
//! ```text
//! EV(n, h) = Σ_o deal(j)[o] · π_j(n, o) · u(n, h, o)  /  Σ_o deal(j)[o] · π_j(n, o)
//! ```
//!
//! over the opponent's hands `o` that share no card with `h`, where `π_j(n, o)` is the
//! opponent's own reach of `n` with `o` ([`PostflopGame::reach`]) and `u(n, h, o)` is `i`'s
//! expected payoff from `n` on when the two hold `h` and `o`, over the cards still to fall. So
//! the opponent's hand is weighted by its range weight and by how often it plays to `n`, with
//! card removal by `h`. `i`'s own reach does not enter: the EV of a hand that never plays to `n`
//! is still what it would get there. Payoffs are those of [`crate::payoff`]: chips, net from the
//! start of the spot, with the root pot counted for neither player and the rake taken from the
//! winner.
//!
//! The EV is `None` when the opponent's reach compatible with `h` is zero, where it would be
//! 0 / 0, and for a hand that holds a card dealt on the way to `n`, which cannot be held there.
//! The denominator comes from [`BoardHands::fold_values`](crate::BoardHands::fold_values),
//! which subtracts the blocked reach from the total, so a reach that is zero in exact arithmetic
//! can come out a few ulps from it; anything at or below [`ZERO_REACH`] of the opponent's whole
//! reach of the node counts as zero.
//!
//! At the root the opponent's reach is its deal, and `Σ_h P(h) · EV(root, h)` over the
//! probability `P(h)` that `i` is dealt `h` is `i`'s value `v_i(σ)` of
//! [`exploitability`](mod@crate::exploitability).
//!
//! # Computation
//!
//! One walk of the public tree per player, as in [`pk_cfr::vector_expected_value`]: it carries
//! the opponent's reach, `deal(j)` times its probabilities so far and the probability of each
//! card dealt, with the hands a card rules out at zero, and returns for each of `i`'s hands the
//! counterfactual value `q(n, h) = Σ_o ρ(o) · u(n, h, o)`. At `i`'s nodes the EV is `q(n, h)`
//! over the compatible reach `Σ_o ρ(o)`, in which the probabilities of the cards dealt cancel.
//! Every quantity is `f64`.

use pk_cfr::{PublicGame, PublicNode, PublicProfile};
use pk_game::PostflopTree;

use crate::game::{PostflopGame, player_of};

// The tests below build the river game by the name they were written with.
#[cfg(test)]
use crate::river::RiverGame;
#[cfg(test)]
use pk_game::Node;

/// The share of the opponent's whole reach of a node at or below which its reach compatible
/// with a hand counts as zero, and the hand's EV as undefined: see the
/// [module documentation](self).
pub const ZERO_REACH: f64 = 1e-12;

/// Each acting player's EV with each of its hands at every player node, from [`node_values`].
#[derive(Debug, Clone, PartialEq)]
pub struct NodeValues {
    /// By node: at a player node, the acting player's EV by hand index, and `None` at a
    /// terminal or a chance node.
    nodes: Vec<Option<Vec<Option<f64>>>>,
}

impl NodeValues {
    /// At the player node `node`, the acting player's EV with each of its hands, by hand index:
    /// `None` for a hand whose EV is undefined. `None` at a terminal, at a chance node or out
    /// of range.
    pub fn get(&self, node: u32) -> Option<&[Option<f64>]> {
        self.nodes.get(node as usize)?.as_deref()
    }

    /// Values by node, as [`NodeValues::get`] gives them. For a solution read from a dump.
    pub(crate) fn from_nodes(nodes: Vec<Option<Vec<Option<f64>>>>) -> NodeValues {
        NodeValues { nodes }
    }
}

/// The EVs of the [module documentation](self) under `profile`, at every player node of
/// `game`.
///
/// Panics if `profile` has no strategy, or one of the wrong size, at a player node.
pub fn node_values(game: &PostflopGame, profile: &PublicProfile) -> NodeValues {
    let mut nodes = vec![None; game.num_nodes()];
    for player in 0..2 {
        let walk = Walk {
            game,
            profile,
            player,
        };
        let possible = vec![true; game.num_hands(player)];
        walk.visit(
            PostflopTree::ROOT,
            game.deal(1 - player),
            &possible,
            &mut nodes,
        );
    }
    NodeValues { nodes }
}

/// One player's walk.
struct Walk<'a> {
    game: &'a PostflopGame,
    profile: &'a PublicProfile,
    player: usize,
}

impl Walk<'_> {
    /// Returns `q(node, h)` for each of the player's hands, given the opponent's reach `ρ`,
    /// and records the EVs at the player's own nodes below. `possible` says which of the
    /// player's hands can be held at `node`.
    fn visit(
        &self,
        node: u32,
        opponent_reach: &[f64],
        possible: &[bool],
        out: &mut [Option<Vec<Option<f64>>>],
    ) -> Vec<f64> {
        let game = self.game;
        let num_hands = game.num_hands(self.player);
        let (actor, num_actions) = match game.node(node as usize) {
            PublicNode::Terminal => {
                let mut values = vec![0.0; num_hands];
                game.terminal_values(node as usize, self.player, opponent_reach, &mut values);
                for (value, &possible) in values.iter_mut().zip(possible) {
                    if !possible {
                        *value = 0.0;
                    }
                }
                return values;
            }
            PublicNode::Chance { num_outcomes } => {
                let opponent = 1 - self.player;
                let mut values = vec![0.0; num_hands];
                for outcome in 0..num_outcomes {
                    let probability = game.outcome_probability(node as usize, outcome);
                    let after: Vec<bool> = possible
                        .iter()
                        .enumerate()
                        .map(|(hand, &p)| {
                            p && !game.rules_out(node as usize, outcome, self.player, hand)
                        })
                        .collect();
                    let reach: Vec<f64> = opponent_reach
                        .iter()
                        .enumerate()
                        .map(|(hand, &r)| {
                            if game.rules_out(node as usize, outcome, opponent, hand) {
                                0.0
                            } else {
                                r * probability
                            }
                        })
                        .collect();
                    let child = game.child(node as usize, outcome) as u32;
                    let below = self.visit(child, &reach, &after, out);
                    for (value, q) in values.iter_mut().zip(&below) {
                        *value += q;
                    }
                }
                return values;
            }
            PublicNode::Player {
                player,
                num_actions,
            } => (player, num_actions),
        };
        let children = game.children(node);
        let strategy = self
            .profile
            .get(node as usize)
            .unwrap_or_else(|| panic!("the profile has no strategy at node {node}"));
        let acting = game.num_hands(actor);
        assert_eq!(
            strategy.len(),
            num_actions * acting,
            "the profile's strategy at node {node} has the wrong size"
        );
        let mut values = vec![0.0; num_hands];
        if actor == self.player {
            for (&child, probabilities) in children.iter().zip(strategy.chunks_exact(acting)) {
                let below = self.visit(child, opponent_reach, possible, out);
                for ((value, &p), q) in values.iter_mut().zip(probabilities).zip(&below) {
                    *value += f64::from(p) * q;
                }
            }
            let mut met = vec![0.0; num_hands];
            game.board_hands()
                .fold_values(player_of(self.player), opponent_reach, 1.0, &mut met);
            let total: f64 = opponent_reach.iter().sum();
            let evs = values
                .iter()
                .zip(&met)
                .zip(possible)
                .map(|((&q, &m), &possible)| {
                    (possible && total > 0.0 && m > ZERO_REACH * total).then(|| q / m)
                })
                .collect();
            out[node as usize] = Some(evs);
        } else {
            for (&child, probabilities) in children.iter().zip(strategy.chunks_exact(acting)) {
                let reach: Vec<f64> = opponent_reach
                    .iter()
                    .zip(probabilities)
                    .map(|(r, &p)| r * f64::from(p))
                    .collect();
                let below = self.visit(child, &reach, possible, out);
                for (value, q) in values.iter_mut().zip(&below) {
                    *value += q;
                }
            }
        }
        values
    }
}

#[cfg(test)]
mod tests {
    use std::cmp::Ordering;

    use pk_cfr::{PublicNode, vector_expected_value};
    use pk_core::evaluate;
    use pk_game::Player;

    use super::*;
    use crate::payoff::Payoff;
    use crate::solve::{SolveOptions, solve};
    use crate::spot::Spot;

    fn load(name: &str) -> Spot {
        let path = format!(
            "{}/../../reference/spots/river/{name}.toml",
            env!("CARGO_MANIFEST_DIR")
        );
        Spot::load(path).unwrap()
    }

    /// The toy game's equilibrium with pure strategies per combo: OOP bets the nuts and two
    /// air combos, IP calls with 8 of its 12 combos and checks behind.
    fn toy_profile(game: &RiverGame) -> PublicProfile {
        let oop = game.hands(Player::Oop);
        let is_nuts =
            |hand: usize| oop.combos()[hand].lo().rank() == oop.combos()[hand].hi().rank();
        let after_bet = game.tree().node(0).children()[1] as usize;
        PublicProfile::from_fn(game, |node, hand| {
            let PublicNode::Player { player, .. } = game.node(node) else {
                unreachable!();
            };
            match (player, node) {
                // Bet the nuts and the first two air combos.
                (0, 0) => {
                    let air_before = (0..hand).filter(|&h| !is_nuts(h)).count();
                    if is_nuts(hand) || air_before < 2 {
                        vec![0.0, 1.0]
                    } else {
                        vec![1.0, 0.0]
                    }
                }
                // Facing a bet after checking, which IP never makes: call with the nuts.
                (0, _) => {
                    if is_nuts(hand) {
                        vec![0.0, 1.0]
                    } else {
                        vec![1.0, 0.0]
                    }
                }
                // Call with the first 8 of the 12 combos.
                (1, n) if n == after_bet => {
                    if hand < 8 {
                        vec![0.0, 1.0]
                    } else {
                        vec![1.0, 0.0]
                    }
                }
                (1, _) => vec![1.0, 0.0],
                _ => unreachable!(),
            }
        })
    }

    #[test]
    fn the_toy_games_evs_are_its_analytical_values() {
        // At the root, each nut combo wins the 10bb pot and two thirds of a 5bb call: 40/3 bb.
        // Each air combo gets 0, whether it bluffs (a third of 10bb, less two thirds of 5bb)
        // or checks and loses. Facing the bet, IP's calls break even against 2 bluffs in 8
        // bets, and folding is worth 0, so every KQ combo gets 0. After a check IP checks
        // behind and wins the pot. And OOP's node facing a bet after a check is never reached
        // by IP, so no EV is defined there.
        let game = RiverGame::new(&load("toy_polarized")).unwrap();
        let profile = toy_profile(&game);
        let values = node_values(&game, &profile);
        let oop = game.hands(Player::Oop);
        let root = values.get(0).unwrap();
        for (hand, combo) in oop.combos().iter().enumerate() {
            let expected = if combo.lo().rank() == combo.hi().rank() {
                4000.0 / 3.0
            } else {
                0.0
            };
            assert!((root[hand].unwrap() - expected).abs() < 1e-9, "{combo}");
        }
        let [after_check, after_bet] = [0, 1].map(|a| game.tree().node(0).children()[a]);
        for ev in values.get(after_bet).unwrap() {
            assert!(ev.unwrap().abs() < 1e-9, "{ev:?}");
        }
        for ev in values.get(after_check).unwrap() {
            assert!((ev.unwrap() - 1000.0).abs() < 1e-9, "{ev:?}");
        }
        let check_bet = game.tree().node(after_check).children()[1];
        assert!(values.get(check_bet).unwrap().iter().all(Option::is_none));
        // Terminals have none.
        let showdown = game.tree().node(after_check).children()[0];
        assert_eq!(values.get(showdown), None);
    }

    #[test]
    fn the_root_evs_weighted_by_the_deal_are_the_players_values() {
        // On a raked spot with shared combos, solved: Σ_h P(h) · EV(root, h) = v_i, where
        // P(h) = deal(i)[h] · Σ_o deal(j)[o] over the compatible o.
        let spot = load("dry_ip_polarized");
        let solution = solve(&spot, &SolveOptions::default(), |_| {}).unwrap();
        let game = solution.game();
        let values = node_values(game, solution.strategy());
        let root = values.get(0).unwrap();
        let oop = 0;
        let mut met = vec![0.0; game.num_hands(oop)];
        game.board_hands()
            .fold_values(Player::Oop, game.deal(1), 1.0, &mut met);
        let total: f64 = (0..game.num_hands(oop))
            .map(|h| game.deal(oop)[h] * met[h] * root[h].unwrap())
            .sum();
        let expected = vector_expected_value(game, solution.strategy(), oop).value;
        assert!(
            (total - expected).abs() < 1e-9,
            "{total} against {expected}"
        );
        assert!((total - solution.values()[0]).abs() < 1e-9);
    }

    /// `player`'s expected payoff from `node` on when it holds `mine` and the opponent holds
    /// `theirs`, both following `profile`: the scalar definition, one pair of hands at a time.
    fn pair_value(
        game: &RiverGame,
        profile: &PublicProfile,
        node: u32,
        player: Player,
        hands: [usize; 2],
    ) -> f64 {
        match game.tree().node(node) {
            Node::Player {
                actor, children, ..
            } => {
                let hand = hands[actor.index()];
                let row = profile.hand(node as usize, hand).unwrap();
                children
                    .iter()
                    .zip(row)
                    .map(|(&child, p)| {
                        f64::from(p) * pair_value(game, profile, child, player, hands)
                    })
                    .sum()
            }
            Node::Terminal { .. } => match game.payoff(node).unwrap() {
                Payoff::Fold(payoffs) => payoffs[player.index()],
                Payoff::Showdown(payoff) => {
                    let board = game.board_hands().board();
                    let strength = |p: Player| {
                        let combo = game.hands(p).combos()[hands[p.index()]];
                        evaluate(board | combo.cards())
                    };
                    match strength(player).cmp(&strength(player.opponent())) {
                        Ordering::Greater => payoff.win,
                        Ordering::Equal => payoff.tie,
                        Ordering::Less => payoff.lose,
                    }
                }
            },
            Node::Chance { .. } => unreachable!("a river tree has no chance node"),
        }
    }

    #[test]
    fn every_ev_is_the_average_over_the_opponents_hands_of_the_scalar_payoff() {
        // At every player node of a solved raked spot with all-in and ties, each EV against
        // the O(n²) definition: the scalar payoff of each pair of hands from the node on,
        // averaged over the opponent's compatible hands weighted by deal and reach.
        let spot = load("paired_low_spr");
        let solution = solve(&spot, &SolveOptions::default(), |_| {}).unwrap();
        let (game, profile) = (solution.game(), solution.strategy());
        let values = node_values(game, profile);
        for node in 0..game.num_nodes() as u32 {
            let Node::Player { actor, .. } = game.tree().node(node) else {
                continue;
            };
            let (me, them) = (*actor, actor.opponent());
            let reach = solution.reach(node);
            let (mine, theirs) = (game.hands(me), game.hands(them));
            let evs = values.get(node).unwrap();
            assert_eq!(evs.len(), mine.len());
            for (h, combo) in mine.combos().iter().enumerate() {
                let (mut total, mut weight) = (0.0, 0.0);
                for (o, other) in theirs.combos().iter().enumerate() {
                    if combo.cards().intersects(other.cards()) {
                        continue;
                    }
                    let w = game.deal(them.index())[o] * reach[them.index()][o];
                    let mut hands = [0; 2];
                    hands[me.index()] = h;
                    hands[them.index()] = o;
                    total += w * pair_value(game, profile, node, me, hands);
                    weight += w;
                }
                match evs[h] {
                    None => assert!(weight < 1e-12, "node {node}, {combo}: {weight}"),
                    Some(ev) => {
                        let expected = total / weight;
                        assert!(
                            (ev - expected).abs() <= 1e-9 * expected.abs().max(1.0),
                            "node {node}, {combo}: {ev} against {expected}"
                        );
                    }
                }
            }
        }
    }
}
