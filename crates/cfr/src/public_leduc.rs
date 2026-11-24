//! Leduc hold'em as a [`PublicGame`], for vector-form CFR.
//!
//! The rules, the cards and the order of the actions are those of [`crate::leduc`], and this
//! module builds the same game from the rules in public-tree form, so that [`VectorCfr`] on it
//! can be checked against [`Cfr`] on [`Leduc`] number for number.
//!
//! # Hands and the deal
//!
//! A player's hand is its card: hand `h` is [`LeducCard::from_index`]`(h)`, so each player has
//! six hands. The deal gives player 0 each card with probability 1/6 and player 1 each of the
//! five others with probability 1/5, so [`PublicGame::deal`] is 1/6 for player 0's hands and
//! 1/5 for player 1's, and every pair of distinct cards has probability 1/30, as at the root
//! of [`Leduc`]. Two hands are compatible when they are different cards.
//!
//! # The public tree
//!
//! A node is the public card, if dealt, and each round's betting, written as in
//! [`crate::leduc`]. Node 0 is the root, and nodes are numbered depth first, a node before its
//! children and children in action order. The first round has 6 player nodes, 4 folds and 5
//! closings. Each closing is a chance node whose outcome `k` deals card `k` face up: it rules
//! out hand `k` for both players and, given two private cards other than `k`, has probability
//! 1/4, one of the four cards left. Each of the 30 second rounds has 6 player nodes, 4 folds
//! and 5 showdowns. That makes 465 nodes, 186 of them player nodes. The information sets are
//! the 36 pairs of a first-round player node and a card, and the 900 pairs of a second-round
//! player node and a card other than the public one: the 936 of [`Leduc`].
//!
//! # Terminals
//!
//! A fold costs the folder what it has put in, whatever the cards. A showdown pays the stake to
//! the stronger hand: a card that pairs the public card, otherwise the higher rank, with equal
//! ranks splitting. [`PublicGame::terminal_values`] sums the payoffs over every opponent card
//! other than the player's own, an O(n²) loop over the six cards. It leaves the public card to
//! the solver, which passes a reach of zero for the opponent's hand that holds it and discards
//! the value of the player's own.
//!
//! # Equivalence with the scalar solver
//!
//! The tests run [`Cfr`] on [`Leduc`] and [`VectorCfr`] on this game under the same schedule,
//! and compare every stored regret and strategy sum, and every action probability of the
//! average and current strategies, through [`PublicLeduc::info_set`]. Measured, under vanilla
//! CFR, CFR+, linear CFR and DCFR(1.5, 0, 2):
//!
//! - Run from the same stored state, by setting the vector solver's values to the scalar
//!   solver's before each iteration, one iteration gives every stored value within 1.2e-7 of
//!   the largest in its row, one or two `f32` ulps, the same average strategy, and current
//!   strategies within 3e-8, at each of 5,000 iterations.
//! - Run side by side from the start, the stored values are the same bit for bit except where
//!   the two `f64` computations of a regret fall on either side of an `f32` rounding point. On
//!   iteration 1 twelve regrets lie exactly halfway between two `f32` values: three actions
//!   are played with `f32(1/3) = 1/3 + 1/(3 · 2^25)`, and with the chance factors that error
//!   comes to exactly half an ulp. Vanilla CFR then stays bit for bit the same from iteration
//!   3 to 5,000, and DCFR nearly so, its strategies matching exactly at every count measured.
//!   Linear CFR parts after iteration 133, and CFR+ after iteration 3, when a regret rounds
//!   apart by one ulp.
//! - CFR+ and linear CFR amplify such a difference. The vector solver against itself, with one
//!   regret moved by one ulp, differs by 1.3e-3 in the current strategy 50 iterations later
//!   under CFR+, and by 0.23 a hundred iterations later under linear CFR, as the two solvers
//!   do. So their strategies agree within 1e-6 up to 5 iterations under CFR+ and up to 100
//!   under linear CFR, and part after that, while one iteration from the same state always
//!   agrees.
//!
//! [`VectorCfr`]: crate::vector::VectorCfr
//! [`Cfr`]: crate::tabular::Cfr

use std::cmp::Ordering;

use crate::leduc::{Leduc, LeducAction, LeducCard, LeducInfoSet};
use crate::tabular::Profile;
use crate::vector::{PublicGame, PublicNode, PublicProfile};

/// Leduc hold'em's public tree, as the [module documentation](self) describes it.
#[derive(Debug, Clone)]
pub struct PublicLeduc {
    nodes: Vec<LeducNode>,
}

#[derive(Debug, Clone)]
struct LeducNode {
    public: Option<LeducCard>,
    betting: [&'static str; 2],
    kind: Kind,
    children: Vec<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// The player acts.
    Player(usize),
    /// The public card is dealt.
    Chance,
    /// `folder` gave up, losing the `lost` chips it has put in.
    Fold { folder: usize, lost: u32 },
    /// The showdown, where each player has put in `stake`.
    Showdown { stake: u32 },
}

/// Each player's hands, the six cards in index order.
const NUM_HANDS: usize = 6;

/// [`PublicGame::deal`]: 1/6 for each of player 0's cards, then 1/5 for each of player 1's.
const DEAL: [[f64; NUM_HANDS]; 2] = [[1.0 / 6.0; NUM_HANDS], [1.0 / 5.0; NUM_HANDS]];

/// Every betting string of one round, so that a string built from them can be named by a
/// `&'static str`, as [`LeducInfoSet`] needs.
const BETTINGS: [&str; 15] = [
    "", "c", "r", "cc", "cr", "rc", "rf", "rr", "crc", "crf", "crr", "rrc", "rrf", "crrc", "crrf",
];

impl PublicLeduc {
    /// Builds the public tree.
    pub fn new() -> PublicLeduc {
        let mut nodes = Vec::new();
        build(&mut nodes, None, ["", ""]);
        PublicLeduc { nodes }
    }

    /// The public card at `node`, or `None` in the first round.
    pub fn public_card(&self, node: usize) -> Option<LeducCard> {
        self.nodes[node].public
    }

    /// The betting of each round at `node`, written as in [`crate::leduc`]. The second is
    /// empty until it starts.
    pub fn betting(&self, node: usize) -> [&'static str; 2] {
        self.nodes[node].betting
    }

    /// The information set of the acting player at `node` when it holds the card of index
    /// `hand`, or `None` if `node` is not a player node or the public card is that card.
    pub fn info_set(&self, node: usize, hand: usize) -> Option<LeducInfoSet> {
        let LeducNode {
            public,
            betting,
            kind: Kind::Player(_),
            ..
        } = self.nodes[node]
        else {
            return None;
        };
        let card = LeducCard::from_index(hand);
        if public == Some(card) {
            return None;
        }
        Some(LeducInfoSet {
            public,
            betting,
            card,
        })
    }

    /// The scalar profile of `profile`: the strategy of every information set at a node where
    /// `profile` has one.
    pub fn profile(&self, profile: &PublicProfile) -> Profile<LeducInfoSet> {
        let mut scalar = Profile::new();
        for node in profile.nodes() {
            for hand in 0..NUM_HANDS {
                if let Some(info_set) = self.info_set(node, hand) {
                    let strategy = profile.hand(node, hand).expect("the node has a strategy");
                    scalar.insert(info_set, strategy);
                }
            }
        }
        scalar
    }

    /// The public profile of `profile`, with a strategy at every player node. A hand ruled out
    /// by the public card plays uniformly.
    ///
    /// Panics if `profile` has no strategy for one of the game's information sets.
    pub fn public_profile(&self, profile: &Profile<LeducInfoSet>) -> PublicProfile {
        PublicProfile::from_fn(self, |node, hand| match self.info_set(node, hand) {
            Some(info_set) => profile[&info_set].to_vec(),
            None => {
                let num_actions = self.nodes[node].children.len();
                vec![1.0 / num_actions as f32; num_actions]
            }
        })
    }
}

impl Default for PublicLeduc {
    fn default() -> PublicLeduc {
        PublicLeduc::new()
    }
}

/// Where a round's betting stands.
enum Status {
    /// The player acts next.
    Acting(usize),
    /// The player folded.
    Folded(usize),
    /// The round is over, and nobody folded.
    Closed,
}

fn status(betting: &str) -> Status {
    let player = betting.len() % 2;
    match betting.chars().last() {
        Some('f') => Status::Folded(1 - player),
        // A check checked behind, or a bet or raise called.
        Some('c') if betting.len() >= 2 => Status::Closed,
        _ => Status::Acting(player),
    }
}

/// The actions open to the player acting after `betting`: check or bet when no bet is faced,
/// and fold, call or raise when one is, with at most one bet and one raise in a round.
fn actions(betting: &str) -> &'static [LeducAction] {
    use LeducAction::{Call, Fold, Raise};
    if !betting.ends_with('r') {
        &[Call, Raise]
    } else if betting.matches('r').count() < 2 {
        &[Fold, Call, Raise]
    } else {
        &[Fold, Call]
    }
}

/// The round's betting after `action`.
fn after(betting: &str, action: LeducAction) -> &'static str {
    let letter = match action {
        LeducAction::Fold => 'f',
        LeducAction::Call => 'c',
        LeducAction::Raise => 'r',
    };
    let next = format!("{betting}{letter}");
    BETTINGS
        .into_iter()
        .find(|&known| known == next)
        .unwrap_or_else(|| panic!("{next:?} is not a Leduc betting string"))
}

/// What each player has put in, the ante included, after the betting of both rounds.
fn contributions(betting: [&str; 2]) -> [u32; 2] {
    let mut put_in = [Leduc::ANTE; 2];
    for (round, letters) in betting.iter().enumerate() {
        let mut this_round = [0, 0];
        for (position, letter) in letters.chars().enumerate() {
            let player = position % 2;
            match letter {
                'c' => this_round[player] = this_round[1 - player],
                'r' => this_round[player] = this_round[1 - player] + Leduc::BET[round],
                _ => {}
            }
        }
        for (total, more) in put_in.iter_mut().zip(this_round) {
            *total += more;
        }
    }
    put_in
}

/// Showdown strength: a pair beats every unpaired card, and unpaired cards go by rank.
fn strength(card: LeducCard, public: LeducCard) -> u8 {
    if card.rank() == public.rank() {
        3
    } else {
        card.rank() as u8
    }
}

/// Adds the node after `betting` with the public card `public`, and the nodes below it, and
/// returns its index.
fn build(
    nodes: &mut Vec<LeducNode>,
    public: Option<LeducCard>,
    betting: [&'static str; 2],
) -> usize {
    let index = nodes.len();
    nodes.push(LeducNode {
        public,
        betting,
        kind: Kind::Chance,
        children: Vec::new(),
    });
    let round = usize::from(public.is_some());
    let (kind, children) = match status(betting[round]) {
        Status::Acting(player) => {
            let children = actions(betting[round])
                .iter()
                .map(|&action| {
                    let mut next = betting;
                    next[round] = after(betting[round], action);
                    build(nodes, public, next)
                })
                .collect();
            (Kind::Player(player), children)
        }
        Status::Closed if round == 0 => {
            let children = LeducCard::ALL
                .into_iter()
                .map(|card| build(nodes, Some(card), betting))
                .collect();
            (Kind::Chance, children)
        }
        Status::Closed => {
            let stake = contributions(betting);
            debug_assert_eq!(stake[0], stake[1], "a showdown with unequal stakes");
            (Kind::Showdown { stake: stake[0] }, Vec::new())
        }
        Status::Folded(folder) => {
            let lost = contributions(betting)[folder];
            (Kind::Fold { folder, lost }, Vec::new())
        }
    };
    nodes[index].kind = kind;
    nodes[index].children = children;
    index
}

impl PublicGame for PublicLeduc {
    fn num_hands(&self, _player: usize) -> usize {
        NUM_HANDS
    }

    fn deal(&self, player: usize) -> &[f64] {
        &DEAL[player]
    }

    fn num_nodes(&self) -> usize {
        self.nodes.len()
    }

    fn root(&self) -> usize {
        0
    }

    fn node(&self, node: usize) -> PublicNode {
        let node = &self.nodes[node];
        match node.kind {
            Kind::Player(player) => PublicNode::Player {
                player,
                num_actions: node.children.len(),
            },
            Kind::Chance => PublicNode::Chance {
                num_outcomes: node.children.len(),
            },
            Kind::Fold { .. } | Kind::Showdown { .. } => PublicNode::Terminal,
        }
    }

    fn child(&self, node: usize, index: usize) -> usize {
        self.nodes[node].children[index]
    }

    /// 1/4: given two private cards other than the one dealt, it is one of the four left.
    fn outcome_probability(&self, _node: usize, _outcome: usize) -> f64 {
        0.25
    }

    /// Outcome `k` deals card `k`, which rules out hand `k`.
    fn rules_out(&self, _node: usize, outcome: usize, _player: usize, hand: usize) -> bool {
        outcome == hand
    }

    fn terminal_values(
        &self,
        node: usize,
        player: usize,
        opponent_reach: &[f64],
        values: &mut [f64],
    ) {
        let node = &self.nodes[node];
        let payoff = |hand: usize, other: usize| -> f64 {
            match node.kind {
                Kind::Fold { folder, lost } => {
                    let lost = f64::from(lost);
                    if folder == player { -lost } else { lost }
                }
                Kind::Showdown { stake } => {
                    let public = node.public.expect("the showdown has a public card");
                    let mine = strength(LeducCard::from_index(hand), public);
                    let theirs = strength(LeducCard::from_index(other), public);
                    match mine.cmp(&theirs) {
                        Ordering::Greater => f64::from(stake),
                        Ordering::Less => -f64::from(stake),
                        Ordering::Equal => 0.0,
                    }
                }
                Kind::Player(_) | Kind::Chance => panic!("{node:?} is not a terminal"),
            }
        };
        for (hand, value) in values.iter_mut().enumerate() {
            let mut total = 0.0;
            for (other, &reach) in opponent_reach.iter().enumerate() {
                // Card removal: the opponent does not hold the player's card.
                if other != hand {
                    total += reach * payoff(hand, other);
                }
            }
            *value = total;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::time::Instant;

    use super::*;
    use crate::leduc::LeducState;
    use crate::regret::{Dcfr, Schedule};
    use crate::tabular::{Cfr, Game, Node};
    use crate::vector::VectorCfr;
    use crate::vector_best_response::vector_exploitability;

    fn schedules() -> [(&'static str, Schedule); 4] {
        [
            ("CFR", Schedule::Vanilla),
            ("CFR+", Schedule::CfrPlus),
            ("linear CFR", Schedule::Linear),
            ("DCFR(1.5, 0, 2)", Schedule::Discounted(Dcfr::default())),
        ]
    }

    #[test]
    fn the_tree_has_465_nodes_and_936_information_sets() {
        let game = PublicLeduc::new();
        let mut counts = [0; 3];
        let mut info_sets = BTreeSet::new();
        for node in 0..game.num_nodes() {
            match game.node(node) {
                PublicNode::Player { player, .. } => {
                    counts[0] += 1;
                    for hand in 0..NUM_HANDS {
                        if let Some(info_set) = game.info_set(node, hand) {
                            assert_eq!(info_set.player(), player);
                            assert!(info_sets.insert(info_set), "{info_set:?} twice");
                        }
                    }
                }
                PublicNode::Chance { num_outcomes } => {
                    assert_eq!(num_outcomes, 6);
                    counts[1] += 1;
                }
                PublicNode::Terminal => counts[2] += 1,
            }
        }
        assert_eq!(game.num_nodes(), 465);
        // Player nodes, the chance nodes that close the first round, and the terminals: the
        // first round's folds and each second round's folds and showdowns.
        assert_eq!(counts, [6 + 30 * 6, 5, 4 + 30 * (4 + 5)]);
        let scalar: BTreeSet<LeducInfoSet> = Cfr::new(Leduc, Schedule::Vanilla)
            .average_strategy()
            .iter()
            .map(|(key, _)| *key)
            .collect();
        assert_eq!(info_sets, scalar);
        assert_eq!(game.root(), 0);
        assert_eq!(game.betting(0), ["", ""]);
    }

    /// Walks the scalar game from `state` and the public tree from `node` together, checking
    /// that they agree for the deal in `state`, and records the public nodes visited.
    fn walk_together(game: &PublicLeduc, node: usize, state: &LeducState, seen: &mut [bool]) {
        seen[node] = true;
        let cards = state.cards().expect("the cards are dealt");
        match (Leduc.node(state), game.node(node)) {
            (
                Node::Player {
                    player,
                    info_set,
                    num_actions,
                },
                PublicNode::Player {
                    player: public_player,
                    num_actions: public_actions,
                },
            ) => {
                assert_eq!((player, num_actions), (public_player, public_actions));
                assert_eq!(game.info_set(node, cards[player].index()), Some(info_set));
                for action in 0..num_actions {
                    let next = Leduc.next(state, action);
                    walk_together(game, game.child(node, action), &next, seen);
                }
            }
            (Node::Chance { probabilities }, PublicNode::Chance { num_outcomes }) => {
                // The scalar tree deals the four cards left; the public tree deals any card,
                // and a card a player holds rules out that hand.
                for outcome in 0..num_outcomes {
                    for player in 0..2 {
                        for hand in 0..NUM_HANDS {
                            let ruled_out = game.rules_out(node, outcome, player, hand);
                            assert_eq!(ruled_out, hand == outcome);
                        }
                    }
                }
                for (k, &probability) in probabilities.iter().enumerate() {
                    let next = Leduc.next(state, k);
                    let outcome = next.public().expect("the public card is dealt").index();
                    assert_eq!(game.outcome_probability(node, outcome), probability);
                    assert_eq!(game.public_card(game.child(node, outcome)), next.public());
                    walk_together(game, game.child(node, outcome), &next, seen);
                }
            }
            (Node::Terminal { utilities }, PublicNode::Terminal) => {
                for player in 0..2 {
                    let mut reach = [0.0; NUM_HANDS];
                    reach[cards[1 - player].index()] = 1.0;
                    let mut values = [f64::NAN; NUM_HANDS];
                    game.terminal_values(node, player, &reach, &mut values);
                    assert_eq!(
                        values[cards[player].index()],
                        utilities[player],
                        "{state:?}"
                    );
                    // Every hand is written, and the opponent's own card contributes nothing.
                    assert!(values.iter().all(|v| v.is_finite()));
                    assert_eq!(values[cards[1 - player].index()], 0.0);
                }
            }
            (scalar, public) => panic!("{state:?} is {scalar:?} but node {node} is {public:?}"),
        }
    }

    #[test]
    fn the_public_tree_is_the_scalar_tree_without_the_cards() {
        // For each of the 30 deals, every history of the scalar game is a node of the public
        // tree with the same acting player, actions, information set, chance probability and
        // payoffs, and every public node is reached by some deal.
        let game = PublicLeduc::new();
        let mut seen = vec![false; game.num_nodes()];
        let Node::Chance { probabilities } = Leduc.node(&Leduc.root()) else {
            panic!("the scalar root deals the cards");
        };
        for (deal, &probability) in probabilities.iter().enumerate() {
            let state = Leduc.next(&Leduc.root(), deal);
            let [first, second] = state.cards().unwrap().map(LeducCard::index);
            let product = game.deal(0)[first] * game.deal(1)[second];
            assert!(
                (product - probability).abs() < 1e-17,
                "{product} against {probability}"
            );
            walk_together(&game, game.root(), &state, &mut seen);
        }
        assert!(seen.iter().all(|&seen| seen));
    }

    #[test]
    fn profiles_convert_both_ways() {
        let game = PublicLeduc::new();
        let mut solver = VectorCfr::new(game.clone(), Schedule::CfrPlus);
        solver.run(20, |_| {});
        let average = solver.average_strategy();
        let scalar = game.profile(&average);
        assert_eq!(scalar.len(), 936);
        let back = game.public_profile(&scalar);
        for node in average.nodes() {
            for hand in 0..NUM_HANDS {
                let row = back.hand(node, hand).unwrap();
                if game.info_set(node, hand).is_some() {
                    assert_eq!(row, average.hand(node, hand).unwrap());
                } else {
                    assert!(row.iter().all(|&p| p == 1.0 / row.len() as f32));
                }
            }
        }
        assert_eq!(back.nodes().count(), 186);
    }

    /// The largest difference between the action probabilities of two profiles over the same
    /// information sets.
    fn largest_difference(expected: &Profile<LeducInfoSet>, actual: &Profile<LeducInfoSet>) -> f64 {
        assert_eq!(expected.len(), actual.len(), "the profiles differ in size");
        let mut largest = 0.0f64;
        for (key, strategy) in expected.iter() {
            let other = &actual[key];
            assert_eq!(strategy.len(), other.len(), "{key:?}");
            for (&a, &b) in strategy.iter().zip(other) {
                largest = largest.max((f64::from(a) - f64::from(b)).abs());
            }
        }
        largest
    }

    /// Every information set of the public tree, with its node and hand.
    fn entries(game: &PublicLeduc) -> Vec<(usize, usize, LeducInfoSet)> {
        let mut entries = Vec::new();
        for node in 0..game.num_nodes() {
            for hand in 0..NUM_HANDS {
                if let Some(info_set) = game.info_set(node, hand) {
                    entries.push((node, hand, info_set));
                }
            }
        }
        entries
    }

    /// The values of one hand in a node's `[actions × hands]` slice.
    fn column(values: &[f32], hand: usize) -> Vec<f32> {
        values
            .iter()
            .skip(hand)
            .step_by(NUM_HANDS)
            .copied()
            .collect()
    }

    /// How the two solvers' stored regrets and strategy sums compare.
    #[derive(Debug, Clone, Copy, Default)]
    struct Stored {
        /// The number of stored values that are not bit for bit the scalar solver's.
        different: usize,
        /// The largest difference relative to the largest magnitude in its row, which is the
        /// regrets or the strategy sum of one information set.
        relative: f64,
    }

    fn compare_stored(
        scalar: &Cfr<Leduc>,
        vector: &VectorCfr<PublicLeduc>,
        entries: &[(usize, usize, LeducInfoSet)],
    ) -> Stored {
        let mut stored = Stored::default();
        for &(node, hand, info_set) in entries {
            let pairs = [
                (scalar.regrets(&info_set), vector.regrets(node)),
                (scalar.strategy_sum(&info_set), vector.strategy_sum(node)),
            ];
            for (expected, actual) in pairs {
                let expected = expected.unwrap();
                let actual = column(actual.unwrap(), hand);
                assert_eq!(expected.len(), actual.len());
                let scale = expected
                    .iter()
                    .chain(&actual)
                    .fold(0.0f64, |m, &x| m.max(f64::from(x).abs()));
                for (&e, &a) in expected.iter().zip(&actual) {
                    if e.to_bits() != a.to_bits() {
                        stored.different += 1;
                        let relative = (f64::from(e) - f64::from(a)).abs() / scale;
                        stored.relative = stored.relative.max(relative);
                    }
                }
            }
        }
        stored
    }

    /// The largest difference in any action probability of the average strategies, and of the
    /// current strategies.
    fn compare_strategies(
        game: &PublicLeduc,
        scalar: &Cfr<Leduc>,
        vector: &VectorCfr<PublicLeduc>,
    ) -> [f64; 2] {
        [
            largest_difference(
                &scalar.average_strategy(),
                &game.profile(&vector.average_strategy()),
            ),
            largest_difference(
                &scalar.current_strategy(),
                &game.profile(&vector.current_strategy()),
            ),
        ]
    }

    /// One schedule's comparison after one number of iterations.
    #[derive(Debug, Clone, Copy)]
    struct Comparison {
        iterations: u64,
        /// The largest difference in any action probability of the average strategy.
        average: f64,
        /// The same for the current strategy.
        current: f64,
        stored: Stored,
    }

    /// What [`trajectory`] measured.
    struct Trajectory {
        rows: Vec<Comparison>,
        /// The runs of consecutive iterations after which some stored value was not the scalar
        /// solver's bit for bit.
        differing: Vec<(u64, u64)>,
        /// Milliseconds per iteration of the scalar and of the vector solver.
        milliseconds: [f64; 2],
        /// The exploitability of the vector solver's average strategy at each checkpoint, if
        /// measured.
        exploitability: Vec<f64>,
    }

    /// Runs `Cfr<Leduc>` and `VectorCfr<PublicLeduc>` side by side under `schedule`, comparing
    /// their stored values after every iteration and their strategies after each count in
    /// `checkpoints`. With `measure`, also measures the vector solver's exploitability at each
    /// checkpoint.
    fn trajectory(schedule: Schedule, checkpoints: &[u64], measure: bool) -> Trajectory {
        let game = PublicLeduc::new();
        let entries = entries(&game);
        assert_eq!(entries.len(), 936);
        let mut scalar = Cfr::new(Leduc, schedule);
        let mut vector = VectorCfr::new(game.clone(), schedule);
        let mut trajectory = Trajectory {
            rows: Vec::new(),
            differing: Vec::new(),
            milliseconds: [0.0; 2],
            exploitability: Vec::new(),
        };
        let mut seconds = [0.0; 2];
        for &target in checkpoints {
            let mut stored = Stored::default();
            while scalar.iterations() < target {
                let start = Instant::now();
                scalar.iterate();
                let middle = Instant::now();
                vector.iterate();
                seconds[0] += (middle - start).as_secs_f64();
                seconds[1] += middle.elapsed().as_secs_f64();
                stored = compare_stored(&scalar, &vector, &entries);
                let t = scalar.iterations();
                if stored.different > 0 {
                    match trajectory.differing.last_mut() {
                        Some((_, last)) if *last + 1 == t => *last = t,
                        _ => trajectory.differing.push((t, t)),
                    }
                }
            }
            let [average, current] = compare_strategies(&game, &scalar, &vector);
            trajectory.rows.push(Comparison {
                iterations: target,
                average,
                current,
                stored,
            });
            if measure {
                let average = vector.average_strategy();
                trajectory
                    .exploitability
                    .push(vector_exploitability(&game, &average));
            }
        }
        let total = checkpoints.last().copied().unwrap_or(0).max(1) as f64;
        trajectory.milliseconds = seconds.map(|s| s * 1e3 / total);
        trajectory
    }

    /// The worst of [`one_step`]'s comparisons, each with the iteration it came after.
    #[derive(Debug, Clone, Copy, Default)]
    struct OneStep {
        stored: (f64, u64),
        average: (f64, u64),
        current: (f64, u64),
    }

    /// Sets the vector solver's stored values to the scalar solver's.
    fn synchronize(
        scalar: &Cfr<Leduc>,
        vector: &mut VectorCfr<PublicLeduc>,
        entries: &[(usize, usize, LeducInfoSet)],
    ) {
        for &(node, hand, info_set) in entries {
            let (regrets, strategy_sum) = vector.stored_mut(node).unwrap();
            let pairs = [
                (regrets, scalar.regrets(&info_set).unwrap()),
                (strategy_sum, scalar.strategy_sum(&info_set).unwrap()),
            ];
            for (values, from) in pairs {
                for (value, &from) in values.iter_mut().skip(hand).step_by(NUM_HANDS).zip(from) {
                    *value = from;
                }
            }
        }
    }

    /// Runs `iterations` iterations of `schedule` in the scalar solver, and before each one
    /// sets the vector solver's stored values to the scalar solver's, so that both run the same
    /// iteration from the same state. Returns the worst comparison of the stored values and
    /// strategies after it.
    fn one_step(schedule: Schedule, iterations: u64) -> OneStep {
        let game = PublicLeduc::new();
        let entries = entries(&game);
        let mut scalar = Cfr::new(Leduc, schedule);
        let mut vector = VectorCfr::new(game.clone(), schedule);
        let mut worst = OneStep::default();
        for t in 1..=iterations {
            synchronize(&scalar, &mut vector, &entries);
            scalar.iterate();
            vector.iterate();
            let stored = compare_stored(&scalar, &vector, &entries).relative;
            let [average, current] = compare_strategies(&game, &scalar, &vector);
            let pairs = [
                (&mut worst.stored, stored),
                (&mut worst.average, average),
                (&mut worst.current, current),
            ];
            for (worst, difference) in pairs {
                if difference > worst.0 {
                    *worst = (difference, t);
                }
            }
        }
        worst
    }

    /// The equivalence the vector form is built for: after the same iterations, every action
    /// probability of the average and of the current strategy agrees with the scalar solver's
    /// within 1e-6.
    const TOLERANCE: f64 = 1e-6;

    /// How many iterations each schedule's trajectory is required to stay within [`TOLERANCE`]
    /// of the scalar solver's, as measured by `equivalence_table`. Both solvers compute each
    /// iteration to within one `f32` rounding of each other, but they round differently now
    /// and then, and CFR+ and linear CFR amplify a one-ulp difference in a single regret past
    /// the tolerance within tens of iterations (see
    /// `a_one_ulp_change_grows_past_the_tolerance_under_cfr_plus_and_linear_cfr`). The two
    /// trajectories part after iteration 3 under CFR+ and after iteration 133 under linear
    /// CFR. Under vanilla CFR they are bit for bit the same from iteration 3 to 5,000, and
    /// under DCFR too but for six short stretches past iteration 2,000, each absorbed within
    /// three iterations, so that their strategies match exactly at every count measured.
    fn horizon(schedule: Schedule) -> u64 {
        match schedule {
            Schedule::CfrPlus => 5,
            Schedule::Linear => 100,
            Schedule::Vanilla | Schedule::Discounted(_) => 5000,
        }
    }

    /// Checks `schedule` at the fast suite's checkpoints within its [`horizon`], both the
    /// strategies and the stored values, then the same iteration from the same state for 150
    /// iterations.
    fn check_equivalence(schedule: Schedule) {
        let checkpoints: Vec<u64> = [1, 2, 3, 5, 10, 30, 100]
            .into_iter()
            .filter(|&t| t <= horizon(schedule))
            .collect();
        let run = trajectory(schedule, &checkpoints, false);
        for row in &run.rows {
            assert!(
                row.average <= TOLERANCE && row.current <= TOLERANCE,
                "{schedule:?} after {} iterations: average {:e}, current {:e}",
                row.iterations,
                row.average,
                row.current
            );
            assert!(
                row.stored.relative <= TOLERANCE,
                "{schedule:?} after {} iterations: stored values differ by {:e}",
                row.iterations,
                row.stored.relative
            );
        }
        check_one_step(schedule, one_step(schedule, 150));
    }

    fn check_one_step(schedule: Schedule, worst: OneStep) {
        for (what, (difference, t)) in [
            ("stored values", worst.stored),
            ("average strategy", worst.average),
            ("current strategy", worst.current),
        ] {
            assert!(
                difference <= TOLERANCE,
                "{schedule:?}, iteration {t} from the scalar solver's state: {what} differ by \
                 {difference:e}"
            );
        }
    }

    // In the fast suite's 150 iterations from the scalar solver's state, the worst stored value
    // was within 6.6e-8 of its row's largest magnitude, the worst average strategy matched
    // exactly and the worst current strategy was within 3e-8, under every schedule.

    #[test]
    fn vanilla_cfr_matches_the_scalar_solver() {
        check_equivalence(Schedule::Vanilla);
    }

    #[test]
    fn cfr_plus_matches_the_scalar_solver() {
        check_equivalence(Schedule::CfrPlus);
    }

    #[test]
    fn linear_cfr_matches_the_scalar_solver() {
        check_equivalence(Schedule::Linear);
    }

    #[test]
    fn dcfr_matches_the_scalar_solver() {
        check_equivalence(Schedule::Discounted(Dcfr::default()));
    }

    /// The player node with the public card `public` and the betting `betting`.
    fn node_of(game: &PublicLeduc, public: Option<LeducCard>, betting: [&str; 2]) -> usize {
        (0..game.num_nodes())
            .find(|&node| {
                game.public_card(node) == public
                    && game.betting(node) == betting
                    && matches!(game.node(node), PublicNode::Player { .. })
            })
            .unwrap()
    }

    /// Where the trajectories first differed under CFR+ and under linear CFR: a node, a hand,
    /// an action, and the iteration after which that regret differed by one ulp.
    fn first_differences(game: &PublicLeduc) -> [(usize, usize, usize, u64); 2] {
        let jack = LeducCard::from_index(0);
        [
            (node_of(game, None, ["r", ""]), 2, 2, 3),
            (node_of(game, Some(jack), ["crc", ""]), 1, 0, 133),
        ]
    }

    /// Runs two vector solvers of `schedule` side by side, adds one ulp to the regret of
    /// `action` for `hand` at `node` in one of them after iteration `at`, and returns the
    /// largest differences between their average and current strategies after each count in
    /// `checkpoints`.
    fn sensitivity(
        schedule: Schedule,
        (node, hand, action, at): (usize, usize, usize, u64),
        checkpoints: &[u64],
    ) -> Vec<(u64, f64, f64)> {
        let game = PublicLeduc::new();
        let mut first = VectorCfr::new(game.clone(), schedule);
        let mut second = VectorCfr::new(game.clone(), schedule);
        let mut rows = Vec::new();
        for &target in checkpoints {
            while first.iterations() < target {
                first.iterate();
                second.iterate();
                if second.iterations() == at {
                    let (regrets, _) = second.stored_mut(node).unwrap();
                    let regret = &mut regrets[action * NUM_HANDS + hand];
                    *regret = f32::from_bits(regret.to_bits() + 1);
                }
            }
            let average = largest_difference(
                &game.profile(&first.average_strategy()),
                &game.profile(&second.average_strategy()),
            );
            let current = largest_difference(
                &game.profile(&first.current_strategy()),
                &game.profile(&second.current_strategy()),
            );
            rows.push((target, average, current));
        }
        rows
    }

    #[test]
    fn a_one_ulp_change_grows_past_the_tolerance_under_cfr_plus_and_linear_cfr() {
        // The vector solver against itself, with one regret one ulp apart where it first
        // differed from the scalar solver. Measured, the current strategies differ by 1.3e-3
        // fifty iterations after the change under CFR+ and by 0.23 a hundred iterations after
        // it under linear CFR, which is how the two solvers' trajectories part. Vanilla CFR
        // loses the change, and its strategies stay identical.
        let game = PublicLeduc::new();
        let [cfr_plus, linear] = first_differences(&game);
        let grown = |schedule, place, after: u64| sensitivity(schedule, place, &[after])[0].2;
        assert!(grown(Schedule::CfrPlus, cfr_plus, 53) > 1e-4);
        assert!(grown(Schedule::Linear, linear, 233) > 1e-2);
        assert_eq!(grown(Schedule::Vanilla, cfr_plus, 53), 0.0);
    }

    /// The iteration counts of the equivalence table.
    const LADDER: [u64; 13] = [1, 2, 3, 5, 10, 20, 50, 100, 200, 500, 1000, 2000, 5000];

    /// What `equivalence_table` measures for one schedule.
    struct Measured {
        trajectory: Trajectory,
        one_step: OneStep,
        /// For each place in [`first_differences`], the growth of a one-ulp change there.
        growth: Vec<Vec<(u64, f64, f64)>>,
    }

    fn measure(schedule: Schedule, places: &[(usize, usize, usize, u64)]) -> Measured {
        let growth = places
            .iter()
            .map(|&place| {
                let at = place.3;
                let counts: Vec<u64> = [1, 2, 5, 10, 20, 50, 100, 500]
                    .iter()
                    .map(|&k| at + k)
                    .chain([1000, 2000])
                    .collect();
                sensitivity(schedule, place, &counts)
            })
            .collect();
        Measured {
            trajectory: trajectory(schedule, &LADDER, true),
            one_step: one_step(schedule, 5000),
            growth,
        }
    }

    #[test]
    #[ignore = "slow"]
    fn equivalence_table() {
        // For each schedule, on its own thread: both solvers side by side to 5,000 iterations,
        // comparing the stored values after every iteration; the same iteration from the same
        // state for 5,000 iterations; and the growth of a one-ulp change. Prints the tables
        // with --nocapture. Every one-step comparison must be within 1e-6, and every
        // trajectory within 1e-6 up to its schedule's horizon.
        let schedules = schedules();
        let places = first_differences(&PublicLeduc::new());
        let results: Vec<Measured> = std::thread::scope(|scope| {
            let handles: Vec<_> = schedules
                .iter()
                .map(|&(_, schedule)| scope.spawn(move || measure(schedule, &places)))
                .collect();
            handles.into_iter().map(|h| h.join().unwrap()).collect()
        });
        for ((name, _), measured) in schedules.iter().zip(&results) {
            let trajectory = &measured.trajectory;
            println!("{name}, side by side:");
            println!(
                "| iterations | average | current | stored values not bit-identical | \
                 largest relative difference among them | vector exploitability |"
            );
            println!("|---:|---:|---:|---:|---:|---:|");
            for (row, exploitability) in trajectory.rows.iter().zip(&trajectory.exploitability) {
                println!(
                    "| {} | {:.2e} | {:.2e} | {} | {:.2e} | {:.4e} |",
                    row.iterations,
                    row.average,
                    row.current,
                    row.stored.different,
                    row.stored.relative,
                    exploitability
                );
            }
            println!(
                "stored values differed after iterations {:?}; {:.3} ms per scalar iteration \
                 and {:.4} ms per vector iteration",
                trajectory.differing, trajectory.milliseconds[0], trajectory.milliseconds[1]
            );
            let one_step = &measured.one_step;
            println!(
                "one iteration from the scalar solver's state, worst over 5,000 (difference, \
                 iteration): stored values {:?}, average {:?}, current {:?}",
                one_step.stored, one_step.average, one_step.current
            );
            for (place, rows) in places.iter().zip(&measured.growth) {
                let cells: Vec<String> = rows
                    .iter()
                    .map(|(t, average, current)| format!("{t}: {average:.1e} / {current:.1e}"))
                    .collect();
                println!(
                    "one ulp added after iteration {} (average / current): {}",
                    place.3,
                    cells.join(", ")
                );
            }
            println!();
        }
        for ((_, schedule), measured) in schedules.iter().zip(&results) {
            check_one_step(*schedule, measured.one_step);
            for row in &measured.trajectory.rows {
                if row.iterations <= horizon(*schedule) {
                    assert!(
                        row.average <= TOLERANCE && row.current <= TOLERANCE,
                        "{schedule:?} after {} iterations: average {:e}, current {:e}",
                        row.iterations,
                        row.average,
                        row.current
                    );
                }
            }
        }
    }
}
