//! Best response and exploitability in vector form, over the public tree of a [`PublicGame`].
//!
//! The quantities are those [`best_response`](mod@crate::best_response) defines, `BR_i(σ)` and
//! `(BR_0(σ) + BR_1(σ)) / 2`, computed by one walk of the public tree per player instead of
//! one walk of the whole game tree. The walk carries the opponent's reach for each of its
//! hands, as in [`crate::vector`], and returns the responder's counterfactual values for each
//! of its hands.
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

use crate::vector::{PublicGame, PublicNode, PublicProfile, check_game};

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
/// are ignored, and may be missing.
///
/// Panics if `player` is not 0 or 1, if `profile` has no strategy, or one of the wrong size,
/// at a node of the other player reachable from the root, or for any reason
/// [`VectorCfr::new`](crate::vector::VectorCfr::new) panics on a malformed game.
pub fn vector_best_response<G: PublicGame>(
    game: &G,
    profile: &PublicProfile,
    player: usize,
) -> VectorBestResponse {
    assert!(player < 2, "player {player} is not 0 or 1");
    check_game(game, |_, _, _, _| {});
    let num_hands = game.num_hands(player);
    let mut walk = Respond {
        game,
        profile,
        player,
        strategy: PublicProfile::empty(game.num_nodes()),
    };
    let hand_values = walk.visit(game.root(), game.deal(1 - player), &vec![true; num_hands]);
    let mut value = 0.0;
    for (&deal, &hand_value) in game.deal(player).iter().zip(&hand_values) {
        value += deal * hand_value;
    }
    VectorBestResponse {
        player,
        value,
        hand_values,
        strategy: walk.strategy,
    }
}

/// `(BR_0(σ) + BR_1(σ)) / 2` for a two-player zero-sum game, as the
/// [module documentation](self) defines it. Zero exactly at a Nash equilibrium.
///
/// Panics for any reason [`vector_best_response`] panics.
pub fn vector_exploitability<G: PublicGame>(game: &G, profile: &PublicProfile) -> f64 {
    let first = vector_best_response(game, profile, 0).value;
    let second = vector_best_response(game, profile, 1).value;
    (first + second) / 2.0
}

/// The working state of one best-response walk.
struct Respond<'a, G> {
    game: &'a G,
    profile: &'a PublicProfile,
    player: usize,
    /// The choices made so far.
    strategy: PublicProfile,
}

impl<G: PublicGame> Respond<'_, G> {
    /// Walks the tree below `node`, returning `q*(node, h)` for each hand `h` of the responder.
    /// `opponent_reach` is `ρ`, and `possible` says which of the responder's hands are possible
    /// at `node`.
    fn visit(&mut self, node: usize, opponent_reach: &[f64], possible: &[bool]) -> Vec<f64> {
        let game = self.game;
        let num_hands = possible.len();
        match game.node(node) {
            PublicNode::Terminal => {
                let mut values = vec![0.0; num_hands];
                game.terminal_values(node, self.player, opponent_reach, &mut values);
                for (value, &possible) in values.iter_mut().zip(possible) {
                    if !possible {
                        *value = 0.0;
                    }
                }
                values
            }
            PublicNode::Chance { num_outcomes } => {
                let opponent = 1 - self.player;
                let mut values = vec![0.0; num_hands];
                for outcome in 0..num_outcomes {
                    let probability = game.outcome_probability(node, outcome);
                    let after: Vec<bool> = possible
                        .iter()
                        .enumerate()
                        .map(|(hand, &p)| p && !game.rules_out(node, outcome, self.player, hand))
                        .collect();
                    let other: Vec<f64> = opponent_reach
                        .iter()
                        .enumerate()
                        .map(|(hand, &reach)| {
                            if game.rules_out(node, outcome, opponent, hand) {
                                0.0
                            } else {
                                reach * probability
                            }
                        })
                        .collect();
                    let child = game.child(node, outcome);
                    let child_values = self.visit(child, &other, &after);
                    for (value, child_value) in values.iter_mut().zip(child_values) {
                        *value += child_value;
                    }
                }
                values
            }
            PublicNode::Player {
                player,
                num_actions,
            } if player == self.player => {
                let mut best = vec![f64::NEG_INFINITY; num_hands];
                let mut choice = vec![0; num_hands];
                for action in 0..num_actions {
                    let child = game.child(node, action);
                    let child_values = self.visit(child, opponent_reach, possible);
                    for hand in 0..num_hands {
                        if child_values[hand] > best[hand] {
                            best[hand] = child_values[hand];
                            choice[hand] = action;
                        }
                    }
                }
                let mut pure = vec![0.0; num_actions * num_hands];
                for (hand, &action) in choice.iter().enumerate() {
                    pure[action * num_hands + hand] = 1.0;
                }
                self.strategy.set(node, num_hands, pure);
                best
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
                let mut values = vec![0.0; num_hands];
                for (action, probabilities) in strategy.chunks_exact(num_other).enumerate() {
                    let reach: Vec<f64> = opponent_reach
                        .iter()
                        .zip(probabilities)
                        .map(|(&reach, &p)| reach * f64::from(p))
                        .collect();
                    let child = game.child(node, action);
                    let child_values = self.visit(child, &reach, possible);
                    for (value, child_value) in values.iter_mut().zip(child_values) {
                        *value += child_value;
                    }
                }
                values
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
    #[should_panic(expected = "no strategy at node")]
    fn the_other_players_strategies_are_required() {
        let game = PublicLeduc::new();
        vector_best_response(&game, &PublicProfile::empty(game.num_nodes()), 0);
    }
}
