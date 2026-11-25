//! Vector-form CFR: counterfactual regret minimization that walks the public tree of a
//! two-player game once per player and iteration, handling every private hand at once.
//!
//! # Public games
//!
//! A [`PublicGame`] is a two-player game in which every move is seen by both players except the
//! deal of one private hand to each. Player `p` holds one of the hands `0..num_hands(p)`.
//! Chance deals them before the root: player 0 gets `h0` and player 1 gets `h1` with
//! probability
//!
//! ```text
//! deal(0)[h0] · deal(1)[h1]    if the two hands are compatible, and 0 otherwise
//! ```
//!
//! What compatible means is the game's business (two hands that share a card are not), and
//! only [`PublicGame::terminal_values`] needs it.
//!
//! From the root on, the game is a tree of public nodes, addressed by index, that both players
//! see: terminals, chance nodes and player nodes. A player node's actions and a chance node's
//! outcomes lead to its children. An outcome may *rule out* hands: it cannot happen while a
//! player holds one of them, as a card dealt face up cannot happen while a player holds it. For
//! any two compatible hands it leaves possible, outcome `o` has the same probability,
//! [`PublicGame::outcome_probability`], and hands it rules out stay ruled out below it. So the
//! probabilities of a chance node sum to 1 over the outcomes that a pair of hands leaves
//! possible, not over all its outcomes: Leduc's public card has six outcomes of 1/4 each, four
//! of them possible for any two private cards.
//!
//! An information set of player `p` is a pair (player node of `p`, hand of `p`) whose hand is
//! possible at the node: the player knows its hand and everything public. Every history of it
//! comes after the same public actions, so the game has perfect recall.
//!
//! # Storage
//!
//! [`VectorCfr`] keeps, for each player node, one slice of regrets and one of strategy sums,
//! each laid out `[actions × hands]` over all the acting player's hands as [`crate::regret`]
//! specifies, and updates them with its functions. The entries of a hand ruled out at the node
//! are kept too, and stay at zero (see below).
//!
//! # One iteration
//!
//! [`VectorCfr`] computes the iteration that [`crate::tabular`] specifies for [`Cfr`],
//! information set by information set. Iteration `t` updates player 0, then player 1, with
//! `d = schedule.discount(t)`. For the traverser `i`, with opponent `j`:
//!
//! 1. Strategies are the [`current_strategy`] of each node's regrets as they stand, so player
//!    1's walk sees player 0's regrets after iteration `t`. A node's strategy is computed when
//!    the walk reaches it. The walk reaches each node once, and updates a node of the traverser
//!    only once it is done with the node's children, so the profile is fixed for the whole
//!    walk.
//! 2. One depth-first walk from the root visits every node, every action and every outcome in
//!    index order, including those with probability zero. It carries two vectors:
//!    - the opponent's reach `ρ(o)` for each hand `o` of player `j`, which starts at
//!      `deal(j)`, is multiplied by `σ(a | o)` at `j`'s nodes and by the outcome's probability
//!      at chance nodes, and is set to zero for the hands an outcome rules out;
//!    - the traverser's own reach `π(h)` for each hand `h` of player `i`, which starts at 1,
//!      is multiplied by `σ(a | h)` at `i`'s nodes, and is set to zero for ruled-out hands.
//!
//!    For the history in which `i` holds `h` and `j` holds `o`, `π(h)` is `π_i` of the scalar
//!    specification and `deal(i)[h] · ρ(o)` is `π_{-i}`, chance included.
//! 3. The walk returns, for each hand `h` of the traverser, the counterfactual value without
//!    its own deal factor, `q(n, h) = Σ_o ρ(o) · v_i(n, h, o)` over the opponent's compatible
//!    hands. A terminal's comes from [`PublicGame::terminal_values`]. A chance node's is the
//!    sum of its outcomes'. An opponent's node's is the sum of its actions'. At the
//!    traverser's node it is `Σ_a σ(a | h) · q(n·a, h)`, summed in action order. A ruled-out
//!    hand's is zero, whatever the game wrote for it.
//! 4. At each node `n` of the traverser, once its children are done, the instantaneous regret
//!    of the information set `(n, h)` is
//!
//!    ```text
//!    r(a, h) = deal(i)[h] · (q(n·a, h) − q(n, h))
//!    ```
//!
//!    which is the scalar `r(I, a) = Σ_{h ∈ I} π_{-i}(h) · (v_i(h·a) − v_i(h))`, summed over
//!    the opponent's hands. The node's slice gets exactly one [`update_regrets`] with `r`
//!    rounded to `f32` and `d`, then one [`accumulate_strategy`] with `σ`, `π` rounded to
//!    `f32` and `d`.
//!
//! The deal factors and outcome probabilities are kept, not dropped as [`crate::tabular`]
//! would allow, so the stored values are the scalar solver's, on the same scale, and round to
//! `f32` the same way. Reaches, values and regrets are `f64` until the update, as in the scalar
//! solver. They are summed in another order, so the two solvers agree up to rounding rather
//! than bit for bit: on Leduc, one iteration from the same stored state gives every stored
//! value within one or two `f32` ulps of the scalar solver's. Under some schedules the
//! iterations amplify such a difference, so that two runs which round differently part after
//! enough iterations. [`crate::public_leduc`] gives the measurements.
//!
//! A hand ruled out at a node gets `r = 0` and reach 0 there on every iteration, so its regrets
//! and strategy sum stay at zero and it plays uniformly. Its strategy never matters: its own
//! reach, or the opponent's reach for it, is zero below the outcome that ruled it out, and
//! every product it enters is multiplied by that zero.
//!
//! # Working memory
//!
//! The walk does not allocate per node. A node that has children holds, while they walk, what
//! it passes down to them and what they return: at a player node its strategy `σ`, the reach
//! after each action and the children's values `q(n·a, h)`; at a chance node the traverser's
//! possible hands after each outcome, both reaches after it and the outcome's values. It holds
//! them in a frame that it takes from the solver's stack of frames when it is reached and gives
//! back when it returns. Nodes return in the reverse order they are reached, so the node at
//! depth `d` always takes the frame the last node at depth `d` gave back, and the walk's inputs
//! at the root, `π = 1` and every hand possible, are held in one frame more. What a node needs
//! only once its children are done, `r` and `π` rounded to `f32`, and the game's room at a
//! terminal ([`PublicGame::terminal_values_with`]), are needed by one node at a time, and are
//! kept once. A buffer grows only when a node needs more than it holds, and then to exactly
//! what that node needs, so after the first iteration an iteration allocates nothing.
//!
//! None of this changes a number: every buffer is written in full before it is read, and each
//! value is computed from the same values in the same order as with vectors made at each node.
//! A walk of a subtree needs only a stack of frames of its own, which may start empty, so that
//! walks of different subtrees can each have one, as the parallel walk's do.
//!
//! # The parallel walk
//!
//! Called on a thread of a rayon pool of more than one thread, [`VectorCfr::iterate`] walks the
//! children of the first chance node on each line from the root as rayon tasks, which the pool's
//! threads share out. Called on a pool of one thread, or outside any pool, it walks the whole
//! tree on the calling thread, as above. The thread count is the pool's: a caller that wants `n`
//! threads runs the iteration in a pool of `n`, with `rayon::ThreadPool::install`.
//!
//! At such a chance node, the walk of the child after outcome `o`:
//!
//! - computes its inputs, the traverser's possible hands, `π` and `ρ` after the outcome, from
//!   the chance node's, by the same code as the walk on one thread;
//! - walks the child's subtree on one thread, chance nodes included, by the same code, with the
//!   rows of the subtree's player nodes, which are consecutive in the solver's rows because rows
//!   are kept in the order the walk first reaches their nodes, and which no other child's walk
//!   is given;
//! - holds its working memory in a workspace of its own, the solver's workspace `o` of those it
//!   keeps for the children of a chance node, from one iteration to the next;
//! - writes its values `q(child, h)` into its own slice of the chance node's frame, which holds
//!   every child's values until the last has returned.
//!
//! The chance node's thread then adds them up in outcome order, starting from zero, as the walk
//! on one thread adds each child's values when it returns. So each child's values come from the
//! same inputs by the same code, each sum has the same terms in the same order, and a row is
//! read and written only by the walk of the subtree that holds its node. No number depends on
//! how many threads there are, which thread walks which child, or in which order the children
//! finish: every result is the walk on one thread's, bit for bit. The thread count decides only
//! whether the chance nodes are split, and the split is the same for every count above one.
//!
//! The solver keeps a workspace for each outcome of the largest chance node it splits, each of
//! which ends as large as the most its children need at each depth, so that the walks of an
//! iteration on several threads, after the first, allocate nothing either. The chance node's
//! frame holds `q` for every outcome, `outcomes × hands` values, instead of one child's. What
//! the pool's own threads allocate is rayon's: a thread registers with its queues the first
//! time it steals a task, in whichever iteration that is.
//!
//! # References
//!
//! - Zinkevich, Johanson, Bowling and Piccione, *Regret Minimization in Games with Incomplete
//!   Information*, NIPS (2007): counterfactual regret.
//! - Johanson, Waugh, Bowling and Zinkevich, *Accelerating Best Response Calculation in Large
//!   Extensive Games*, IJCAI (2011): walking the public tree with a vector of reaches over the
//!   private hands, so that a terminal can be evaluated for all of them at once.
//! - Johanson, Bard, Lanctot, Gibson and Bowling, *Efficient Nash Equilibrium Approximation
//!   through Monte Carlo Counterfactual Regret Minimization*, AAMAS (2012): CFR in this vector
//!   form, with one regret per information set kept per (public node, private hand).
//!
//! [`Cfr`]: crate::tabular::Cfr

use std::iter;
use std::ops::Range;

use crate::regret::{
    Discount, Schedule, accumulate_strategy, average_strategy, current_strategy, update_regrets,
};

/// What happens at a node of a public tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublicNode {
    /// The game is over, and [`PublicGame::terminal_values`] gives its payoffs.
    Terminal,
    /// Chance picks one of the outcomes `0..num_outcomes`, which both players see. There is at
    /// least one.
    Chance {
        /// The number of outcomes.
        num_outcomes: usize,
    },
    /// `player`, 0 or 1, picks one of the actions `0..num_actions`, which the other player
    /// sees. There is at least one.
    Player {
        /// The acting player.
        player: usize,
        /// The number of actions.
        num_actions: usize,
    },
}

/// A two-player game whose only private information is one hand per player, described by its
/// public tree. The [module documentation](self) gives the model.
///
/// Nodes are indices in `0..num_nodes()`, as in an arena. The nodes reachable from the root
/// must form a tree: each is the child of exactly one node. Players are 0 and 1, and the
/// player who is not `p` is `1 - p`.
pub trait PublicGame {
    /// The number of hands player `player` may hold, at least 1.
    fn num_hands(&self, player: usize) -> usize;

    /// Chance's factor for each of `player`'s hands in the probability of the deal, one
    /// non-negative entry per hand: the deal gives the players `h0` and `h1` with probability
    /// `deal(0)[h0] · deal(1)[h1]` when the two are compatible.
    ///
    /// Scaling either vector by a positive constant changes no strategy. Values computed from
    /// it, such as a [best response](mod@crate::vector_best_response)'s, are in the game's
    /// units when those probabilities sum to 1 over the compatible pairs, and are scaled by
    /// their total otherwise.
    fn deal(&self, player: usize) -> &[f64];

    /// The number of nodes. Nodes are `0..num_nodes()`.
    fn num_nodes(&self) -> usize;

    /// The node at which play starts, once the hands are dealt.
    fn root(&self) -> usize;

    /// What happens at `node`.
    fn node(&self, node: usize) -> PublicNode;

    /// The node after action `index` at a player node, or after outcome `index` at a chance
    /// node. Not called on terminals.
    fn child(&self, node: usize, index: usize) -> usize;

    /// The probability of outcome `outcome` at the chance node `node`, given any two
    /// compatible hands that it does not rule out. Non-negative and finite.
    fn outcome_probability(&self, node: usize, outcome: usize) -> f64;

    /// Whether outcome `outcome` of the chance node `node` rules out `player`'s hand `hand`:
    /// the outcome cannot happen while the player holds it.
    fn rules_out(&self, node: usize, outcome: usize, player: usize, hand: usize) -> bool;

    /// The hands of `player` that outcome `outcome` of the chance node `node` rules out: each
    /// hand for which [`PublicGame::rules_out`] is true, once, in hand order.
    ///
    /// The walks ask for them at every outcome of every chance node, for both players. The
    /// default tests every hand with `rules_out`. A game that can list them without testing
    /// every hand, as when an outcome rules out the few hands that hold the card it deals,
    /// should.
    fn hands_ruled_out(
        &self,
        node: usize,
        outcome: usize,
        player: usize,
    ) -> impl Iterator<Item = usize> {
        (0..self.num_hands(player)).filter(move |&hand| self.rules_out(node, outcome, player, hand))
    }

    /// At the terminal `node`, writes `player`'s value for each of its hands given the
    /// opponent's reach:
    ///
    /// ```text
    /// values[h] = Σ_o opponent_reach[o] · u(h, o)
    /// ```
    ///
    /// over the opponent's hands `o` compatible with `h`, where `u(h, o)` is `player`'s payoff
    /// at this terminal when it holds `h` and the opponent holds `o`. `opponent_reach` has
    /// `num_hands(1 - player)` entries and `values` has `num_hands(player)`, and every entry of
    /// `values` must be written: the solver passes working memory, which holds whatever it was
    /// last used for.
    ///
    /// The solver passes a reach of zero for every opponent hand ruled out above the node, and
    /// replaces whatever is written for the player's own ruled-out hands by zero, so this need
    /// only handle the compatibility of the two private hands.
    fn terminal_values(
        &self,
        node: usize,
        player: usize,
        opponent_reach: &[f64],
        values: &mut [f64],
    );

    /// [`PublicGame::terminal_values`], with room to work in. The walks call this at every
    /// terminal, and keep `scratch` from one call to the next, so that a game whose evaluation
    /// needs memory of its own can keep it there instead of allocating it at every terminal.
    /// The game may resize `scratch` and leave anything in it; on entry it holds what the last
    /// call left, or nothing. The default ignores it and calls `terminal_values`.
    fn terminal_values_with(
        &self,
        node: usize,
        player: usize,
        opponent_reach: &[f64],
        values: &mut [f64],
        _scratch: &mut Vec<f64>,
    ) {
        self.terminal_values(node, player, opponent_reach, values);
    }
}

/// A strategy for some of the player nodes of a public tree: at each, the probability of each
/// action for each hand of the acting player, stored as `f32`.
///
/// A node's strategy is laid out `[actions × hands]`, action-major, as in [`crate::regret`]: the
/// probability of action `a` with hand `h` is at `a * num_hands + h`.
#[derive(Debug, Clone, PartialEq)]
pub struct PublicProfile {
    /// By node: the strategy at the node, if the profile has one.
    nodes: Vec<Option<NodeStrategy>>,
}

#[derive(Debug, Clone, PartialEq)]
struct NodeStrategy {
    num_hands: usize,
    probabilities: Vec<f32>,
}

impl PublicProfile {
    /// A profile with a strategy at every player node of `game`, where `strategy(node, hand)`
    /// gives the probability of each action at `node` for the acting player's hand `hand`.
    ///
    /// Panics if a strategy has the wrong number of actions.
    pub fn from_fn<G: PublicGame>(
        game: &G,
        mut strategy: impl FnMut(usize, usize) -> Vec<f32>,
    ) -> PublicProfile {
        let mut profile = PublicProfile::empty(game.num_nodes());
        for node in 0..game.num_nodes() {
            let PublicNode::Player {
                player,
                num_actions,
            } = game.node(node)
            else {
                continue;
            };
            let num_hands = game.num_hands(player);
            let mut probabilities = vec![0.0; num_actions * num_hands];
            for hand in 0..num_hands {
                let row = strategy(node, hand);
                assert_eq!(
                    row.len(),
                    num_actions,
                    "the strategy at node {node} for hand {hand} has the wrong number of actions"
                );
                for (action, probability) in row.into_iter().enumerate() {
                    probabilities[action * num_hands + hand] = probability;
                }
            }
            profile.set(node, num_hands, probabilities);
        }
        profile
    }

    /// A profile over `num_nodes` nodes with no strategies.
    pub(crate) fn empty(num_nodes: usize) -> PublicProfile {
        PublicProfile {
            nodes: vec![None; num_nodes],
        }
    }

    /// Sets the strategy at `node`, laid out `[actions × hands]` over `num_hands` hands.
    pub(crate) fn set(&mut self, node: usize, num_hands: usize, probabilities: Vec<f32>) {
        debug_assert!(probabilities.len().is_multiple_of(num_hands));
        self.nodes[node] = Some(NodeStrategy {
            num_hands,
            probabilities,
        });
    }

    /// The strategy at `node`, laid out `[actions × hands]`, if the profile has one.
    pub fn get(&self, node: usize) -> Option<&[f32]> {
        let strategy = self.nodes.get(node)?.as_ref()?;
        Some(&strategy.probabilities)
    }

    /// The probability of each action at `node` for the hand `hand`, if the profile has a
    /// strategy at `node`. Panics if the acting player has no hand `hand`.
    pub fn hand(&self, node: usize, hand: usize) -> Option<Vec<f32>> {
        let strategy = self.nodes.get(node)?.as_ref()?;
        assert!(
            hand < strategy.num_hands,
            "node {node} has {} hands, not {hand}",
            strategy.num_hands
        );
        let row = strategy
            .probabilities
            .iter()
            .skip(hand)
            .step_by(strategy.num_hands)
            .copied()
            .collect();
        Some(row)
    }

    /// The nodes that have a strategy, in index order.
    pub fn nodes(&self) -> impl Iterator<Item = usize> + '_ {
        (0..self.nodes.len()).filter(|&node| self.nodes[node].is_some())
    }
}

/// Vector-form counterfactual regret minimization over the public tree of a [`PublicGame`],
/// with the regret and averaging schedule of a [`Schedule`]. One iteration is specified in the
/// [module documentation](self).
///
/// [`VectorCfr::new`] walks the tree once to check its shape and to find, for each player node,
/// which of the acting player's hands are possible there.
#[derive(Debug, Clone)]
pub struct VectorCfr<G: PublicGame> {
    game: G,
    schedule: Schedule,
    iterations: u64,
    /// For each node, its position in `rows` if it is a player node reachable from the root.
    row_of: Vec<Option<usize>>,
    /// One row per player node, in the order the walk first reaches them.
    rows: Vec<NodeRow>,
    /// The walk's working memory, kept from one iteration to the next.
    workspace: Workspace,
    /// The working memory of the walks of a split chance node's children, one per outcome,
    /// kept from one iteration to the next: see the
    /// [module documentation](self#the-parallel-walk). Empty until an iteration splits.
    branches: Vec<Workspace>,
}

/// The stored state of one player node.
#[derive(Debug, Clone)]
struct NodeRow {
    player: usize,
    /// Whether each hand of the acting player is possible at the node.
    possible: Vec<bool>,
    /// `[actions × hands]`.
    regrets: Vec<f32>,
    /// `[actions × hands]`.
    strategy_sum: Vec<f32>,
}

impl<G: PublicGame> VectorCfr<G> {
    /// A solver for `game` with every regret and strategy sum at zero.
    ///
    /// Panics if the game is malformed: a player with no hands or a deal vector of the wrong
    /// length or with an entry that is negative or not finite, a node out of range or reached
    /// twice from the root, a chance node with no outcomes or an outcome probability that is
    /// negative or not finite, or a player node with no actions or an acting player other than
    /// 0 or 1.
    pub fn new(game: G, schedule: Schedule) -> VectorCfr<G> {
        let mut row_of = vec![None; game.num_nodes()];
        let mut rows = Vec::new();
        check_game(&game, |node, player, num_actions, possible| {
            row_of[node] = Some(rows.len());
            let len = num_actions * possible.len();
            rows.push(NodeRow {
                player,
                possible: possible.to_vec(),
                regrets: vec![0.0; len],
                strategy_sum: vec![0.0; len],
            });
        });
        VectorCfr {
            game,
            schedule,
            iterations: 0,
            row_of,
            rows,
            workspace: Workspace::default(),
            branches: Vec::new(),
        }
    }

    /// The game being solved.
    pub fn game(&self) -> &G {
        &self.game
    }

    /// The schedule the solver runs.
    pub fn schedule(&self) -> Schedule {
        self.schedule
    }

    /// The number of iterations completed.
    pub fn iterations(&self) -> u64 {
        self.iterations
    }

    /// The average strategy at every player node, which is the strategy CFR converges to. A
    /// hand that is ruled out at a node, or that its player never reached there, plays
    /// uniformly.
    pub fn average_strategy(&self) -> PublicProfile {
        self.profile(|row, out| average_strategy(&row.strategy_sum, row.possible.len(), out))
    }

    /// The strategy the next iteration will play at every player node: regret matching on the
    /// stored regrets.
    pub fn current_strategy(&self) -> PublicProfile {
        self.profile(|row, out| current_strategy(&row.regrets, row.possible.len(), out))
    }

    /// The stored regrets of a player node, laid out `[actions × hands]`.
    pub fn regrets(&self, node: usize) -> Option<&[f32]> {
        Some(&self.row(node)?.regrets)
    }

    /// The stored strategy sum of a player node, laid out `[actions × hands]`.
    pub fn strategy_sum(&self, node: usize) -> Option<&[f32]> {
        Some(&self.row(node)?.strategy_sum)
    }

    /// Whether each hand of the acting player is possible at a player node, that is, not ruled
    /// out by a chance outcome on the way there. The pairs of a player node and a possible hand
    /// are the game's information sets.
    pub fn possible_hands(&self, node: usize) -> Option<&[bool]> {
        Some(&self.row(node)?.possible)
    }

    /// Replaces the stored regrets and strategy sum of a player node, each laid out
    /// `[actions × hands]` as [`VectorCfr::regrets`] and [`VectorCfr::strategy_sum`] give them.
    /// The iteration count is left as it is, so the next iteration is still number
    /// `iterations() + 1` and applies that iteration's discount.
    ///
    /// This is for checks that run one iteration from a state set from outside, such as another
    /// solver's, and compare what comes out. The values are not checked against what a run
    /// could produce: a hand ruled out at the node should hold zeros, as the
    /// [module documentation](self) explains, and a test may set them otherwise.
    ///
    /// Panics if `node` is not a player node reachable from the root, if either slice has the
    /// wrong length, if a value is not finite, or if a strategy-sum entry is negative.
    pub fn set_stored(&mut self, node: usize, regrets: &[f32], strategy_sum: &[f32]) {
        let position =
            self.row_of.get(node).copied().flatten().unwrap_or_else(|| {
                panic!("node {node} is not a player node reachable from the root")
            });
        let row = &mut self.rows[position];
        assert!(
            regrets.len() == row.regrets.len() && strategy_sum.len() == row.strategy_sum.len(),
            "node {node} stores {} values in each slice",
            row.regrets.len()
        );
        assert!(
            regrets.iter().all(|r| r.is_finite()),
            "a regret set at node {node} is not finite"
        );
        assert!(
            strategy_sum.iter().all(|&s| s.is_finite() && s >= 0.0),
            "a strategy-sum entry set at node {node} is negative or not finite"
        );
        row.regrets.copy_from_slice(regrets);
        row.strategy_sum.copy_from_slice(strategy_sum);
    }

    fn row(&self, node: usize) -> Option<&NodeRow> {
        let position = (*self.row_of.get(node)?)?;
        Some(&self.rows[position])
    }

    /// The stored regrets and strategy sum of a player node, for tests that set them.
    #[cfg(test)]
    pub(crate) fn stored_mut(&mut self, node: usize) -> Option<(&mut [f32], &mut [f32])> {
        let position = (*self.row_of.get(node)?)?;
        let row = &mut self.rows[position];
        Some((&mut row.regrets, &mut row.strategy_sum))
    }

    fn profile(&self, strategy: impl Fn(&NodeRow, &mut [f32])) -> PublicProfile {
        let mut profile = PublicProfile::empty(self.game.num_nodes());
        for (node, position) in self.row_of.iter().enumerate() {
            if let Some(position) = *position {
                let row = &self.rows[position];
                let mut out = vec![0.0; row.regrets.len()];
                strategy(row, &mut out);
                profile.set(node, row.possible.len(), out);
            }
        }
        profile
    }
}

impl<G: PublicGame + Sync> VectorCfr<G> {
    /// Runs one iteration, as the [module documentation](self) specifies: on the threads of the
    /// rayon pool it is called on, if it has more than one (see
    /// [the parallel walk](self#the-parallel-walk)), and otherwise on the calling thread, with
    /// the same result.
    pub fn iterate(&mut self) {
        let t = self.iterations + 1;
        let discount = self.schedule.discount(t);
        let parallel = splits_chance_nodes();
        for traverser in 0..2 {
            let opponent = 1 - traverser;
            let num_hands = self.game.num_hands(traverser);
            // The walk's inputs at the root, and the values it returns there.
            let mut root = self.workspace.frames.take();
            refill(&mut root.own_reach, iter::repeat_n(1.0, num_hands));
            refill(&mut root.possible, iter::repeat_n(true, num_hands));
            let values = resized(&mut root.values, num_hands, 0.0);
            let mut walk = Walk {
                game: &self.game,
                row_of: &self.row_of,
                rows: &mut self.rows,
                first_row: 0,
                traverser,
                deal: self.game.deal(traverser),
                discount,
                workspace: &mut self.workspace,
                branches: parallel.then_some(&mut self.branches),
            };
            walk.visit(
                self.game.root(),
                &root.own_reach,
                self.game.deal(opponent),
                &root.possible,
                values,
            );
            self.workspace.frames.give(root);
        }
        self.iterations = t;
    }

    /// Runs `iterations` iterations, calling `on_iteration` after each one with the solver as
    /// it stands.
    pub fn run(&mut self, iterations: u64, mut on_iteration: impl FnMut(&VectorCfr<G>)) {
        for _ in 0..iterations {
            self.iterate();
            on_iteration(self);
        }
    }
}

/// Whether a walk started here splits chance nodes: on a thread of a rayon pool of more than
/// one thread, as the [module documentation](self#the-parallel-walk) says. Outside any pool it
/// does not ask rayon for the number of threads, which would start rayon's global pool.
pub(crate) fn splits_chance_nodes() -> bool {
    rayon::current_thread_index().is_some() && rayon::current_num_threads() > 1
}

/// The position of the first row in the subtree of `node`, from the rows' positions `row_of`,
/// or `None` if the subtree has no player node. A subtree's rows are consecutive, from its
/// first.
fn subtree_row<G: PublicGame>(game: &G, row_of: &[Option<usize>], node: usize) -> Option<usize> {
    match game.node(node) {
        PublicNode::Player { .. } => row_of[node],
        PublicNode::Terminal => None,
        PublicNode::Chance { num_outcomes } => (0..num_outcomes)
            .find_map(|outcome| subtree_row(game, row_of, game.child(node, outcome))),
    }
}

/// Checks the shape of `game`, as [`VectorCfr::new`] documents, calling `player_node` for each
/// player node reachable from the root, in depth-first order, with the acting player, its
/// number of actions and whether each of that player's hands is possible there.
pub(crate) fn check_game<G: PublicGame>(
    game: &G,
    mut player_node: impl FnMut(usize, usize, usize, &[bool]),
) {
    let possible = [0, 1].map(|player| {
        let num_hands = game.num_hands(player);
        assert!(num_hands >= 1, "player {player} has no hands");
        let deal = game.deal(player);
        assert_eq!(
            deal.len(),
            num_hands,
            "player {player}'s deal needs one entry per hand"
        );
        assert!(
            deal.iter().all(|&p| p.is_finite() && p >= 0.0),
            "player {player}'s deal has an entry that is negative or not finite"
        );
        vec![true; num_hands]
    });
    let mut seen = vec![false; game.num_nodes()];
    let mut frames = Frames::default();
    discover(
        game,
        game.root(),
        &possible,
        &mut seen,
        &mut frames,
        &mut player_node,
    );
}

/// Checks the tree below `node`, where `possible` says which of each player's hands are
/// possible. A chance node holds both players' possible hands after each outcome in a frame
/// from `frames`, as the walks hold their buffers.
fn discover<G: PublicGame>(
    game: &G,
    node: usize,
    possible: &[Vec<bool>; 2],
    seen: &mut [bool],
    frames: &mut Frames<[Vec<bool>; 2]>,
    player_node: &mut impl FnMut(usize, usize, usize, &[bool]),
) {
    assert!(
        node < seen.len(),
        "node {node} is out of range for {} nodes",
        seen.len()
    );
    assert!(
        !seen[node],
        "node {node} is reached twice from the root, so the public tree is not a tree"
    );
    seen[node] = true;
    match game.node(node) {
        PublicNode::Terminal => {}
        PublicNode::Chance { num_outcomes } => {
            assert!(num_outcomes >= 1, "the chance node {node} has no outcomes");
            let mut after = frames.take();
            for outcome in 0..num_outcomes {
                let probability = game.outcome_probability(node, outcome);
                assert!(
                    probability.is_finite() && probability >= 0.0,
                    "outcome {outcome} of node {node} has probability {probability}"
                );
                for (player, flags) in after.iter_mut().enumerate() {
                    let flags = refill(flags, possible[player].iter().copied());
                    for hand in game.hands_ruled_out(node, outcome, player) {
                        flags[hand] = false;
                    }
                }
                let child = game.child(node, outcome);
                discover(game, child, &after, seen, frames, player_node);
            }
            frames.give(after);
        }
        PublicNode::Player {
            player,
            num_actions,
        } => {
            assert!(player < 2, "player {player} acts at node {node}");
            assert!(num_actions >= 1, "node {node} has no actions");
            player_node(node, player, num_actions, &possible[player]);
            for action in 0..num_actions {
                let child = game.child(node, action);
                discover(game, child, possible, seen, frames, player_node);
            }
        }
    }
}

/// A stack of frames of working memory for a walk, kept from one walk to the next: see the
/// [module documentation](self#working-memory).
#[derive(Debug, Clone, Default)]
pub(crate) struct Frames<F> {
    /// The frames no node holds, the one for the shallowest depth on top.
    free: Vec<F>,
}

impl<F: Default> Frames<F> {
    /// A frame for a node to hold while its children walk: the one the last node at the same
    /// depth gave back, or an empty one at a depth not reached before.
    pub(crate) fn take(&mut self) -> F {
        self.free.pop().unwrap_or_default()
    }

    /// Gives back the frame a node took, once its children are done.
    pub(crate) fn give(&mut self, frame: F) {
        self.free.push(frame);
    }
}

/// `buffer` emptied and filled from `items`, keeping its memory: it grows only when the items
/// are more than it can hold, and then to exactly their number.
pub(crate) fn refill<T>(buffer: &mut Vec<T>, items: impl ExactSizeIterator<Item = T>) -> &mut [T] {
    buffer.clear();
    buffer.reserve_exact(items.len());
    buffer.extend(items);
    buffer
}

/// `buffer` with `len` entries, keeping its memory as [`refill`] does, for a buffer that is
/// written in full before it is read: its entries keep what they held, and any new one is
/// `fill`.
pub(crate) fn resized<T: Clone>(buffer: &mut Vec<T>, len: usize, fill: T) -> &mut [T] {
    if len > buffer.capacity() {
        buffer.reserve_exact(len - buffer.len());
    }
    buffer.resize(len, fill);
    buffer
}

/// The CFR walk's working memory: see the [module documentation](self#working-memory).
#[derive(Debug, Clone, Default)]
struct Workspace {
    frames: Frames<Frame>,
    /// The instantaneous regrets `r` of the node being updated, rounded to `f32`,
    /// `[actions × hands]`.
    regrets: Vec<f32>,
    /// The traverser's reach `π` at the node being updated, rounded to `f32`.
    reach: Vec<f32>,
    /// The game's room at a terminal.
    terminal: Vec<f64>,
}

/// What a node of the CFR walk holds while its children walk. A field a node does not use
/// keeps what it held.
#[derive(Debug, Clone, Default)]
struct Frame {
    /// At a player node, its strategy `σ`, `[actions × hands]`.
    strategy: Vec<f32>,
    /// What the children return: at the traverser's node `q(n·a, h)` for each action,
    /// `[actions × hands]`, and elsewhere one child's values.
    values: Vec<f64>,
    /// The traverser's reach `π` passed to a child.
    own_reach: Vec<f64>,
    /// The opponent's reach `ρ` passed to a child.
    opponent_reach: Vec<f64>,
    /// At a chance node, the traverser's possible hands after the outcome being walked.
    possible: Vec<bool>,
}

impl Frame {
    /// At the chance node `node`, walked for `traverser` with its possible hands `possible`, its
    /// reach `own_reach` and the opponent's `opponent_reach`, writes what the child after
    /// `outcome` is walked with: the traverser's possible hands, and both reaches, the
    /// opponent's times the outcome's probability, zero for the hands the outcome rules out.
    #[allow(clippy::too_many_arguments)]
    fn after_outcome<G: PublicGame>(
        &mut self,
        game: &G,
        node: usize,
        outcome: usize,
        traverser: usize,
        possible: &[bool],
        own_reach: &[f64],
        opponent_reach: &[f64],
    ) {
        let probability = game.outcome_probability(node, outcome);
        let after = refill(&mut self.possible, possible.iter().copied());
        for hand in game.hands_ruled_out(node, outcome, traverser) {
            after[hand] = false;
        }
        let own = own_reach
            .iter()
            .zip(&self.possible)
            .map(|(&reach, &p)| if p { reach } else { 0.0 });
        refill(&mut self.own_reach, own);
        let other = opponent_reach.iter().map(|&reach| reach * probability);
        let other = refill(&mut self.opponent_reach, other);
        for hand in game.hands_ruled_out(node, outcome, 1 - traverser) {
            other[hand] = 0.0;
        }
    }
}

/// The working state of one player's walk on one iteration, or of the walk of one child of a
/// split chance node.
struct Walk<'a, G> {
    game: &'a G,
    row_of: &'a [Option<usize>],
    /// The rows of the subtree walked, and maybe others: those from position `first_row` on.
    rows: &'a mut [NodeRow],
    first_row: usize,
    traverser: usize,
    /// `deal(traverser)`.
    deal: &'a [f64],
    discount: Discount,
    workspace: &'a mut Workspace,
    /// The workspaces of the children of a chance node, if the walk splits the chance nodes it
    /// reaches: see the [module documentation](self#the-parallel-walk).
    branches: Option<&'a mut Vec<Workspace>>,
}

impl<G: PublicGame + Sync> Walk<'_, G> {
    /// Walks the tree below `node`, writing `q(node, h)` for each hand `h` of the traverser
    /// into `values`. `own_reach` is `π`, `opponent_reach` is `ρ`, and `possible` says which of
    /// the traverser's hands are possible at `node`.
    fn visit(
        &mut self,
        node: usize,
        own_reach: &[f64],
        opponent_reach: &[f64],
        possible: &[bool],
        values: &mut [f64],
    ) {
        let game = self.game;
        match game.node(node) {
            PublicNode::Terminal => {
                let scratch = &mut self.workspace.terminal;
                game.terminal_values_with(node, self.traverser, opponent_reach, values, scratch);
                for (value, &possible) in values.iter_mut().zip(possible) {
                    if !possible {
                        *value = 0.0;
                    }
                }
            }
            PublicNode::Chance { num_outcomes } if self.branches.is_some() => {
                self.split_chance(
                    node,
                    num_outcomes,
                    own_reach,
                    opponent_reach,
                    possible,
                    values,
                );
            }
            PublicNode::Chance { num_outcomes } => {
                let mut frame = self.workspace.frames.take();
                values.fill(0.0);
                for outcome in 0..num_outcomes {
                    frame.after_outcome(
                        game,
                        node,
                        outcome,
                        self.traverser,
                        possible,
                        own_reach,
                        opponent_reach,
                    );
                    let child = game.child(node, outcome);
                    let child_values = resized(&mut frame.values, values.len(), 0.0);
                    self.visit(
                        child,
                        &frame.own_reach,
                        &frame.opponent_reach,
                        &frame.possible,
                        child_values,
                    );
                    for (value, &child_value) in values.iter_mut().zip(&frame.values) {
                        *value += child_value;
                    }
                }
                self.workspace.frames.give(frame);
            }
            PublicNode::Player {
                player,
                num_actions,
            } => {
                let position = self.row_of[node].expect("every reachable player node has a row");
                let row = &self.rows[position - self.first_row];
                assert!(
                    row.player == player && row.regrets.len() == num_actions * row.possible.len(),
                    "node {node} changed its acting player or number of actions"
                );
                if player == self.traverser {
                    self.own_node(node, position, own_reach, opponent_reach, possible, values);
                } else {
                    self.opponent_node(node, position, own_reach, opponent_reach, possible, values);
                }
            }
        }
    }

    /// The chance node `node`, its children walked as rayon tasks, as the
    /// [module documentation](self#the-parallel-walk) describes: writes `q(node, h)` into
    /// `values`, the sum of the children's in outcome order, once every child has returned.
    fn split_chance(
        &mut self,
        node: usize,
        num_outcomes: usize,
        own_reach: &[f64],
        opponent_reach: &[f64],
        possible: &[bool],
        values: &mut [f64],
    ) {
        let num_hands = values.len();
        let branches = self
            .branches
            .as_deref_mut()
            .expect("a walk that splits has the children's workspaces");
        if branches.len() < num_outcomes {
            branches.reserve_exact(num_outcomes - branches.len());
            branches.resize_with(num_outcomes, Workspace::default);
        }
        let mut frame = self.workspace.frames.take();
        let outcome_values = resized(&mut frame.values, num_outcomes * num_hands, 0.0);
        let split = Split {
            game: self.game,
            row_of: self.row_of,
            traverser: self.traverser,
            deal: self.deal,
            discount: self.discount,
            node,
            own_reach,
            opponent_reach,
            possible,
        };
        split.walk(
            0..num_outcomes,
            &mut *self.rows,
            self.first_row,
            &mut branches[..num_outcomes],
            outcome_values,
        );
        values.fill(0.0);
        for child_values in outcome_values.chunks_exact(num_hands) {
            for (value, &child_value) in values.iter_mut().zip(child_values) {
                *value += child_value;
            }
        }
        self.workspace.frames.give(frame);
    }

    /// The strategy `σ` at the node whose row is at `position`, the [`current_strategy`] of its
    /// regrets, written into `strategy`.
    fn strategy<'s>(&self, position: usize, strategy: &'s mut Vec<f32>) -> &'s [f32] {
        let row = &self.rows[position - self.first_row];
        let strategy = resized(strategy, row.regrets.len(), 0.0);
        current_strategy(&row.regrets, row.possible.len(), strategy);
        strategy
    }

    /// A node of the traverser, whose row is at `position`: walks its actions, writes its
    /// values, then updates its row.
    fn own_node(
        &mut self,
        node: usize,
        position: usize,
        own_reach: &[f64],
        opponent_reach: &[f64],
        possible: &[bool],
        values: &mut [f64],
    ) {
        let num_hands = own_reach.len();
        let mut frame = self.workspace.frames.take();
        let strategy = self.strategy(position, &mut frame.strategy);
        let action_values = resized(&mut frame.values, strategy.len(), 0.0);
        for (action, (probabilities, child_values)) in strategy
            .chunks_exact(num_hands)
            .zip(action_values.chunks_exact_mut(num_hands))
            .enumerate()
        {
            let reach = own_reach
                .iter()
                .zip(probabilities)
                .map(|(&reach, &p)| reach * f64::from(p));
            let reach = refill(&mut frame.own_reach, reach);
            let child = self.game.child(node, action);
            self.visit(child, reach, opponent_reach, possible, child_values);
        }
        values.fill(0.0);
        for (probabilities, action_values) in strategy
            .chunks_exact(num_hands)
            .zip(action_values.chunks_exact(num_hands))
        {
            for ((value, &p), &action_value) in
                values.iter_mut().zip(probabilities).zip(action_values)
            {
                *value += f64::from(p) * action_value;
            }
        }
        let instantaneous = resized(&mut self.workspace.regrets, strategy.len(), 0.0);
        for (regrets, action_values) in instantaneous
            .chunks_exact_mut(num_hands)
            .zip(action_values.chunks_exact(num_hands))
        {
            for (((regret, &action_value), &value), &deal) in regrets
                .iter_mut()
                .zip(action_values)
                .zip(&*values)
                .zip(self.deal)
            {
                *regret = (deal * (action_value - value)) as f32;
            }
        }
        let reach = refill(
            &mut self.workspace.reach,
            own_reach.iter().map(|&r| r as f32),
        );
        let row = &mut self.rows[position - self.first_row];
        update_regrets(&mut row.regrets, instantaneous, self.discount);
        accumulate_strategy(&mut row.strategy_sum, strategy, reach, self.discount);
        self.workspace.frames.give(frame);
    }

    /// A node of the opponent, whose row is at `position`: walks its actions, each with the
    /// opponent's reach for it, and writes its values.
    fn opponent_node(
        &mut self,
        node: usize,
        position: usize,
        own_reach: &[f64],
        opponent_reach: &[f64],
        possible: &[bool],
        values: &mut [f64],
    ) {
        let num_hands = opponent_reach.len();
        let mut frame = self.workspace.frames.take();
        let strategy = self.strategy(position, &mut frame.strategy);
        values.fill(0.0);
        for (action, probabilities) in strategy.chunks_exact(num_hands).enumerate() {
            let reach = opponent_reach
                .iter()
                .zip(probabilities)
                .map(|(&reach, &p)| reach * f64::from(p));
            let reach = refill(&mut frame.opponent_reach, reach);
            let child = self.game.child(node, action);
            let child_values = resized(&mut frame.values, values.len(), 0.0);
            self.visit(child, own_reach, reach, possible, child_values);
            for (value, &child_value) in values.iter_mut().zip(&frame.values) {
                *value += child_value;
            }
        }
        self.workspace.frames.give(frame);
    }
}

/// A chance node whose children walk as rayon tasks, with what their walks share: the chance
/// node's inputs, read by every child.
struct Split<'a, G> {
    game: &'a G,
    row_of: &'a [Option<usize>],
    traverser: usize,
    deal: &'a [f64],
    discount: Discount,
    node: usize,
    own_reach: &'a [f64],
    opponent_reach: &'a [f64],
    possible: &'a [bool],
}

impl<G: PublicGame + Sync> Split<'_, G> {
    /// Walks the children after `outcomes`, each with the workspace of its place in
    /// `workspaces`, writing each one's values into its place in `values`, `hands` entries a
    /// child. `rows` are the rows from position `first_row` on, and hold every row of the
    /// children's subtrees. Halves the outcomes, and the rows at the first row of the second
    /// half's subtrees, and walks the halves as two rayon tasks, down to one child a task.
    fn walk(
        &self,
        outcomes: Range<usize>,
        rows: &mut [NodeRow],
        first_row: usize,
        workspaces: &mut [Workspace],
        values: &mut [f64],
    ) {
        if outcomes.len() == 1 {
            self.walk_child(outcomes.start, rows, first_row, &mut workspaces[0], values);
            return;
        }
        let half = outcomes.len() / 2;
        let middle = outcomes.start + half;
        let (game, node) = (self.game, self.node);
        let middle_row = (middle..outcomes.end)
            .find_map(|outcome| subtree_row(game, self.row_of, game.child(node, outcome)))
            .unwrap_or(first_row + rows.len());
        let (first_rows, second_rows) = rows.split_at_mut(middle_row - first_row);
        let (first_workspaces, second_workspaces) = workspaces.split_at_mut(half);
        let (first_values, second_values) = values.split_at_mut(half * self.own_reach.len());
        rayon::join(
            || {
                let outcomes = outcomes.start..middle;
                self.walk(
                    outcomes,
                    first_rows,
                    first_row,
                    first_workspaces,
                    first_values,
                );
            },
            || {
                let outcomes = middle..outcomes.end;
                self.walk(
                    outcomes,
                    second_rows,
                    middle_row,
                    second_workspaces,
                    second_values,
                );
            },
        );
    }

    /// Walks the child after `outcome` on this thread, from its inputs, which it computes into a
    /// frame of `workspace`, writing its values into `values`.
    fn walk_child(
        &self,
        outcome: usize,
        rows: &mut [NodeRow],
        first_row: usize,
        workspace: &mut Workspace,
        values: &mut [f64],
    ) {
        let mut inputs = workspace.frames.take();
        inputs.after_outcome(
            self.game,
            self.node,
            outcome,
            self.traverser,
            self.possible,
            self.own_reach,
            self.opponent_reach,
        );
        let mut walk = Walk {
            game: self.game,
            row_of: self.row_of,
            rows,
            first_row,
            traverser: self.traverser,
            deal: self.deal,
            discount: self.discount,
            workspace: &mut *workspace,
            branches: None,
        };
        let child = self.game.child(self.node, outcome);
        walk.visit(
            child,
            &inputs.own_reach,
            &inputs.opponent_reach,
            &inputs.possible,
            values,
        );
        workspace.frames.give(inputs);
    }
}

#[cfg(test)]
mod tests {
    use rand_chacha::ChaCha8Rng;
    use rand_chacha::rand_core::{Rng, SeedableRng};

    use super::*;
    use crate::public_leduc::PublicLeduc;
    use crate::regret::Dcfr;

    fn schedules() -> [Schedule; 4] {
        [
            Schedule::Vanilla,
            Schedule::CfrPlus,
            Schedule::Linear,
            Schedule::Discounted(Dcfr::default()),
        ]
    }

    /// Every player node of `solver`'s game, with its row's hands.
    fn player_nodes<G: PublicGame>(solver: &VectorCfr<G>) -> Vec<usize> {
        (0..solver.game().num_nodes())
            .filter(|&node| solver.possible_hands(node).is_some())
            .collect()
    }

    /// The `(node, hand)` pairs whose hand is ruled out at the node.
    fn ruled_out<G: PublicGame>(solver: &VectorCfr<G>) -> Vec<(usize, usize)> {
        let mut pairs = Vec::new();
        for node in player_nodes(solver) {
            let possible = solver.possible_hands(node).unwrap();
            for (hand, &possible) in possible.iter().enumerate() {
                if !possible {
                    pairs.push((node, hand));
                }
            }
        }
        pairs
    }

    #[test]
    fn a_new_solver_plays_uniformly() {
        let solver = VectorCfr::new(PublicLeduc::new(), Schedule::Vanilla);
        assert_eq!(solver.iterations(), 0);
        let nodes = player_nodes(&solver);
        assert_eq!(nodes.len(), 186);
        for profile in [solver.current_strategy(), solver.average_strategy()] {
            assert_eq!(profile.nodes().collect::<Vec<_>>(), nodes);
            for &node in &nodes {
                let strategy = profile.get(node).unwrap();
                let uniform = 1.0 / (strategy.len() / 6) as f32;
                assert!(strategy.iter().all(|&p| p == uniform));
            }
        }
        for &node in &nodes {
            assert!(solver.regrets(node).unwrap().iter().all(|&r| r == 0.0));
            assert!(solver.strategy_sum(node).unwrap().iter().all(|&s| s == 0.0));
        }
        // The root is a player node; its first child after a check is too, and a fold is not.
        assert!(solver.regrets(0).is_some());
        let fold = (0..solver.game().num_nodes())
            .find(|&node| solver.game().node(node) == PublicNode::Terminal)
            .unwrap();
        assert_eq!(solver.regrets(fold), None);
        assert_eq!(solver.possible_hands(fold), None);
    }

    #[test]
    fn possible_hands_are_those_the_public_card_does_not_rule_out() {
        let game = PublicLeduc::new();
        let solver = VectorCfr::new(game.clone(), Schedule::Vanilla);
        for node in player_nodes(&solver) {
            let possible = solver.possible_hands(node).unwrap();
            for (hand, &possible) in possible.iter().enumerate() {
                let expected = game
                    .public_card(node)
                    .is_none_or(|card| card.index() != hand);
                assert_eq!(possible, expected, "node {node}, hand {hand}");
                assert_eq!(possible, game.info_set(node, hand).is_some());
            }
        }
        assert_eq!(ruled_out(&solver).len(), 180);
    }

    #[test]
    fn run_calls_the_hook_after_every_iteration() {
        let mut solver = VectorCfr::new(PublicLeduc::new(), Schedule::Linear);
        let mut seen = Vec::new();
        let record = |solver: &VectorCfr<PublicLeduc>, seen: &mut Vec<(u64, Vec<f32>)>| {
            seen.push((solver.iterations(), solver.regrets(0).unwrap().to_vec()));
        };
        solver.run(5, |solver| record(solver, &mut seen));
        solver.run(2, |solver| record(solver, &mut seen));
        let mut stepped = VectorCfr::new(PublicLeduc::new(), Schedule::Linear);
        for (t, (iterations, regrets)) in (1..).zip(&seen) {
            stepped.iterate();
            assert_eq!(*iterations, t);
            assert_eq!(regrets, stepped.regrets(0).unwrap());
        }
        assert_eq!(seen.len(), 7);
    }

    #[test]
    fn the_solver_is_deterministic() {
        // Two solvers, one run in chunks, end with the same stored values bit for bit.
        for schedule in schedules() {
            let mut first = VectorCfr::new(PublicLeduc::new(), schedule);
            first.run(40, |_| {});
            let mut second = VectorCfr::new(PublicLeduc::new(), schedule);
            for _ in 0..4 {
                second.run(10, |_| {});
            }
            for node in player_nodes(&first) {
                for (a, b) in [
                    (first.regrets(node), second.regrets(node)),
                    (first.strategy_sum(node), second.strategy_sum(node)),
                ] {
                    let bits = |values: Option<&[f32]>| -> Vec<u32> {
                        values.unwrap().iter().map(|v| v.to_bits()).collect()
                    };
                    assert_eq!(bits(a), bits(b), "{schedule:?}, node {node}");
                }
            }
            assert_eq!(first.average_strategy(), second.average_strategy());
        }
    }

    /// The bits of every stored regret and strategy sum, node by node.
    fn stored_bits<G: PublicGame>(solver: &VectorCfr<G>) -> Vec<(Vec<u32>, Vec<u32>)> {
        let bits = |values: &[f32]| -> Vec<u32> { values.iter().map(|v| v.to_bits()).collect() };
        player_nodes(solver)
            .into_iter()
            .map(|node| {
                let regrets = bits(solver.regrets(node).unwrap());
                let sums = bits(solver.strategy_sum(node).unwrap());
                (regrets, sums)
            })
            .collect()
    }

    /// The bits of every probability of a profile, node by node.
    fn profile_bits(profile: &PublicProfile) -> Vec<(usize, Vec<u32>)> {
        let bits = |values: &[f32]| -> Vec<u32> { values.iter().map(|v| v.to_bits()).collect() };
        let nodes = profile.nodes();
        nodes
            .map(|node| (node, bits(profile.get(node).unwrap())))
            .collect()
    }

    #[test]
    fn an_iteration_on_several_threads_is_the_one_on_one_thread_bit_for_bit() {
        // Leduc's chance nodes deal the public card, six outcomes each, so on a pool of more
        // than one thread every iteration walks their children as rayon tasks. The pools have
        // more threads than the CI's machines have cores, so that the tasks interleave there
        // too. Every stored value and the average strategy must be those of the walk on the
        // calling thread, bit for bit, under every schedule.
        for schedule in schedules() {
            let mut alone = VectorCfr::new(PublicLeduc::new(), schedule);
            alone.run(40, |_| {});
            assert!(alone.branches.is_empty(), "the walk outside a pool split");
            for threads in [1, 2, 5, 16] {
                let pool = rayon::ThreadPoolBuilder::new()
                    .num_threads(threads)
                    .build()
                    .unwrap();
                let mut pooled = VectorCfr::new(PublicLeduc::new(), schedule);
                pool.install(|| pooled.run(40, |_| {}));
                // It split on several threads, with a workspace for each of the six cards, and
                // not on one.
                let expected = if threads > 1 { 6 } else { 0 };
                assert_eq!(pooled.branches.len(), expected, "{threads} threads");
                assert_eq!(
                    stored_bits(&pooled),
                    stored_bits(&alone),
                    "{schedule:?}, {threads} threads"
                );
                assert_eq!(
                    profile_bits(&pooled.average_strategy()),
                    profile_bits(&alone.average_strategy()),
                    "{schedule:?}, {threads} threads"
                );
            }
        }
    }

    #[test]
    fn a_solver_moves_between_one_thread_and_several_without_a_change() {
        // Iterations alternate between the calling thread and a pool of several, and the state
        // after each is that of a solver that runs every iteration on the calling thread.
        let schedule = Schedule::Discounted(Dcfr::default());
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(7)
            .build()
            .unwrap();
        let mut alone = VectorCfr::new(PublicLeduc::new(), schedule);
        let mut mixed = VectorCfr::new(PublicLeduc::new(), schedule);
        for t in 0..30 {
            alone.iterate();
            if t % 3 == 0 {
                mixed.iterate();
            } else {
                pool.install(|| mixed.iterate());
            }
            assert_eq!(stored_bits(&mixed), stored_bits(&alone), "iteration {t}");
        }
    }

    #[test]
    fn ruled_out_hands_stay_at_zero_and_play_uniformly() {
        // The acting player's card that the public card rules out receives no regret and no
        // reach, under every schedule, although Leduc's terminals write a value for it.
        for schedule in schedules() {
            let mut solver = VectorCfr::new(PublicLeduc::new(), schedule);
            solver.run(30, |_| {});
            let (average, current) = (solver.average_strategy(), solver.current_strategy());
            let pairs = ruled_out(&solver);
            assert_eq!(pairs.len(), 180);
            for (node, hand) in pairs {
                let regrets = solver.regrets(node).unwrap();
                let sums = solver.strategy_sum(node).unwrap();
                for values in [regrets, sums] {
                    let column: Vec<f32> = values.iter().skip(hand).step_by(6).copied().collect();
                    assert!(column.iter().all(|&v| v == 0.0), "{schedule:?}: {column:?}");
                }
                for profile in [&average, &current] {
                    let row = profile.hand(node, hand).unwrap();
                    assert!(row.iter().all(|&p| p == 1.0 / row.len() as f32));
                }
            }
            // A possible hand at the same nodes does learn.
            let learned = player_nodes(&solver)
                .into_iter()
                .filter(|&node| solver.regrets(node).unwrap().iter().any(|&r| r != 0.0))
                .count();
            assert!(learned > 150, "{schedule:?}: {learned}");
        }
    }

    #[test]
    fn ruled_out_hands_never_affect_a_possible_one() {
        // After every iteration, the stored values of every ruled-out hand are overwritten
        // with random ones, so that it plays a random strategy and carries a random average.
        // The possible hands' stored values must stay bit for bit those of an untouched run.
        let mut rng = ChaCha8Rng::seed_from_u64(21);
        let mut random = move || {
            let unit = (rng.next_u64() >> 40) as f32 / (1u64 << 24) as f32;
            20.0 * unit - 10.0
        };
        for schedule in schedules() {
            let mut clean = VectorCfr::new(PublicLeduc::new(), schedule);
            let mut poisoned = VectorCfr::new(PublicLeduc::new(), schedule);
            let pairs = ruled_out(&clean);
            for _ in 0..20 {
                clean.iterate();
                poisoned.iterate();
                for &(node, hand) in &pairs {
                    let (regrets, sums) = poisoned.stored_mut(node).unwrap();
                    for value in regrets.iter_mut().skip(hand).step_by(6) {
                        *value = random();
                    }
                    for value in sums.iter_mut().skip(hand).step_by(6) {
                        *value = random().abs();
                    }
                }
                for node in player_nodes(&clean) {
                    let possible = clean.possible_hands(node).unwrap();
                    let pairs = [
                        (clean.regrets(node), poisoned.regrets(node)),
                        (clean.strategy_sum(node), poisoned.strategy_sum(node)),
                    ];
                    for (a, b) in pairs {
                        for (index, (a, b)) in a.unwrap().iter().zip(b.unwrap()).enumerate() {
                            if possible[index % 6] {
                                assert_eq!(a.to_bits(), b.to_bits(), "{schedule:?}, node {node}");
                            }
                        }
                    }
                }
            }
            // The poison did reach the ruled-out hands' strategies.
            let current = poisoned.current_strategy();
            let mixed = pairs
                .iter()
                .filter(|&&(node, hand)| {
                    let row = current.hand(node, hand).unwrap();
                    row.iter().any(|&p| p != 1.0 / row.len() as f32)
                })
                .count();
            assert!(mixed > 100, "{schedule:?}: {mixed}");
        }
    }

    #[test]
    fn a_state_set_from_outside_is_the_state_the_next_iteration_starts_from() {
        // Two solvers run 12 iterations of the same schedule. Every row of the second is
        // overwritten with random values, which the getters and strategies then show, and
        // then with the first solver's values. Both then run the same 13th iteration, so their
        // states must match bit for bit after it.
        let mut rng = ChaCha8Rng::seed_from_u64(27);
        let mut random = move || (rng.next_u64() >> 40) as f32 / (1u64 << 24) as f32;
        let schedule = Schedule::Discounted(Dcfr::default());
        let mut source = VectorCfr::new(PublicLeduc::new(), schedule);
        source.run(12, |_| {});
        let mut copy = VectorCfr::new(PublicLeduc::new(), schedule);
        copy.run(12, |_| {});
        let nodes = player_nodes(&source);
        for &node in &nodes {
            let len = copy.regrets(node).unwrap().len();
            let noise: Vec<f32> = (0..len).map(|_| random()).collect();
            copy.set_stored(node, &noise, &noise);
            assert_eq!(copy.regrets(node).unwrap(), noise);
            assert_eq!(copy.strategy_sum(node).unwrap(), noise);
        }
        assert_ne!(copy.current_strategy(), source.current_strategy());
        assert_ne!(copy.average_strategy(), source.average_strategy());
        for &node in &nodes {
            let regrets = source.regrets(node).unwrap().to_vec();
            let sums = source.strategy_sum(node).unwrap().to_vec();
            copy.set_stored(node, &regrets, &sums);
        }
        // Setting the state leaves the iteration count, which sets the next discount.
        assert_eq!(copy.iterations(), 12);
        assert_eq!(copy.current_strategy(), source.current_strategy());
        assert_eq!(copy.average_strategy(), source.average_strategy());
        source.iterate();
        copy.iterate();
        let bits = |values: &[f32]| -> Vec<u32> { values.iter().map(|v| v.to_bits()).collect() };
        for &node in &nodes {
            assert_eq!(
                bits(source.regrets(node).unwrap()),
                bits(copy.regrets(node).unwrap())
            );
            assert_eq!(
                bits(source.strategy_sum(node).unwrap()),
                bits(copy.strategy_sum(node).unwrap())
            );
        }
    }

    #[test]
    #[should_panic(expected = "not a player node")]
    fn a_state_cannot_be_set_at_a_terminal() {
        let mut solver = VectorCfr::new(PublicLeduc::new(), Schedule::Vanilla);
        let fold = (0..solver.game().num_nodes())
            .find(|&node| solver.game().node(node) == PublicNode::Terminal)
            .unwrap();
        solver.set_stored(fold, &[], &[]);
    }

    #[test]
    #[should_panic(expected = "stores 12 values")]
    fn a_state_needs_one_value_per_action_and_hand() {
        let mut solver = VectorCfr::new(PublicLeduc::new(), Schedule::Vanilla);
        solver.set_stored(0, &[0.0; 6], &[0.0; 12]);
    }

    #[test]
    #[should_panic(expected = "negative or not finite")]
    fn a_strategy_sum_cannot_be_negative() {
        let mut solver = VectorCfr::new(PublicLeduc::new(), Schedule::Vanilla);
        let mut sums = [0.0; 12];
        sums[3] = -1.0;
        solver.set_stored(0, &[0.0; 12], &sums);
    }

    #[test]
    fn profiles_lay_strategies_out_action_major() {
        let game = PublicLeduc::new();
        // At each player node, hand h plays action a with "probability" (a + 1) / (h + 10).
        let profile = PublicProfile::from_fn(&game, |node, hand| {
            let PublicNode::Player { num_actions, .. } = game.node(node) else {
                panic!("node {node} is not a player node");
            };
            (0..num_actions)
                .map(|action| (action + 1) as f32 / (hand + 10) as f32)
                .collect()
        });
        let root = profile.get(0).unwrap();
        assert_eq!(root.len(), 12);
        assert_eq!(root[0], 0.1);
        assert_eq!(root[6 + 2], 2.0 / 12.0);
        assert_eq!(profile.hand(0, 3).unwrap(), [1.0 / 13.0, 2.0 / 13.0]);
        assert_eq!(profile.get(game.num_nodes()), None);
        assert_eq!(profile.nodes().count(), 186);
    }

    #[test]
    #[should_panic(expected = "wrong number of actions")]
    fn a_profile_needs_one_probability_per_action() {
        PublicProfile::from_fn(&PublicLeduc::new(), |_, _| vec![1.0]);
    }

    /// A public game given as an explicit arena with two hands per player, whose terminals are
    /// worth nothing, for checking the solver's checks of a tree.
    struct Arena {
        nodes: Vec<(PublicNode, Vec<usize>)>,
        deal: [Vec<f64>; 2],
    }

    impl PublicGame for Arena {
        fn num_hands(&self, _player: usize) -> usize {
            2
        }

        fn deal(&self, player: usize) -> &[f64] {
            &self.deal[player]
        }

        fn num_nodes(&self) -> usize {
            self.nodes.len()
        }

        fn root(&self) -> usize {
            0
        }

        fn node(&self, node: usize) -> PublicNode {
            self.nodes[node].0
        }

        fn child(&self, node: usize, index: usize) -> usize {
            self.nodes[node].1[index]
        }

        fn outcome_probability(&self, _node: usize, _outcome: usize) -> f64 {
            1.0
        }

        fn rules_out(&self, _node: usize, _outcome: usize, _player: usize, _hand: usize) -> bool {
            false
        }

        fn terminal_values(&self, _: usize, _: usize, _: &[f64], values: &mut [f64]) {
            values.fill(0.0);
        }
    }

    fn arena(nodes: Vec<(PublicNode, Vec<usize>)>) -> Arena {
        Arena {
            nodes,
            deal: [vec![0.5; 2], vec![0.5; 2]],
        }
    }

    const BET: PublicNode = PublicNode::Player {
        player: 0,
        num_actions: 2,
    };

    #[test]
    fn a_tree_solves() {
        let game = arena(vec![
            (BET, vec![1, 2]),
            (PublicNode::Terminal, vec![]),
            (PublicNode::Terminal, vec![]),
        ]);
        let mut solver = VectorCfr::new(game, Schedule::Vanilla);
        solver.run(3, |_| {});
        assert_eq!(solver.average_strategy().get(0).unwrap(), [0.5; 4]);
    }

    #[test]
    #[should_panic(expected = "reached twice")]
    fn a_node_with_two_parents_is_rejected() {
        let game = arena(vec![(BET, vec![1, 1]), (PublicNode::Terminal, vec![])]);
        VectorCfr::new(game, Schedule::Vanilla);
    }

    #[test]
    #[should_panic(expected = "out of range")]
    fn a_child_out_of_range_is_rejected() {
        let game = arena(vec![(BET, vec![1, 2]), (PublicNode::Terminal, vec![])]);
        VectorCfr::new(game, Schedule::Vanilla);
    }

    #[test]
    #[should_panic(expected = "one entry per hand")]
    fn a_deal_needs_one_entry_per_hand() {
        let mut game = arena(vec![(PublicNode::Terminal, vec![])]);
        game.deal[1] = vec![1.0];
        VectorCfr::new(game, Schedule::Vanilla);
    }

    #[test]
    #[should_panic(expected = "player 2 acts")]
    fn a_third_player_is_rejected() {
        let third = PublicNode::Player {
            player: 2,
            num_actions: 1,
        };
        let game = arena(vec![(third, vec![1]), (PublicNode::Terminal, vec![])]);
        VectorCfr::new(game, Schedule::Vanilla);
    }
}
