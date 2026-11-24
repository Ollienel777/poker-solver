//! Cards and sets of cards.

use std::fmt;
use std::ops::{BitAnd, BitOr, Sub};
use std::str::FromStr;

use thiserror::Error;

pub const NUM_RANKS: usize = 13;
pub const NUM_SUITS: usize = 4;
pub const NUM_CARDS: usize = 52;

const RANK_CHARS: &[u8; NUM_RANKS] = b"23456789TJQKA";
const SUIT_CHARS: &[u8; NUM_SUITS] = b"cdhs";

/// Returns the character for a rank, where rank 0 is the deuce and rank 12 is the ace.
pub fn rank_char(rank: u8) -> char {
    RANK_CHARS[rank as usize] as char
}

/// Parses a rank character. Letters are accepted in either case.
pub fn parse_rank(c: char) -> Option<u8> {
    let upper = c.to_ascii_uppercase();
    RANK_CHARS
        .iter()
        .position(|&r| r as char == upper)
        .map(|i| i as u8)
}

fn parse_suit(c: char) -> Option<u8> {
    let lower = c.to_ascii_lowercase();
    SUIT_CHARS
        .iter()
        .position(|&s| s as char == lower)
        .map(|i| i as u8)
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ParseCardError {
    #[error("`{0}` is not a card: expected a rank and a suit, such as `As` or `Td`")]
    BadCard(String),
    #[error("`{0}` is not a list of cards: expected pairs of rank and suit, such as `Ks7d2c`")]
    BadLength(String),
    #[error("card `{0}` appears more than once")]
    Duplicate(Card),
}

/// A playing card, stored as `4 * rank + suit`.
///
/// Rank 0 is the deuce and rank 12 is the ace. Suits 0 to 3 are clubs, diamonds, hearts, spades.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Card(u8);

impl Card {
    pub const fn new(rank: u8, suit: u8) -> Card {
        assert!(rank < NUM_RANKS as u8 && suit < NUM_SUITS as u8);
        Card(4 * rank + suit)
    }

    pub const fn from_index(index: usize) -> Card {
        assert!(index < NUM_CARDS);
        Card(index as u8)
    }

    pub const fn index(self) -> usize {
        self.0 as usize
    }

    pub const fn rank(self) -> u8 {
        self.0 / 4
    }

    pub const fn suit(self) -> u8 {
        self.0 % 4
    }

    /// All 52 cards in index order.
    pub fn all() -> impl Iterator<Item = Card> {
        (0..NUM_CARDS as u8).map(Card)
    }
}

impl fmt::Display for Card {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}{}",
            rank_char(self.rank()),
            SUIT_CHARS[self.suit() as usize] as char
        )
    }
}

impl fmt::Debug for Card {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl FromStr for Card {
    type Err = ParseCardError;

    fn from_str(s: &str) -> Result<Card, ParseCardError> {
        let mut chars = s.chars();
        match (chars.next(), chars.next(), chars.next()) {
            (Some(r), Some(u), None) => match (parse_rank(r), parse_suit(u)) {
                (Some(rank), Some(suit)) => Ok(Card::new(rank, suit)),
                _ => Err(ParseCardError::BadCard(s.to_string())),
            },
            _ => Err(ParseCardError::BadCard(s.to_string())),
        }
    }
}

/// A set of cards as a bitmask. Bit `i` is the card with index `i`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct CardSet(u64);

impl CardSet {
    pub const EMPTY: CardSet = CardSet(0);
    pub const FULL: CardSet = CardSet((1 << NUM_CARDS) - 1);

    pub const fn from_bits(bits: u64) -> CardSet {
        assert!(bits >> NUM_CARDS == 0);
        CardSet(bits)
    }

    pub const fn bits(self) -> u64 {
        self.0
    }

    pub const fn single(card: Card) -> CardSet {
        CardSet(1 << card.0)
    }

    pub const fn contains(self, card: Card) -> bool {
        self.0 & (1 << card.0) != 0
    }

    pub const fn len(self) -> usize {
        self.0.count_ones() as usize
    }

    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    pub const fn intersects(self, other: CardSet) -> bool {
        self.0 & other.0 != 0
    }

    /// Adds a card. Returns false if it was already in the set.
    pub fn insert(&mut self, card: Card) -> bool {
        let added = !self.contains(card);
        self.0 |= 1 << card.0;
        added
    }

    pub fn remove(&mut self, card: Card) {
        self.0 &= !(1 << card.0);
    }

    /// The cards in ascending index order.
    pub fn iter(self) -> CardSetIter {
        CardSetIter(self.0)
    }
}

impl BitOr for CardSet {
    type Output = CardSet;

    fn bitor(self, rhs: CardSet) -> CardSet {
        CardSet(self.0 | rhs.0)
    }
}

impl BitAnd for CardSet {
    type Output = CardSet;

    fn bitand(self, rhs: CardSet) -> CardSet {
        CardSet(self.0 & rhs.0)
    }
}

/// Set difference.
impl Sub for CardSet {
    type Output = CardSet;

    fn sub(self, rhs: CardSet) -> CardSet {
        CardSet(self.0 & !rhs.0)
    }
}

pub struct CardSetIter(u64);

impl Iterator for CardSetIter {
    type Item = Card;

    fn next(&mut self) -> Option<Card> {
        if self.0 == 0 {
            return None;
        }
        let index = self.0.trailing_zeros() as u8;
        self.0 &= self.0 - 1;
        Some(Card(index))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let n = self.0.count_ones() as usize;
        (n, Some(n))
    }
}

impl ExactSizeIterator for CardSetIter {}

impl IntoIterator for CardSet {
    type Item = Card;
    type IntoIter = CardSetIter;

    fn into_iter(self) -> CardSetIter {
        self.iter()
    }
}

impl FromIterator<Card> for CardSet {
    fn from_iter<I: IntoIterator<Item = Card>>(iter: I) -> CardSet {
        let mut set = CardSet::EMPTY;
        for card in iter {
            set.insert(card);
        }
        set
    }
}

/// Cards from highest to lowest, such as `Ks7d2c`.
impl fmt::Display for CardSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut bits = self.0;
        while bits != 0 {
            let index = 63 - bits.leading_zeros() as u8;
            bits &= !(1 << index);
            write!(f, "{}", Card(index))?;
        }
        Ok(())
    }
}

impl fmt::Debug for CardSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "CardSet({self})")
    }
}

/// Parses cards written one after another, such as `Ks7d2c`. Whitespace is ignored.
impl FromStr for CardSet {
    type Err = ParseCardError;

    fn from_str(s: &str) -> Result<CardSet, ParseCardError> {
        let chars: Vec<char> = s.chars().filter(|c| !c.is_whitespace()).collect();
        if !chars.len().is_multiple_of(2) {
            return Err(ParseCardError::BadLength(s.to_string()));
        }
        let mut set = CardSet::EMPTY;
        for pair in chars.chunks(2) {
            let card: Card = pair.iter().collect::<String>().parse()?;
            if !set.insert(card) {
                return Err(ParseCardError::Duplicate(card));
            }
        }
        Ok(set)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_card_round_trips() {
        for (index, card) in Card::all().enumerate() {
            assert_eq!(card.index(), index);
            assert_eq!(Card::new(card.rank(), card.suit()), card);
            assert_eq!(Card::from_index(index), card);
            assert_eq!(card.to_string().parse::<Card>(), Ok(card));
        }
        assert_eq!(Card::all().count(), NUM_CARDS);
    }

    #[test]
    fn card_layout_and_names() {
        let two_clubs: Card = "2c".parse().unwrap();
        let ace_spades: Card = "As".parse().unwrap();
        assert_eq!(two_clubs.index(), 0);
        assert_eq!(ace_spades.index(), 51);
        assert_eq!((ace_spades.rank(), ace_spades.suit()), (12, 3));
        assert_eq!("td".parse::<Card>().unwrap().to_string(), "Td");
        assert_eq!("AS".parse::<Card>(), Ok(ace_spades));
    }

    #[test]
    fn bad_cards_are_rejected() {
        for bad in ["", "A", "Ax", "1s", "Asd", "10s"] {
            assert!(bad.parse::<Card>().is_err(), "{bad:?} should not parse");
        }
    }

    #[test]
    fn card_set_operations() {
        let mut set: CardSet = "Ks7d2c".parse().unwrap();
        assert_eq!(set.len(), 3);
        assert_eq!(set.to_string(), "Ks7d2c");
        assert!(set.contains("7d".parse().unwrap()));
        assert!(!set.insert("Ks".parse().unwrap()));
        assert!(set.insert("Ah".parse().unwrap()));
        assert_eq!(set.to_string(), "AhKs7d2c");
        set.remove("Ks".parse().unwrap());
        assert_eq!(set.to_string(), "Ah7d2c");

        let other: CardSet = "Ah Qh".parse().unwrap();
        assert!(set.intersects(other));
        assert_eq!((set | other).len(), 4);
        assert_eq!((set & other).to_string(), "Ah");
        assert_eq!((set - other).to_string(), "7d2c");
        assert_eq!(set.iter().collect::<CardSet>(), set);

        assert_eq!(CardSet::FULL.len(), NUM_CARDS);
        assert!(CardSet::EMPTY.is_empty());
        assert_eq!(Card::all().collect::<CardSet>(), CardSet::FULL);
    }

    #[test]
    fn card_set_iterates_in_index_order() {
        let set: CardSet = "As2c7d".parse().unwrap();
        let indexes: Vec<usize> = set.iter().map(Card::index).collect();
        assert_eq!(indexes, vec![0, 21, 51]);
        assert_eq!(set.iter().len(), 3);
    }

    #[test]
    fn bad_card_sets_are_rejected() {
        assert!(matches!(
            "Ks7".parse::<CardSet>(),
            Err(ParseCardError::BadLength(_))
        ));
        assert!(matches!(
            "KsKs".parse::<CardSet>(),
            Err(ParseCardError::Duplicate(_))
        ));
        assert!(matches!(
            "Ks7x".parse::<CardSet>(),
            Err(ParseCardError::BadCard(_))
        ));
        assert_eq!("".parse::<CardSet>(), Ok(CardSet::EMPTY));
    }
}
