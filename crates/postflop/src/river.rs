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
//! to a node with each hand. Summing a strategy over the acting player's range at a node can
//! then answer two different questions, and the two answers differ.
//!
//! **The probability of each action once the node is reached.** [`RiverGame::hand_weights`]
//! gives the probability that the acting player holds each hand once the node is reached. For
//! the acting player `i`, the opponent `j` and own reaches `π` under a profile,
//!
//! ```text
//! P(h | n) ∝ deal(i)[h] · π_i(n, h) · Σ_o deal(j)[o] · π_j(n, o)    over o sharing no card with h
//! ```
//!
//! which is exact, card removal by the opponent's range at the node included.
//! [`RiverGame::action_frequencies`] is then `Σ_h P(h | n) · σ(a | h)`: how often the action
//! is actually taken when play reaches the node. Use it for the probabilities of the play, such
//! as how often a bet is called, or for anything weighted by how often a line happens.
//!
//! **What a range view shows.** [`RiverGame::range_view_weights`] weighs each hand by the
//! player's own combos alone, its range weight times its own reach:
//!
//! ```text
//! W(h | n) = w_i(h) · π_i(n, h) / Σ_h' w_i(h') · π_i(n, h')
//! ```
//!
//! with no card removal by the opponent's range, and [`RiverGame::range_view_frequencies`] is
//! `Σ_h W(h | n) · σ(a | h)`. This is what a solver's range view shows, and what a person
//! compares between two solvers, so the cross-solver comparison
//! ([`compare`](mod@crate::compare)) uses it.
//!
//! The two differ wherever the opponent's range at the node blocks some of the player's hands
//! more than others. On `four_flush`, solved to 0.05% of the pot, IP's re-raise facing OOP's
//! check-raise, a node reached about 4% of the time, is 10.1% by the first and 18.7% by the
//! second: the hands IP re-raises with share cards with OOP's check-raising range more often
//! than its other hands do. A test pins the two apart.
//!
//! **A range view by hand class.** [`RiverGame::range_view_classes`] sums the range view over
//! each of the 169 hand classes ([`HandClass`]) in the acting player's range at the node: the
//! class's share of the range, `S(c | n) = Σ W(h | n)` over its hands, and each action's
//! frequency over its hands under the same weights, `Σ W(h | n) · σ(a | h) / S(c | n)`. The
//! shares sum to 1, and the shares times the class frequencies sum to the range view's
//! frequencies.
//!
//! **How often a node is reached.** [`RiverGame::reach_probability`] is the probability that a
//! hand dealt plays to the node: `Σ deal(0)[h] · π_0(n, h) · deal(1)[o] · π_1(n, o)` over the
//! pairs of hands that share no card.
//!
//! [`VectorCfr`]: pk_cfr::VectorCfr

use std::collections::BTreeMap;

use pk_cfr::{PublicGame, PublicNode, PublicProfile};
use pk_core::HandClass;
use pk_game::{Chips, Node, Player, PostflopTree, Rake};

use crate::hands::HandList;
use crate::payoff::Payoff;
use crate::solve::SolveError;
use crate::spot::Spot;
use crate::terminal::BoardHands;

/// One hand class in the acting player's range at a node, as a range view shows it: see
/// [`RiverGame::range_view_classes`].
#[derive(Debug, Clone, PartialEq)]
pub struct ClassView {
    pub class: HandClass,
    /// The acting player's hands in the class, as hand indices in hand order, whether or not
    /// each plays to the node.
    pub hands: Vec<usize>,
    /// The class's share of the range at the node: the sum of its hands'
    /// [`range_view_weights`](RiverGame::range_view_weights). Always more than 0.
    pub share: f64,
    /// Each action's frequency over the class's hands, each hand weighted by its range-view
    /// weight, in the order of the node's actions.
    pub frequencies: Vec<f64>,
}

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
    /// [`RiverGame::hand_weights`], card removal by the opponent's range included. This is how
    /// often each action is actually taken when play reaches the node. It is not what a range
    /// view shows, which [`RiverGame::range_view_frequencies`] gives: see the
    /// [module documentation](self#ranges-at-a-node).
    ///
    /// `None` if `node` is a terminal, or if `profile` reaches it with probability zero. Panics
    /// as [`RiverGame::reach`] does, or if `profile` has no strategy at `node`.
    pub fn action_frequencies(&self, profile: &PublicProfile, node: u32) -> Option<Vec<f64>> {
        let weights = self.hand_weights(profile, node)?;
        Some(self.frequencies_with(profile, node, &weights))
    }

    /// At a player node, each hand's share of the acting player's range there by its own
    /// combos alone: its range weight times its own reach of the node under `profile`,
    /// normalised to sum to 1. The opponent's range does not enter, so there is no card removal
    /// by it. This is the weighting of a solver's range view: see the
    /// [module documentation](self#ranges-at-a-node).
    ///
    /// `None` if `node` is a terminal, or if the acting player's own reach of it is zero with
    /// every hand. Panics as [`RiverGame::reach`] does.
    pub fn range_view_weights(&self, profile: &PublicProfile, node: u32) -> Option<Vec<f64>> {
        let Node::Player { actor, .. } = self.tree.node(node) else {
            return None;
        };
        let me = actor.index();
        let reach = self.reach(profile, node);
        // The deal is the range weight over √Z, which the normalisation cancels.
        let mut weights: Vec<f64> = self.deal[me]
            .iter()
            .zip(&reach[me])
            .map(|(d, r)| d * r)
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

    /// At a player node, each action's frequency as a range view shows it:
    /// `Σ_h W(h | n) · σ(a | h)` with the weights of [`RiverGame::range_view_weights`], the
    /// acting player's own combos weighted by range weight and own reach, with no card removal
    /// by the opponent's range. Use it to compare a strategy with another solver's range view.
    /// For how often each action is actually taken at the node, use
    /// [`RiverGame::action_frequencies`].
    ///
    /// `None` if `node` is a terminal, or if the acting player's own reach of it is zero with
    /// every hand. Panics as [`RiverGame::reach`] does, or if `profile` has no strategy at
    /// `node`.
    pub fn range_view_frequencies(&self, profile: &PublicProfile, node: u32) -> Option<Vec<f64>> {
        let weights = self.range_view_weights(profile, node)?;
        Some(self.frequencies_with(profile, node, &weights))
    }

    /// At a player node, each hand class in the acting player's range there, in grid order:
    /// its share of the range and its frequency for each action, both as a range view weighs
    /// them ([`RiverGame::range_view_weights`]), as the
    /// [module documentation](self#ranges-at-a-node) defines them. A class is in the range when
    /// one of its hands has a positive weight there; the other classes are left out.
    ///
    /// `None` if `node` is a terminal, or if the acting player's own reach of it is zero with
    /// every hand. Panics as [`RiverGame::reach`] does, or if `profile` has no strategy at
    /// `node`.
    pub fn range_view_classes(&self, profile: &PublicProfile, node: u32) -> Option<Vec<ClassView>> {
        let weights = self.range_view_weights(profile, node)?;
        let Node::Player { actor, actions, .. } = self.tree.node(node) else {
            unreachable!("the weights are None at a terminal");
        };
        let combos = self.hands(*actor).combos();
        let strategy = self.strategy_at(profile, node, actions.len(), combos.len());
        let mut by_class: BTreeMap<HandClass, Vec<usize>> = BTreeMap::new();
        for (hand, &combo) in combos.iter().enumerate() {
            by_class.entry(HandClass::of(combo)).or_default().push(hand);
        }
        let classes = by_class
            .into_iter()
            .filter_map(|(class, hands)| {
                let share: f64 = hands.iter().map(|&h| weights[h]).sum();
                if share <= 0.0 {
                    return None;
                }
                let frequencies = (0..actions.len())
                    .map(|a| {
                        let total: f64 = hands
                            .iter()
                            .map(|&h| weights[h] * f64::from(strategy[a * combos.len() + h]))
                            .sum();
                        total / share
                    })
                    .collect();
                Some(ClassView {
                    class,
                    hands,
                    share,
                    frequencies,
                })
            })
            .collect();
        Some(classes)
    }

    /// The probability that a hand dealt plays to `node` under `profile`:
    /// `Σ deal(0)[h] · π_0(n, h) · deal(1)[o] · π_1(n, o)` over the pairs of hands that share no
    /// card. It is 1 at the root, up to rounding, and it is defined at a terminal too.
    ///
    /// Panics as [`RiverGame::reach`] does.
    pub fn reach_probability(&self, profile: &PublicProfile, node: u32) -> f64 {
        let reach = self.reach(profile, node);
        let ip: Vec<f64> = reach[1]
            .iter()
            .zip(&self.deal[1])
            .map(|(r, d)| r * d)
            .collect();
        let mut met = vec![0.0; reach[0].len()];
        self.hands.fold_values(Player::Oop, &ip, 1.0, &mut met);
        self.deal[0]
            .iter()
            .zip(&reach[0])
            .zip(&met)
            .map(|((d, r), m)| d * r * m)
            .sum()
    }

    /// `Σ_h weights[h] · σ(a | h)` for each action at the player node `node`.
    fn frequencies_with(&self, profile: &PublicProfile, node: u32, weights: &[f64]) -> Vec<f64> {
        let Node::Player { actions, .. } = self.tree.node(node) else {
            unreachable!("the weights are None at a terminal");
        };
        let strategy = self.strategy_at(profile, node, actions.len(), weights.len());
        strategy
            .chunks_exact(weights.len())
            .map(|probabilities| {
                probabilities
                    .iter()
                    .zip(weights)
                    .map(|(&p, &w)| f64::from(p) * w)
                    .sum()
            })
            .collect()
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
