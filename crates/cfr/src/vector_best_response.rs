//! Best response, expected values and exploitability in vector form, over the public tree of a
//! [`PublicGame`].
//!
//! The quantities are those [`best_response`](mod@crate::best_response) defines, `BR_i(σ)` and
//! `(BR_0(σ) + BR_1(σ)) / 2`, and the expected values that
//! [`expected_values`](crate::tabular::expected_values) gives, computed by one walk of the
//! public tree per player instead of one walk of the whole game tree. The walk carries the
//! opponent's reach for each of its hands, as in [`crate::vector`], and returns the player's
//! counterfactual values for each of its hands.
//!
//! # Best response
//!
//! Fix a profile `σ` and a responder `i` with opponent `j`. The walk starts from `ρ = deal(j)`,
//! multiplies `ρ(o)` by `σ(a | o)` at `j`'s nodes and by the outcome's probability at chance
//! nodes, and sets it to zero for the hands an outcome rules out. It returns, for each hand `h`
//! of the responder,
//!
//! ```text
//! q*(n, h) = Σ_o ρ(o) · v*_i(n, h, o)
//! ```
//!
//! where `v*_i` is the responder's value when it plays the best response below `n`. At a
//! terminal that comes from [`PublicGame::terminal_values`], at a chance node it is the sum
//! over the outcomes, at a node of `j` the sum over the actions, and it is zero for a hand
//! ruled out at `n`. At a node `n` of the responder,
//!
//! ```text
//! q*(n, h) = max_a q*(n·a, h)
//! ```
//!
//! and the best response plays the action that attains it, the lowest-numbered one when
//! several do (compared exactly, in `f64`). Each pair `(n, h)` is one information set `I`,
//! whose counterfactual action values in [`best_response`](mod@crate::best_response) are
//!
//! ```text
//! q(I, a) = deal(i)[h] · q*(n·a, h)
//! ```
//!
//! so this is the choice that module makes, up to the rounding of near-ties (and except where
//! `deal(i)[h]` is zero, where that module plays action 0). A hand ruled out at `n` has every
//! value zero and plays action 0. Then
//!
//! ```text
//! BR_i(σ) = Σ_h deal(i)[h] · q*(root, h)
//! ```
//!
//! in the game's units when the deal's probabilities sum to 1 (see [`PublicGame::deal`]).
//!
//! Every quantity is `f64`, and the `f32` probabilities of `σ` are widened exactly.
//!
//! # Expected values
//!
//! Player `i`'s expected value `v_i(σ)`, when both players play `σ`, comes from the same walk
//! with the player's own nodes following `σ` instead of maximising:
//!
//! ```text
//! q(n, h) = Σ_a σ(a | h) · q(n·a, h)
//! ```
//!
//! summed in action order, and
//!
//! ```text
//! v_i(σ) = Σ_h deal(i)[h] · q(root, h)
//! ```
//!
//! [`vector_expected_value`] computes it, and [`vector_expected_values`] gives both players'.
//! They need the profile's strategies at both players' nodes. Neither they nor the best
//! response assume that the game is zero-sum, so a game that is not, such as a raked pot, can
//! measure how much each player gains by deviating, `BR_i(σ) − v_i(σ)`: never negative, and
//! zero for both players exactly at a Nash equilibrium.
//!
//! # Working memory
//!
//! Each walk holds its working memory as [`crate::vector`]'s walk does: a node that has
//! children holds the vectors it passes down and the values they return in a frame from the
//! walk's stack of frames, so that the walk allocates its working memory at each depth once,
//! not at each node. The only allocation per node left is the best response's own strategy,
//! one vector at each node of the responder, which the walk returns. The working memory is the
//! walk's own and is dropped with it.
//!
//! # The parallel walk
//!
//! Called on a thread of a rayon pool of more than one thread, each walk splits the first
//! chance node on each line from the root as [`crate::vector`]'s walk does (see
//! [its parallel walk](crate::vector#the-parallel-walk)): the child after outcome `o` computes
//! its inputs from the chance node's, walks its subtree on one thread with a workspace of its
//! own, the walk's workspace `o` for a chance node's children, and writes its values into its
//! own slice of the chance node's frame; once every child has returned, the chance node adds
//! them up in outcome order, starting from zero. Every value is then the walk on one thread's,
//! bit for bit, whatever the number of threads. A best response's choice at a node depends only
//! on the values below it, and each node of the responder is set once, by the walk of the child
//! that holds it, so its strategy is the same too. The children's walks set their choices in
//! the one strategy the walk returns, through a lock.
//!
//! # Exploitability
//!
//! For a two-player zero-sum game, `(BR_0(σ) + BR_1(σ)) / 2`, which is zero exactly at a Nash
//! equilibrium (see [`best_response`](mod@crate::best_response)). The [`PublicGame`] trait
//! does not expose the payoffs, so [`vector_exploitability`] cannot check that the game is
//! zero-sum: the caller must know it is.
//!
//! # References
//!
//! - Johanson, Waugh, Bowling and Zinkevich, *Accelerating Best Response Calculation in Large
//!   Extensive Games*, IJCAI (2011): best response by one walk of the public tree, with a
//!   vector of the opponent's reaches over its private hands.

use std::ops::Range;
use std::sync::{Mutex, PoisonError};
use std::{iter, mem};

use crate::vector::{
    Frames, PublicGame, PublicNode, PublicProfile, check_game, refill, resized, splits_chance_nodes,
};

/// A best response of one player to the other's strategy, from [`vector_best_response`].
#[derive(Debug, Clone, PartialEq)]
pub struct VectorBestResponse {
    /// The player who responds.
    pub player: usize,
    /// `BR_i(σ)`, the responder's expected utility when it plays `strategy` against the other
    /// player's strategy.
    pub value: f64,
    /// `q*(root, h)` for each hand `h` of the responder: its counterfactual value at the root,
    /// without its own deal factor, so that `value` is the sum of `deal(i)[h] · q*(root, h)`.
    pub hand_values: Vec<f64>,
    /// A pure best response: at every node of the responder reachable from the root, for every
    /// hand, probability 1 on the chosen action and 0 on the others.
    pub strategy: PublicProfile,
}

/// Player `player`'s best response to the other player's strategy in `profile`, as the
/// [module documentation](self) defines it. The profile's strategies at `player`'s own nodes
/// are ignored, and may be missing. The walk runs on the threads of the rayon pool it is called
/// on, if it has more than one (see [the parallel walk](self#the-parallel-walk)).
///
/// Panics if `player` is not 0 or 1, if `profile` has no strategy, or one of the wrong size,
/// at a node of the other player reachable from the root, or for any reason
/// [`VectorCfr::new`](crate::vector::VectorCfr::new) panics on a malformed game.
pub fn vector_best_response<G: PublicGame + Sync>(
    game: &G,
    profile: &PublicProfile,
    player: usize,
) -> VectorBestResponse {
    assert!(player < 2, "player {player} is not 0 or 1");
    check_game(game, |_, _, _, _| {});
    let num_hands = game.num_hands(player);
    let mut strategy = PublicProfile::empty(game.num_nodes());
    let mut workspace = Workspace::default();
    let mut branches = Vec::new();
    let mut walk = Respond {
        game,
        profile,
        player,
        strategy: Choices::Own(&mut strategy),
        workspace: &mut workspace,
        branches: splits_chance_nodes().then_some(&mut branches),
    };
    let mut hand_values = vec![0.0; num_hands];
    let possible = vec![true; num_hands];
    walk.visit(
        game.root(),
        game.deal(1 - player),
        &possible,
        &mut hand_values,
    );
    let mut value = 0.0;
    for (&deal, &hand_value) in game.deal(player).iter().zip(&hand_values) {
        value += deal * hand_value;
    }
    VectorBestResponse {
        player,
        value,
        hand_values,
        strategy,
    }
}

/// `(BR_0(σ) + BR_1(σ)) / 2` for a two-player zero-sum game, as the
/// [module documentation](self) defines it. Zero exactly at a Nash equilibrium.
///
/// Panics for any reason [`vector_best_response`] panics.
pub fn vector_exploitability<G: PublicGame + Sync>(game: &G, profile: &PublicProfile) -> f64 {
    let first = vector_best_response(game, profile, 0).value;
    let second = vector_best_response(game, profile, 1).value;
    (first + second) / 2.0
}

/// One player's expected value under a profile, from [`vector_expected_value`].
#[derive(Debug, Clone, PartialEq)]
pub struct VectorExpectedValue {
    /// The player whose value it is.
    pub player: usize,
    /// `v_i(σ)`, the player's expected utility when both players follow the profile.
    pub value: f64,
    /// `q(root, h)` for each hand `h` of the player: its counterfactual value at the root,
    /// without its own deal factor, so that `value` is the sum of `deal(i)[h] · q(root, h)`.
    pub hand_values: Vec<f64>,
}

/// Player `player`'s expected value when both players follow `profile`, as the
/// [module documentation](self) defines it. The walk runs on the threads of the rayon pool it
/// is called on, if it has more than one (see [the parallel walk](self#the-parallel-walk)).
///
/// Panics if `player` is not 0 or 1, if `profile` has no strategy, or one of the wrong size,
/// at a player node reachable from the root, or for any reason
/// [`VectorCfr::new`](crate::vector::VectorCfr::new) panics on a malformed game.
pub fn vector_expected_value<G: PublicGame + Sync>(
    game: &G,
    profile: &PublicProfile,
    player: usize,
) -> VectorExpectedValue {
    assert!(player < 2, "player {player} is not 0 or 1");
    check_game(game, |_, _, _, _| {});
    let num_hands = game.num_hands(player);
    let mut workspace = Workspace::default();
    let mut branches = Vec::new();
    let mut walk = Follow {
        game,
        profile,
        player,
        workspace: &mut workspace,
        branches: splits_chance_nodes().then_some(&mut branches),
    };
    let mut hand_values = vec![0.0; num_hands];
    let possible = vec![true; num_hands];
    walk.visit(
        game.root(),
        game.deal(1 - player),
        &possible,
        &mut hand_values,
    );
    let mut value = 0.0;
    for (&deal, &hand_value) in game.deal(player).iter().zip(&hand_values) {
        value += deal * hand_value;
    }
    VectorExpectedValue {
        player,
        value,
        hand_values,
    }
}

/// Both players' expected values when they follow `profile`, `[v_0(σ), v_1(σ)]`, as the
/// [module documentation](self) defines them.
///
/// Panics for any reason [`vector_expected_value`] panics.
pub fn vector_expected_values<G: PublicGame + Sync>(game: &G, profile: &PublicProfile) -> [f64; 2] {
    [0, 1].map(|player| vector_expected_value(game, profile, player).value)
}

/// The strategy `profile` holds at `node`, checked against the node's shape.
fn strategy_at(
    profile: &PublicProfile,
    node: usize,
    player: usize,
    num_actions: usize,
    num_hands: usize,
) -> &[f32] {
    let strategy = profile.get(node).unwrap_or_else(|| {
        panic!("the profile has no strategy at node {node}, where player {player} acts")
    });
    assert_eq!(
        strategy.len(),
        num_actions * num_hands,
        "the profile's strategy at node {node} has the wrong size"
    );
    strategy
}

/// The working memory of an expected-value or best-response walk: see the
/// [module documentation](self#working-memory).
#[derive(Debug, Default)]
struct Workspace {
    frames: Frames<Frame>,
    /// The game's room at a terminal.
    terminal: Vec<f64>,
}

/// What a node of an expected-value or best-response walk holds while its children walk. A
/// field a node does not use keeps what it held.
#[derive(Debug, Default)]
struct Frame {
    /// One child's values, `q(n·a, h)` or `q*(n·a, h)`.
    values: Vec<f64>,
    /// The opponent's reach `ρ` passed to a child.
    opponent_reach: Vec<f64>,
    /// At a chance node, the player's possible hands after the outcome being walked.
    possible: Vec<bool>,
    /// At a node of the responder, the action each hand's best value so far comes from.
    choices: Vec<usize>,
}

impl Workspace {
    /// At the terminal `node`, writes `player`'s values `q(node, h)` into `values`, zero for
    /// the hands that are not `possible`.
    fn terminal<G: PublicGame>(
        &mut self,
        game: &G,
        node: usize,
        player: usize,
        opponent_reach: &[f64],
        possible: &[bool],
        values: &mut [f64],
    ) {
        game.terminal_values_with(node, player, opponent_reach, values, &mut self.terminal);
        for (value, &possible) in values.iter_mut().zip(possible) {
            if !possible {
                *value = 0.0;
            }
        }
    }
}

impl Frame {
    /// At the chance node `node`, walked for `player`, whose possible hands there are
    /// `possible` and whose opponent's reach is `opponent_reach`, writes what a child holds
    /// after outcome `outcome`: the player's possible hands, and the opponent's reach times the
    /// outcome's probability, zero for the hands it rules out.
    fn after_outcome<G: PublicGame>(
        &mut self,
        game: &G,
        node: usize,
        outcome: usize,
        player: usize,
        possible: &[bool],
        opponent_reach: &[f64],
    ) {
        let probability = game.outcome_probability(node, outcome);
        let after = refill(&mut self.possible, possible.iter().copied());
        for hand in game.hands_ruled_out(node, outcome, player) {
            after[hand] = false;
        }
        let other = opponent_reach.iter().map(|&reach| reach * probability);
        let other = refill(&mut self.opponent_reach, other);
        for hand in game.hands_ruled_out(node, outcome, 1 - player) {
            other[hand] = 0.0;
        }
    }
}

/// A chance node whose children walk as rayon tasks, with what their walks share: the chance
/// node's inputs, read by every child.
struct Split<'a, G> {
    game: &'a G,
    node: usize,
    player: usize,
    opponent_reach: &'a [f64],
    possible: &'a [bool],
}

impl<G: PublicGame + Sync> Split<'_, G> {
    /// Walks the chance node's `num_outcomes` children as the
    /// [module documentation](self#the-parallel-walk) describes, holding their values in a frame
    /// of `workspace`, and writes `q(node, h)` or `q*(node, h)` into `values`: their sum in
    /// outcome order, once every child has returned. Child `o` computes its inputs into a frame of
    /// `branches[o]`, which grows to the outcomes if it has fewer, and `walk_child(workspace,
    /// opponent_reach, possible, child, values)` walks it on its thread with that workspace.
    fn walk(
        &self,
        workspace: &mut Workspace,
        branches: &mut Vec<Workspace>,
        num_outcomes: usize,
        values: &mut [f64],
        walk_child: &(impl Fn(&mut Workspace, &[f64], &[bool], usize, &mut [f64]) + Sync),
    ) {
        let num_hands = values.len();
        if branches.len() < num_outcomes {
            branches.reserve_exact(num_outcomes - branches.len());
            branches.resize_with(num_outcomes, Workspace::default);
        }
        let mut frame = workspace.frames.take();
        let outcome_values = resized(&mut frame.values, num_outcomes * num_hands, 0.0);
        let walk_one = |outcome: usize, workspace: &mut Workspace, values: &mut [f64]| {
            let (game, node, player) = (self.game, self.node, self.player);
            let mut inputs = workspace.frames.take();
            inputs.after_outcome(
                game,
                node,
                outcome,
                player,
                self.possible,
                self.opponent_reach,
            );
            let child = game.child(node, outcome);
            walk_child(
                workspace,
                &inputs.opponent_reach,
                &inputs.possible,
                child,
                values,
            );
            workspace.frames.give(inputs);
        };
        fork(
            0..num_outcomes,
            &mut branches[..num_outcomes],
            outcome_values,
            num_hands,
            &walk_one,
        );
        values.fill(0.0);
        for child_values in outcome_values.chunks_exact(num_hands) {
            for (value, &child_value) in values.iter_mut().zip(child_values) {
                *value += child_value;
            }
        }
        workspace.frames.give(frame);
    }
}

/// Calls `walk_one(outcome, workspace, values)` for each of `outcomes`, with the workspace of its
/// place in `workspaces` and its place in `values`, `num_hands` entries an outcome: halves the
/// outcomes and walks the halves as two rayon tasks, down to one outcome a task.
fn fork(
    outcomes: Range<usize>,
    workspaces: &mut [Workspace],
    values: &mut [f64],
    num_hands: usize,
    walk_one: &(impl Fn(usize, &mut Workspace, &mut [f64]) + Sync),
) {
    if outcomes.len() == 1 {
        walk_one(outcomes.start, &mut workspaces[0], values);
        return;
    }
    let half = outcomes.len() / 2;
    let middle = outcomes.start + half;
    let (first_workspaces, second_workspaces) = workspaces.split_at_mut(half);
    let (first_values, second_values) = values.split_at_mut(half * num_hands);
    rayon::join(
        || {
            let outcomes = outcomes.start..middle;
            fork(
                outcomes,
                first_workspaces,
                first_values,
                num_hands,
                walk_one,
            );
        },
        || {
            let outcomes = middle..outcomes.end;
            fork(
                outcomes,
                second_workspaces,
                second_values,
                num_hands,
                walk_one,
            );
        },
    );
}

/// The working state of one expected-value walk, or of the walk of one child of a split chance
/// node.
struct Follow<'a, G> {
    game: &'a G,
    profile: &'a PublicProfile,
    player: usize,
    workspace: &'a mut Workspace,
    /// The workspaces of the children of a chance node, if the walk splits the chance nodes it
    /// reaches: see the [module documentation](self#the-parallel-walk).
    branches: Option<&'a mut Vec<Workspace>>,
}

impl<G: PublicGame + Sync> Follow<'_, G> {
    /// Walks the tree below `node`, writing `q(node, h)` for each hand `h` of the player into
    /// `values`. `opponent_reach` is `ρ`, and `possible` says which of the player's hands are
    /// possible at `node`.
    fn visit(
        &mut self,
        node: usize,
        opponent_reach: &[f64],
        possible: &[bool],
        values: &mut [f64],
    ) {
        let game = self.game;
        let num_hands = possible.len();
        match game.node(node) {
            PublicNode::Terminal => {
                let player = self.player;
                let workspace = &mut self.workspace;
                workspace.terminal(game, node, player, opponent_reach, possible, values);
            }
            PublicNode::Chance { num_outcomes } if self.branches.is_some() => {
                let (profile, player) = (self.profile, self.player);
                let branches = self
                    .branches
                    .as_deref_mut()
                    .expect("a walk that splits has the children's workspaces");
                let split = Split {
                    game,
                    node,
                    player,
                    opponent_reach,
                    possible,
                };
                let walk_child = |workspace: &mut Workspace,
                                  opponent_reach: &[f64],
                                  possible: &[bool],
                                  child: usize,
                                  values: &mut [f64]| {
                    let mut walk = Follow {
                        game,
                        profile,
                        player,
                        workspace,
                        branches: None,
                    };
                    walk.visit(child, opponent_reach, possible, values);
                };
                split.walk(self.workspace, branches, num_outcomes, values, &walk_child);
            }
            PublicNode::Chance { num_outcomes } => {
                let mut frame = self.workspace.frames.take();
                values.fill(0.0);
                for outcome in 0..num_outcomes {
                    frame.after_outcome(game, node, outcome, self.player, possible, opponent_reach);
                    let child = game.child(node, outcome);
                    let child_values = resized(&mut frame.values, num_hands, 0.0);
                    self.visit(child, &frame.opponent_reach, &frame.possible, child_values);
                    for (value, &child_value) in values.iter_mut().zip(&frame.values) {
                        *value += child_value;
                    }
                }
                self.workspace.frames.give(frame);
            }
            PublicNode::Player {
                player,
                num_actions,
            } if player == self.player => {
                let strategy = strategy_at(self.profile, node, player, num_actions, num_hands);
                let mut frame = self.workspace.frames.take();
                values.fill(0.0);
                for (action, probabilities) in strategy.chunks_exact(num_hands).enumerate() {
                    let child = game.child(node, action);
                    let child_values = resized(&mut frame.values, num_hands, 0.0);
                    self.visit(child, opponent_reach, possible, child_values);
                    for ((value, &p), &child_value) in
                        values.iter_mut().zip(probabilities).zip(&frame.values)
                    {
                        *value += f64::from(p) * child_value;
                    }
                }
                self.workspace.frames.give(frame);
            }
            PublicNode::Player {
                player,
                num_actions,
            } => {
                let num_other = opponent_reach.len();
                let strategy = strategy_at(self.profile, node, player, num_actions, num_other);
                let mut frame = self.workspace.frames.take();
                values.fill(0.0);
                for (action, probabilities) in strategy.chunks_exact(num_other).enumerate() {
                    let reach = opponent_reach
                        .iter()
                        .zip(probabilities)
                        .map(|(&reach, &p)| reach * f64::from(p));
                    let reach = refill(&mut frame.opponent_reach, reach);
                    let child = game.child(node, action);
                    let child_values = resized(&mut frame.values, num_hands, 0.0);
                    self.visit(child, reach, possible, child_values);
                    for (value, &child_value) in values.iter_mut().zip(&frame.values) {
                        *value += child_value;
                    }
                }
                self.workspace.frames.give(frame);
            }
        }
    }
}

/// Where a best-response walk sets the choices it makes.
enum Choices<'a> {
    /// The strategy the walk returns, which it alone sets.
    Own(&'a mut PublicProfile),
    /// The same, shared with the walks of the other children of a split chance node.
    Shared(&'a Mutex<PublicProfile>),
}

impl Choices<'_> {
    /// Sets the strategy at `node`, laid out `[actions × hands]` over `num_hands` hands.
    fn set(&mut self, node: usize, num_hands: usize, probabilities: Vec<f32>) {
        match self {
            Choices::Own(strategy) => strategy.set(node, num_hands, probabilities),
            Choices::Shared(strategy) => {
                let mut strategy = strategy.lock().unwrap_or_else(PoisonError::into_inner);
                strategy.set(node, num_hands, probabilities);
            }
        }
    }
}

/// The working state of one best-response walk, or of the walk of one child of a split chance
/// node.
struct Respond<'a, G> {
    game: &'a G,
    profile: &'a PublicProfile,
    player: usize,
    /// The choices made so far.
    strategy: Choices<'a>,
    workspace: &'a mut Workspace,
    /// The workspaces of the children of a chance node, if the walk splits the chance nodes it
    /// reaches: see the [module documentation](self#the-parallel-walk).
    branches: Option<&'a mut Vec<Workspace>>,
}

impl<G: PublicGame + Sync> Respond<'_, G> {
    /// Walks the tree below `node`, writing `q*(node, h)` for each hand `h` of the responder
    /// into `values`. `opponent_reach` is `ρ`, and `possible` says which of the responder's
    /// hands are possible at `node`.
    fn visit(
        &mut self,
        node: usize,
        opponent_reach: &[f64],
        possible: &[bool],
        values: &mut [f64],
    ) {
        let game = self.game;
        let num_hands = possible.len();
        match game.node(node) {
            PublicNode::Terminal => {
                let player = self.player;
                let workspace = &mut self.workspace;
                workspace.terminal(game, node, player, opponent_reach, possible, values);
            }
            PublicNode::Chance { num_outcomes } if self.branches.is_some() => {
                let (profile, player) = (self.profile, self.player);
                let branches = self
                    .branches
                    .as_deref_mut()
                    .expect("a walk that splits has the children's workspaces");
                let Choices::Own(strategy) = &mut self.strategy else {
                    unreachable!("only the walk from the root splits, and it owns its strategy");
                };
                // Moved, not copied, into the lock and back.
                let shared = Mutex::new(mem::replace(&mut **strategy, PublicProfile::empty(0)));
                let split = Split {
                    game,
                    node,
                    player,
                    opponent_reach,
                    possible,
                };
                let walk_child = |workspace: &mut Workspace,
                                  opponent_reach: &[f64],
                                  possible: &[bool],
                                  child: usize,
                                  values: &mut [f64]| {
                    let mut walk = Respond {
                        game,
                        profile,
                        player,
                        strategy: Choices::Shared(&shared),
                        workspace,
                        branches: None,
                    };
                    walk.visit(child, opponent_reach, possible, values);
                };
                split.walk(self.workspace, branches, num_outcomes, values, &walk_child);
                **strategy = shared.into_inner().unwrap_or_else(PoisonError::into_inner);
            }
            PublicNode::Chance { num_outcomes } => {
                let mut frame = self.workspace.frames.take();
                values.fill(0.0);
                for outcome in 0..num_outcomes {
                    frame.after_outcome(game, node, outcome, self.player, possible, opponent_reach);
                    let child = game.child(node, outcome);
                    let child_values = resized(&mut frame.values, num_hands, 0.0);
                    self.visit(child, &frame.opponent_reach, &frame.possible, child_values);
                    for (value, &child_value) in values.iter_mut().zip(&frame.values) {
                        *value += child_value;
                    }
                }
                self.workspace.frames.give(frame);
            }
            PublicNode::Player {
                player,
                num_actions,
            } if player == self.player => {
                let mut frame = self.workspace.frames.take();
                // The best value so far, and the action it comes from.
                values.fill(f64::NEG_INFINITY);
                refill(&mut frame.choices, iter::repeat_n(0, num_hands));
                for action in 0..num_actions {
                    let child = game.child(node, action);
                    let child_values = resized(&mut frame.values, num_hands, 0.0);
                    self.visit(child, opponent_reach, possible, child_values);
                    for hand in 0..num_hands {
                        if frame.values[hand] > values[hand] {
                            values[hand] = frame.values[hand];
                            frame.choices[hand] = action;
                        }
                    }
                }
                let mut pure = vec![0.0; num_actions * num_hands];
                for (hand, &action) in frame.choices.iter().enumerate() {
                    pure[action * num_hands + hand] = 1.0;
                }
                self.strategy.set(node, num_hands, pure);
                self.workspace.frames.give(frame);
            }
            PublicNode::Player {
                player,
                num_actions,
            } => {
                let strategy = self.profile.get(node).unwrap_or_else(|| {
                    panic!("the profile has no strategy at node {node}, where player {player} acts")
                });
                let num_other = opponent_reach.len();
                assert_eq!(
                    strategy.len(),
                    num_actions * num_other,
                    "the profile's strategy at node {node} has the wrong size"
                );
                let mut frame = self.workspace.frames.take();
                values.fill(0.0);
                for (action, probabilities) in strategy.chunks_exact(num_other).enumerate() {
                    let reach = opponent_reach
                        .iter()
                        .zip(probabilities)
                        .map(|(&reach, &p)| reach * f64::from(p));
                    let reach = refill(&mut frame.opponent_reach, reach);
                    let child = game.child(node, action);
                    let child_values = resized(&mut frame.values, num_hands, 0.0);
                    self.visit(child, reach, possible, child_values);
                    for (value, &child_value) in values.iter_mut().zip(&frame.values) {
                        *value += child_value;
                    }
                }
                self.workspace.frames.give(frame);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use rand_chacha::ChaCha8Rng;
    use rand_chacha::rand_core::{Rng, SeedableRng};

    use super::*;
    use crate::best_response::{best_response, exploitability};
    use crate::leduc::{Leduc, LeducAction, LeducInfoSet};
    use crate::public_leduc::PublicLeduc;
    use crate::regret::{Dcfr, Schedule};
    use crate::tabular::{Cfr, Profile, expected_values};
    use crate::vector::VectorCfr;

    /// Every Leduc information set, each with a uniform strategy.
    fn uniform() -> Profile<LeducInfoSet> {
        Cfr::new(Leduc, Schedule::Vanilla).average_strategy()
    }

    /// A random profile over every information set, with about one probability in five
    /// exactly zero.
    fn random_profile(rng: &mut ChaCha8Rng) -> Profile<LeducInfoSet> {
        uniform()
            .iter()
            .map(|(key, strategy)| {
                let mut weights: Vec<f64> = (0..strategy.len())
                    .map(|_| {
                        let unit = (rng.next_u64() >> 11) as f64 / (1u64 << 53) as f64;
                        if unit < 0.2 { 0.0 } else { unit }
                    })
                    .collect();
                if weights.iter().all(|&w| w == 0.0) {
                    weights[0] = 1.0;
                }
                let total: f64 = weights.iter().sum();
                (*key, weights.iter().map(|w| (w / total) as f32).collect())
            })
            .collect()
    }

    /// The pure profile that plays `choose(key)` at every information set.
    fn pure(choose: impl Fn(&LeducInfoSet) -> LeducAction) -> Profile<LeducInfoSet> {
        uniform()
            .iter()
            .map(|(key, _)| {
                let chosen = choose(key);
                let row = key.actions().iter().map(|&a| f32::from(a == chosen));
                (*key, row.collect())
            })
            .collect()
    }

    /// The vector solver's average or current strategy after `iterations` of `schedule`, as a
    /// scalar profile.
    fn solved(schedule: Schedule, iterations: u64, average: bool) -> Profile<LeducInfoSet> {
        let game = PublicLeduc::new();
        let mut solver = VectorCfr::new(game.clone(), schedule);
        solver.run(iterations, |_| {});
        if average {
            game.profile(&solver.average_strategy())
        } else {
            game.profile(&solver.current_strategy())
        }
    }

    /// Profiles to respond to: uniform, random with zeros, pure, and CFR's strategies early on,
    /// none of them an equilibrium.
    fn profiles() -> Vec<(&'static str, Profile<LeducInfoSet>)> {
        let mut rng = ChaCha8Rng::seed_from_u64(29);
        let dcfr = Schedule::Discounted(Dcfr::default());
        vec![
            ("uniform", uniform()),
            ("random 1", random_profile(&mut rng)),
            ("random 2", random_profile(&mut rng)),
            ("random 3", random_profile(&mut rng)),
            ("always the first action", pure(|key| key.actions()[0])),
            (
                "always bet or raise",
                pure(|key| {
                    if key.actions().contains(&LeducAction::Raise) {
                        LeducAction::Raise
                    } else {
                        LeducAction::Call
                    }
                }),
            ),
            ("CFR+ average after 10", solved(Schedule::CfrPlus, 10, true)),
            ("DCFR average after 100", solved(dcfr, 100, true)),
            ("DCFR current after 30", solved(dcfr, 30, false)),
        ]
    }

    /// `profile` with the strategies of `response` in place of its own.
    fn with_response(
        profile: &Profile<LeducInfoSet>,
        response: &Profile<LeducInfoSet>,
    ) -> Profile<LeducInfoSet> {
        let mut merged = profile.clone();
        for (key, strategy) in response.iter() {
            merged.insert(*key, strategy.to_vec());
        }
        merged
    }

    #[test]
    fn best_responses_match_the_scalar_ones_on_leduc() {
        // For each profile and player: the value equals the scalar best response's, the pure
        // strategy is worth that value when the scalar expected_values plays it, and it makes
        // the scalar response's choice at every information set. Measured, the values agree
        // within 8.9e-16, and no near-tie is broken the other way.
        let game = PublicLeduc::new();
        for (name, profile) in profiles() {
            let public = game.public_profile(&profile);
            for player in 0..2 {
                let scalar = best_response(&Leduc, &profile, player);
                let vector = vector_best_response(&game, &public, player);
                assert_eq!(vector.player, player);
                assert!(
                    (vector.value - scalar.value).abs() < 1e-12,
                    "{name}, player {player}: {} against {}",
                    vector.value,
                    scalar.value
                );
                let mut total = 0.0;
                for (&deal, &value) in game.deal(player).iter().zip(&vector.hand_values) {
                    total += deal * value;
                }
                assert_eq!(total, vector.value);
                let strategy = game.profile(&vector.strategy);
                assert_eq!(strategy.len(), 468, "{name}");
                let played = expected_values(&Leduc, &with_response(&profile, &strategy));
                assert!(
                    (played[player] - scalar.value).abs() < 1e-12,
                    "{name}, player {player}: playing the response is worth {}",
                    played[player]
                );
                for (key, row) in strategy.iter() {
                    assert_eq!(key.player(), player);
                    assert_eq!(row, &scalar.strategy[key], "{name}: {key:?}");
                }
            }
            let scalar = exploitability(&Leduc, &profile);
            let vector = vector_exploitability(&game, &public);
            assert!(
                (vector - scalar).abs() < 1e-12,
                "{name}: {vector} against {scalar}"
            );
            assert!(scalar > 1e-3, "{name} is close to an equilibrium: {scalar}");
        }
    }

    #[test]
    fn the_responders_own_strategy_is_ignored() {
        let game = PublicLeduc::new();
        let mut rng = ChaCha8Rng::seed_from_u64(7);
        let profile = random_profile(&mut rng);
        let other = random_profile(&mut rng);
        for player in 0..2 {
            let response = vector_best_response(&game, &game.public_profile(&profile), player);
            let mut changed = profile.clone();
            for (key, strategy) in other.iter() {
                if key.player() == player {
                    changed.insert(*key, strategy.to_vec());
                }
            }
            let public = game.public_profile(&changed);
            assert_eq!(vector_best_response(&game, &public, player), response);
            // With the responder's nodes left out of the profile altogether.
            let mut missing = PublicProfile::empty(game.num_nodes());
            for node in public.nodes() {
                if let PublicNode::Player { player: acting, .. } = game.node(node)
                    && acting != player
                {
                    missing.set(node, 6, public.get(node).unwrap().to_vec());
                }
            }
            assert_eq!(vector_best_response(&game, &missing, player), response);
        }
    }

    #[test]
    fn vector_cfr_reaches_the_scalar_thresholds_on_leduc() {
        // The thresholds of the scalar solver's fast tests after 500 iterations, measured here by
        // the vector best response and checked against the scalar one. Measured, the vector
        // solver's average strategy is 4.7320e-4 exploitable under DCFR, as the scalar
        // solver's is, the two runs being bit for bit the same, and 9.2392e-4 under CFR+,
        // against the scalar solver's 9.3555e-4, the two runs having parted by rounding (see
        // `public_leduc`). Its best responses bracket Leduc's value, -0.085606.
        let game = PublicLeduc::new();
        let cases = [
            (Schedule::CfrPlus, 1.2e-3),
            (Schedule::Discounted(Dcfr::default()), 6e-4),
        ];
        for (schedule, threshold) in cases {
            let mut solver = VectorCfr::new(game.clone(), schedule);
            solver.run(500, |_| {});
            let average = solver.average_strategy();
            let measured = vector_exploitability(&game, &average);
            assert!(measured < threshold, "{schedule:?}: {measured:e}");
            let scalar = exploitability(&Leduc, &game.profile(&average));
            assert!(
                (measured - scalar).abs() < 1e-12,
                "{measured} against {scalar}"
            );
            let low = -vector_best_response(&game, &average, 1).value;
            let high = vector_best_response(&game, &average, 0).value;
            assert!(
                low - 5e-7 <= -0.085606 && -0.085606 <= high + 5e-7,
                "[{low}, {high}]"
            );
        }
    }

    #[test]
    fn on_several_threads_the_walks_give_what_they_give_on_one_bit_for_bit() {
        // Leduc's chance nodes are split on a pool of more than one thread, here with more
        // threads than the CI's machines have cores. Each best response's value, hand values
        // and pure strategy, each expected value and the exploitability are those of the walks
        // on the calling thread, bit for bit.
        let game = PublicLeduc::new();
        let bits = |values: &[f64]| -> Vec<u64> { values.iter().map(|v| v.to_bits()).collect() };
        let walks = |public: &PublicProfile| {
            let responses = [0, 1].map(|p| vector_best_response(&game, public, p));
            let values = [0, 1].map(|p| vector_expected_value(&game, public, p));
            (responses, values, vector_exploitability(&game, public))
        };
        for (name, profile) in profiles() {
            let public = game.public_profile(&profile);
            let (responses, values, exploitability) = walks(&public);
            for threads in [2, 5, 16] {
                let pool = rayon::ThreadPoolBuilder::new()
                    .num_threads(threads)
                    .build()
                    .unwrap();
                let (pooled, pooled_values, pooled_exploitability) =
                    pool.install(|| walks(&public));
                let at = format!("{name}, {threads} threads");
                for player in 0..2 {
                    let (one, many) = (&responses[player], &pooled[player]);
                    assert_eq!(many.value.to_bits(), one.value.to_bits(), "{at}");
                    assert_eq!(bits(&many.hand_values), bits(&one.hand_values), "{at}");
                    assert_eq!(many.strategy, one.strategy, "{at}");
                    let (one, many) = (&values[player], &pooled_values[player]);
                    assert_eq!(many.value.to_bits(), one.value.to_bits(), "{at}");
                    assert_eq!(bits(&many.hand_values), bits(&one.hand_values), "{at}");
                }
                assert_eq!(
                    pooled_exploitability.to_bits(),
                    exploitability.to_bits(),
                    "{at}"
                );
            }
        }
    }

    #[test]
    #[should_panic(expected = "no strategy at node")]
    fn on_several_threads_a_missing_strategy_below_a_chance_node_is_reported() {
        // The panic of a child's walk reaches the caller through the pool.
        let game = PublicLeduc::new();
        let full = game.public_profile(&uniform());
        let below = (0..game.num_nodes())
            .find(|&node| {
                game.public_card(node).is_some()
                    && matches!(game.node(node), PublicNode::Player { player: 1, .. })
            })
            .unwrap();
        let mut partial = PublicProfile::empty(game.num_nodes());
        for node in full.nodes().filter(|&node| node != below) {
            partial.set(node, 6, full.get(node).unwrap().to_vec());
        }
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(4)
            .build()
            .unwrap();
        pool.install(|| vector_best_response(&game, &partial, 0));
    }

    #[test]
    #[should_panic(expected = "no strategy at node")]
    fn the_other_players_strategies_are_required() {
        let game = PublicLeduc::new();
        vector_best_response(&game, &PublicProfile::empty(game.num_nodes()), 0);
    }

    #[test]
    fn expected_values_match_the_scalar_ones_on_leduc() {
        // For each profile and player, the value equals what the scalar expected_values gives,
        // and is the deal-weighted sum of the hand values. Measured, the values agree within
        // 3.4e-16. Leduc is zero-sum, so the two values cancel.
        let game = PublicLeduc::new();
        for (name, profile) in profiles() {
            let public = game.public_profile(&profile);
            let scalar = expected_values(&Leduc, &profile);
            let vector = vector_expected_values(&game, &public);
            for player in 0..2 {
                let single = vector_expected_value(&game, &public, player);
                assert_eq!(single.player, player);
                assert_eq!(single.value, vector[player]);
                assert_eq!(single.hand_values.len(), 6);
                let mut total = 0.0;
                for (&deal, &value) in game.deal(player).iter().zip(&single.hand_values) {
                    total += deal * value;
                }
                assert_eq!(total, single.value);
                assert!(
                    (vector[player] - scalar[player]).abs() < 1e-12,
                    "{name}, player {player}: {} against {}",
                    vector[player],
                    scalar[player]
                );
            }
            assert!((vector[0] + vector[1]).abs() < 1e-12, "{name}: {vector:?}");
        }
    }

    #[test]
    fn a_player_who_plays_its_best_response_gets_its_value() {
        // Replacing the player's own strategies by its best response makes its expected value
        // the best response's, which is at least its value under the profile itself. In a
        // zero-sum game the mean of the two gains is the exploitability.
        let game = PublicLeduc::new();
        for (name, profile) in profiles() {
            let public = game.public_profile(&profile);
            for player in 0..2 {
                let response = vector_best_response(&game, &public, player);
                let mut merged = public.clone();
                for node in response.strategy.nodes() {
                    merged.set(node, 6, response.strategy.get(node).unwrap().to_vec());
                }
                let played = vector_expected_value(&game, &merged, player);
                assert!(
                    (played.value - response.value).abs() < 1e-12,
                    "{name}, player {player}: {} against {}",
                    played.value,
                    response.value
                );
                let own = vector_expected_value(&game, &public, player).value;
                assert!(response.value >= own - 1e-12, "{name}, player {player}");
            }
            let gains: f64 = (0..2)
                .map(|player| {
                    vector_best_response(&game, &public, player).value
                        - vector_expected_value(&game, &public, player).value
                })
                .sum();
            let exploitability = vector_exploitability(&game, &public);
            assert!(
                (gains / 2.0 - exploitability).abs() < 1e-12,
                "{name}: {} against {exploitability}",
                gains / 2.0
            );
        }
    }

    #[test]
    #[should_panic(expected = "no strategy at node")]
    fn expected_values_need_both_players_strategies() {
        // Only player 1's strategies, so player 0's own nodes are missing.
        let game = PublicLeduc::new();
        let full = game.public_profile(&uniform());
        let mut partial = PublicProfile::empty(game.num_nodes());
        for node in full.nodes() {
            if game.node(node)
                == (PublicNode::Player {
                    player: 1,
                    num_actions: full.get(node).unwrap().len() / 6,
                })
            {
                partial.set(node, 6, full.get(node).unwrap().to_vec());
            }
        }
        vector_expected_value(&game, &partial, 0);
    }
}
