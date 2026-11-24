//! The river game: a river [`Spot`] as a [`PublicGame`], which [`VectorCfr`] solves.
//!
//! # Players and hands
//!
//! Player 0 is OOP and player 1 is IP, as [`Player::index`] numbers them. A player's hands are
//! its [`HandList`] on the board ([`crate::hands`]): the combos of its range that have a
//! positive weight and miss the board, in combo order. A combo the board blocks, or one outside
//! the range, has no hand index, so no per-hand vector of the solver holds it, and each player
//! node stores regrets and strategy sums for the acting player's in-range combos only.
//!
//! # The deal
//!
//! Chance deals OOP the hand `h` and IP the hand `o` with probability proportional to the
//! product of their range weights, `w_0(h) · w_1(o)`, if the two combos share no card, and never
//! otherwise. Normalised so that the probabilities sum to 1 over the pairs that can be dealt:
//!
//! ```text
//! Z          = Σ w_0(h) · w_1(o)    over the pairs (h, o) that share no card
//! deal(p)[h] = w_p(h) / √Z
//! ```
//!
//! so that `deal(0)[h] · deal(1)[o] = w_0(h) · w_1(o) / Z`, and every value computed from the
//! deal, such as a player's expected value or best response, is in chips per hand dealt.
//! [`RiverGame::pair_weight`] is `Z`, which [`BoardHands::fold_values`] gives in O(n). Any split
//! of `1 / Z` between the players gives the same strategies; splitting it evenly keeps the two
//! players' stored regrets on the same scale.
//!
//! # The public tree
//!
//! The public tree is the spot's [`PostflopTree`], node for node: public node `n` is tree node
//! `n`, the root is node 0, a player node's actions are the tree's, in the tree's order, and the
//! acting player is the actor's [`Player::index`]. A river tree has no chance nodes, so every
//! hand is possible at every node, and the information sets are the pairs of a player node and
//! one of the acting player's hands.
//!
//! # Terminals
//!
//! Each terminal's [`Payoff`] is computed once, when the game is built, from the root pot, the
//! terminal's outcome and contributions, and the spot's rake ([`crate::payoff`]). Payoffs are
//! net chips from the start of the river, so over both players they sum to the pot less the
//! rake: the game is not zero-sum, which is why [exploitability](mod@crate::exploitability) has
//! a definition of its own. A fold's values come from [`BoardHands::fold_values`] with the traverser's payoff,
//! and a showdown's from [`BoardHands::showdown_values`], each in O(n) with card removal.
//!
//! # Ranges at a node
//!
//! For reading a strategy, [`RiverGame::reach`] gives each player's own probability of playing
//! to a node with each hand, and [`RiverGame::hand_weights`] the probability that the acting
//! player holds each hand once the node is reached. For the acting player `i`, the opponent `j`
//! and own reaches `π` under a profile,
//!
//! ```text
//! P(h | n) ∝ deal(i)[h] · π_i(n, h) · Σ_o deal(j)[o] · π_j(n, o)    over o sharing no card with h
//! ```
//!
//! which is exact, card removal by the opponent's range at the node included.
//! [`RiverGame::action_frequencies`] is then the probability of each action at the node,
//! `Σ_h P(h | n) · σ(a | h)`: how often the acting player takes it over its whole range there.
//!
//! [`VectorCfr`]: pk_cfr::VectorCfr

use pk_cfr::{PublicGame, PublicNode, PublicProfile};
use pk_game::{Chips, Node, Player, PostflopTree, Rake};

use crate::hands::HandList;
use crate::payoff::Payoff;
use crate::solve::SolveError;
use crate::spot::Spot;
use crate::terminal::BoardHands;

/// A river spot as a public game, as the [module documentation](self) describes it.
#[derive(Debug, Clone)]
pub struct RiverGame {
    tree: PostflopTree,
    hands: BoardHands,
    pot: Chips,
    rake: Rake,
    /// `Z`, the total weight of the pairs of hands that can be dealt.
    pair_weight: f64,
    /// [`PublicGame::deal`] for OOP, then IP.
    deal: [Vec<f64>; 2],
    /// By node: the payoffs at a terminal, and `None` at a player node.
    payoffs: Vec<Option<Payoff>>,
}

/// The player of index 0 or 1.
pub(crate) fn player_of(index: usize) -> Player {
    match index {
        0 => Player::Oop,
        1 => Player::Ip,
        _ => panic!("player {index} is not 0 or 1"),
    }
}

impl RiverGame {
    /// The game of `spot`, which must start on the river.
    ///
    /// Fails with [`SolveError::NotRiver`] if the board does not have five cards, with
    /// [`SolveError::Spot`] if the spot does not pass [`Spot::validate`], and with
    /// [`SolveError::Tree`] if its tree has more nodes than the builder's default limit.
    pub fn new(spot: &Spot) -> Result<RiverGame, SolveError> {
        if spot.board.len() != 5 {
            return Err(SolveError::NotRiver {
                cards: spot.board.len(),
            });
        }
        spot.validate()?;
        let tree = spot.build_tree()?;
        let hands = spot.board_hands();
        let rake = spot.rake();
        let payoffs = tree
            .nodes()
            .iter()
            .map(|node| match *node {
                Node::Terminal {
                    outcome,
                    contributions,
                    ..
                } => Some(Payoff::new(spot.pot, outcome, contributions, &rake)),
                Node::Player { .. } => None,
                Node::Chance { .. } => panic!("a tree that starts on the river has no chance node"),
            })
            .collect();
        let weights =
            [Player::Oop, Player::Ip].map(|player| hands.hands(player).weights().to_vec());
        // For each OOP hand, the weight of the IP hands it can be dealt with.
        let mut met = vec![0.0; weights[0].len()];
        hands.fold_values(Player::Oop, &weights[1], 1.0, &mut met);
        let pair_weight: f64 = weights[0].iter().zip(&met).map(|(w, m)| w * m).sum();
        // `Spot::validate` checks that some pair of hands can be dealt, and every weight in a
        // hand list is positive.
        assert!(
            pair_weight > 0.0,
            "a valid spot has a pair of hands that can be dealt together"
        );
        let scale = pair_weight.sqrt();
        let deal = weights.map(|weights| weights.iter().map(|w| w / scale).collect());
        Ok(RiverGame {
            tree,
            hands,
            pot: spot.pot,
            rake,
            pair_weight,
            deal,
            payoffs,
        })
    }

    /// The betting tree, whose node indices are the public tree's.
    pub fn tree(&self) -> &PostflopTree {
        &self.tree
    }

    /// Both players' hands on the board, with the terminal evaluations.
    pub fn board_hands(&self) -> &BoardHands {
        &self.hands
    }

    /// One player's hands. Hand `h` of player `player.index()` is `hands(player).combos()[h]`.
    pub fn hands(&self, player: Player) -> &HandList {
        self.hands.hands(player)
    }

    /// The pot at the root, in chips.
    pub fn pot(&self) -> Chips {
        self.pot
    }

    /// The spot's rake, from which the terminal payoffs were computed.
    pub fn rake(&self) -> &Rake {
        &self.rake
    }

    /// `Z`, the sum of `w_0(h) · w_1(o)` over the pairs of hands that share no card.
    pub fn pair_weight(&self) -> f64 {
        self.pair_weight
    }

    /// The payoffs at a terminal, or `None` at a player node. Panics if `node` is out of range.
    pub fn payoff(&self, node: u32) -> Option<Payoff> {
        self.payoffs[node as usize]
    }

    /// The actions from the root to `node`, as pairs of a player node and the index of the
    /// action taken there.
    ///
    /// Panics if `node` is out of range.
    pub fn path(&self, node: u32) -> Vec<(u32, usize)> {
        let num_nodes = self.tree.nodes().len();
        assert!(
            (node as usize) < num_nodes,
            "node {node} is out of range for {num_nodes} nodes"
        );
        let mut path = Vec::new();
        let mut at = PostflopTree::ROOT;
        while at != node {
            // Every subtree takes a contiguous range of indices in pre-order, children in
            // action order, so `node` lies below the last child that comes at or before it.
            let children = self.tree.node(at).children();
            let action = children
                .iter()
                .rposition(|&child| child <= node)
                .expect("the tree is in pre-order");
            path.push((at, action));
            at = children[action];
        }
        path
    }

    /// Each player's own reach of `node` under `profile`, by hand: the product of the
    /// probabilities of its own actions on the way from the root. At the root every hand's is
    /// 1.
    ///
    /// Panics if `node` is out of range, or if `profile` has no strategy, or one of the wrong
    /// size, at a player node on the way.
    pub fn reach(&self, profile: &PublicProfile, node: u32) -> [Vec<f64>; 2] {
        let mut reach = [0, 1].map(|player| vec![1.0; self.num_hands(player)]);
        for (at, action) in self.path(node) {
            let Node::Player { actor, actions, .. } = self.tree.node(at) else {
                unreachable!("a path passes through player nodes only");
            };
            let own = &mut reach[actor.index()];
            let num_hands = own.len();
            let strategy = self.strategy_at(profile, at, actions.len(), num_hands);
            let probabilities = &strategy[action * num_hands..(action + 1) * num_hands];
            for (reach, &p) in own.iter_mut().zip(probabilities) {
                *reach *= f64::from(p);
            }
        }
        reach
    }

    /// At a player node, the probability that the acting player holds each of its hands once
    /// the node is reached under `profile`, as the [module documentation](self) defines it. The
    /// weights sum to 1.
    ///
    /// `None` if `node` is a terminal, or if `profile` reaches it with probability zero. Panics
    /// as [`RiverGame::reach`] does.
    pub fn hand_weights(&self, profile: &PublicProfile, node: u32) -> Option<Vec<f64>> {
        let Node::Player { actor, .. } = self.tree.node(node) else {
            return None;
        };
        let (me, them) = (actor.index(), actor.opponent().index());
        let reach = self.reach(profile, node);
        let opponent: Vec<f64> = reach[them]
            .iter()
            .zip(&self.deal[them])
            .map(|(r, d)| r * d)
            .collect();
        let mut met = vec![0.0; self.num_hands(me)];
        self.hands.fold_values(*actor, &opponent, 1.0, &mut met);
        let mut weights: Vec<f64> = self.deal[me]
            .iter()
            .zip(&reach[me])
            .zip(&met)
            .map(|((d, r), m)| d * r * m)
            .collect();
        let total: f64 = weights.iter().sum();
        if total <= 0.0 {
            return None;
        }
        for weight in &mut weights {
            *weight /= total;
        }
        Some(weights)
    }

    /// At a player node, the probability of each action once the node is reached under
    /// `profile`: `Σ_h P(h | n) · σ(a | h)` over the acting player's hands, with the weights of
    /// [`RiverGame::hand_weights`]. These are the frequencies of a range view: how often the
    /// acting player takes each action there, over its whole range.
    ///
    /// `None` if `node` is a terminal, or if `profile` reaches it with probability zero. Panics
    /// as [`RiverGame::reach`] does, or if `profile` has no strategy at `node`.
    pub fn action_frequencies(&self, profile: &PublicProfile, node: u32) -> Option<Vec<f64>> {
        let weights = self.hand_weights(profile, node)?;
        let Node::Player { actions, .. } = self.tree.node(node) else {
            unreachable!("hand_weights is None at a terminal");
        };
        let strategy = self.strategy_at(profile, node, actions.len(), weights.len());
        let frequencies = strategy
            .chunks_exact(weights.len())
            .map(|probabilities| {
                probabilities
                    .iter()
                    .zip(&weights)
                    .map(|(&p, &w)| f64::from(p) * w)
                    .sum()
            })
            .collect();
        Some(frequencies)
    }

    /// The strategy `profile` holds at the player node `node`, checked against its shape.
    fn strategy_at<'a>(
        &self,
        profile: &'a PublicProfile,
        node: u32,
        num_actions: usize,
        num_hands: usize,
    ) -> &'a [f32] {
        let strategy = profile
            .get(node as usize)
            .unwrap_or_else(|| panic!("the profile has no strategy at node {node}"));
        assert_eq!(
            strategy.len(),
            num_actions * num_hands,
            "the profile's strategy at node {node} has the wrong size"
        );
        strategy
    }
}

impl PublicGame for RiverGame {
    fn num_hands(&self, player: usize) -> usize {
        self.deal[player].len()
    }

    fn deal(&self, player: usize) -> &[f64] {
        &self.deal[player]
    }

    fn num_nodes(&self) -> usize {
        self.tree.nodes().len()
    }

    fn root(&self) -> usize {
        PostflopTree::ROOT as usize
    }

    fn node(&self, node: usize) -> PublicNode {
        match self.tree.node(node as u32) {
            Node::Player { actor, actions, .. } => PublicNode::Player {
                player: actor.index(),
                num_actions: actions.len(),
            },
            Node::Terminal { .. } => PublicNode::Terminal,
            Node::Chance { .. } => unreachable!("a river tree has no chance node"),
        }
    }

    fn child(&self, node: usize, index: usize) -> usize {
        self.tree.node(node as u32).children()[index] as usize
    }

    fn outcome_probability(&self, _node: usize, _outcome: usize) -> f64 {
        unreachable!("a river tree has no chance node")
    }

    fn rules_out(&self, _node: usize, _outcome: usize, _player: usize, _hand: usize) -> bool {
        unreachable!("a river tree has no chance node")
    }

    fn terminal_values(
        &self,
        node: usize,
        player: usize,
        opponent_reach: &[f64],
        values: &mut [f64],
    ) {
        let traverser = player_of(player);
        match self.payoffs[node] {
            Some(Payoff::Fold(payoffs)) => {
                self.hands
                    .fold_values(traverser, opponent_reach, payoffs[player], values);
            }
            Some(Payoff::Showdown(payoff)) => {
                self.hands
                    .showdown_values(traverser, opponent_reach, payoff, values);
            }
            None => panic!("node {node} is not a terminal"),
        }
    }
}

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
