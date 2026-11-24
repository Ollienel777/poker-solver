//! Leduc hold'em (Southey et al. 2005) on the [`Game`] trait, with its six cards kept distinct.
//!
//! # Rules
//!
//! The deck has six cards, a jack, a queen and a king in each of two suits. Ranks order jack,
//! queen, king; suits do not rank. Each player antes 1 and is dealt one private card. A first
//! betting round follows, then one public card is dealt from the four cards left, then a
//! second betting round, then the showdown.
//!
//! In each round player 0 acts first. A player who faces no bet checks or bets, and a player
//! who faces a bet folds, calls or raises, with at most one bet and one raise per round: the
//! player facing the raise folds or calls. Bets and raises are 2 chips in the first round and
//! 4 in the second. A round ends when a check is checked behind or a bet is called, and a fold
//! ends the game, the folder losing everything it has put in. At the showdown a private card
//! that pairs the public card wins, otherwise the higher rank wins, and equal ranks split the
//! pot. Utilities are chips won or lost, so the game is zero-sum.
//!
//! # Cards, actions and histories
//!
//! A [`LeducCard`] has the index `2 × rank + suit`, with rank 0 the jack and suits 0 and 1, so
//! the cards in index order are `J0 J1 Q0 Q1 K0 K1`. A player's information set knows its exact
//! card and the exact public card, not just their ranks.
//!
//! The actions at a node are [`LeducInfoSet::actions`], addressed by position. A
//! [`LeducAction::Call`] is a check when there is nothing to call, and a
//! [`LeducAction::Raise`] is a bet when nothing has been bet. A round's betting is written as
//! a string with one letter per action, `c` for a check or call, `r` for a bet or raise and `f`
//! for a fold. The six strings at which a player acts are
//!
//! | betting | acting player | actions |
//! |---------|---------------|---------|
//! | `""`    | 0             | check, bet |
//! | `"c"`   | 1             | check, bet |
//! | `"r"`   | 1             | fold, call, raise |
//! | `"cr"`  | 0             | fold, call, raise |
//! | `"rr"`  | 0             | fold, call |
//! | `"crr"` | 1             | fold, call |
//!
//! `"cc"`, `"rc"`, `"crc"`, `"rrc"` and `"crrc"` close the round, and `"rf"`, `"crf"`, `"rrf"`
//! and `"crrf"` end the game.
//!
//! # Chance
//!
//! The root deals the 30 ordered pairs of distinct cards (player 0's, player 1's), each with
//! probability 1/30, in index order: outcome `o` gives player 0 the card of index `o / 5`, and
//! player 1 the card at position `o % 5`, counting from 0, among the five others in index
//! order. When the first round closes, a chance node deals the public card from the four cards
//! left, each with probability 1/4: outcome `k` is the card at position `k` among them in index
//! order. So every assignment of three distinct cards to (player 0, player 1, public) has
//! probability 1/120, and every history of an information set has the same chance
//! probability, 1/30 in the first round and 1/120 in the second. A vector form may therefore
//! leave chance out of its reaches (see [`crate::tabular`]), as long as it removes the cards
//! the opponent cannot hold.
//!
//! # Information sets
//!
//! A [`LeducInfoSet`] is the public card (none in the first round), the betting of each round
//! and the acting player's card. There are 6 cards × 6 betting strings = 36 in the first round
//! and 5 closed first rounds × 6 betting strings × 30 (card, public card) pairs = 900 in the
//! second, 936 in all. Keying on ranks alone would give 288.
//!
//! Player 0's game value is about −0.0856 chips per hand. The figure usually quoted is
//! −0.085606. After 5,000 iterations of DCFR, the best responses here bracket it in
//! `[−0.0856107, −0.0855928]` (the [`best_response`](mod@crate::best_response) module says
//! why they bracket it).
//!
//! # References
//!
//! - Southey, Bowling, Larson, Piccione, Burch, Billings and Rayner, *Bayes' Bluff: Opponent
//!   Modelling in Poker*, UAI (2005): the rules.

use std::cmp::Ordering;
use std::fmt;

use crate::tabular::{Game, Node};

/// The rank of a Leduc card.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LeducRank {
    /// The lowest rank.
    Jack,
    /// The middle rank.
    Queen,
    /// The highest rank.
    King,
}

impl LeducRank {
    /// The three ranks, lowest first.
    pub const ALL: [LeducRank; 3] = [LeducRank::Jack, LeducRank::Queen, LeducRank::King];
}

/// One of Leduc's six cards: a rank and a suit, 0 or 1. Cards order by their index,
/// `2 × rank + suit`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LeducCard(u8);

impl LeducCard {
    /// The six cards in index order.
    pub const ALL: [LeducCard; 6] = [
        LeducCard(0),
        LeducCard(1),
        LeducCard(2),
        LeducCard(3),
        LeducCard(4),
        LeducCard(5),
    ];

    /// The card of `rank` in suit `suit`. Panics unless `suit` is 0 or 1.
    pub fn new(rank: LeducRank, suit: u8) -> LeducCard {
        assert!(suit < 2, "Leduc has suits 0 and 1, not {suit}");
        LeducCard(2 * rank as u8 + suit)
    }

    /// The card with index `index`, below 6.
    pub fn from_index(index: usize) -> LeducCard {
        LeducCard::ALL[index]
    }

    /// The card's index, `2 × rank + suit`.
    pub fn index(self) -> usize {
        usize::from(self.0)
    }

    /// The card's rank.
    pub fn rank(self) -> LeducRank {
        LeducRank::ALL[usize::from(self.0 / 2)]
    }

    /// The card's suit, 0 or 1.
    pub fn suit(self) -> u8 {
        self.0 % 2
    }
}

impl fmt::Debug for LeducCard {
    /// The rank's initial and the suit, as in `Q1`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let rank = ['J', 'Q', 'K'][usize::from(self.0 / 2)];
        write!(f, "{rank}{}", self.suit())
    }
}

/// A betting action. Which ones a node offers, and in what order, is
/// [`LeducInfoSet::actions`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LeducAction {
    /// Give up the pot. Offered only to a player facing a bet.
    Fold,
    /// Check when there is nothing to call, or call the bet faced.
    Call,
    /// Bet when nothing has been bet this round, or raise the bet faced.
    Raise,
}

/// Leduc hold'em, two players with player 0 acting first in both rounds.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Leduc;

impl Leduc {
    /// Each player's ante, in chips.
    pub const ANTE: u32 = 1;
    /// The size of a bet or raise in the first and the second round, in chips.
    pub const BET: [u32; 2] = [2, 4];
}

/// A Leduc history: the cards dealt so far and each round's betting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LeducState {
    cards: Option<[LeducCard; 2]>,
    public: Option<LeducCard>,
    betting: [&'static str; 2],
}

impl LeducState {
    /// Player 0's and player 1's private cards, once dealt.
    pub fn cards(&self) -> Option<[LeducCard; 2]> {
        self.cards
    }

    /// The public card, once dealt.
    pub fn public(&self) -> Option<LeducCard> {
        self.public
    }

    /// The betting of the first and second rounds, written as in the
    /// [module documentation](self). The second is empty until it starts.
    pub fn betting(&self) -> [&'static str; 2] {
        self.betting
    }
}

/// A Leduc information set: what the acting player knows. Keys order by public card (none
/// first), then betting, then card, so the keys of one public node are adjacent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LeducInfoSet {
    /// The public card, or `None` in the first round.
    pub public: Option<LeducCard>,
    /// The betting of the first and second rounds, written as in the
    /// [module documentation](self). The second is empty in the first round.
    pub betting: [&'static str; 2],
    /// The acting player's card.
    pub card: LeducCard,
}

impl LeducInfoSet {
    /// The round being played, 0 or 1.
    pub fn round(&self) -> usize {
        usize::from(self.public.is_some())
    }

    /// The acting player, which the betting determines.
    pub fn player(&self) -> usize {
        self.betting[self.round()].len() % 2
    }

    /// The actions on offer, in the order the [`Game`] numbers them.
    pub fn actions(&self) -> &'static [LeducAction] {
        actions(self.betting[self.round()])
    }
}

/// The actions at a round's betting string where a player acts.
fn actions(betting: &str) -> &'static [LeducAction] {
    use LeducAction::{Call, Fold, Raise};
    match betting {
        "" | "c" => &[Call, Raise],
        "r" | "cr" => &[Fold, Call, Raise],
        "rr" | "crr" => &[Fold, Call],
        _ => panic!("no player acts after {betting:?}"),
    }
}

/// The round's betting string after `action`.
fn after(betting: &str, action: LeducAction) -> &'static str {
    use LeducAction::{Call, Fold, Raise};
    match (betting, action) {
        ("", Call) => "c",
        ("", Raise) => "r",
        ("c", Call) => "cc",
        ("c", Raise) => "cr",
        ("r", Fold) => "rf",
        ("r", Call) => "rc",
        ("r", Raise) => "rr",
        ("cr", Fold) => "crf",
        ("cr", Call) => "crc",
        ("cr", Raise) => "crr",
        ("rr", Fold) => "rrf",
        ("rr", Call) => "rrc",
        ("crr", Fold) => "crrf",
        ("crr", Call) => "crrc",
        _ => panic!("{action:?} is not an action after {betting:?}"),
    }
}

/// Where a round's betting stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Round {
    /// The player acts next.
    Acting(usize),
    /// The player folded.
    Folded(usize),
    /// The round is over, and nobody folded.
    Closed,
}

fn round_status(betting: &str) -> Round {
    match betting {
        "" | "c" | "r" | "cr" | "rr" | "crr" => Round::Acting(betting.len() % 2),
        "rf" | "crf" | "rrf" | "crrf" => Round::Folded((betting.len() - 1) % 2),
        "cc" | "rc" | "crc" | "rrc" | "crrc" => Round::Closed,
        _ => panic!("{betting:?} is not a Leduc betting string"),
    }
}

/// What each player put in during one round's betting, with bets of `bet`.
fn round_contributions(betting: &str, bet: u32) -> [u32; 2] {
    let mut put_in = [0, 0];
    for (position, action) in betting.chars().enumerate() {
        let player = position % 2;
        match action {
            'c' => put_in[player] = put_in[1 - player],
            'r' => put_in[player] = put_in[1 - player] + bet,
            _ => {}
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

impl Game for Leduc {
    type State = LeducState;
    type InfoSet = LeducInfoSet;

    fn num_players(&self) -> usize {
        2
    }

    fn root(&self) -> LeducState {
        LeducState {
            cards: None,
            public: None,
            betting: ["", ""],
        }
    }

    fn node(&self, state: &LeducState) -> Node<LeducInfoSet> {
        let Some(cards) = state.cards else {
            return Node::Chance {
                probabilities: vec![1.0 / 30.0; 30],
            };
        };
        let round = usize::from(state.public.is_some());
        let betting = state.betting[round];
        match round_status(betting) {
            Round::Acting(player) => Node::Player {
                player,
                info_set: LeducInfoSet {
                    public: state.public,
                    betting: state.betting,
                    card: cards[player],
                },
                num_actions: actions(betting).len(),
            },
            Round::Closed if round == 0 => Node::Chance {
                probabilities: vec![0.25; 4],
            },
            Round::Folded(folder) => {
                // The folder loses what it put in.
                let lost = f64::from(contributions(state.betting)[folder]);
                let payoff = if folder == 0 { -lost } else { lost };
                Node::Terminal {
                    utilities: vec![payoff, -payoff],
                }
            }
            Round::Closed => {
                let put_in = contributions(state.betting);
                debug_assert_eq!(put_in[0], put_in[1], "a showdown with unequal stakes");
                let public = state.public.expect("the showdown has a public card");
                let stake = f64::from(put_in[0]);
                let payoff = match strength(cards[0], public).cmp(&strength(cards[1], public)) {
                    Ordering::Greater => stake,
                    Ordering::Less => -stake,
                    Ordering::Equal => 0.0,
                };
                Node::Terminal {
                    utilities: vec![payoff, -payoff],
                }
            }
        }
    }

    fn next(&self, state: &LeducState, index: usize) -> LeducState {
        let Some(cards) = state.cards else {
            assert!(index < 30, "no deal {index}");
            let first = index / 5;
            let second = index % 5 + usize::from(index % 5 >= first);
            return LeducState {
                cards: Some([LeducCard::from_index(first), LeducCard::from_index(second)]),
                ..*state
            };
        };
        let round = usize::from(state.public.is_some());
        let betting = state.betting[round];
        if round == 0 && round_status(betting) == Round::Closed {
            let public = LeducCard::ALL
                .into_iter()
                .filter(|card| !cards.contains(card))
                .nth(index)
                .unwrap_or_else(|| panic!("no public card {index}"));
            return LeducState {
                public: Some(public),
                ..*state
            };
        }
        let action = *actions(betting)
            .get(index)
            .unwrap_or_else(|| panic!("no action {index} after {betting:?}"));
        let mut next = *state;
        next.betting[round] = after(betting, action);
        next
    }
}

/// What each player has put in, ante included, after the betting `betting`.
fn contributions(betting: [&str; 2]) -> [u32; 2] {
    let first = round_contributions(betting[0], Leduc::BET[0]);
    let second = round_contributions(betting[1], Leduc::BET[1]);
    [0, 1].map(|player| Leduc::ANTE + first[player] + second[player])
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::time::Instant;

    use rand_chacha::ChaCha8Rng;
    use rand_chacha::rand_core::{Rng, SeedableRng};

    use LeducAction::{Call, Fold, Raise};
    use LeducRank::{Jack, King, Queen};

    use super::*;
    use crate::best_response::{best_response, exploitability};
    use crate::regret::{Dcfr, Schedule};
    use crate::tabular::{Cfr, Profile, expected_values};

    /// The literature's value for player 0 (see the module documentation), to six decimals.
    const VALUE: f64 = -0.085606;

    fn card(rank: LeducRank, suit: u8) -> LeducCard {
        LeducCard::new(rank, suit)
    }

    /// The state after dealing `cards`, playing `first`, dealing `public` and playing `second`,
    /// each betting string written as in the module documentation.
    fn play(
        cards: [LeducCard; 2],
        first: &str,
        public: Option<LeducCard>,
        second: &str,
    ) -> LeducState {
        let deal = (0..30)
            .find(|&o| Leduc.next(&Leduc.root(), o).cards == Some(cards))
            .unwrap();
        let mut state = Leduc.next(&Leduc.root(), deal);
        let bet = |state: &mut LeducState, letters: &str| {
            for letter in letters.chars() {
                let action = match letter {
                    'f' => Fold,
                    'c' => Call,
                    _ => Raise,
                };
                let round = usize::from(state.public.is_some());
                let index = actions(state.betting[round])
                    .iter()
                    .position(|&a| a == action)
                    .unwrap();
                *state = Leduc.next(state, index);
            }
        };
        bet(&mut state, first);
        if let Some(public) = public {
            let index = (0..4)
                .find(|&k| Leduc.next(&state, k).public == Some(public))
                .unwrap();
            state = Leduc.next(&state, index);
            bet(&mut state, second);
        }
        state
    }

    fn utilities(state: &LeducState) -> Vec<f64> {
        match Leduc.node(state) {
            Node::Terminal { utilities } => utilities,
            node => panic!("{state:?} is not terminal: {node:?}"),
        }
    }

    #[test]
    fn cards_index_by_rank_then_suit() {
        let names: Vec<String> = LeducCard::ALL.iter().map(|c| format!("{c:?}")).collect();
        assert_eq!(names, ["J0", "J1", "Q0", "Q1", "K0", "K1"]);
        for (index, card) in LeducCard::ALL.into_iter().enumerate() {
            assert_eq!(card.index(), index);
            assert_eq!(LeducCard::from_index(index), card);
            assert_eq!(LeducCard::new(card.rank(), card.suit()), card);
        }
        assert_eq!(card(Queen, 1).index(), 3);
        assert_eq!(card(King, 0).rank(), King);
    }

    #[test]
    fn chance_deals_every_ordered_pair_then_a_remaining_public_card() {
        let Node::Chance { probabilities } = Leduc.node(&Leduc.root()) else {
            panic!("the root is not a chance node");
        };
        assert_eq!(probabilities, [1.0 / 30.0; 30]);
        let deals: Vec<[LeducCard; 2]> = (0..30)
            .map(|o| Leduc.next(&Leduc.root(), o).cards.unwrap())
            .collect();
        let mut sorted = deals.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted, deals, "deals come in index order, once each");
        assert!(deals.iter().all(|[a, b]| a != b));
        assert_eq!(deals[0], [card(Jack, 0), card(Jack, 1)]);
        assert_eq!(deals[29], [card(King, 1), card(King, 0)]);
        for cards in deals {
            for first in ["cc", "rc", "crc", "rrc", "crrc"] {
                let state = play(cards, first, None, "");
                let Node::Chance { probabilities } = Leduc.node(&state) else {
                    panic!("{state:?} does not deal the public card");
                };
                assert_eq!(probabilities, [0.25; 4]);
                let publics: Vec<LeducCard> = (0..4)
                    .map(|k| Leduc.next(&state, k).public.unwrap())
                    .collect();
                let rest: Vec<LeducCard> = LeducCard::ALL
                    .into_iter()
                    .filter(|c| !cards.contains(c))
                    .collect();
                assert_eq!(publics, rest);
            }
        }
    }

    #[test]
    fn each_round_has_six_decision_points_four_folds_and_five_closings() {
        let mut decisions = BTreeSet::new();
        let mut folds = BTreeSet::new();
        let mut closings = BTreeSet::new();
        let mut stack = vec![""];
        while let Some(betting) = stack.pop() {
            match round_status(betting) {
                Round::Acting(_) => {
                    decisions.insert(betting);
                    stack.extend(actions(betting).iter().map(|&a| after(betting, a)));
                }
                Round::Folded(_) => {
                    folds.insert(betting);
                }
                Round::Closed => {
                    closings.insert(betting);
                }
            }
        }
        let decisions: Vec<&str> = decisions.into_iter().collect();
        assert_eq!(decisions, ["", "c", "cr", "crr", "r", "rr"]);
        assert_eq!(folds.len(), 4);
        assert_eq!(closings.len(), 5);
        let expected: [(&str, usize, &[LeducAction]); 6] = [
            ("", 0, &[Call, Raise]),
            ("c", 1, &[Call, Raise]),
            ("r", 1, &[Fold, Call, Raise]),
            ("cr", 0, &[Fold, Call, Raise]),
            ("rr", 0, &[Fold, Call]),
            ("crr", 1, &[Fold, Call]),
        ];
        for (betting, player, offered) in expected {
            assert_eq!(round_status(betting), Round::Acting(player), "{betting:?}");
            assert_eq!(actions(betting), offered, "{betting:?}");
        }
    }

    #[test]
    fn a_player_sees_its_own_card_the_public_card_and_the_betting() {
        let (q0, k1) = (card(Queen, 0), card(King, 1));
        for other in [card(Jack, 0), card(King, 0), card(Queen, 1)] {
            let node = Leduc.node(&play([q0, other], "", None, ""));
            let expected = Node::Player {
                player: 0,
                info_set: LeducInfoSet {
                    public: None,
                    betting: ["", ""],
                    card: q0,
                },
                num_actions: 2,
            };
            assert_eq!(node, expected, "against {other:?}");
            // In the second round player 1, holding the king, faces a bet.
            let node = Leduc.node(&play([other, k1], "rc", Some(q0), "r"));
            let expected = Node::Player {
                player: 1,
                info_set: LeducInfoSet {
                    public: Some(q0),
                    betting: ["rc", "r"],
                    card: k1,
                },
                num_actions: 3,
            };
            assert_eq!(node, expected, "against {other:?}");
        }
        let key = LeducInfoSet {
            public: Some(q0),
            betting: ["crc", "crr"],
            card: k1,
        };
        assert_eq!((key.round(), key.player()), (1, 1));
        assert_eq!(key.actions(), [Fold, Call]);
    }

    #[test]
    fn there_are_936_information_sets() {
        let profile = Cfr::new(Leduc, Schedule::Vanilla).average_strategy();
        assert_eq!(profile.len(), 936);
        let first = profile.iter().filter(|(k, _)| k.public.is_none()).count();
        assert_eq!((first, profile.len() - first), (36, 900));
        // The private card never equals the public card, and every pair of them appears.
        let pairs: BTreeSet<(LeducCard, LeducCard)> = profile
            .iter()
            .filter_map(|(k, _)| k.public.map(|public| (k.card, public)))
            .collect();
        assert_eq!(pairs.len(), 30);
        assert!(pairs.iter().all(|(card, public)| card != public));
        for (key, strategy) in profile.iter() {
            assert_eq!(strategy.len(), key.actions().len(), "{key:?}");
        }
        // Per deal, 6 player states and 4 folds in the first round, 5 chance states for the
        // public card, and below each of their 4 cards 6 player states, 4 folds and 5
        // showdowns.
        assert_eq!(
            count_states(&Leduc.root()),
            [30 * 126, 1 + 30 * 5, 30 * 184]
        );
    }

    /// The numbers of player, chance and terminal states from `state` down.
    fn count_states(state: &LeducState) -> [usize; 3] {
        let (kind, children) = match Leduc.node(state) {
            Node::Player { num_actions, .. } => (0, num_actions),
            Node::Chance { probabilities } => (1, probabilities.len()),
            Node::Terminal { .. } => (2, 0),
        };
        let mut counts = [0; 3];
        counts[kind] = 1;
        for index in 0..children {
            let below = count_states(&Leduc.next(state, index));
            for (count, more) in counts.iter_mut().zip(below) {
                *count += more;
            }
        }
        counts
    }

    #[test]
    fn payoffs_follow_the_rules() {
        let (j0, j1, q0, q1, k0, k1) = (
            card(Jack, 0),
            card(Jack, 1),
            card(Queen, 0),
            card(Queen, 1),
            card(King, 0),
            card(King, 1),
        );
        // Player 0's payoff. A fold loses the ante and whatever the folder bet; a showdown
        // wins what the loser put in.
        let cases = [
            // Folds in the first round: a bet of 2, then a raise to 4.
            ([k0, j0], "rf", None, "", 1.0),
            ([k0, j0], "crf", None, "", -1.0),
            ([k0, j0], "rrf", None, "", -3.0),
            ([j0, k0], "crrf", None, "", 3.0),
            // Folds in the second round, after 3 each in the first: a bet of 4, a raise to 8.
            ([j0, k0], "rc", Some(q0), "rf", 3.0),
            ([k0, j0], "crc", Some(q0), "crf", -3.0),
            ([k0, j0], "rc", Some(q0), "crrf", 7.0),
            ([k0, j0], "rc", Some(q0), "rrf", -7.0),
            // The higher card wins when nobody pairs.
            ([k0, j0], "cc", Some(q0), "cc", 1.0),
            ([q1, k1], "rc", Some(j0), "cc", -3.0),
            ([k0, q0], "crc", Some(j1), "rc", 7.0),
            ([k0, j0], "rrc", Some(q0), "rrc", 13.0),
            // A pair beats a higher card.
            ([k0, j0], "cc", Some(j1), "cc", -1.0),
            ([j1, k0], "crrc", Some(j0), "crrc", 13.0),
            ([k0, q0], "rc", Some(q1), "rc", -7.0),
            // Equal ranks split the pot.
            ([k0, k1], "rrc", Some(q0), "rrc", 0.0),
            ([q0, q1], "cc", Some(j0), "rc", 0.0),
        ];
        for (cards, first, public, second, payoff) in cases {
            let state = play(cards, first, public, second);
            assert_eq!(
                utilities(&state),
                [payoff, -payoff],
                "{cards:?} {first:?} {public:?} {second:?}"
            );
        }
    }

    #[test]
    fn checking_down_is_worth_two_fifths_with_a_king() {
        // With no betting the pot is the antes. Holding a king, player 0 ties the other king
        // (1/5), and against a queen or a jack (4/5) it loses only when the public card pairs
        // that card (1/4), for 4/5 · (3/4 - 1/4) = 2/5. A jack is the mirror, -2/5, and a
        // queen wins as often as it loses.
        let mut by_rank = [0.0; 3];
        for deal in 0..30 {
            let dealt = Leduc.next(&Leduc.root(), deal);
            let closed = play(dealt.cards.unwrap(), "cc", None, "");
            for k in 0..4 {
                let public = Leduc.next(&closed, k).public;
                let state = play(dealt.cards.unwrap(), "cc", public, "cc");
                let rank = dealt.cards.unwrap()[0].rank() as usize;
                // Each rank has 10 of the 30 deals.
                by_rank[rank] += utilities(&state)[0] / 10.0 / 4.0;
            }
        }
        let expected = [-0.4, 0.0, 0.4];
        for (got, expected) in by_rank.iter().zip(expected) {
            assert!((got - expected).abs() < 1e-12, "{by_rank:?}");
        }
    }

    #[test]
    fn simple_strategies_have_the_values_they_should() {
        // Every information set plays its first action: player 0 checks, player 1 checks, and
        // a player facing a bet folds, which never happens. That checks down, worth 0.
        let shapes = Cfr::new(Leduc, Schedule::Vanilla).average_strategy();
        let pure = |choose: &dyn Fn(&LeducInfoSet) -> LeducAction| -> Profile<LeducInfoSet> {
            shapes
                .iter()
                .map(|(key, _)| {
                    let chosen = choose(key);
                    let row = key.actions().iter().map(|&a| f32::from(a == chosen));
                    (*key, row.collect())
                })
                .collect()
        };
        let close = |values: Vec<f64>, expected: f64| {
            assert!((values[0] - expected).abs() < 1e-12, "{values:?}");
            assert_eq!(values[1], -values[0]);
        };
        close(expected_values(&Leduc, &pure(&|key| key.actions()[0])), 0.0);
        // Player 0 always bets and player 1 folds to it: player 0 wins player 1's ante.
        let bluff = pure(&|key| match key.player() {
            0 => Raise,
            _ => key.actions()[0],
        });
        close(expected_values(&Leduc, &bluff), 1.0);
        // Both always bet or raise, then call the raise: each round goes "rrc", so every
        // showdown is for 13, and the deal is symmetric, so the value is 0.
        let maniac = pure(&|key| {
            if key.actions().contains(&Raise) {
                Raise
            } else {
                Call
            }
        });
        close(expected_values(&Leduc, &maniac), 0.0);
    }

    /// A random profile over every information set, with about one probability in five
    /// exactly zero.
    fn random_profile(rng: &mut ChaCha8Rng) -> Profile<LeducInfoSet> {
        let shapes = Cfr::new(Leduc, Schedule::Vanilla).average_strategy();
        shapes
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

    #[test]
    fn best_responses_cannot_be_improved_at_any_one_information_set() {
        // Leduc has far too many pure strategies for a brute force, so this checks the best
        // response against a random profile in two other ways: playing its strategy is worth
        // its value, and changing its action at any one information set, which
        // expected_values evaluates independently, gains nothing.
        let mut rng = ChaCha8Rng::seed_from_u64(19);
        let profile = random_profile(&mut rng);
        for player in 0..2 {
            let response = best_response(&Leduc, &profile, player);
            assert_eq!(response.strategy.len(), 468);
            let mut played = profile.clone();
            for (key, strategy) in response.strategy.iter() {
                assert_eq!(key.player(), player);
                played.insert(*key, strategy.to_vec());
            }
            let value = expected_values(&Leduc, &played)[player];
            assert!(
                (value - response.value).abs() < 1e-12,
                "{value} against {}",
                response.value
            );
            assert!(value > expected_values(&Leduc, &profile)[player]);
            for (key, strategy) in response.strategy.iter() {
                for action in (0..strategy.len()).filter(|&a| strategy[a] == 0.0) {
                    let mut deviation = played.clone();
                    let mut pure = vec![0.0; strategy.len()];
                    pure[action] = 1.0;
                    deviation.insert(*key, pure);
                    let deviated = expected_values(&Leduc, &deviation)[player];
                    assert!(
                        deviated <= response.value + 1e-12,
                        "{key:?}: action {action} gains {}",
                        deviated - response.value
                    );
                }
            }
        }
    }

    /// `key` with the suits of its cards swapped.
    fn swap_suits(key: &LeducInfoSet) -> LeducInfoSet {
        let swap = |card: LeducCard| LeducCard::new(card.rank(), 1 - card.suit());
        LeducInfoSet {
            public: key.public.map(swap),
            betting: key.betting,
            card: swap(key.card),
        }
    }

    #[test]
    fn strategies_do_not_depend_on_suits() {
        // Swapping the suits maps the game onto itself, so CFR's strategies are the same for
        // an information set and its suit-swapped twin, up to the order in which the walk
        // sums the histories. A showdown that ranked suits would break this. Measured after 100
        // iterations of CFR+, the twins' strategies were identical, bit for bit.
        let average = solve(Schedule::CfrPlus, 100);
        let mut largest = 0.0f32;
        for (key, strategy) in average.iter() {
            let twin = &average[&swap_suits(key)];
            for (a, b) in strategy.iter().zip(twin) {
                largest = largest.max((a - b).abs());
            }
        }
        assert!(largest <= 1e-6, "{largest}");
    }

    /// The average strategy after `iterations` iterations of `schedule`.
    fn solve(schedule: Schedule, iterations: u64) -> Profile<LeducInfoSet> {
        let mut cfr = Cfr::new(Leduc, schedule);
        cfr.run(iterations, |_| {});
        cfr.average_strategy()
    }

    /// `[−BR_1, BR_0]` for `profile`, which must hold the game value (see the
    /// [`best_response`](mod@crate::best_response) module). Checks that it holds the
    /// literature's value, give or take that figure's rounding to six decimals.
    fn value_bracket(profile: &Profile<LeducInfoSet>) -> (f64, f64) {
        let low = -best_response(&Leduc, profile, 1).value;
        let high = best_response(&Leduc, profile, 0).value;
        assert!(
            low - 5e-7 <= VALUE && VALUE <= high + 5e-7,
            "[{low}, {high}] does not hold {VALUE}"
        );
        (low, high)
    }

    /// Checks that `schedule`'s average strategy after `iterations` iterations is less
    /// exploitable than `threshold`, and returns it.
    fn check_threshold(
        schedule: Schedule,
        iterations: u64,
        threshold: f64,
    ) -> Profile<LeducInfoSet> {
        let average = solve(schedule, iterations);
        let measured = exploitability(&Leduc, &average);
        assert!(
            measured < threshold,
            "{schedule:?}: {measured:e} after {iterations} iterations is not below {threshold:e}"
        );
        average
    }

    // Regression thresholds for the fast suite, after 500 iterations. The exploitability
    // measured in the first good run, in chips:
    //
    // | Schedule        | measured   | threshold |
    // |-----------------|------------|-----------|
    // | CFR             | 2.1459e-2  | 2.7e-2    |
    // | CFR+            | 9.3555e-4  | 1.2e-3    |
    // | linear CFR      | 8.8507e-3  | 1.1e-2    |
    // | DCFR(1.5, 0, 2) | 4.7320e-4  | 6e-4      |
    //
    // Each threshold is about 1.25 times its measurement. The full curves, and thresholds at
    // 5,000 iterations, are in `convergence_table`.

    #[test]
    fn vanilla_cfr_is_below_its_threshold_after_500_iterations() {
        check_threshold(Schedule::Vanilla, 500, 2.7e-2);
    }

    #[test]
    fn cfr_plus_is_below_its_threshold_after_500_iterations() {
        let average = check_threshold(Schedule::CfrPlus, 500, 1.2e-3);
        value_bracket(&average);
    }

    #[test]
    fn linear_cfr_is_below_its_threshold_after_500_iterations() {
        check_threshold(Schedule::Linear, 500, 1.1e-2);
    }

    #[test]
    fn dcfr_is_below_its_threshold_after_500_iterations() {
        let average = check_threshold(Schedule::Discounted(Dcfr::default()), 500, 6e-4);
        value_bracket(&average);
    }

    /// The schedules of the convergence table, with their names and their thresholds after
    /// 5,000 iterations.
    fn table_schedules() -> [(&'static str, Schedule, f64); 4] {
        [
            ("CFR", Schedule::Vanilla, 4e-3),
            ("CFR+", Schedule::CfrPlus, 2.5e-5),
            ("linear CFR", Schedule::Linear, 2.7e-3),
            (
                "DCFR(1.5, 0, 2)",
                Schedule::Discounted(Dcfr::default()),
                1.2e-5,
            ),
        ]
    }

    /// The iteration counts of the convergence table.
    const LADDER: [u64; 13] = [1, 2, 5, 10, 20, 50, 100, 200, 500, 1000, 2000, 5000, 10000];

    /// One schedule's column of the convergence table: at each count of [`LADDER`], the
    /// average strategy's exploitability and player 0's value, with the profile after 5,000
    /// iterations and the milliseconds per iteration.
    struct Column {
        points: Vec<(f64, f64)>,
        at_5000: Profile<LeducInfoSet>,
        milliseconds: f64,
    }

    fn column(schedule: Schedule) -> Column {
        let mut cfr = Cfr::new(Leduc, schedule);
        let mut points = Vec::new();
        let mut at_5000 = Profile::new();
        let mut seconds = 0.0;
        for target in LADDER {
            let start = Instant::now();
            cfr.run(target - cfr.iterations(), |_| {});
            seconds += start.elapsed().as_secs_f64();
            let average = cfr.average_strategy();
            let value = expected_values(&Leduc, &average)[0];
            points.push((exploitability(&Leduc, &average), value));
            if target == 5000 {
                at_5000 = average;
            }
        }
        let milliseconds = seconds * 1e3 / LADDER[LADDER.len() - 1] as f64;
        Column {
            points,
            at_5000,
            milliseconds,
        }
    }

    #[test]
    #[ignore = "slow"]
    fn convergence_table() {
        // Runs each schedule to 10,000 iterations on its own thread and prints the table of
        // exploitability, in chips, with --nocapture. Every schedule's exploitability falls
        // at each step of the ladder up to 5,000 iterations, where the thresholds are frozen.
        // As measured in the first good run:
        //
        // | Schedule        | at 5,000   | threshold |
        // |-----------------|------------|-----------|
        // | CFR             | 3.1388e-3  | 4e-3      |
        // | CFR+            | 1.9218e-5  | 2.5e-5    |
        // | linear CFR      | 2.1277e-3  | 2.7e-3    |
        // | DCFR(1.5, 0, 2) | 8.9713e-6  | 1.2e-5    |
        //
        // Past 5,000, CFR+ and DCFR stop improving at about 1e-5 and drift up: DCFR was
        // 1.5e-5 at 10,000, and CFR+ 3.9e-5 at 20,000. An experiment that ran the same
        // iteration in f64 throughout kept CFR+ falling (6.7e-6 at 10,000 and 3.4e-6 at
        // 20,000), so the floor is one of f32 precision, and nothing is frozen beyond 5,000.
        let schedules = table_schedules();
        let columns: Vec<Column> = std::thread::scope(|scope| {
            let handles: Vec<_> = schedules
                .iter()
                .map(|&(_, schedule, _)| scope.spawn(move || column(schedule)))
                .collect();
            handles.into_iter().map(|h| h.join().unwrap()).collect()
        });
        let names: Vec<&str> = schedules.iter().map(|(name, ..)| *name).collect();
        println!("| iterations | {} |", names.join(" | "));
        println!("|---:|{}", "---:|".repeat(names.len()));
        for (row, iterations) in LADDER.iter().enumerate() {
            let cells: Vec<String> = columns
                .iter()
                .map(|c| format!("{:.3e}", c.points[row].0))
                .collect();
            println!("| {iterations} | {} |", cells.join(" | "));
        }
        let five_thousand = LADDER.iter().position(|&t| t == 5000).unwrap();
        for ((name, _, threshold), column) in schedules.iter().zip(&columns) {
            let (exploitability, value) = column.points[five_thousand];
            let (low, high) = value_bracket(&column.at_5000);
            println!(
                "{name}: {:.2} ms per iteration; after 5,000, exploitability \
                 {exploitability:.4e}, player 0's value {value:.7}, game value in \
                 [{low:.7}, {high:.7}]",
                column.milliseconds
            );
            for (row, pair) in column.points[..=five_thousand].windows(2).enumerate() {
                assert!(
                    pair[1].0 < pair[0].0,
                    "{name}: exploitability rose from {} to {} iterations",
                    LADDER[row],
                    LADDER[row + 1]
                );
            }
            assert!(
                exploitability < *threshold,
                "{name}: {exploitability:e} after 5,000 iterations is not below {threshold:e}"
            );
        }
    }
}
