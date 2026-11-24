//! Kuhn poker (Kuhn 1950) on the [`Game`] trait.
//!
//! The deck is a jack, a queen and a king, ranked in that order. Each player antes 1 and is
//! dealt one card, which only they see; the third card is not used. There is one betting round
//! with a single bet of 1 and no raise. Player 0 checks or bets. After a check, player 1 checks,
//! which goes to showdown, or bets. A player facing a bet folds or calls, and a call goes to
//! showdown. At showdown the higher card takes the pot.
//!
//! Action 0 is [`Kuhn::PASS`], a check or a fold, and action 1 is [`Kuhn::BET`], a bet or a
//! call. A history is written as a string of `p` and `b`, one letter per action. Player 0 acts
//! after `""` and `"pb"`, player 1 after `"p"` and `"b"`, and `"pp"`, `"pbp"`, `"pbb"`, `"bp"` and
//! `"bb"` end the game.
//!
//! Kuhn solved the game. Player 0's value is −1/18. Player 0's equilibrium strategies form a
//! family with one parameter α in [0, 1/3]: bet the jack with probability α, and fold it to a
//! bet; check the queen, and call a bet with probability α + 1/3; bet the king with probability
//! 3α, and call a bet (which never comes when α = 1/3). Player 1 has one equilibrium strategy:
//! with the jack, bet 1/3 of the time after a check and fold to a bet; with the queen, check
//! after a check and call a bet 1/3 of the time; with the king, always bet or call.
//!
//! # References
//!
//! - Kuhn, *A Simplified Two-Person Poker*, in Contributions to the Theory of Games I, Annals
//!   of Mathematics Studies 24 (1950).

use crate::tabular::{Game, Node};

/// A Kuhn poker card.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum KuhnCard {
    /// The lowest card.
    Jack,
    /// The middle card.
    Queen,
    /// The highest card.
    King,
}

/// Kuhn poker, two players with player 0 acting first.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Kuhn;

impl Kuhn {
    /// Check, or fold to a bet.
    pub const PASS: usize = 0;
    /// Bet, or call a bet.
    pub const BET: usize = 1;
}

/// A Kuhn poker history: the deal, once chance has made it, and the actions since.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KuhnState {
    cards: Option<[KuhnCard; 2]>,
    history: &'static str,
}

/// A Kuhn poker information set: the acting player's card and the actions so far, written as
/// in the [module documentation](self).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct KuhnInfoSet {
    /// The acting player's card.
    pub card: KuhnCard,
    /// The actions so far: `""` or `"pb"` for player 0, `"p"` or `"b"` for player 1.
    pub history: &'static str,
}

/// The deals, as (player 0's card, player 1's card), in the order of the chance outcomes.
const DEALS: [[KuhnCard; 2]; 6] = {
    use KuhnCard::{Jack, King, Queen};
    [
        [Jack, Queen],
        [Jack, King],
        [Queen, Jack],
        [Queen, King],
        [King, Jack],
        [King, Queen],
    ]
};

impl Game for Kuhn {
    type State = KuhnState;
    type InfoSet = KuhnInfoSet;

    fn num_players(&self) -> usize {
        2
    }

    fn root(&self) -> KuhnState {
        KuhnState {
            cards: None,
            history: "",
        }
    }

    /// The root deals the six ordered pairs of distinct cards, each with probability 1/6.
    fn node(&self, state: &KuhnState) -> Node<KuhnInfoSet> {
        let Some(cards) = state.cards else {
            return Node::Chance {
                probabilities: vec![1.0 / 6.0; DEALS.len()],
            };
        };
        let player = match state.history {
            "" | "pb" => 0,
            "p" | "b" => 1,
            history => {
                let payoff = payoff(cards, history);
                return Node::Terminal {
                    utilities: vec![payoff, -payoff],
                };
            }
        };
        Node::Player {
            player,
            info_set: KuhnInfoSet {
                card: cards[player],
                history: state.history,
            },
            num_actions: 2,
        }
    }

    fn next(&self, state: &KuhnState, index: usize) -> KuhnState {
        let Some(cards) = state.cards else {
            return KuhnState {
                cards: Some(DEALS[index]),
                history: "",
            };
        };
        let history = match (state.history, index) {
            ("", Kuhn::PASS) => "p",
            ("", Kuhn::BET) => "b",
            ("p", Kuhn::PASS) => "pp",
            ("p", Kuhn::BET) => "pb",
            ("pb", Kuhn::PASS) => "pbp",
            ("pb", Kuhn::BET) => "pbb",
            ("b", Kuhn::PASS) => "bp",
            ("b", Kuhn::BET) => "bb",
            (history, index) => panic!("no action {index} after {history:?}"),
        };
        KuhnState {
            cards: Some(cards),
            history,
        }
    }
}

/// Player 0's payoff at the end of a terminal history.
fn payoff(cards: [KuhnCard; 2], history: &str) -> f64 {
    let showdown = if cards[0] > cards[1] { 1.0 } else { -1.0 };
    match history {
        "pp" => showdown,
        "pbb" | "bb" => 2.0 * showdown,
        // Player 0 folds to player 1's bet.
        "pbp" => -1.0,
        // Player 1 folds to player 0's bet.
        "bp" => 1.0,
        _ => unreachable!("{history:?} is not a terminal history"),
    }
}

#[cfg(test)]
mod tests {
    use KuhnCard::{Jack, King, Queen};

    use super::*;
    use crate::regret::{Dcfr, Schedule};
    use crate::tabular::{Cfr, Profile, expected_values};

    const THIRD: f64 = 1.0 / 3.0;
    const VALUE: f64 = -1.0 / 18.0;

    fn info_set(card: KuhnCard, history: &'static str) -> KuhnInfoSet {
        KuhnInfoSet { card, history }
    }

    /// The state after dealing `cards` and playing `history`.
    fn play(cards: [KuhnCard; 2], history: &str) -> KuhnState {
        let deal = DEALS.iter().position(|&deal| deal == cards).unwrap();
        let mut state = Kuhn.next(&Kuhn.root(), deal);
        for action in history.chars() {
            let index = if action == 'p' { Kuhn::PASS } else { Kuhn::BET };
            state = Kuhn.next(&state, index);
        }
        state
    }

    #[test]
    fn chance_deals_each_ordered_pair_of_cards_once() {
        let Node::Chance { probabilities } = Kuhn.node(&Kuhn.root()) else {
            panic!("the root is not a chance node");
        };
        assert_eq!(probabilities, [1.0 / 6.0; 6]);
        let mut deals: Vec<[KuhnCard; 2]> = (0..6)
            .map(|outcome| Kuhn.next(&Kuhn.root(), outcome).cards.unwrap())
            .collect();
        deals.sort();
        deals.dedup();
        assert_eq!(deals.len(), 6);
        assert!(deals.iter().all(|[first, second]| first != second));
    }

    #[test]
    fn each_player_sees_their_own_card_and_the_actions() {
        let cards = [Queen, King];
        let cases = [
            ("", 0, Queen),
            ("p", 1, King),
            ("b", 1, King),
            ("pb", 0, Queen),
        ];
        for (history, player, card) in cases {
            let expected = Node::Player {
                player,
                info_set: info_set(card, history),
                num_actions: 2,
            };
            assert_eq!(Kuhn.node(&play(cards, history)), expected, "{history:?}");
        }
        // Three cards for each of the four histories where someone acts.
        assert_eq!(
            Cfr::new(Kuhn, Schedule::Vanilla).average_strategy().len(),
            12
        );
    }

    #[test]
    fn payoffs_follow_the_rules() {
        // Player 0's payoff with the king against the jack, then with the queen against the
        // king. Showdowns go to the higher card, and a fold loses the ante whatever the cards.
        let cases = [
            (
                [King, Jack],
                [
                    ("pp", 1.0),
                    ("pbp", -1.0),
                    ("pbb", 2.0),
                    ("bp", 1.0),
                    ("bb", 2.0),
                ],
            ),
            (
                [Queen, King],
                [
                    ("pp", -1.0),
                    ("pbp", -1.0),
                    ("pbb", -2.0),
                    ("bp", 1.0),
                    ("bb", -2.0),
                ],
            ),
        ];
        for (cards, payoffs) in cases {
            for (history, payoff) in payoffs {
                let expected = Node::Terminal {
                    utilities: vec![payoff, -payoff],
                };
                assert_eq!(
                    Kuhn.node(&play(cards, history)),
                    expected,
                    "{cards:?} {history:?}"
                );
            }
        }
    }

    /// The equilibrium of the module documentation with parameter `alpha`.
    fn equilibrium(alpha: f64) -> Profile<KuhnInfoSet> {
        let bets = [
            (Jack, "", alpha),
            (Queen, "", 0.0),
            (King, "", 3.0 * alpha),
            (Jack, "pb", 0.0),
            (Queen, "pb", alpha + THIRD),
            (King, "pb", 1.0),
            (Jack, "p", THIRD),
            (Queen, "p", 0.0),
            (King, "p", 1.0),
            (Jack, "b", 0.0),
            (Queen, "b", THIRD),
            (King, "b", 1.0),
        ];
        bets.into_iter()
            .map(|(card, history, bet)| {
                (
                    info_set(card, history),
                    vec![(1.0 - bet) as f32, bet as f32],
                )
            })
            .collect()
    }

    #[test]
    fn every_equilibrium_is_worth_minus_one_eighteenth() {
        for alpha in [0.0, 1.0 / 6.0, THIRD] {
            let values = expected_values(&Kuhn, &equilibrium(alpha));
            assert!((values[0] - VALUE).abs() < 1e-7, "α = {alpha}: {values:?}");
            assert_eq!(values[1], -values[0]);
        }
        // Uniform play is worth 1/8 to player 0. The endings pp, pbp, pbb, bp and bb have
        // probabilities 1/4, 1/8, 1/8, 1/4 and 1/4. Holding the higher card they pay 1, -1, 2,
        // 1 and 2, for 9/8; holding the lower card -1, -1, -2, 1 and -2, for -7/8.
        let uniform = Cfr::new(Kuhn, Schedule::Vanilla).average_strategy();
        let values = expected_values(&Kuhn, &uniform);
        assert!((values[0] - 0.125).abs() < 1e-12, "{values:?}");
    }

    /// How far `profile` is from the equilibrium family: the largest violation of the module
    /// documentation's conditions, with α read off player 0's jack, and the α it read.
    fn distance_from_equilibrium(profile: &Profile<KuhnInfoSet>) -> (f64, f64) {
        let bet = |card, history| f64::from(profile[&info_set(card, history)][Kuhn::BET]);
        let alpha = bet(Jack, "");
        let violations = [
            (alpha - THIRD).max(0.0),
            bet(Queen, ""),
            (bet(King, "") - 3.0 * alpha).abs(),
            bet(Jack, "pb"),
            (bet(Queen, "pb") - (alpha + THIRD)).abs(),
            1.0 - bet(King, "pb"),
            (bet(Jack, "p") - THIRD).abs(),
            bet(Queen, "p"),
            1.0 - bet(King, "p"),
            bet(Jack, "b"),
            (bet(Queen, "b") - THIRD).abs(),
            1.0 - bet(King, "b"),
        ];
        (violations.into_iter().fold(0.0, f64::max), alpha)
    }

    #[test]
    fn the_distance_measures_the_known_family() {
        for alpha in [0.0, 0.1, THIRD] {
            let (distance, read) = distance_from_equilibrium(&equilibrium(alpha));
            assert!(distance < 1e-7, "α = {alpha}: {distance}");
            assert!((read - alpha).abs() < 1e-7);
        }
        // Player 1 calling with the queen half the time is 1/6 away from the family.
        let mut off = equilibrium(0.2);
        off.insert(info_set(Queen, "b"), vec![0.5, 0.5]);
        let (distance, _) = distance_from_equilibrium(&off);
        assert!((distance - (0.5 - THIRD)).abs() < 1e-7, "{distance}");
        // Uniform play bets the king half the time, a full 1 below 3α = 3/2.
        let uniform = Cfr::new(Kuhn, Schedule::Vanilla).average_strategy();
        assert_eq!(distance_from_equilibrium(&uniform), (1.0, 0.5));
    }

    /// Runs `iterations` iterations and checks that the average strategy is within
    /// `distance` of the equilibrium family and its value within `value` of -1/18.
    fn check_convergence(schedule: Schedule, iterations: u64, distance: f64, value: f64) {
        let mut cfr = Cfr::new(Kuhn, schedule);
        cfr.run(iterations, |_| {});
        let average = cfr.average_strategy();
        let (measured, alpha) = distance_from_equilibrium(&average);
        assert!(
            measured <= distance,
            "{schedule:?}: distance {measured:e} from the family at α = {alpha}"
        );
        let values = expected_values(&Kuhn, &average);
        let error = (values[0] - VALUE).abs();
        assert!(
            error <= value,
            "{schedule:?}: value {} is off by {error:e}",
            values[0]
        );
    }

    // Measured after 3,000 iterations. Each tolerance is about twice what was reached, except
    // that value tolerances stay at 1e-7, about the rounding of `f32` strategies:
    //
    // | Schedule        | α read | distance | value error |
    // |-----------------|--------|----------|-------------|
    // | vanilla CFR     | 0.199  | 2.5e-3   | 2.5e-5      |
    // | CFR+            | 0.223  | 9.7e-4   | 2.0e-8      |
    // | linear CFR      | 0.233  | 3.4e-4   | 9.9e-10     |
    // | DCFR(1.5, 0, 2) | 0.189  | 3.0e-4   | 1.8e-8      |
    //
    // Vanilla CFR's distance is its average still betting the queen on about 7.5 / T of
    // hands, left over from the uniform early iterations; it was 7.5e-4 after 10,000.

    #[test]
    fn vanilla_cfr_converges_to_the_equilibrium_family() {
        check_convergence(Schedule::Vanilla, 3_000, 5e-3, 5e-5);
    }

    #[test]
    fn cfr_plus_converges_to_the_equilibrium_family() {
        check_convergence(Schedule::CfrPlus, 3_000, 2e-3, 1e-7);
    }

    #[test]
    fn linear_cfr_converges_to_the_equilibrium_family() {
        check_convergence(Schedule::Linear, 3_000, 1e-3, 1e-7);
    }

    #[test]
    fn discounted_cfr_converges_to_the_equilibrium_family() {
        check_convergence(Schedule::Discounted(Dcfr::default()), 3_000, 1e-3, 1e-7);
    }
}
