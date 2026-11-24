//! A game trait for small games, and tabular CFR that walks the whole game tree on every
//! iteration.
//!
//! [`Game`] describes a finite extensive-form game with perfect recall and any number of
//! players: a tree of terminal, chance and player nodes, where every player node belongs to an
//! information set named by an ordered key. [`Cfr`] runs counterfactual regret minimization on
//! such a game. It keeps one row of regrets and one row of strategy sums per information set,
//! updated by the functions of [`crate::regret`] with `num_hands = 1`, and reads strategies out
//! as a [`Profile`]. [`expected_values`] gives each player's expected utility under a profile.
//! This is for Kuhn, Leduc and smaller test games; the poker solvers use the vector form.
//!
//! # Notation
//!
//! A history `h` is a path from the root. Under a strategy profile `σ`, the reach `π(h)` is the
//! product of every probability on that path, chance outcomes and actions alike. For a player
//! `i` it splits as `π(h) = π_i(h) · π_{-i}(h)`: `π_i(h)` multiplies player `i`'s own action
//! probabilities, and `π_{-i}(h)` everything else, the other players' actions and chance. Under
//! perfect recall every history in an information set `I` of player `i` has the same `π_i(h)`,
//! written `π_i(I)`. Player `i`'s value `v_i(h)` is its expected utility from `h` on: its
//! utility at a terminal, `Σ_o p(o) · v_i(h·o)` at a chance node, and `Σ_a σ(I, a) · v_i(h·a)`
//! at a node in information set `I`.
//!
//! # One iteration
//!
//! Iterations are numbered from `t = 1`, and every regret and strategy sum starts at zero. Let
//! `d = schedule.discount(t)`. Iteration `t` updates the players one at a time, player 0 first
//! (alternating updates). For each player `i` in `0, 1, …, n − 1`:
//!
//! 1. For every information set `I` of every player, `σ(I)` is the [`current_strategy`] of the
//!    regrets stored for `I` at this point. Players before `i` have already been updated on
//!    iteration `t`, so their regrets include it; player `i` and the players after it have not.
//!    With two players, player 0's walk plays `σ_t` against `σ_t`, and player 1's walk plays
//!    `σ_t` against player 0's `σ_{t+1}`. The profile stays fixed until step 4.
//! 2. One depth-first walk from the root, starting from `π_i = π_{-i} = 1`, visits every
//!    history: every action and every chance outcome, in index order, including those whose
//!    probability is zero. Nothing is pruned.
//! 3. At each history `h` in an information set `I` of player `i`, and for each action `a`:
//!
//!    ```text
//!    r(I, a) += π_{-i}(h) · (v_i(h·a) − v_i(h))
//!    ```
//!
//!    The sum runs over every history in `I`, in walk order. This is the counterfactual regret
//!    of Zinkevich et al. The walk also records `π_i(I)`, and checks that every history in `I`
//!    gives the same value.
//! 4. After the walk, each information set `I` of player `i`, in key order, gets exactly one
//!    [`update_regrets`] with `r(I, ·)` rounded to `f32` and `d`, then exactly one
//!    [`accumulate_strategy`] with `σ(I)`, the reach `π_i(I)` rounded to `f32`, and `d`.
//!
//! The walk reaches every information set, so each one is updated and discounted once per
//! iteration, even when `r(I, ·)` or `π_i(I)` is zero. Within the walk, `σ` is widened exactly
//! from `f32`, and reaches, values and `r` are all `f64`. The strategy to report is the
//! [`average_strategy`] of each strategy sum.
//!
//! Another traversal can compute the same iteration (the vector form computes `r` for many
//! information sets at once) and must then agree up to rounding. Note that `π_{-i}` includes
//! chance. Multiplying every `r(I, ·)` of one information set by the same positive constant on
//! every iteration changes none of its strategies under any schedule, and neither does
//! multiplying its `π_i(I)` by such a constant. So another implementation may leave out a
//! chance factor shared by all the histories of `I`, but not one that differs between them.
//!
//! # References
//!
//! - Zinkevich, Johanson, Bowling and Piccione, *Regret Minimization in Games with Incomplete
//!   Information*, NIPS (2007): counterfactual regret, and the average strategy weighted by
//!   `π_i(I)`.
//! - Tammelin, Burch, Johanson and Bowling, *Solving Heads-up Limit Texas Hold'em*, IJCAI
//!   (2015): CFR+ with alternating updates.
//! - Brown and Sandholm, *Solving Imperfect-Information Games via Discounted Regret
//!   Minimization*, AAAI (2019): linear and discounted CFR, also with alternating updates.

use std::collections::BTreeMap;
use std::fmt::Debug;
use std::ops::Index;

use crate::regret::{
    Schedule, accumulate_strategy, average_strategy, current_strategy, update_regrets,
};

/// What happens at a game state: the game ends, chance moves, or a player acts.
#[derive(Debug, Clone, PartialEq)]
pub enum Node<K> {
    /// The game is over, and `utilities[p]` is player `p`'s payoff, one entry per player.
    Terminal {
        /// The payoff of each player.
        utilities: Vec<f64>,
    },
    /// Chance picks outcome `o` with probability `probabilities[o]`. The probabilities are
    /// non-negative and sum to 1, and there is at least one.
    Chance {
        /// The probability of each outcome.
        probabilities: Vec<f64>,
    },
    /// `player` picks one of the actions `0..num_actions` in the information set `info_set`.
    Player {
        /// The acting player, below [`Game::num_players`].
        player: usize,
        /// The information set the state belongs to.
        info_set: K,
        /// The number of actions, at least 1.
        num_actions: usize,
    },
}

/// A finite extensive-form game with perfect recall, for any number of players.
///
/// A state is a history: everything that has happened since the root, chance outcomes
/// included. [`Game::node`] says what happens at a state, and [`Game::next`] follows one of its
/// actions or chance outcomes, addressed by index.
///
/// Two states share an information-set key exactly when the acting player cannot tell them
/// apart. They then have the same acting player and the same number of actions, and action `a`
/// means the same in both. Keys of different players differ. The game must have perfect
/// recall: every state in an information set was reached through the same sequence of that
/// player's own information sets and actions. The key's order fixes the order in which
/// information sets are processed and reported, which keeps results deterministic.
pub trait Game {
    /// A history.
    type State: Clone + Debug;
    /// The key of an information set.
    type InfoSet: Ord + Clone + Debug;

    /// The number of players, at least 1. Players are numbered `0..num_players`.
    fn num_players(&self) -> usize;

    /// The state at the start of the game.
    fn root(&self) -> Self::State;

    /// What happens at `state`.
    fn node(&self, state: &Self::State) -> Node<Self::InfoSet>;

    /// The state after action `index` at a player node, or after outcome `index` at a chance
    /// node. Not called on terminal states.
    fn next(&self, state: &Self::State, index: usize) -> Self::State;
}

/// A strategy for each information set: the probability of each of its actions, stored as
/// `f32` and kept in key order.
///
/// [`Cfr::average_strategy`] and [`Cfr::current_strategy`] produce profiles, and
/// [`expected_values`] reads them.
#[derive(Debug, Clone, PartialEq)]
pub struct Profile<K> {
    strategies: BTreeMap<K, Vec<f32>>,
}

impl<K> Default for Profile<K> {
    fn default() -> Profile<K> {
        Profile {
            strategies: BTreeMap::new(),
        }
    }
}

impl<K: Ord> Profile<K> {
    /// An empty profile.
    pub fn new() -> Profile<K> {
        Profile::default()
    }

    /// Sets the strategy of one information set, returning the one it replaces.
    pub fn insert(&mut self, info_set: K, strategy: Vec<f32>) -> Option<Vec<f32>> {
        self.strategies.insert(info_set, strategy)
    }

    /// The strategy of an information set, one probability per action.
    pub fn get(&self, info_set: &K) -> Option<&[f32]> {
        self.strategies.get(info_set).map(Vec::as_slice)
    }

    /// Every information set and its strategy, in key order.
    pub fn iter(&self) -> impl Iterator<Item = (&K, &[f32])> {
        self.strategies
            .iter()
            .map(|(info_set, strategy)| (info_set, strategy.as_slice()))
    }

    /// The number of information sets.
    pub fn len(&self) -> usize {
        self.strategies.len()
    }

    /// Whether the profile has no information sets.
    pub fn is_empty(&self) -> bool {
        self.strategies.is_empty()
    }
}

impl<K: Ord> FromIterator<(K, Vec<f32>)> for Profile<K> {
    fn from_iter<I: IntoIterator<Item = (K, Vec<f32>)>>(iter: I) -> Profile<K> {
        Profile {
            strategies: iter.into_iter().collect(),
        }
    }
}

impl<K: Ord + Debug> Index<&K> for Profile<K> {
    type Output = [f32];

    /// The strategy of an information set. Panics if the profile has none for it.
    fn index(&self, info_set: &K) -> &[f32] {
        self.get(info_set)
            .unwrap_or_else(|| panic!("the profile has no strategy for {info_set:?}"))
    }
}

/// Each player's expected utility when every player follows `profile`, computed in `f64` by
/// walking the whole tree.
///
/// Panics if the profile has no strategy for an information set in the tree, or one with the
/// wrong number of actions.
pub fn expected_values<G: Game>(game: &G, profile: &Profile<G::InfoSet>) -> Vec<f64> {
    let values = value_under(game, profile, &game.root());
    assert_eq!(values.len(), game.num_players(), "one value per player");
    values
}

fn value_under<G: Game>(game: &G, profile: &Profile<G::InfoSet>, state: &G::State) -> Vec<f64> {
    let (weights, children): (Vec<f64>, Vec<G::State>) = match game.node(state) {
        Node::Terminal { utilities } => return utilities,
        Node::Chance { probabilities } => {
            let children = (0..probabilities.len())
                .map(|outcome| game.next(state, outcome))
                .collect();
            (probabilities, children)
        }
        Node::Player {
            info_set,
            num_actions,
            ..
        } => {
            let strategy = &profile[&info_set];
            assert_eq!(
                strategy.len(),
                num_actions,
                "the profile's strategy for {info_set:?} has the wrong number of actions"
            );
            let weights = strategy.iter().map(|&p| f64::from(p)).collect();
            let children = (0..num_actions)
                .map(|action| game.next(state, action))
                .collect();
            (weights, children)
        }
    };
    let mut values = vec![0.0; game.num_players()];
    for (weight, child) in weights.iter().zip(&children) {
        for (value, child_value) in values.iter_mut().zip(value_under(game, profile, child)) {
            *value += weight * child_value;
        }
    }
    values
}

/// Tabular counterfactual regret minimization over the whole tree of a [`Game`], with the
/// regret and averaging schedule of a [`Schedule`]. One iteration is specified in the
/// [module documentation](self).
///
/// [`Cfr::new`] walks the tree once to find every information set and check the game's shape.
/// The table then holds a row of regrets and a row of strategy sums for each information set.
#[derive(Debug, Clone)]
pub struct Cfr<G: Game> {
    game: G,
    schedule: Schedule,
    iterations: u64,
    /// The position of each information set in `info_sets`, which is in key order.
    index: BTreeMap<G::InfoSet, usize>,
    info_sets: Vec<InfoSetRow>,
}

/// The stored state of one information set.
#[derive(Debug, Clone)]
struct InfoSetRow {
    player: usize,
    regrets: Vec<f32>,
    strategy_sum: Vec<f32>,
}

/// The working state of one player's walk on one iteration.
struct Walk {
    traverser: usize,
    /// `σ(I)` for every information set, by position.
    strategies: Vec<Vec<f32>>,
    /// `r(I, ·)` for every information set, by position. Only the traverser's are added to.
    regrets: Vec<Vec<f64>>,
    /// `π_i(I)` for each information set of the traverser, once the walk has reached it.
    own_reach: Vec<Option<f64>>,
}

impl<G: Game> Cfr<G> {
    /// A solver for `game` with every regret and strategy sum at zero.
    ///
    /// Panics if the game is malformed: no players, a player node with no actions or an acting
    /// player out of range, two states with one key but different acting players or numbers of
    /// actions, chance probabilities that are negative or do not sum to 1 (within 1e-9), or
    /// utilities that are not finite or not one per player.
    pub fn new(game: G, schedule: Schedule) -> Cfr<G> {
        assert!(game.num_players() >= 1, "a game needs at least one player");
        let mut shapes = BTreeMap::new();
        discover(&game, &game.root(), &mut shapes);
        let index = shapes.keys().cloned().zip(0..).collect();
        let info_sets = shapes
            .into_values()
            .map(|(player, num_actions)| InfoSetRow {
                player,
                regrets: vec![0.0; num_actions],
                strategy_sum: vec![0.0; num_actions],
            })
            .collect();
        Cfr {
            game,
            schedule,
            iterations: 0,
            index,
            info_sets,
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

    /// Runs one iteration, as the [module documentation](self) specifies.
    ///
    /// Panics if the game turns out not to have perfect recall: two states with one key that
    /// the player reached with different probabilities.
    pub fn iterate(&mut self) {
        let t = self.iterations + 1;
        let discount = self.schedule.discount(t);
        for traverser in 0..self.game.num_players() {
            let strategies: Vec<Vec<f32>> = self
                .info_sets
                .iter()
                .map(|row| {
                    let mut strategy = vec![0.0; row.regrets.len()];
                    current_strategy(&row.regrets, 1, &mut strategy);
                    strategy
                })
                .collect();
            let regrets = strategies.iter().map(|s| vec![0.0; s.len()]).collect();
            let mut walk = Walk {
                traverser,
                strategies,
                regrets,
                own_reach: vec![None; self.info_sets.len()],
            };
            walk_tree(
                &self.game,
                &self.index,
                &mut walk,
                &self.game.root(),
                1.0,
                1.0,
            );
            for (position, row) in self.info_sets.iter_mut().enumerate() {
                if row.player != traverser {
                    continue;
                }
                let own_reach = walk.own_reach[position]
                    .expect("the walk reaches every information set of the traverser");
                let instantaneous: Vec<f32> =
                    walk.regrets[position].iter().map(|&r| r as f32).collect();
                update_regrets(&mut row.regrets, &instantaneous, discount);
                accumulate_strategy(
                    &mut row.strategy_sum,
                    &walk.strategies[position],
                    &[own_reach as f32],
                    discount,
                );
            }
        }
        self.iterations = t;
    }

    /// Runs `iterations` iterations, calling `on_iteration` after each one with the solver as
    /// it stands.
    pub fn run(&mut self, iterations: u64, mut on_iteration: impl FnMut(&Cfr<G>)) {
        for _ in 0..iterations {
            self.iterate();
            on_iteration(self);
        }
    }

    /// The average strategy of every information set, which is the strategy CFR converges
    /// with. An information set whose player never reached it plays uniformly.
    pub fn average_strategy(&self) -> Profile<G::InfoSet> {
        self.profile(|row, out| average_strategy(&row.strategy_sum, 1, out))
    }

    /// The strategy the next iteration will play at every information set: regret matching on
    /// the stored regrets.
    pub fn current_strategy(&self) -> Profile<G::InfoSet> {
        self.profile(|row, out| current_strategy(&row.regrets, 1, out))
    }

    /// The stored regrets of an information set, one per action.
    pub fn regrets(&self, info_set: &G::InfoSet) -> Option<&[f32]> {
        let &position = self.index.get(info_set)?;
        Some(&self.info_sets[position].regrets)
    }

    /// The stored strategy sum of an information set, one per action.
    pub fn strategy_sum(&self, info_set: &G::InfoSet) -> Option<&[f32]> {
        let &position = self.index.get(info_set)?;
        Some(&self.info_sets[position].strategy_sum)
    }

    fn profile(&self, strategy: impl Fn(&InfoSetRow, &mut [f32])) -> Profile<G::InfoSet> {
        self.index
            .iter()
            .map(|(info_set, &position)| {
                let row = &self.info_sets[position];
                let mut out = vec![0.0; row.regrets.len()];
                strategy(row, &mut out);
                (info_set.clone(), out)
            })
            .collect()
    }
}

/// Records the acting player and number of actions of every information set below `state`,
/// checking that the game is well formed.
fn discover<G: Game>(
    game: &G,
    state: &G::State,
    shapes: &mut BTreeMap<G::InfoSet, (usize, usize)>,
) {
    let num_children = match game.node(state) {
        Node::Terminal { utilities } => {
            assert_eq!(
                utilities.len(),
                game.num_players(),
                "the terminal {state:?} needs one utility per player"
            );
            assert!(
                utilities.iter().all(|u| u.is_finite()),
                "the terminal {state:?} has a utility that is not finite"
            );
            return;
        }
        Node::Chance { probabilities } => {
            assert!(
                !probabilities.is_empty(),
                "the chance node {state:?} has no outcomes"
            );
            assert!(
                probabilities.iter().all(|&p| p.is_finite() && p >= 0.0),
                "the chance node {state:?} has an outcome probability that is negative or not finite"
            );
            let total: f64 = probabilities.iter().sum();
            assert!(
                (total - 1.0).abs() <= 1e-9,
                "the chance probabilities at {state:?} sum to {total}, not 1"
            );
            probabilities.len()
        }
        Node::Player {
            player,
            info_set,
            num_actions,
        } => {
            assert!(
                player < game.num_players(),
                "player {player} acts at {state:?}, but the game has {} players",
                game.num_players()
            );
            assert!(num_actions >= 1, "{info_set:?} has no actions");
            let shape = (player, num_actions);
            let known = *shapes.entry(info_set.clone()).or_insert(shape);
            assert_eq!(
                known, shape,
                "{info_set:?} has states with different (player, number of actions)"
            );
            num_actions
        }
    };
    for index in 0..num_children {
        discover(game, &game.next(state, index), shapes);
    }
}

/// Walks the tree below `state` for the traverser `i`, returning `v_i(h)`. `own_reach` is
/// `π_i(h)` and `other_reach` is `π_{-i}(h)`.
fn walk_tree<G: Game>(
    game: &G,
    index: &BTreeMap<G::InfoSet, usize>,
    walk: &mut Walk,
    state: &G::State,
    own_reach: f64,
    other_reach: f64,
) -> f64 {
    match game.node(state) {
        Node::Terminal { utilities } => utilities[walk.traverser],
        Node::Chance { probabilities } => {
            let mut value = 0.0;
            for (outcome, &probability) in probabilities.iter().enumerate() {
                let child = game.next(state, outcome);
                value += probability
                    * walk_tree(
                        game,
                        index,
                        walk,
                        &child,
                        own_reach,
                        other_reach * probability,
                    );
            }
            value
        }
        Node::Player {
            player,
            info_set,
            num_actions,
        } => {
            let position = index[&info_set];
            let strategy: Vec<f64> = walk.strategies[position]
                .iter()
                .map(|&p| f64::from(p))
                .collect();
            assert_eq!(
                strategy.len(),
                num_actions,
                "{info_set:?} changed its number of actions"
            );
            let acting = player == walk.traverser;
            let mut action_values = Vec::with_capacity(num_actions);
            for (action, &probability) in strategy.iter().enumerate() {
                let child = game.next(state, action);
                let (own, other) = if acting {
                    (own_reach * probability, other_reach)
                } else {
                    (own_reach, other_reach * probability)
                };
                action_values.push(walk_tree(game, index, walk, &child, own, other));
            }
            let mut value = 0.0;
            for (&probability, &action_value) in strategy.iter().zip(&action_values) {
                value += probability * action_value;
            }
            if acting {
                for (regret, &action_value) in walk.regrets[position].iter_mut().zip(&action_values)
                {
                    *regret += other_reach * (action_value - value);
                }
                match walk.own_reach[position] {
                    None => walk.own_reach[position] = Some(own_reach),
                    Some(recorded) => assert!(
                        recorded == own_reach,
                        "{info_set:?} is reached with own reach {recorded} and {own_reach}, \
                         so the game does not have perfect recall"
                    ),
                }
            }
            value
        }
    }
}

#[cfg(test)]
mod tests {
    use rand_chacha::ChaCha8Rng;
    use rand_chacha::rand_core::{Rng, SeedableRng};

    use super::*;
    use crate::regret::Dcfr;

    fn schedules() -> [Schedule; 5] {
        [
            Schedule::Vanilla,
            Schedule::CfrPlus,
            Schedule::Linear,
            Schedule::Discounted(Dcfr::default()),
            Schedule::Discounted(Dcfr {
                alpha: 0.5,
                beta: -1.0,
                gamma: 3.0,
            }),
        ]
    }

    fn below(rng: &mut ChaCha8Rng, bound: usize) -> usize {
        ((u128::from(rng.next_u64()) * bound as u128) >> 64) as usize
    }

    fn uniform(rng: &mut ChaCha8Rng, low: f64, high: f64) -> f64 {
        let unit = (rng.next_u64() >> 11) as f64 / (1u64 << 53) as f64;
        low + (high - low) * unit
    }

    /// Asserts that every value is within `tolerance` times the largest expected magnitude (or
    /// times 1, if that is smaller).
    fn assert_close(actual: &[f32], expected: &[f64], tolerance: f64, what: &str) {
        assert_eq!(actual.len(), expected.len(), "{what}: lengths differ");
        let scale = expected.iter().fold(1.0f64, |max, &e| max.max(e.abs()));
        for (&a, &e) in actual.iter().zip(expected) {
            assert!(
                (f64::from(a) - e).abs() <= tolerance * scale,
                "{what}: got {actual:?}, expected {expected:?}"
            );
        }
    }

    /// A node of a [`TreeGame`].
    #[derive(Debug, Clone)]
    enum Spec {
        Terminal(Vec<f64>),
        Chance(Vec<f64>, Vec<usize>),
        Player(usize, String, Vec<usize>),
    }

    /// A game given as an explicit tree, whose states are node indices. Nodes are added
    /// children first, and the last node added is the root.
    #[derive(Debug, Clone)]
    struct TreeGame {
        num_players: usize,
        nodes: Vec<Spec>,
    }

    impl TreeGame {
        fn new(num_players: usize) -> TreeGame {
            TreeGame {
                num_players,
                nodes: Vec::new(),
            }
        }

        fn add(&mut self, spec: Spec) -> usize {
            self.nodes.push(spec);
            self.nodes.len() - 1
        }

        fn terminal(&mut self, utilities: &[f64]) -> usize {
            self.add(Spec::Terminal(utilities.to_vec()))
        }

        fn chance(&mut self, outcomes: &[(f64, usize)]) -> usize {
            let (probabilities, children) = outcomes.iter().copied().unzip();
            self.add(Spec::Chance(probabilities, children))
        }

        fn player(&mut self, player: usize, key: &str, children: &[usize]) -> usize {
            self.add(Spec::Player(player, key.to_string(), children.to_vec()))
        }
    }

    impl Game for TreeGame {
        type State = usize;
        type InfoSet = String;

        fn num_players(&self) -> usize {
            self.num_players
        }

        fn root(&self) -> usize {
            self.nodes.len() - 1
        }

        fn node(&self, &state: &usize) -> Node<String> {
            match &self.nodes[state] {
                Spec::Terminal(utilities) => Node::Terminal {
                    utilities: utilities.clone(),
                },
                Spec::Chance(probabilities, _) => Node::Chance {
                    probabilities: probabilities.clone(),
                },
                Spec::Player(player, key, children) => Node::Player {
                    player: *player,
                    info_set: key.clone(),
                    num_actions: children.len(),
                },
            }
        }

        fn next(&self, &state: &usize, index: usize) -> usize {
            match &self.nodes[state] {
                Spec::Terminal(_) => panic!("terminal states have no children"),
                Spec::Chance(_, children) | Spec::Player(_, _, children) => children[index],
            }
        }
    }

    /// Matching pennies with unequal payoffs. Player 0 picks heads or tails (actions 0 and 1),
    /// then player 1 picks heads or tails without seeing player 0's coin, so player 1's one
    /// information set holds two histories. Player 0 wins 3 on two heads, 1 on two tails, and
    /// loses 1 otherwise. The only equilibrium has both players play heads 1/3 of the time,
    /// and player 0's value is 1/3.
    fn pennies() -> TreeGame {
        let mut game = TreeGame::new(2);
        let outcome = |game: &mut TreeGame, u: f64| game.terminal(&[u, -u]);
        let (hh, ht) = (outcome(&mut game, 3.0), outcome(&mut game, -1.0));
        let (th, tt) = (outcome(&mut game, -1.0), outcome(&mut game, 1.0));
        let heads = game.player(1, "second", &[hh, ht]);
        let tails = game.player(1, "second", &[th, tt]);
        game.player(0, "first", &[heads, tails]);
        game
    }

    fn key(name: &str) -> String {
        name.to_string()
    }

    /// The regrets and strategy sums of both of [`pennies`]' information sets after each of the
    /// first two iterations, computed by hand.
    struct PenniesWorked {
        first_regrets: [[f64; 2]; 2],
        first_sums: [[f64; 2]; 2],
        second_regrets: [[f64; 2]; 2],
        second_sums: [[f64; 2]; 2],
    }

    fn check_pennies(schedule: Schedule, worked: PenniesWorked) {
        let mut cfr = Cfr::new(pennies(), schedule);
        let (first, second) = (key("first"), key("second"));
        for i in 0..2 {
            cfr.iterate();
            let checks = [
                (
                    cfr.regrets(&first),
                    worked.first_regrets[i],
                    "first regrets",
                ),
                (cfr.strategy_sum(&first), worked.first_sums[i], "first sums"),
                (
                    cfr.regrets(&second),
                    worked.second_regrets[i],
                    "second regrets",
                ),
                (
                    cfr.strategy_sum(&second),
                    worked.second_sums[i],
                    "second sums",
                ),
            ];
            for (actual, expected, name) in checks {
                let what = format!("{schedule:?}, iteration {}, {name}", i + 1);
                assert_close(actual.unwrap(), &expected, 1e-6, &what);
            }
        }
    }

    #[test]
    fn vanilla_cfr_matches_the_worked_pennies_example() {
        // Iteration 1. Player 0 walks against (1/2, 1/2): heads is worth 3/2 - 1/2 = 1 and
        // tails 0, so the node is worth 1/2 and r = (1/2, -1/2). Player 0 then plays (1, 0),
        // and player 1's walk sees that: after heads (reach 1) heads is worth -3 and tails 1
        // against a node value of -1, so r = (-2, 2); after tails the reach is 0. Updating both
        // players from the same uniform profile would give player 1 r = (-1/2, 1/2) instead.
        // Iteration 2. Player 0 walks against (0, 1): heads is worth -1 and tails 1, and
        // playing (1, 0) the node is worth -1, so r = (0, 2) and R = (1/2, 3/2). Player 1 walks
        // against (1/4, 3/4) playing (0, 1): heads gains 1/4 · (-3 - 1) after heads and
        // 3/4 · (1 + 1) after tails, so r = (1/2, 0) and R = (-3/2, 2).
        check_pennies(
            Schedule::Vanilla,
            PenniesWorked {
                first_regrets: [[0.5, -0.5], [0.5, 1.5]],
                first_sums: [[0.5, 0.5], [1.5, 0.5]],
                second_regrets: [[-2.0, 2.0], [-1.5, 2.0]],
                second_sums: [[0.5, 0.5], [0.5, 1.5]],
            },
        );
    }

    #[test]
    fn cfr_plus_matches_the_worked_pennies_example() {
        // The instantaneous regrets of iteration 1 are those of vanilla CFR, floored at zero:
        // R = (1/2, 0) and (0, 2), and the sums are halved. Iteration 2: player 0 plays (1, 0)
        // against (0, 1) as before, so R = (1/2, 2) and S = (1/4 + 1, 1/4) · 2/3. Player 1 walks
        // against (1/5, 4/5): r = 1/5 · (-4, 0) + 4/5 · (2, 0) = (4/5, 0), so R = (4/5, 2), and
        // S = (1/4, 1/4 + 1) · 2/3.
        check_pennies(
            Schedule::CfrPlus,
            PenniesWorked {
                first_regrets: [[0.5, 0.0], [0.5, 2.0]],
                first_sums: [[0.25, 0.25], [5.0 / 6.0, 1.0 / 6.0]],
                second_regrets: [[0.0, 2.0], [0.8, 2.0]],
                second_sums: [[0.25, 0.25], [1.0 / 6.0, 5.0 / 6.0]],
            },
        );
    }

    #[test]
    fn expected_values_follow_the_profile() {
        let game = pennies();
        let profile = |first: [f32; 2], second: [f32; 2]| -> Profile<String> {
            [
                (key("first"), first.to_vec()),
                (key("second"), second.to_vec()),
            ]
            .into_iter()
            .collect()
        };
        // Player 1 always plays heads: player 0's heads wins 3 and tails loses 1.
        let values = expected_values(&game, &profile([0.5, 0.5], [1.0, 0.0]));
        assert_eq!(values, [1.0, -1.0]);
        let values = expected_values(&game, &profile([0.0, 1.0], [0.25, 0.75]));
        assert_eq!(values, [0.5, -0.5]);
        let third = 1.0 / 3.0;
        let equilibrium = profile([third, 1.0 - third], [third, 1.0 - third]);
        let values = expected_values(&game, &equilibrium);
        assert!((values[0] - 1.0 / 3.0).abs() < 1e-6, "{values:?}");
        assert_eq!(values[0], -values[1]);
    }

    #[test]
    fn pennies_converges_to_its_equilibrium_under_every_schedule() {
        // Measured after 2,000 iterations, the average strategies were within 5.1e-4 of
        // (1/3, 2/3) and the value within 1.5e-6 of 1/3 under every schedule but
        // DCFR(0.5, -1, 3), which reached 2.9e-3 and 4.7e-5.
        for schedule in schedules() {
            let mut cfr = Cfr::new(pennies(), schedule);
            cfr.run(2000, |_| {});
            let average = cfr.average_strategy();
            for name in ["first", "second"] {
                assert_close(
                    &average[&key(name)],
                    &[1.0 / 3.0, 2.0 / 3.0],
                    5e-3,
                    &format!("{schedule:?}, {name}"),
                );
            }
            let value = expected_values(cfr.game(), &average)[0];
            assert!(
                (value - 1.0 / 3.0).abs() < 1e-4,
                "{schedule:?}: value {value}"
            );
        }
    }

    #[test]
    fn run_calls_the_hook_after_every_iteration() {
        let first = key("first");
        let mut cfr = Cfr::new(pennies(), Schedule::Linear);
        let mut seen = Vec::new();
        cfr.run(5, |cfr| {
            seen.push((cfr.iterations(), cfr.regrets(&first).unwrap().to_vec()))
        });
        cfr.run(2, |cfr| {
            seen.push((cfr.iterations(), cfr.regrets(&first).unwrap().to_vec()))
        });
        let mut stepped = Cfr::new(pennies(), Schedule::Linear);
        for (t, (iterations, regrets)) in (1..).zip(&seen) {
            stepped.iterate();
            assert_eq!(*iterations, t);
            assert_eq!(regrets, stepped.regrets(&first).unwrap());
        }
        assert_eq!(seen.len(), 7);
    }

    #[test]
    fn a_new_solver_plays_uniformly() {
        let cfr = Cfr::new(pennies(), Schedule::Vanilla);
        assert_eq!(cfr.iterations(), 0);
        for profile in [cfr.current_strategy(), cfr.average_strategy()] {
            let keys: Vec<&String> = profile.iter().map(|(key, _)| key).collect();
            assert_eq!(keys, ["first", "second"]);
            assert!(profile.iter().all(|(_, strategy)| strategy == [0.5, 0.5]));
        }
        assert_eq!(cfr.regrets(&key("third")), None);
    }

    /// One step of a path from the root to a terminal.
    #[derive(Clone)]
    enum Step<K> {
        Chance(f64),
        Player {
            player: usize,
            key: K,
            action: usize,
        },
    }

    /// A path from the root to a terminal, and the terminal's utilities.
    struct Path<K> {
        steps: Vec<Step<K>>,
        utilities: Vec<f64>,
    }

    fn collect_paths<G: Game>(
        game: &G,
        state: &G::State,
        steps: &mut Vec<Step<G::InfoSet>>,
        paths: &mut Vec<Path<G::InfoSet>>,
    ) {
        match game.node(state) {
            Node::Terminal { utilities } => paths.push(Path {
                steps: steps.clone(),
                utilities,
            }),
            Node::Chance { probabilities } => {
                for (outcome, &p) in probabilities.iter().enumerate() {
                    steps.push(Step::Chance(p));
                    collect_paths(game, &game.next(state, outcome), steps, paths);
                    steps.pop();
                }
            }
            Node::Player {
                player,
                info_set,
                num_actions,
            } => {
                for action in 0..num_actions {
                    steps.push(Step::Player {
                        player,
                        key: info_set.clone(),
                        action,
                    });
                    collect_paths(game, &game.next(state, action), steps, paths);
                    steps.pop();
                }
            }
        }
    }

    fn regret_matching(regrets: &[f64]) -> Vec<f64> {
        let total: f64 = regrets.iter().map(|r| r.max(0.0)).sum();
        if total > 0.0 {
            regrets.iter().map(|r| r.max(0.0) / total).collect()
        } else {
            vec![1.0 / regrets.len() as f64; regrets.len()]
        }
    }

    /// The iteration of the module documentation, computed in `f64` from the definitions, path
    /// by path instead of by a recursive walk. The counterfactual value of action `a` in
    /// information set `I` sums, over every terminal whose path takes `a` at a history in `I`,
    /// the probability of the other players' and chance's steps before that history, times
    /// the probability of every step after it, times the utility.
    struct Oracle<K> {
        schedule: Schedule,
        num_players: usize,
        paths: Vec<Path<K>>,
        players: BTreeMap<K, usize>,
        regrets: BTreeMap<K, Vec<f64>>,
        sums: BTreeMap<K, Vec<f64>>,
    }

    impl<K: Ord + Clone> Oracle<K> {
        fn new<G: Game<InfoSet = K>>(game: &G, schedule: Schedule) -> Oracle<K> {
            let mut paths = Vec::new();
            collect_paths(game, &game.root(), &mut Vec::new(), &mut paths);
            // An information set has one more action than the largest any path takes there.
            let mut players = BTreeMap::new();
            let mut regrets: BTreeMap<K, Vec<f64>> = BTreeMap::new();
            for step in paths.iter().flat_map(|path| &path.steps) {
                if let Step::Player {
                    player,
                    key,
                    action,
                } = step
                {
                    players.insert(key.clone(), *player);
                    let row = regrets.entry(key.clone()).or_default();
                    if row.len() <= *action {
                        row.resize(action + 1, 0.0);
                    }
                }
            }
            let sums = regrets.clone();
            Oracle {
                schedule,
                num_players: game.num_players(),
                paths,
                players,
                regrets,
                sums,
            }
        }

        fn probabilities(path: &Path<K>, sigma: &BTreeMap<K, Vec<f64>>) -> Vec<f64> {
            path.steps
                .iter()
                .map(|step| match step {
                    Step::Chance(p) => *p,
                    Step::Player { key, action, .. } => sigma[key][*action],
                })
                .collect()
        }

        fn iterate(&mut self, t: u64) {
            let discount = self.schedule.discount(t);
            for i in 0..self.num_players {
                let sigma: BTreeMap<K, Vec<f64>> = self
                    .regrets
                    .iter()
                    .map(|(key, regrets)| (key.clone(), regret_matching(regrets)))
                    .collect();
                let mut action_values: BTreeMap<K, Vec<f64>> = BTreeMap::new();
                let mut own_reach: BTreeMap<K, f64> = BTreeMap::new();
                for path in &self.paths {
                    let probabilities = Self::probabilities(path, &sigma);
                    let mine = |k: usize| matches!(&path.steps[k], Step::Player { player, .. } if *player == i);
                    for (j, step) in path.steps.iter().enumerate() {
                        let Step::Player { key, action, .. } = step else {
                            continue;
                        };
                        if !mine(j) {
                            continue;
                        }
                        let before = |own: bool| -> f64 {
                            (0..j)
                                .filter(|&k| mine(k) == own)
                                .map(|k| probabilities[k])
                                .product()
                        };
                        let after: f64 = probabilities[j + 1..].iter().product();
                        let values = action_values
                            .entry(key.clone())
                            .or_insert_with(|| vec![0.0; sigma[key].len()]);
                        values[*action] += before(false) * after * path.utilities[i];
                        own_reach.insert(key.clone(), before(true));
                    }
                }
                let expected = self.players.values().filter(|&&p| p == i).count();
                assert_eq!(
                    action_values.len(),
                    expected,
                    "an information set was missed"
                );
                for (key, values) in &action_values {
                    let strategy = &sigma[key];
                    let value: f64 = strategy.iter().zip(values).map(|(p, v)| p * v).sum();
                    let regrets = self.regrets.get_mut(key).unwrap();
                    for (regret, v) in regrets.iter_mut().zip(values) {
                        let x = *regret + (v - value);
                        *regret = x * if x > 0.0 {
                            discount.positive
                        } else {
                            discount.negative
                        };
                    }
                    let sums = self.sums.get_mut(key).unwrap();
                    for (sum, p) in sums.iter_mut().zip(strategy) {
                        *sum = (*sum + own_reach[key] * p) * discount.strategy;
                    }
                }
            }
        }

        /// Each player's expected utility under `profile`, summed over terminals.
        fn values(&self, profile: &Profile<K>) -> Vec<f64> {
            let sigma: BTreeMap<K, Vec<f64>> = self
                .regrets
                .keys()
                .map(|key| {
                    let strategy = profile.get(key).unwrap();
                    (
                        key.clone(),
                        strategy.iter().map(|&p| f64::from(p)).collect(),
                    )
                })
                .collect();
            let mut values = vec![0.0; self.num_players];
            for path in &self.paths {
                let reach: f64 = Self::probabilities(path, &sigma).iter().product();
                for (value, u) in values.iter_mut().zip(&path.utilities) {
                    *value += reach * u;
                }
            }
            values
        }
    }

    /// A probability distribution over `n` outcomes, with some outcomes at zero when `zeros`
    /// is set.
    fn random_distribution(rng: &mut ChaCha8Rng, n: usize, zeros: bool) -> Vec<f64> {
        let mut weights: Vec<f64> = (0..n)
            .map(|_| {
                if zeros && below(rng, 4) == 0 {
                    0.0
                } else {
                    uniform(rng, 0.05, 1.0)
                }
            })
            .collect();
        if weights.iter().all(|&w| w == 0.0) {
            weights[n - 1] = 1.0;
        }
        let total: f64 = weights.iter().sum();
        weights.iter().map(|w| w / total).collect()
    }

    /// A node of the public tree that every deal of a [`random_game`] shares.
    enum Public {
        Terminal,
        Chance(Vec<f64>, Vec<usize>),
        Player(usize, Vec<usize>),
    }

    fn random_public_tree(
        rng: &mut ChaCha8Rng,
        num_players: usize,
        depth: usize,
        tree: &mut Vec<Public>,
    ) -> usize {
        let public = if depth >= 4 || (depth >= 2 && below(rng, 3) == 0) {
            Public::Terminal
        } else if depth >= 1 && below(rng, 5) == 0 {
            let n = 2 + below(rng, 2);
            let probabilities = random_distribution(rng, n, true);
            let children = (0..n)
                .map(|_| random_public_tree(rng, num_players, depth + 1, tree))
                .collect();
            Public::Chance(probabilities, children)
        } else {
            let player = below(rng, num_players);
            let n = if below(rng, 6) == 0 {
                1
            } else {
                2 + below(rng, 2)
            };
            let children = (0..n)
                .map(|_| random_public_tree(rng, num_players, depth + 1, tree))
                .collect();
            Public::Player(player, children)
        };
        tree.push(public);
        tree.len() - 1
    }

    fn instantiate(
        rng: &mut ChaCha8Rng,
        game: &mut TreeGame,
        tree: &[Public],
        public: usize,
        types: &[usize],
    ) -> usize {
        match &tree[public] {
            Public::Terminal => {
                let utilities: Vec<f64> =
                    (0..types.len()).map(|_| uniform(rng, -1.0, 1.0)).collect();
                game.terminal(&utilities)
            }
            Public::Chance(probabilities, children) => {
                let outcomes: Vec<(f64, usize)> = probabilities
                    .iter()
                    .zip(children)
                    .map(|(&p, &child)| (p, instantiate(rng, game, tree, child, types)))
                    .collect();
                game.chance(&outcomes)
            }
            Public::Player(player, children) => {
                let children: Vec<usize> = children
                    .iter()
                    .map(|&child| instantiate(rng, game, tree, child, types))
                    .collect();
                let key = format!("player {player}, type {}, node {public}", types[*player]);
                game.player(*player, &key, &children)
            }
        }
    }

    /// A random general-sum game shaped like Leduc: chance deals each player one of two
    /// private types, then the players act in a random public tree that has public chance
    /// nodes. A player's information set is its own type and the public node, so the game has
    /// perfect recall. Some deals and chance outcomes have probability zero. With
    /// `player_0_never_low`, so does every deal that gives player 0 type 0, and player 0's
    /// type-0 information sets are reached only through probability zero.
    fn random_game(rng: &mut ChaCha8Rng, num_players: usize, player_0_never_low: bool) -> TreeGame {
        let mut tree = Vec::new();
        let root = random_public_tree(rng, num_players, 0, &mut tree);
        let num_deals = 1 << num_players;
        let mut deal = random_distribution(rng, num_deals, true);
        if player_0_never_low {
            for (index, p) in deal.iter_mut().enumerate() {
                if index & 1 == 0 {
                    *p = 0.0;
                }
            }
            if deal.iter().all(|&p| p == 0.0) {
                deal[num_deals - 1] = 1.0;
            }
            let total: f64 = deal.iter().sum();
            deal.iter_mut().for_each(|p| *p /= total);
        }
        let mut game = TreeGame::new(num_players);
        let outcomes: Vec<(f64, usize)> = deal
            .iter()
            .enumerate()
            .map(|(index, &p)| {
                let types: Vec<usize> = (0..num_players).map(|i| (index >> i) & 1).collect();
                (p, instantiate(rng, &mut game, &tree, root, &types))
            })
            .collect();
        game.chance(&outcomes);
        game
    }

    #[test]
    fn iterations_match_a_path_by_path_oracle() {
        // Two- and three-player random games under every schedule, compared after every
        // iteration.
        let mut rng = ChaCha8Rng::seed_from_u64(16);
        let iterations = 40;
        for index in 0..8 {
            let num_players = 2 + index % 2;
            let game = random_game(&mut rng, num_players, index % 4 < 2);
            for schedule in schedules() {
                let mut cfr = Cfr::new(game.clone(), schedule);
                let mut oracle = Oracle::new(&game, schedule);
                for t in 1..=iterations {
                    cfr.iterate();
                    oracle.iterate(t);
                    for (key, regrets) in &oracle.regrets {
                        let what = format!("game {index}, {schedule:?}, iteration {t}, {key}");
                        let actual = cfr.regrets(key).unwrap();
                        assert_close(actual, regrets, 1e-5, &format!("{what}, regrets"));
                        let actual = cfr.strategy_sum(key).unwrap();
                        let expected = &oracle.sums[key];
                        assert_close(actual, expected, 1e-5, &format!("{what}, sums"));
                    }
                }
                let average = cfr.average_strategy();
                assert_eq!(average.len(), oracle.regrets.len());
                let expected = oracle.values(&average);
                let actual = expected_values(&game, &average);
                for (a, e) in actual.iter().zip(&expected) {
                    assert!((a - e).abs() < 1e-12, "{actual:?} against {expected:?}");
                }
            }
        }
    }

    #[test]
    #[should_panic(expected = "different (player, number of actions)")]
    fn one_key_with_two_action_counts_is_rejected() {
        let mut game = TreeGame::new(1);
        let leaves: Vec<usize> = (0..5).map(|_| game.terminal(&[0.0])).collect();
        let two = game.player(0, "same", &leaves[..2]);
        let three = game.player(0, "same", &leaves[2..]);
        game.chance(&[(0.5, two), (0.5, three)]);
        Cfr::new(game, Schedule::Vanilla);
    }

    #[test]
    #[should_panic(expected = "sum to 0.9")]
    fn chance_probabilities_must_sum_to_one() {
        let mut game = TreeGame::new(1);
        let leaves: Vec<usize> = (0..2).map(|_| game.terminal(&[0.0])).collect();
        game.chance(&[(0.5, leaves[0]), (0.4, leaves[1])]);
        Cfr::new(game, Schedule::Vanilla);
    }

    #[test]
    #[should_panic(expected = "does not have perfect recall")]
    fn forgetting_an_own_action_is_detected() {
        // A one-player game where the player picks left or right, then forgets which. Left
        // gains regret on iteration 1, so on iteration 2 the two histories of "forgot" are
        // reached with probabilities 1 and 0.
        let mut game = TreeGame::new(1);
        let leaves: Vec<usize> = [1.0, 0.0, 0.0, 0.0]
            .iter()
            .map(|&u| game.terminal(&[u]))
            .collect();
        let left = game.player(0, "forgot", &leaves[..2]);
        let right = game.player(0, "forgot", &leaves[2..]);
        game.player(0, "start", &[left, right]);
        Cfr::new(game, Schedule::Vanilla).run(2, |_| {});
    }
}
