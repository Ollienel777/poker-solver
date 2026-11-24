//! Exact best response and exploitability for any [`Game`], by walking the whole tree.
//!
//! This is the measuring stick for every solver in the workspace: the vector-form best
//! response of the postflop solver must compute the same two quantities, defined here.
//!
//! # Best response
//!
//! Fix a profile `σ` and a player `i`. Player `i`'s best-response value against the other
//! players is
//!
//! ```text
//! BR_i(σ) = max over pure strategies s of player i of u_i(s, σ_{-i})
//! ```
//!
//! where `u_i(s, σ_{-i})` is player `i`'s expected utility when it plays `s`, every other player
//! plays `σ` and chance plays its probabilities. Only the other players' strategies enter:
//! `σ`'s entries for player `i`'s own information sets are ignored, and may be missing.
//!
//! [`best_response`] computes `BR_i(σ)` and a pure strategy that attains it, choosing one
//! action per information set. In the notation of [`crate::tabular`], for an information set
//! `I` of player `i` and an action `a`, the counterfactual value of `a` is
//!
//! ```text
//! q(I, a) = Σ_{h ∈ I} π_{-i}(h) · v*_i(h·a)
//! ```
//!
//! where `π_{-i}(h)` is the probability of the other players' actions and chance's outcomes on
//! the way to `h`, and `v*_i(h)` is player `i`'s expected utility from `h` on when the others
//! play `σ` and player `i` plays the best response at each of its information sets below `h`.
//! The best response plays the action with the largest `q(I, a)`, the lowest-numbered one when
//! several are equal (compared exactly, in `f64`), and `q*(I) = max_a q(I, a)`. Since
//! `v*_i(h·a)` depends on the choices made at player `i`'s information sets below `I`, those
//! are settled first: perfect recall makes player `i`'s information sets a tree, and they are
//! settled from the leaves up. Then
//!
//! ```text
//! BR_i(σ) = v*_i(root)
//! ```
//!
//! The choice is made once per information set, on `q` summed over all the histories in it.
//! Choosing the best action separately at each history would let player `i` act on what it
//! cannot see, such as the other players' cards, and would overstate the value.
//!
//! The strategy names an action at every information set of player `i` in the tree, including
//! those its own earlier choices avoid, and maximises `q(I, ·)` at each of them. At an
//! information set the other players and chance never reach, where `π_{-i}(h) = 0` for every
//! `h ∈ I`, every `q(I, a)` is zero, so every action is a best response there and the strategy
//! plays action 0. Choices at such an information set, and at those its own earlier choices
//! avoid, do not change `BR_i(σ)`.
//!
//! The walk visits every history, including those reached with probability zero, so that it
//! finds every information set. Every quantity is computed in `f64`, and the `f32`
//! probabilities of `σ` are widened exactly. Another implementation that sums in another order
//! may break a near-tie the other way; its value then agrees with this one up to rounding.
//!
//! # Exploitability
//!
//! For a two-player zero-sum game, one where `u_0 = −u_1` at every terminal:
//!
//! ```text
//! exploitability(σ) = (BR_0(σ) + BR_1(σ)) / 2
//! ```
//!
//! in the game's own units of utility, which are chips for Kuhn and Leduc. If `v` is player
//! 0's game value, then `BR_0(σ) ≥ v` and `BR_1(σ) ≥ −v`, so
//!
//! ```text
//! exploitability(σ) = ((BR_0(σ) − v) + (BR_1(σ) + v)) / 2 ≥ 0
//! ```
//!
//! which is the mean, over the two players, of what a best response to `σ` wins beyond the
//! game value. It is zero exactly when `σ` is a Nash equilibrium. Both `v` and player 0's
//! value under `σ` lie in `[−BR_1(σ), BR_0(σ)]`, an interval `2 · exploitability(σ)` wide. The
//! postflop solver reports the same quantity as a percentage of the pot at the root of its
//! tree.
//!
//! The definition needs a zero-sum game, and [`exploitability`] refuses any other. A raked pot
//! is not zero-sum: `BR_0 + BR_1` is then below zero at an equilibrium, so a raked game needs a
//! definition of its own, such as subtracting each player's value under `σ` from its
//! best-response value, which this module does not give.
//!
//! # References
//!
//! - Johanson, Waugh, Bowling and Zinkevich, *Accelerating Best Response Calculation in Large
//!   Extensive Games*, IJCAI (2011): best response by one walk of the game tree, choosing per
//!   information set from the counterfactual values summed over its histories.
//! - Zinkevich, Johanson, Bowling and Piccione, *Regret Minimization in Games with Incomplete
//!   Information*, NIPS (2007): counterfactual values, and the bound that links average regret
//!   to an ε-Nash equilibrium.

use std::collections::BTreeMap;

use crate::tabular::{Game, Node, Profile};

/// A best response of one player to the other players' strategies, from [`best_response`].
#[derive(Debug, Clone, PartialEq)]
pub struct BestResponse<K> {
    /// The player who responds.
    pub player: usize,
    /// `BR_i(σ)`, the responder's expected utility when it plays `strategy` against the other
    /// players' strategies.
    pub value: f64,
    /// A pure best response: at every information set of the responder in the tree,
    /// probability 1 on the chosen action and 0 on the others.
    pub strategy: Profile<K>,
}

/// Player `player`'s best response to the other players' strategies in `profile`, as the
/// [module documentation](self) defines it.
///
/// Panics if `player` is not a player of the game, if `profile` has no strategy, or one with
/// the wrong number of actions, for an information set of another player in the tree, or if
/// the game turns out not to have perfect recall for `player`: an information set reached
/// after different choices of the player's own, or with different numbers of actions.
pub fn best_response<G: Game>(
    game: &G,
    profile: &Profile<G::InfoSet>,
    player: usize,
) -> BestResponse<G::InfoSet> {
    SequenceTree::walk(game, profile, player).solve()
}

/// `(BR_0(σ) + BR_1(σ)) / 2` for a two-player zero-sum game, as the
/// [module documentation](self) defines it. Zero exactly at a Nash equilibrium.
///
/// Panics if the game does not have two players, if a terminal's utilities do not sum to zero
/// (within `1e-9` of their size), or for any reason [`best_response`] panics.
pub fn exploitability<G: Game>(game: &G, profile: &Profile<G::InfoSet>) -> f64 {
    assert_eq!(
        game.num_players(),
        2,
        "exploitability is defined for two-player games"
    );
    let mut total = 0.0;
    for player in 0..2 {
        let tree = SequenceTree::walk(game, profile, player);
        if let Some(utilities) = &tree.not_zero_sum {
            panic!("exploitability needs a zero-sum game, but a terminal pays {utilities:?}");
        }
        total += tree.solve().value;
    }
    total / 2.0
}

/// The responder's information sets as a tree of its sequences, each with the value of the
/// terminals it leads to without another choice of the responder's.
///
/// A sequence is an information set of the responder and one of its actions, or the empty
/// sequence at the root. Under perfect recall every history of an information set comes after
/// the same sequence, its parent. A sequence's terminal value is the sum of
/// `π_{-i}(z) · u_i(z)` over the terminals `z` whose last responder step it is. Then
/// `q(I, a)` is the terminal value of `(I, a)` plus `q*` of each information set whose parent
/// is `(I, a)`, and `BR_i` is the terminal value of the empty sequence plus `q*` of each
/// information set whose parent it is.
struct SequenceTree<K> {
    player: usize,
    /// The position of each information set in `sets`, which is in the order the walk found
    /// them.
    index: BTreeMap<K, usize>,
    sets: Vec<InfoSetEntry>,
    root: Sequence,
    /// The utilities of a terminal whose utilities do not sum to zero, if the walk met one.
    not_zero_sum: Option<Vec<f64>>,
}

struct InfoSetEntry {
    /// The sequence every history of the information set comes after.
    parent: Option<(usize, usize)>,
    /// One sequence per action.
    sequences: Vec<Sequence>,
}

#[derive(Default)]
struct Sequence {
    terminal_value: f64,
    /// The information sets whose parent this sequence is, in the order the walk found them.
    children: Vec<usize>,
}

impl<K: Ord + Clone + std::fmt::Debug> SequenceTree<K> {
    fn walk<G: Game<InfoSet = K>>(game: &G, profile: &Profile<K>, player: usize) -> Self {
        assert!(
            player < game.num_players(),
            "player {player} is not one of the game's {} players",
            game.num_players()
        );
        let mut tree = SequenceTree {
            player,
            index: BTreeMap::new(),
            sets: Vec::new(),
            root: Sequence::default(),
            not_zero_sum: None,
        };
        tree.visit(game, profile, &game.root(), None, 1.0);
        tree
    }

    fn sequence(&mut self, sequence: Option<(usize, usize)>) -> &mut Sequence {
        match sequence {
            None => &mut self.root,
            Some((position, action)) => &mut self.sets[position].sequences[action],
        }
    }

    /// Walks the tree below `state`, which comes after the responder's sequence `parent` and
    /// has `π_{-i} = other_reach`.
    fn visit<G: Game<InfoSet = K>>(
        &mut self,
        game: &G,
        profile: &Profile<K>,
        state: &G::State,
        parent: Option<(usize, usize)>,
        other_reach: f64,
    ) {
        match game.node(state) {
            Node::Terminal { utilities } => {
                let total: f64 = utilities.iter().sum();
                let size = utilities.iter().fold(1.0f64, |max, u| max.max(u.abs()));
                if total.abs() > 1e-9 * size && self.not_zero_sum.is_none() {
                    self.not_zero_sum = Some(utilities.clone());
                }
                self.sequence(parent).terminal_value += other_reach * utilities[self.player];
            }
            Node::Chance { probabilities } => {
                for (outcome, &probability) in probabilities.iter().enumerate() {
                    let child = game.next(state, outcome);
                    self.visit(game, profile, &child, parent, other_reach * probability);
                }
            }
            Node::Player {
                player,
                info_set,
                num_actions,
            } if player == self.player => {
                let position = self.enter(info_set, num_actions, parent);
                for action in 0..num_actions {
                    let child = game.next(state, action);
                    self.visit(game, profile, &child, Some((position, action)), other_reach);
                }
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
                for (action, &probability) in strategy.iter().enumerate() {
                    let child = game.next(state, action);
                    let reach = other_reach * f64::from(probability);
                    self.visit(game, profile, &child, parent, reach);
                }
            }
        }
    }

    /// The position of the responder's information set `info_set`, reached after `parent`,
    /// adding it to the tree the first time.
    fn enter(&mut self, info_set: K, num_actions: usize, parent: Option<(usize, usize)>) -> usize {
        if let Some(&position) = self.index.get(&info_set) {
            let entry = &self.sets[position];
            assert_eq!(
                entry.sequences.len(),
                num_actions,
                "{info_set:?} has states with different numbers of actions"
            );
            assert!(
                entry.parent == parent,
                "{info_set:?} is reached after different choices of player {}, so the game \
                 does not have perfect recall",
                self.player
            );
            return position;
        }
        assert!(num_actions >= 1, "{info_set:?} has no actions");
        let position = self.sets.len();
        self.sequence(parent).children.push(position);
        self.sets.push(InfoSetEntry {
            parent,
            sequences: (0..num_actions).map(|_| Sequence::default()).collect(),
        });
        self.index.insert(info_set, position);
        position
    }

    fn solve(self) -> BestResponse<K> {
        let mut choices = vec![0; self.sets.len()];
        let mut value = self.root.terminal_value;
        for &child in &self.root.children {
            value += self.settle(child, &mut choices);
        }
        let strategy = self
            .index
            .into_iter()
            .map(|(info_set, position)| {
                let mut strategy = vec![0.0; self.sets[position].sequences.len()];
                strategy[choices[position]] = 1.0;
                (info_set, strategy)
            })
            .collect();
        BestResponse {
            player: self.player,
            value,
            strategy,
        }
    }

    /// Settles the information set at `position` and every one below it, recording each
    /// choice, and returns `q*` of the one at `position`.
    fn settle(&self, position: usize, choices: &mut [usize]) -> f64 {
        let mut best: Option<(usize, f64)> = None;
        for (action, sequence) in self.sets[position].sequences.iter().enumerate() {
            let mut q = sequence.terminal_value;
            for &child in &sequence.children {
                q += self.settle(child, choices);
            }
            if best.is_none_or(|(_, best_q)| q > best_q) {
                best = Some((action, q));
            }
        }
        let (action, q) = best.expect("an information set has at least one action");
        choices[position] = action;
        q
    }
}

#[cfg(test)]
mod tests {
    use rand_chacha::ChaCha8Rng;
    use rand_chacha::rand_core::{Rng, SeedableRng};

    use super::*;
    use crate::kuhn::{Kuhn, KuhnCard, KuhnInfoSet};
    use crate::regret::{Dcfr, Schedule};
    use crate::tabular::{Cfr, expected_values};

    use KuhnCard::{Jack, King, Queen};

    const THIRD: f64 = 1.0 / 3.0;
    const KUHN_VALUE: f64 = -1.0 / 18.0;

    fn below(rng: &mut ChaCha8Rng, bound: usize) -> usize {
        ((u128::from(rng.next_u64()) * bound as u128) >> 64) as usize
    }

    fn uniform(rng: &mut ChaCha8Rng, low: f64, high: f64) -> f64 {
        let unit = (rng.next_u64() >> 11) as f64 / (1u64 << 53) as f64;
        low + (high - low) * unit
    }

    fn info_set(card: KuhnCard, history: &'static str) -> KuhnInfoSet {
        KuhnInfoSet { card, history }
    }

    /// Player `player`'s six Kuhn information sets, in key order.
    fn kuhn_info_sets(player: usize) -> Vec<KuhnInfoSet> {
        let histories: &[&'static str] = if player == 0 {
            &["", "pb"]
        } else {
            &["b", "p"]
        };
        let mut sets: Vec<KuhnInfoSet> = [Jack, Queen, King]
            .into_iter()
            .flat_map(|card| {
                histories
                    .iter()
                    .map(move |&history| info_set(card, history))
            })
            .collect();
        sets.sort();
        sets
    }

    /// A Kuhn profile from the probability of betting (or calling) at each information set.
    fn kuhn_profile(bets: &[(KuhnInfoSet, f64)]) -> Profile<KuhnInfoSet> {
        bets.iter()
            .map(|&(key, bet)| (key, vec![(1.0 - bet) as f32, bet as f32]))
            .collect()
    }

    fn uniform_kuhn() -> Profile<KuhnInfoSet> {
        Cfr::new(Kuhn, Schedule::Vanilla).average_strategy()
    }

    /// `profile` with `player`'s strategies replaced by `response`.
    fn with_response<K: Ord + Clone>(profile: &Profile<K>, response: &Profile<K>) -> Profile<K> {
        let mut merged = profile.clone();
        for (key, strategy) in response.iter() {
            merged.insert(key.clone(), strategy.to_vec());
        }
        merged
    }

    /// The action a pure strategy plays at `key`.
    fn chosen(strategy: &Profile<KuhnInfoSet>, key: &KuhnInfoSet) -> usize {
        let row = &strategy[key];
        assert!(
            row.iter().all(|&p| p == 0.0 || p == 1.0),
            "{row:?} is not pure"
        );
        row.iter().position(|&p| p == 1.0).unwrap()
    }

    #[test]
    fn uniform_kuhn_matches_the_hand_computed_best_responses() {
        // Against uniform play every bet or call comes half the time, whatever the card.
        // Player 0 facing "pb" folds the jack (-1 against -2), calls the queen (0 against -1)
        // and the king (2). At the root the jack bets (1/2 · 1 + 1/2 · -2 = -1/2 against -1),
        // the queen bets (1/2 against 0), and the king is worth 3/2 either way: BR_0 = 1/2.
        // Player 1 facing "b" folds the jack, calls the queen (0) and the king (2); after "p"
        // it bets the jack (-1/2 against -1), the queen (1/2 against 0) and the king (3/2
        // against 1). Each history comes half the time, so BR_1 = (1/2 + 3/4) / 3 = 5/12.
        let uniform = uniform_kuhn();
        let first = best_response(&Kuhn, &uniform, 0);
        let second = best_response(&Kuhn, &uniform, 1);
        assert!((first.value - 0.5).abs() < 1e-12, "{}", first.value);
        assert!(
            (second.value - 5.0 / 12.0).abs() < 1e-12,
            "{}",
            second.value
        );
        assert_eq!((first.player, second.player), (0, 1));
        let expected = [
            ((Jack, "pb"), Kuhn::PASS),
            ((Queen, "pb"), Kuhn::BET),
            ((King, "pb"), Kuhn::BET),
            ((Jack, ""), Kuhn::BET),
            ((Queen, ""), Kuhn::BET),
            ((Jack, "b"), Kuhn::PASS),
            ((Queen, "b"), Kuhn::BET),
            ((King, "b"), Kuhn::BET),
            ((Jack, "p"), Kuhn::BET),
            ((Queen, "p"), Kuhn::BET),
            ((King, "p"), Kuhn::BET),
        ];
        for ((card, history), action) in expected {
            let key = info_set(card, history);
            // Player 0 acts after an even number of actions, player 1 after an odd number.
            let response = [&first, &second][key.history.len() % 2];
            assert_eq!(chosen(&response.strategy, &key), action, "{key:?}");
        }
        // The king at the root is a tie, so only its being pure is checked.
        chosen(&first.strategy, &info_set(King, ""));
        assert_eq!(first.strategy.len(), 6);
        assert_eq!(second.strategy.len(), 6);
        let exploitability = exploitability(&Kuhn, &uniform);
        assert!(
            (exploitability - 11.0 / 24.0).abs() < 1e-12,
            "{exploitability}"
        );
    }

    /// Player `player`'s value when it can see everything and picks the best action at each
    /// history: the wrong best response, which knows the other player's card.
    fn clairvoyant<G: Game>(
        game: &G,
        profile: &Profile<G::InfoSet>,
        player: usize,
        state: &G::State,
    ) -> f64 {
        match game.node(state) {
            Node::Terminal { utilities } => utilities[player],
            Node::Chance { probabilities } => probabilities
                .iter()
                .enumerate()
                .map(|(o, p)| p * clairvoyant(game, profile, player, &game.next(state, o)))
                .sum(),
            Node::Player {
                player: acting,
                info_set,
                num_actions,
            } => {
                let values = (0..num_actions)
                    .map(|a| clairvoyant(game, profile, player, &game.next(state, a)));
                if acting == player {
                    values.fold(f64::NEG_INFINITY, f64::max)
                } else {
                    values
                        .zip(&profile[&info_set])
                        .map(|(v, &p)| v * f64::from(p))
                        .sum()
                }
            }
        }
    }

    /// The largest value `player` gets from any of its pure strategies against `profile`,
    /// each evaluated with [`expected_values`], and the number of pure strategies.
    fn brute_force<G: Game>(
        game: &G,
        profile: &Profile<G::InfoSet>,
        player: usize,
        own: &[(G::InfoSet, usize)],
    ) -> (f64, usize) {
        let count: usize = own.iter().map(|(_, n)| n).product();
        let mut best = f64::NEG_INFINITY;
        for mut code in 0..count {
            let mut pure = profile.clone();
            for (key, num_actions) in own {
                let mut strategy = vec![0.0; *num_actions];
                strategy[code % num_actions] = 1.0;
                code /= num_actions;
                pure.insert(key.clone(), strategy);
            }
            best = best.max(expected_values(game, &pure)[player]);
        }
        (best, count)
    }

    /// A random Kuhn profile. About one probability in six is exactly 0 and one in six exactly
    /// 1, which leaves some information sets unreached.
    fn random_kuhn(rng: &mut ChaCha8Rng) -> Profile<KuhnInfoSet> {
        let bets: Vec<(KuhnInfoSet, f64)> = (0..2)
            .flat_map(kuhn_info_sets)
            .map(|key| {
                let bet = match below(rng, 6) {
                    0 => 0.0,
                    1 => 1.0,
                    _ => uniform(rng, 0.0, 1.0),
                };
                (key, bet)
            })
            .collect();
        kuhn_profile(&bets)
    }

    #[test]
    fn kuhn_best_responses_match_a_brute_force_over_all_pure_strategies() {
        // Each player has six information sets with two actions, so 64 pure strategies. The
        // best response must be worth as much as the best of them, and must be one of them.
        // On most profiles the clairvoyant value, which picks per history, comes out well
        // above that, so the comparison would catch the mistake. It ties only where zeros in
        // the profile leave the hidden card nothing to decide: measured, 163 of the 200 cases
        // have a gap above 0.01, the median gap is 0.098, and none is below -1e-15.
        let mut rng = ChaCha8Rng::seed_from_u64(18);
        let mut wide_gaps = 0;
        for _ in 0..100 {
            let profile = random_kuhn(&mut rng);
            for player in 0..2 {
                let own: Vec<(KuhnInfoSet, usize)> =
                    kuhn_info_sets(player).into_iter().map(|k| (k, 2)).collect();
                let (best, count) = brute_force(&Kuhn, &profile, player, &own);
                assert_eq!(count, 64);
                let response = best_response(&Kuhn, &profile, player);
                assert!(
                    (response.value - best).abs() < 1e-12,
                    "player {player}: best response {} against brute force {best}",
                    response.value
                );
                let played = expected_values(&Kuhn, &with_response(&profile, &response.strategy));
                assert!((played[player] - best).abs() < 1e-12, "{played:?}");
                let keys: Vec<&KuhnInfoSet> = response.strategy.iter().map(|(k, _)| k).collect();
                assert_eq!(keys, own.iter().map(|(k, _)| k).collect::<Vec<_>>());
                let gap = clairvoyant(&Kuhn, &profile, player, &Kuhn.root()) - response.value;
                assert!(gap > -1e-12, "player {player}: seeing the cards lost {gap}");
                if gap > 0.01 {
                    wide_gaps += 1;
                }
            }
        }
        assert!(wide_gaps >= 150, "{wide_gaps}");
    }

    #[test]
    fn unreached_information_sets_play_action_zero() {
        // Player 0 never bets at the root, so player 1's information sets after "b" are never
        // reached. Every action is worth zero there, and the best response checks or folds.
        let mut bets: Vec<(KuhnInfoSet, f64)> = kuhn_info_sets(0)
            .into_iter()
            .map(|k| (k, if k.history.is_empty() { 0.0 } else { 0.5 }))
            .collect();
        bets.extend(kuhn_info_sets(1).into_iter().map(|k| (k, 0.5)));
        let profile = kuhn_profile(&bets);
        let response = best_response(&Kuhn, &profile, 1);
        for card in [Jack, Queen, King] {
            assert_eq!(chosen(&response.strategy, &info_set(card, "b")), Kuhn::PASS);
        }
        // The rest of the response still matches the brute force.
        let own: Vec<(KuhnInfoSet, usize)> =
            kuhn_info_sets(1).into_iter().map(|k| (k, 2)).collect();
        let (best, _) = brute_force(&Kuhn, &profile, 1, &own);
        assert!((response.value - best).abs() < 1e-12);
    }

    #[test]
    fn the_responders_own_strategy_is_ignored() {
        let mut rng = ChaCha8Rng::seed_from_u64(7);
        let profile = random_kuhn(&mut rng);
        let other = random_kuhn(&mut rng);
        for player in 0..2 {
            let response = best_response(&Kuhn, &profile, player);
            let own = kuhn_info_sets(player);
            // Replace the responder's strategies, then drop them altogether.
            let mut changed = profile.clone();
            let mut missing = Profile::new();
            for (key, strategy) in profile.iter() {
                if own.contains(key) {
                    changed.insert(*key, other[key].to_vec());
                } else {
                    missing.insert(*key, strategy.to_vec());
                }
            }
            assert_eq!(best_response(&Kuhn, &changed, player), response);
            assert_eq!(best_response(&Kuhn, &missing, player), response);
        }
    }

    /// Kuhn's equilibrium with parameter `alpha`, from the [`crate::kuhn`] documentation.
    fn kuhn_equilibrium(alpha: f64) -> Profile<KuhnInfoSet> {
        kuhn_profile(&[
            (info_set(Jack, ""), alpha),
            (info_set(Queen, ""), 0.0),
            (info_set(King, ""), 3.0 * alpha),
            (info_set(Jack, "pb"), 0.0),
            (info_set(Queen, "pb"), alpha + THIRD),
            (info_set(King, "pb"), 1.0),
            (info_set(Jack, "p"), THIRD),
            (info_set(Queen, "p"), 0.0),
            (info_set(King, "p"), 1.0),
            (info_set(Jack, "b"), 0.0),
            (info_set(Queen, "b"), THIRD),
            (info_set(King, "b"), 1.0),
        ])
    }

    #[test]
    fn kuhn_equilibria_cannot_be_exploited() {
        // The probabilities are rounded to f32, which leaves about 1e-8.
        for alpha in [0.0, 1.0 / 6.0, THIRD] {
            let equilibrium = kuhn_equilibrium(alpha);
            let first = best_response(&Kuhn, &equilibrium, 0).value;
            let second = best_response(&Kuhn, &equilibrium, 1).value;
            assert!((first - KUHN_VALUE).abs() < 1e-7, "α = {alpha}: {first}");
            assert!((second + KUHN_VALUE).abs() < 1e-7, "α = {alpha}: {second}");
            let exploitability = exploitability(&Kuhn, &equilibrium);
            assert!(exploitability.abs() < 1e-7, "α = {alpha}: {exploitability}");
        }
        // Moving player 1's queen call off 1/3 lets player 0 exploit it.
        let mut off = kuhn_equilibrium(0.2);
        off.insert(info_set(Queen, "b"), vec![0.5, 0.5]);
        assert!(exploitability(&Kuhn, &off) > 1e-3);
    }

    fn schedules() -> [Schedule; 4] {
        [
            Schedule::Vanilla,
            Schedule::CfrPlus,
            Schedule::Linear,
            Schedule::Discounted(Dcfr::default()),
        ]
    }

    #[test]
    fn kuhn_exploitability_falls_toward_zero_as_cfr_converges() {
        // Measured exploitability of the average strategy, which starts at 11/24 = 0.458:
        //
        // | Schedule        | 10      | 100     | 1,000   | 10,000  |
        // |-----------------|---------|---------|---------|---------|
        // | vanilla CFR     | 6.87e-2 | 8.23e-3 | 9.38e-4 | 1.13e-4 |
        // | CFR+            | 3.27e-2 | 1.19e-3 | 8.74e-5 | 9.40e-6 |
        // | linear CFR      | 2.13e-2 | 1.09e-3 | 1.00e-4 | 7.68e-6 |
        // | DCFR(1.5, 0, 2) | 2.28e-2 | 1.67e-3 | 1.49e-4 | 1.98e-5 |
        //
        // Each bound below is about twice the measurement at 1,000 iterations.
        let bounds = [2e-3, 2e-4, 2e-4, 3e-4];
        for (schedule, bound) in schedules().into_iter().zip(bounds) {
            let mut cfr = Cfr::new(Kuhn, schedule);
            let mut previous = exploitability(&Kuhn, &cfr.average_strategy());
            for target in [10, 100, 1000] {
                cfr.run(target - cfr.iterations(), |_| {});
                let measured = exploitability(&Kuhn, &cfr.average_strategy());
                assert!(
                    measured < previous,
                    "{schedule:?}: {measured} after {target} is not below {previous}"
                );
                previous = measured;
            }
            assert!(previous < bound, "{schedule:?}: {previous}");
        }
    }

    /// Player 0's value and the exploitability of the average strategy after each iteration.
    fn kuhn_curve(schedule: Schedule, iterations: u64) -> Vec<(f64, f64)> {
        let mut cfr = Cfr::new(Kuhn, schedule);
        let mut curve = Vec::new();
        cfr.run(iterations, |cfr| {
            let average = cfr.average_strategy();
            let value = expected_values(&Kuhn, &average)[0];
            curve.push((value, exploitability(&Kuhn, &average)));
        });
        curve
    }

    /// The first iteration from which the gate's two conditions hold on every iteration up to
    /// the end of `curve`.
    fn settled_from(curve: &[(f64, f64)]) -> Option<usize> {
        let good = |&(value, exploitability): &(f64, f64)| {
            (value - KUHN_VALUE).abs() < 1e-3 && exploitability < 1e-3
        };
        let last_bad = curve.iter().rposition(|point| !good(point));
        match last_bad {
            None => Some(1),
            Some(index) if index + 1 < curve.len() => Some(index + 2),
            Some(_) => None,
        }
    }

    #[test]
    fn kuhn_gate_cfr_plus_and_dcfr_reach_the_value_and_low_exploitability() {
        // The M2 gate: player 0's value within 1e-3 of -1/18 and exploitability below 1e-3.
        // Measured over 20,000 iterations, checking after every one:
        //
        // | Schedule        | value first within 1e-3 | both first hold | both hold from |
        // |-----------------|-------------------------|-----------------|----------------|
        // | CFR+            | 6                       | 68              | 137            |
        // | DCFR(1.5, 0, 2) | 10                      | 136             | 205            |
        //
        // The exploitability is the condition that binds, and after first meeting it each
        // schedule fails it again on some iterations before it settles. The test runs 600
        // iterations, about three times the later settling point, and requires both
        // conditions on every iteration from 300 on.
        for schedule in [Schedule::CfrPlus, Schedule::Discounted(Dcfr::default())] {
            let curve = kuhn_curve(schedule, 600);
            let from = settled_from(&curve).unwrap_or(usize::MAX);
            assert!(from <= 300, "{schedule:?}: settled from iteration {from}");
            let (value, exploitability) = curve[curve.len() - 1];
            assert!((value - KUHN_VALUE).abs() < 1e-3, "{schedule:?}: {value}");
            assert!(exploitability < 1e-3, "{schedule:?}: {exploitability}");
        }
    }

    /// A node of a [`TreeGame`].
    #[derive(Debug, Clone)]
    enum Spec {
        Terminal(Vec<f64>),
        Chance(Vec<f64>, Vec<usize>),
        Player(usize, Key, Vec<usize>),
    }

    /// An information set of a [`TreeGame`]: the player, its private type and its node of
    /// the public tree.
    type Key = (usize, usize, usize);

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
    }

    impl Game for TreeGame {
        type State = usize;
        type InfoSet = Key;

        fn num_players(&self) -> usize {
            self.num_players
        }

        fn root(&self) -> usize {
            self.nodes.len() - 1
        }

        fn node(&self, &state: &usize) -> Node<Key> {
            match &self.nodes[state] {
                Spec::Terminal(utilities) => Node::Terminal {
                    utilities: utilities.clone(),
                },
                Spec::Chance(probabilities, _) => Node::Chance {
                    probabilities: probabilities.clone(),
                },
                Spec::Player(player, key, children) => Node::Player {
                    player: *player,
                    info_set: *key,
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

    /// A probability distribution over `n` outcomes, about a quarter of them zero.
    fn random_distribution(rng: &mut ChaCha8Rng, n: usize) -> Vec<f64> {
        let mut weights: Vec<f64> = (0..n)
            .map(|_| {
                if below(rng, 4) == 0 {
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
            let probabilities = random_distribution(rng, n);
            let children = (0..n)
                .map(|_| random_public_tree(rng, num_players, depth + 1, tree))
                .collect();
            Public::Chance(probabilities, children)
        } else {
            let n = 2 + below(rng, 2);
            let children = (0..n)
                .map(|_| random_public_tree(rng, num_players, depth + 1, tree))
                .collect();
            Public::Player(below(rng, num_players), children)
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
        let spec = match &tree[public] {
            Public::Terminal => {
                Spec::Terminal(types.iter().map(|_| uniform(rng, -1.0, 1.0)).collect())
            }
            Public::Chance(probabilities, children) => {
                let children = children
                    .iter()
                    .map(|&child| instantiate(rng, game, tree, child, types))
                    .collect();
                Spec::Chance(probabilities.clone(), children)
            }
            Public::Player(player, children) => {
                let children = children
                    .iter()
                    .map(|&child| instantiate(rng, game, tree, child, types))
                    .collect();
                Spec::Player(*player, (*player, types[*player], public), children)
            }
        };
        game.add(spec)
    }

    /// A random general-sum game with perfect recall: chance deals each player one of two
    /// private types, some deals with probability zero, then the players act in a random
    /// public tree with chance nodes in it. An information set is the player, its type and
    /// the public node, so a player acting twice on one path has information sets below
    /// others, each holding a history for every type of the other players.
    fn random_game(rng: &mut ChaCha8Rng, num_players: usize) -> TreeGame {
        let mut tree = Vec::new();
        let root = random_public_tree(rng, num_players, 0, &mut tree);
        let num_deals = 1 << num_players;
        let deal = random_distribution(rng, num_deals);
        let mut game = TreeGame::new(num_players);
        let children = (0..num_deals)
            .map(|index| {
                let types: Vec<usize> = (0..num_players).map(|i| (index >> i) & 1).collect();
                instantiate(rng, &mut game, &tree, root, &types)
            })
            .collect();
        game.add(Spec::Chance(deal, children));
        game
    }

    /// Whether `player` acts at some state below another state where it acts, so that some of
    /// its information sets must be settled before others.
    fn acts_twice(game: &TreeGame, player: usize, state: usize, acted: bool) -> bool {
        match &game.nodes[state] {
            Spec::Terminal(_) => false,
            Spec::Chance(_, children) => children
                .iter()
                .any(|&child| acts_twice(game, player, child, acted)),
            Spec::Player(acting, _, children) => {
                let mine = *acting == player;
                (mine && acted)
                    || children
                        .iter()
                        .any(|&child| acts_twice(game, player, child, acted || mine))
            }
        }
    }

    #[test]
    fn random_game_best_responses_match_a_brute_force() {
        // Two- and three-player games, against random strategies with some zeros. A player
        // with more than 4,096 pure strategies is skipped. Measured: 79 of the 150 players
        // are checked, and 59 of those act twice on some path.
        let mut rng = ChaCha8Rng::seed_from_u64(1818);
        let mut checked = 0;
        let mut nested = 0;
        for index in 0..60 {
            let num_players = 2 + index % 2;
            let game = random_game(&mut rng, num_players);
            let shapes = Cfr::new(game.clone(), Schedule::Vanilla).average_strategy();
            let profile: Profile<Key> = shapes
                .iter()
                .map(|(&key, strategy)| {
                    let random = random_distribution(&mut rng, strategy.len());
                    (key, random.iter().map(|&p| p as f32).collect())
                })
                .collect();
            for player in 0..num_players {
                let own: Vec<(Key, usize)> = shapes
                    .iter()
                    .filter(|(key, _)| key.0 == player)
                    .map(|(&key, strategy)| (key, strategy.len()))
                    .collect();
                if own.iter().map(|(_, n)| n).product::<usize>() > 4096 {
                    continue;
                }
                let (best, _) = brute_force(&game, &profile, player, &own);
                let response = best_response(&game, &profile, player);
                assert!(
                    (response.value - best).abs() < 1e-12,
                    "game {index}, player {player}: {} against {best}",
                    response.value
                );
                let played = expected_values(&game, &with_response(&profile, &response.strategy));
                assert!((played[player] - best).abs() < 1e-12);
                assert_eq!(response.strategy.len(), own.len());
                checked += 1;
                if acts_twice(&game, player, game.root(), false) {
                    nested += 1;
                }
            }
        }
        assert!(
            checked >= 70 && nested >= 50,
            "{checked} checked, {nested} nested"
        );
    }

    #[test]
    #[should_panic(expected = "two-player games")]
    fn exploitability_needs_two_players() {
        let mut game = TreeGame::new(3);
        game.add(Spec::Terminal(vec![1.0, -0.5, -0.5]));
        exploitability(&game, &Profile::new());
    }

    #[test]
    #[should_panic(expected = "zero-sum game")]
    fn exploitability_needs_a_zero_sum_game() {
        let mut game = TreeGame::new(2);
        let leaves = [
            game.add(Spec::Terminal(vec![1.0, -1.0])),
            game.add(Spec::Terminal(vec![1.0, 0.0])),
        ];
        game.add(Spec::Chance(vec![0.5, 0.5], leaves.to_vec()));
        exploitability(&game, &Profile::new());
    }

    #[test]
    #[should_panic(expected = "does not have perfect recall")]
    fn forgetting_an_own_action_is_detected() {
        // One player picks left or right, then forgets which.
        let mut game = TreeGame::new(1);
        let leaves: Vec<usize> = (0..4)
            .map(|u| game.add(Spec::Terminal(vec![f64::from(u)])))
            .collect();
        let forgot = (0, 0, 1);
        let left = game.add(Spec::Player(0, forgot, leaves[..2].to_vec()));
        let right = game.add(Spec::Player(0, forgot, leaves[2..].to_vec()));
        game.add(Spec::Player(0, (0, 0, 0), vec![left, right]));
        best_response(&game, &Profile::new(), 0);
    }

    #[test]
    #[should_panic(expected = "no strategy for")]
    fn the_other_players_strategies_are_required() {
        best_response(&Kuhn, &Profile::new(), 0);
    }
}
