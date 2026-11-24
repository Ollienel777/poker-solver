//! Hand evaluation for five to seven cards.
//!
//! Written from scratch with rank bitmasks. It is not built for speed: the solvers sort each
//! river board's hands once, so evaluation never sits in an inner loop.

use std::fmt;

use crate::card::{CardSet, NUM_RANKS, NUM_SUITS};

/// The nine hand categories, weakest first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Category {
    HighCard,
    Pair,
    TwoPair,
    ThreeOfAKind,
    Straight,
    Flush,
    FullHouse,
    FourOfAKind,
    StraightFlush,
}

impl Category {
    pub const ALL: [Category; 9] = [
        Category::HighCard,
        Category::Pair,
        Category::TwoPair,
        Category::ThreeOfAKind,
        Category::Straight,
        Category::Flush,
        Category::FullHouse,
        Category::FourOfAKind,
        Category::StraightFlush,
    ];
}

impl fmt::Display for Category {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad(match self {
            Category::HighCard => "high card",
            Category::Pair => "one pair",
            Category::TwoPair => "two pair",
            Category::ThreeOfAKind => "three of a kind",
            Category::Straight => "straight",
            Category::Flush => "flush",
            Category::FullHouse => "full house",
            Category::FourOfAKind => "four of a kind",
            Category::StraightFlush => "straight flush",
        })
    }
}

/// The strength of the best five-card hand. A higher value beats a lower one, and equal values
/// tie.
///
/// Bits 20 and up hold the category. Below that are five 4-bit ranks, most significant first:
/// the ranks that define the category, then the kickers.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct HandRank(u32);

impl HandRank {
    const CATEGORY_SHIFT: u32 = 20;

    fn new(category: Category, ranks: u32) -> HandRank {
        HandRank((category as u32) << Self::CATEGORY_SHIFT | ranks)
    }

    pub fn category(self) -> Category {
        Category::ALL[(self.0 >> Self::CATEGORY_SHIFT) as usize]
    }

    pub const fn value(self) -> u32 {
        self.0
    }
}

impl fmt::Debug for HandRank {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let ranks = self.0 & ((1 << Self::CATEGORY_SHIFT) - 1);
        write!(f, "HandRank({}, {ranks:05x})", self.category())
    }
}

/// The highest rank in a non-empty rank mask.
fn highest(mask: u16) -> u32 {
    debug_assert!(mask != 0);
    15 - mask.leading_zeros()
}

/// The `count` highest ranks of a mask, packed four bits each with the highest first.
fn top_ranks(mut mask: u16, count: u32) -> u32 {
    let mut packed = 0;
    for _ in 0..count {
        let rank = highest(mask);
        packed = packed << 4 | rank;
        mask &= !(1 << rank);
    }
    packed
}

/// The top rank of the best straight in a rank mask, if there is one. The wheel (A2345) is
/// five-high, so it returns the rank of the five.
fn straight_high(mask: u16) -> Option<u32> {
    // Shift up one and copy the ace into bit 0, so it can also play low.
    let ranks = mask << 1 | mask >> 12 & 1;
    let runs = ranks & ranks >> 1 & ranks >> 2 & ranks >> 3 & ranks >> 4;
    // Bit `i` of `runs` is a straight whose lowest card sits at shifted position `i`.
    (runs != 0).then(|| highest(runs) + 3)
}

/// Evaluates the best five-card hand from five, six or seven cards.
pub fn evaluate(cards: CardSet) -> HandRank {
    debug_assert!(
        (5..=7).contains(&cards.len()),
        "evaluate needs 5 to 7 cards"
    );

    let mut suit_ranks = [0u16; NUM_SUITS];
    let mut rank_counts = [0u8; NUM_RANKS];
    for card in cards {
        suit_ranks[card.suit() as usize] |= 1 << card.rank();
        rank_counts[card.rank() as usize] += 1;
    }

    // Seven cards holding a flush cannot also hold quads or a full house, so a flush suit
    // settles the hand.
    for &ranks in &suit_ranks {
        if ranks.count_ones() >= 5 {
            return match straight_high(ranks) {
                Some(high) => HandRank::new(Category::StraightFlush, high << 16),
                None => HandRank::new(Category::Flush, top_ranks(ranks, 5)),
            };
        }
    }

    let (mut all, mut pairs, mut trips, mut quads) = (0u16, 0u16, 0u16, 0u16);
    for (rank, &count) in rank_counts.iter().enumerate() {
        let bit = 1 << rank;
        match count {
            0 => continue,
            1 => {}
            2 => pairs |= bit,
            3 => trips |= bit,
            _ => quads |= bit,
        }
        all |= bit;
    }

    if quads != 0 {
        let quad = highest(quads);
        let kicker = highest(all & !(1 << quad));
        return HandRank::new(Category::FourOfAKind, quad << 16 | kicker << 12);
    }

    if trips != 0 {
        let trip = highest(trips);
        // A second set of trips plays as the pair.
        let pair_ranks = (trips & !(1 << trip)) | pairs;
        if pair_ranks != 0 {
            return HandRank::new(Category::FullHouse, trip << 16 | highest(pair_ranks) << 12);
        }
    }

    if let Some(high) = straight_high(all) {
        return HandRank::new(Category::Straight, high << 16);
    }

    if trips != 0 {
        let trip = highest(trips);
        let kickers = top_ranks(all & !(1 << trip), 2);
        return HandRank::new(Category::ThreeOfAKind, trip << 16 | kickers << 8);
    }

    if pairs.count_ones() >= 2 {
        let two = top_ranks(pairs, 2);
        let (first, second) = (two >> 4, two & 0xF);
        // With three pairs, the third pair's rank can be the kicker.
        let kicker = highest(all & !(1 << first) & !(1 << second));
        return HandRank::new(Category::TwoPair, two << 12 | kicker << 8);
    }

    if pairs != 0 {
        let pair = highest(pairs);
        let kickers = top_ranks(all & !(1 << pair), 3);
        return HandRank::new(Category::Pair, pair << 16 | kickers << 4);
    }

    HandRank::new(Category::HighCard, top_ranks(all, 5))
}

#[cfg(test)]
mod tests {
    use std::cmp::Ordering;

    use super::*;

    fn rank(cards: &str) -> HandRank {
        evaluate(cards.parse().unwrap())
    }

    fn category(cards: &str) -> Category {
        rank(cards).category()
    }

    #[test]
    fn each_category_is_recognised() {
        for (cards, expected) in [
            ("As Kd 9h 7c 5s 3d 2h", Category::HighCard),
            ("As Ad 9h 7c 5s 3d 2h", Category::Pair),
            ("As Ad 9h 9c 5s 3d 2h", Category::TwoPair),
            ("As Ad Ah 9c 5s 3d 2h", Category::ThreeOfAKind),
            ("9s 8d 7h 6c 5s Ad Kh", Category::Straight),
            ("As Js 9s 7s 5s 3d 2h", Category::Flush),
            ("As Ad Ah 9c 9s 3d 2h", Category::FullHouse),
            ("As Ad Ah Ac 9s 3d 2h", Category::FourOfAKind),
            ("9s 8s 7s 6s 5s Ad Kh", Category::StraightFlush),
        ] {
            assert_eq!(category(cards), expected, "{cards}");
        }
    }

    #[test]
    fn categories_rank_in_order() {
        let hands = [
            "As Kd 9h 7c 5s 3d 2h",
            "2s 2d 9h 7c 5s 4d 3h",
            "3s 3d 2h 2c 9s 7d 5h",
            "2s 2d 2h 9c 7s 5d 4h",
            "5s 4d 3h 2c As Kd Qh",
            "7s 5s 4s 3s 2s Kd Qh",
            "2s 2d 2h 3c 3s Kd Qh",
            "2s 2d 2h 2c 3s Kd Qh",
            "5s 4s 3s 2s As Kd Qh",
        ];
        for pair in hands.windows(2) {
            assert!(
                rank(pair[0]) < rank(pair[1]),
                "{} should lose to {}",
                pair[0],
                pair[1]
            );
        }
    }

    #[test]
    fn straights_including_the_wheel() {
        assert_eq!(category("As 2d 3h 4c 5s 9d Kh"), Category::Straight);
        assert_eq!(category("As Kd Qh Jc Ts 3d 2h"), Category::Straight);
        // The wheel is the lowest straight.
        assert!(rank("As 2d 3h 4c 5s 9d Kh") < rank("2s 3d 4h 5c 6s 9d Kh"));
        assert!(rank("9s Td Jh Qc Ks 2d 3h") < rank("Ts Jd Qh Kc As 2d 3h"));
        // A six-card run plays its top five.
        assert_eq!(rank("4s 5d 6h 7c 8s 9d Kh"), rank("5s 6d 7h 8c 9s 2d Kh"));
        // Ace and king do not wrap around.
        assert_eq!(category("Qs Kd Ah 2c 3s 8d 9h"), Category::HighCard);
        // Four to a straight is not a straight.
        assert_eq!(category("2s 3d 4h 5c 7s 9d Kh"), Category::HighCard);
    }

    #[test]
    fn straight_flushes_including_the_steel_wheel() {
        assert_eq!(category("As 2s 3s 4s 5s 9d Kh"), Category::StraightFlush);
        assert_eq!(category("As Ks Qs Js Ts 3d 2h"), Category::StraightFlush);
        assert!(rank("As 2s 3s 4s 5s 9d Kh") < rank("2s 3s 4s 5s 6s 9d Kh"));
        // A straight and a flush in different cards is only a flush.
        assert_eq!(category("9s 8s 7s 6s 5d Ks 2h"), Category::Flush);
        // The straight flush wins even when the same suit holds a higher card.
        let steel_nine = rank("As 9s 8s 7s 6s 5s 2h");
        assert_eq!(steel_nine.category(), Category::StraightFlush);
        assert_eq!(steel_nine, rank("9s 8s 7s 6s 5s 2d 2h"));
    }

    #[test]
    fn flushes_use_their_five_highest_cards() {
        assert!(rank("As Js 9s 7s 5s 3d 2h") > rank("Ks Qs Js Ts 8s 3d 2h"));
        assert!(rank("As Js 9s 7s 5s 3d 2h") > rank("As Js 9s 7s 4s 3d 2h"));
        // Sixth and seventh cards of the suit do not count.
        assert_eq!(rank("As Ks Qs Js 9s 3s 2s"), rank("As Ks Qs Js 9s 8d 7d"));
        // A flush beats a straight held in the same seven cards.
        assert_eq!(category("9s 8d 7s 6s 5s Ks 2s"), Category::Flush);
    }

    #[test]
    fn full_houses_and_quads() {
        // Two sets of trips: the higher plays as trips, the lower as the pair.
        assert_eq!(rank("As Ad Ah Kc Ks Kd 2h"), rank("As Ad Ah Kc Ks 3d 2h"));
        assert!(rank("As Ad Ah Kc Ks Kd 2h") > rank("Ks Kd Kh Ac As 3d 2h"));
        // Trips with two pairs uses the higher pair.
        assert_eq!(rank("As Ad Ah Kc Ks Qd Qh"), rank("As Ad Ah Kc Ks 3d 2h"));
        assert!(rank("2s 2d 2h 3c 3s Kd Qh") < rank("3s 3d 3h 2c 2s Kd Qh"));
        // Quads take the best kicker, even from a pair or trips.
        assert!(rank("9s 9d 9h 9c As 3d 2h") > rank("9s 9d 9h 9c Ks Kd Kh"));
        assert_eq!(rank("9s 9d 9h 9c Ks Kd 2h"), rank("9s 9d 9h 9c Kh 3d 2h"));
        assert_eq!(category("9s 9d 9h 9c Ks Kd Kh"), Category::FourOfAKind);
    }

    #[test]
    fn kickers_break_ties() {
        // One pair, three kickers.
        assert!(rank("As Ad Kh 9c 5s 3d 2h") > rank("As Ad Qh Jc Ts 3d 2h"));
        assert!(rank("As Ad Kh 9c 5s 3d 2h") > rank("As Ad Kh 8c 5s 3d 2h"));
        assert!(rank("As Ad Kh 9c 5s 3d 2h") > rank("As Ad Kh 9c 4s 3d 2h"));
        assert_eq!(rank("As Ad Kh 9c 5s 3d 2h"), rank("Ac Ah Ks 9d 5h 4d 2s"));
        // Two pair, one kicker.
        assert!(rank("As Ad 9h 9c Ks 3d 2h") > rank("As Ad 9h 9c Qs 3d 2h"));
        assert!(rank("As Ad 9h 9c 5s 3d 2h") > rank("Ks Kd Qh Qc As 3d 2h"));
        // Three pairs: the best two play and the third can be the kicker.
        assert_eq!(rank("As Ad 9h 9c 5s 5d 2h"), rank("As Ad 9h 9c 5s 3d 2h"));
        assert!(rank("As Ad 9h 9c 5s 5d Kh") > rank("As Ad 9h 9c 5s 5d Qh"));
        // Trips, two kickers.
        assert!(rank("9s 9d 9h Ac Ks 3d 2h") > rank("9s 9d 9h Ac Qs 3d 2h"));
        // High card, five kickers.
        assert!(rank("As Kd 9h 7c 5s 3d 2h") > rank("As Kd 9h 7c 4s 3d 2h"));
        assert_eq!(rank("As Kd 9h 7c 5s 3d 2h"), rank("Ah Kc 9s 7d 5h 4d 2s"));
    }

    #[test]
    fn the_board_can_play_for_both_players() {
        let board = "As Ks Qs Js Ts";
        assert_eq!(
            rank(&format!("{board} 2d 2h")),
            rank(&format!("{board} 9d 9h"))
        );
        let board = "Ad Kc Qh Js Ts";
        assert_eq!(
            rank(&format!("{board} 2d 2h")),
            rank(&format!("{board} 9d 3h"))
        );
    }

    #[test]
    fn five_and_six_card_hands_evaluate() {
        assert_eq!(category("As Ad Ah Kc Ks"), Category::FullHouse);
        assert_eq!(category("As 2d 3h 4c 5s"), Category::Straight);
        assert_eq!(category("As Kd 9h 7c 5s 3d"), Category::HighCard);
        assert_eq!(
            rank("As Ad Kh 9c 5s").cmp(&rank("As Ad Kh 9c 5s 3d 2h")),
            Ordering::Equal
        );
    }
}
