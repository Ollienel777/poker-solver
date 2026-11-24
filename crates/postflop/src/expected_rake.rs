//! The expected rake of a river strategy, worked out apart from the payoffs, and the value the
//! two players' expected values must sum to.
//!
//! # What is computed
//!
//! For a profile `σ`, the probability that play ends at the terminal `t` is
//!
//! ```text
//! P(t) = Σ w_0(h) · π_0(t, h) · w_1(o) · π_1(t, o) / Z    over the pairs (h, o) that share no card
//! ```
//!
//! where `w_p` is player `p`'s range weight for each of its hands, `π_p(t, h)` its own reach of
//! `t` with `h`, the product of its action probabilities on the way from the root, and `Z` the
//! same sum with every reach 1: the total weight of the pairs that can be dealt, as the
//! [deal](crate::river#the-deal) defines it. The rake at `t` depends only on the chips. The pot
//! that is raked is the root pot plus twice the smaller of the two contributions, because the
//! part of a bet that was not called goes back to the bettor unraked, and the house takes
//! `percent` of it, at most `cap`, and nothing when the percent is zero. No flop no drop does
//! not apply, since every pot here has seen the flop. Then
//!
//! ```text
//! M       = Σ_t P(t)              the probability of ending somewhere
//! E[rake] = Σ_t P(t) · rake(t)    in chips per hand dealt
//! ```
//!
//! At every terminal, for every pair of hands, the two players' payoffs sum to the root pot less
//! that terminal's rake ([`crate::payoff`]). So for any profile, the two players' expected values
//! must sum to
//!
//! ```text
//! v_0(σ) + v_1(σ) = Σ_t P(t) · (pot − rake(t)) = pot · M − E[rake]
//! ```
//!
//! which is [`ExpectedRake::value_sum`]. When each hand's action probabilities sum to 1 at
//! every node, `M` is 1 and this is the pot less the expected rake. The `f32` strategies a
//! solve produces sum to 1 only to a few `f32` ulps, so that `M` is off by up to 1.9e-8 on the
//! river test spots and `pot − E[rake]` by that much of the pot. A comparison at `f64`
//! precision uses `pot · M − E[rake]` instead.
//!
//! # Independence
//!
//! This is a check on the payoffs and the walks that sum them, so it shares none of their code.
//! It reads the tree's shape and each terminal's contributions, each player's combos and range
//! weights, the root pot and the rake settings ([`RiverGame::tree`],
//! [`RiverGame::hands`], [`RiverGame::pot`] and [`RiverGame::rake`]), and the profile. It never
//! reads a terminal's [`Payoff`](crate::Payoff), [`rake_on`](crate::rake_on), the deal vectors,
//! [`RiverGame::reach`] or the evaluations of [`BoardHands`](crate::BoardHands):
//!
//! - The rake at a terminal is worked out here from its contributions, by the rule above.
//! - The reaches are multiplied down the tree from the root by one walk here.
//! - The pairs that share a card are removed by counting cards, not hands. With
//!   `a(h) = w_0(h) · π_0(t, h)` and `b(o) = w_1(o) · π_1(t, o)`, their totals `A` and `B`, and
//!   `A_c` and `B_c` their sums over the hands that hold the card `c`,
//!
//!   ```text
//!   Σ a(h) · b(o) over the pairs that share no card = A · B − Σ_c A_c · B_c + Σ a(h) · b(h)
//!   ```
//!
//!   where the last sum runs over the combos both players hold. A pair that shares one card is
//!   counted once in `Σ_c A_c · B_c`, and a pair of the same combo, which shares both, twice.
//!   This is O(n) per terminal, plus the 52 cards. The terminal evaluations remove the pairs
//!   hand by hand instead ([`crate::terminal`]), and the tests check this against the O(n²) sum
//!   over every pair.
//!
//! The walk visits every node once, at O(n) per action and per terminal.

use pk_cfr::PublicProfile;
use pk_core::NUM_CARDS;
use pk_game::{Chips, Node, Player, PostflopTree, Rake};

use crate::hands::HandList;
use crate::river::RiverGame;

/// One terminal of the tree, as a profile reaches it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TerminalOutcome {
    /// The terminal's node index.
    pub node: u32,
    /// `P(t)`, the probability that play ends here.
    pub probability: f64,
    /// The pot the rake is taken from, in chips: the root pot plus twice the smaller
    /// contribution.
    pub raked_pot: f64,
    /// The rake taken here, in chips.
    pub rake: f64,
}

/// How a profile ends and the rake it pays, as the [module documentation](self) defines them.
#[derive(Debug, Clone, PartialEq)]
pub struct ExpectedRake {
    /// The pot at the root, in chips.
    pub pot: Chips,
    /// Every terminal of the tree, in node order.
    pub terminals: Vec<TerminalOutcome>,
    /// `M = Σ_t P(t)`, the probability of ending at some terminal: 1 when every strategy sums
    /// to 1.
    pub probability: f64,
    /// `E[rake] = Σ_t P(t) · rake(t)`, in chips per hand dealt.
    pub rake: f64,
}

impl ExpectedRake {
    /// `pot · M − E[rake]`: what the two players' expected values under the profile must sum to.
    pub fn value_sum(&self) -> f64 {
        f64::from(self.pot) * self.probability - self.rake
    }
}

/// The probability of every terminal of `game` under `profile`, the rake taken at each, and the
/// expected rake, as the [module documentation](self) defines them.
///
/// Panics if `profile` has no strategy, or one of the wrong size, at a player node.
pub fn expected_rake(game: &RiverGame, profile: &PublicProfile) -> ExpectedRake {
    let hands = [game.hands(Player::Oop), game.hands(Player::Ip)];
    let pairs = Pairs::new(hands);
    // Each player's weight times its own reach, by hand: the range weights at the root.
    let weights = hands.map(|list| list.weights().to_vec());
    let total = pairs.weight(&weights);
    assert!(
        total > 0.0,
        "a river game has a pair of hands that can be dealt together"
    );
    let mut walk = Walk {
        tree: game.tree(),
        profile,
        pot: game.pot(),
        rake: game.rake(),
        pairs: &pairs,
        total,
        terminals: Vec::new(),
    };
    walk.visit(PostflopTree::ROOT, &weights);
    let mut terminals = walk.terminals;
    terminals.sort_by_key(|terminal| terminal.node);
    let probability = terminals.iter().map(|t| t.probability).sum();
    let rake = terminals.iter().map(|t| t.probability * t.rake).sum();
    ExpectedRake {
        pot: game.pot(),
        terminals,
        probability,
        rake,
    }
}

/// The rake at a terminal where the players put in `contributions` during the tree, by the rule
/// of the [module documentation](self): the raked pot and the rake taken from it.
fn terminal_rake(root_pot: Chips, contributions: [Chips; 2], rake: &Rake) -> (f64, f64) {
    let called = contributions[0].min(contributions[1]);
    let raked_pot = f64::from(root_pot) + 2.0 * f64::from(called);
    let taken = if rake.percent == 0.0 {
        0.0
    } else {
        (raked_pot * rake.percent / 100.0).min(f64::from(rake.cap))
    };
    (raked_pot, taken)
}

/// Both players' hands as their cards, for summing over the pairs of hands that share no card.
struct Pairs {
    /// By player, each hand's two card indices.
    cards: [Vec<[usize; 2]>; 2],
    /// The combos both players hold, as (OOP hand, IP hand).
    same: Vec<(usize, usize)>,
}

impl Pairs {
    fn new(hands: [&HandList; 2]) -> Pairs {
        let cards = hands.map(|list| {
            list.combos()
                .iter()
                .map(|combo| [combo.lo().index(), combo.hi().index()])
                .collect()
        });
        let same = hands[0]
            .combos()
            .iter()
            .enumerate()
            .filter_map(|(h, &combo)| Some((h, hands[1].position(combo)?)))
            .collect();
        Pairs { cards, same }
    }

    /// `Σ a(h) · b(o)` over the pairs of hands that share no card, for `[a, b]`, by counting
    /// cards as the [module documentation](self) describes.
    fn weight(&self, weights: &[Vec<f64>; 2]) -> f64 {
        let mut totals = [0.0f64; 2];
        let mut by_card = [[0.0f64; NUM_CARDS]; 2];
        for player in 0..2 {
            for (&w, &[lo, hi]) in weights[player].iter().zip(&self.cards[player]) {
                totals[player] += w;
                by_card[player][lo] += w;
                by_card[player][hi] += w;
            }
        }
        let sharing_a_card: f64 = (0..NUM_CARDS)
            .map(|card| by_card[0][card] * by_card[1][card])
            .sum();
        let same_combo: f64 = self
            .same
            .iter()
            .map(|&(h, o)| weights[0][h] * weights[1][o])
            .sum();
        totals[0] * totals[1] - sharing_a_card + same_combo
    }
}

/// One walk of the tree, collecting the terminals.
struct Walk<'a> {
    tree: &'a PostflopTree,
    profile: &'a PublicProfile,
    pot: Chips,
    rake: &'a Rake,
    pairs: &'a Pairs,
    /// `Z`.
    total: f64,
    terminals: Vec<TerminalOutcome>,
}

impl Walk<'_> {
    /// Visits `node`, where `weights` is each player's range weight times its own reach of the
    /// node, by hand.
    fn visit(&mut self, node: u32, weights: &[Vec<f64>; 2]) {
        match self.tree.node(node) {
            Node::Player {
                actor,
                actions,
                children,
                ..
            } => {
                let me = actor.index();
                let num_hands = weights[me].len();
                let strategy = self
                    .profile
                    .get(node as usize)
                    .unwrap_or_else(|| panic!("the profile has no strategy at node {node}"));
                assert_eq!(
                    strategy.len(),
                    actions.len() * num_hands,
                    "the profile's strategy at node {node} has the wrong size"
                );
                for (probabilities, &child) in strategy.chunks_exact(num_hands).zip(children) {
                    let mut next = weights.clone();
                    for (weight, &p) in next[me].iter_mut().zip(probabilities) {
                        *weight *= f64::from(p);
                    }
                    self.visit(child, &next);
                }
            }
            Node::Terminal { contributions, .. } => {
                let (raked_pot, rake) = terminal_rake(self.pot, *contributions, self.rake);
                self.terminals.push(TerminalOutcome {
                    node,
                    probability: self.pairs.weight(weights) / self.total,
                    raked_pot,
                    rake,
                });
            }
            Node::Chance { .. } => panic!("a river tree has no chance node"),
        }
    }
}

#[cfg(test)]
mod tests {
    use pk_cfr::{PublicGame, PublicNode};
    use pk_game::Outcome;

    use super::*;
    use crate::spot::Spot;

    /// A raked spot whose ranges share two combos (AhAd and KhQd) and several cards, with a
    /// raise, so that a fold can leave part of a bet uncalled. 5% capped at 1bb: the unbet pot
    /// pays 0.5bb, and every pot with a bet called hits the cap.
    const SPOT: &str = r#"format = 1
name = "small"
description = "Illustrative ranges, for the expected rake's tests."
board = "Qh9c7d4s2h"
pot_bb = 10.0
effective_stack_bb = 25.0
oop_range = "AhAd,AcAs:0.5,KhQd,KcQc:0.75,9s8s,JhTh"
ip_range = "AhAd:0.5,KsQs,KhQd:0.25,QcJc,9d8d:0.75,ThTc"

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

    /// A profile in which each hand's probabilities at each node depend on the node and the
    /// hand, none zero, and sum to 1 up to `f32` rounding.
    fn mixed(game: &RiverGame) -> PublicProfile {
        PublicProfile::from_fn(game, |node, hand| {
            let PublicNode::Player { num_actions, .. } = game.node(node) else {
                unreachable!();
            };
            let raw: Vec<f64> = (0..num_actions)
                .map(|a| 1.0 + ((node * 7 + hand * 3 + a * 5) % 11) as f64)
                .collect();
            let total: f64 = raw.iter().sum();
            raw.iter().map(|r| (r / total) as f32).collect()
        })
    }

    /// `P(t)` for every terminal by the double loop over every pair of hands, with the reaches
    /// from [`RiverGame::reach`].
    fn by_pairs(game: &RiverGame, profile: &PublicProfile) -> Vec<(u32, f64)> {
        let (oop, ip) = (game.hands(Player::Oop), game.hands(Player::Ip));
        let mut total = 0.0;
        for (a, w) in oop.combos().iter().zip(oop.weights()) {
            for (b, v) in ip.combos().iter().zip(ip.weights()) {
                if !a.cards().intersects(b.cards()) {
                    total += w * v;
                }
            }
        }
        (0..game.num_nodes() as u32)
            .filter(|&node| game.node(node as usize) == PublicNode::Terminal)
            .map(|node| {
                let reach = game.reach(profile, node);
                let mut sum = 0.0;
                for (h, a) in oop.combos().iter().enumerate() {
                    for (o, b) in ip.combos().iter().enumerate() {
                        if !a.cards().intersects(b.cards()) {
                            sum += oop.weights()[h] * reach[0][h] * ip.weights()[o] * reach[1][o];
                        }
                    }
                }
                (node, sum / total)
            })
            .collect()
    }

    #[test]
    fn terminal_probabilities_are_the_sum_over_the_pairs_that_can_be_dealt() {
        let game = RiverGame::new(&spot()).unwrap();
        // The ranges share combos and cards, so the card counting has something to remove.
        let (oop, ip) = (game.hands(Player::Oop), game.hands(Player::Ip));
        assert!(oop.combos().iter().any(|&c| ip.position(c).is_some()));
        for profile in [
            PublicProfile::from_fn(&game, |node, _| {
                let PublicNode::Player { num_actions, .. } = game.node(node) else {
                    unreachable!();
                };
                vec![1.0 / num_actions as f32; num_actions]
            }),
            mixed(&game),
        ] {
            let expected = expected_rake(&game, &profile);
            let pairs = by_pairs(&game, &profile);
            assert_eq!(expected.terminals.len(), pairs.len());
            for (terminal, &(node, probability)) in expected.terminals.iter().zip(&pairs) {
                assert_eq!(terminal.node, node);
                assert!(
                    (terminal.probability - probability).abs() < 1e-15,
                    "node {node}: {} against {probability}",
                    terminal.probability
                );
                assert!(terminal.probability > 0.0);
            }
            assert!((expected.probability - 1.0).abs() < 1e-6, "{expected:?}");
        }
    }

    #[test]
    fn the_total_weight_is_the_games_pair_weight() {
        let game = RiverGame::new(&spot()).unwrap();
        let pairs = Pairs::new([game.hands(Player::Oop), game.hands(Player::Ip)]);
        let weights = [Player::Oop, Player::Ip].map(|p| game.hands(p).weights().to_vec());
        let total = pairs.weight(&weights);
        assert!((total - game.pair_weight()).abs() <= 1e-14 * total);
    }

    #[test]
    fn the_rake_at_each_terminal_comes_from_the_called_chips() {
        // 10bb root pot, 5% capped at 1bb.
        let rake = spot().rake();
        // Checked down: 5% of 1,000 chips.
        assert_eq!(terminal_rake(1000, [0, 0], &rake), (1000.0, 50.0));
        // OOP bets 500 and IP folds: nothing was called.
        assert_eq!(terminal_rake(1000, [500, 0], &rake), (1000.0, 50.0));
        // A bet called, 2,000 chips: the cap.
        assert_eq!(terminal_rake(1000, [500, 500], &rake), (2000.0, 100.0));
        // OOP bets 500, IP raises to 1,500 and OOP folds: only 500 of the raise was called.
        assert_eq!(terminal_rake(1000, [500, 1500], &rake), (2000.0, 100.0));
        // Below the cap, and not rounded to whole chips.
        assert_eq!(terminal_rake(555, [0, 0], &rake), (555.0, 27.75));
        // No rake at all when the percent is zero, whatever the cap.
        let free = Rake {
            percent: 0.0,
            ..rake.clone()
        };
        assert_eq!(terminal_rake(1000, [500, 500], &free), (2000.0, 0.0));
        let no_cap = Rake { cap: 0, ..rake };
        assert_eq!(terminal_rake(1000, [500, 500], &no_cap), (2000.0, 0.0));
    }

    #[test]
    fn the_terminals_of_the_small_spot_pay_the_rake_their_chips_give() {
        let game = RiverGame::new(&spot()).unwrap();
        let expected = expected_rake(&game, &mixed(&game));
        let mut capped = 0;
        for terminal in &expected.terminals {
            let Node::Terminal {
                outcome,
                contributions,
                ..
            } = game.tree().node(terminal.node)
            else {
                panic!("node {} is not a terminal", terminal.node);
            };
            let called = contributions[0].min(contributions[1]);
            assert_eq!(terminal.raked_pot, f64::from(1000 + 2 * called));
            if called == 0 {
                assert_eq!(terminal.rake, 50.0);
            } else {
                assert_eq!(terminal.rake, 100.0);
                capped += 1;
            }
            if *outcome == Outcome::Showdown {
                assert_eq!(contributions[0], contributions[1]);
            }
        }
        assert!(capped > 0);
        // The expected rake is between the smallest and the largest rake.
        assert!(expected.rake > 50.0 * expected.probability);
        assert!(expected.rake < 100.0 * expected.probability);
        assert_eq!(
            expected.value_sum(),
            1000.0 * expected.probability - expected.rake
        );
    }
}
