//! Hole-card combos and the 169 strategic hand classes.

use std::fmt;
use std::str::FromStr;

use thiserror::Error;

use crate::card::{Card, CardSet, NUM_CARDS, NUM_SUITS, ParseCardError, parse_rank, rank_char};

pub const NUM_COMBOS: usize = 1326;
pub const NUM_CLASSES: usize = 169;

/// The two cards of every combo, in combo-index order.
const COMBO_CARDS: [(u8, u8); NUM_COMBOS] = {
    let mut table = [(0, 0); NUM_COMBOS];
    let mut index = 0;
    let mut lo = 0;
    while lo < NUM_CARDS as u8 {
        let mut hi = lo + 1;
        while hi < NUM_CARDS as u8 {
            table[index] = (lo, hi);
            index += 1;
            hi += 1;
        }
        lo += 1;
    }
    table
};

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ParseHandError {
    #[error(transparent)]
    Card(#[from] ParseCardError),
    #[error("`{0}` is not a combo: expected two different cards, such as `AsKs`")]
    BadCombo(String),
    #[error("`{0}` is not a hand class: expected a pair such as `QQ`, or `AKs` or `AKo`")]
    BadClass(String),
}

/// Two different hole cards.
///
/// Combos are numbered 0 to 1325 by `(lo, hi)` card index, with the lower card varying slowest.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Combo {
    lo: Card,
    hi: Card,
}

impl Combo {
    pub fn new(a: Card, b: Card) -> Combo {
        assert!(a != b, "a combo needs two different cards");
        if a < b {
            Combo { lo: a, hi: b }
        } else {
            Combo { lo: b, hi: a }
        }
    }

    pub const fn from_index(index: usize) -> Combo {
        let (lo, hi) = COMBO_CARDS[index];
        Combo {
            lo: Card::from_index(lo as usize),
            hi: Card::from_index(hi as usize),
        }
    }

    pub const fn index(self) -> usize {
        let (lo, hi) = (self.lo.index(), self.hi.index());
        lo * (2 * NUM_CARDS - 1 - lo) / 2 + hi - lo - 1
    }

    /// The card with the lower index.
    pub const fn lo(self) -> Card {
        self.lo
    }

    /// The card with the higher index.
    pub const fn hi(self) -> Card {
        self.hi
    }

    pub const fn cards(self) -> CardSet {
        CardSet::from_bits(CardSet::single(self.lo).bits() | CardSet::single(self.hi).bits())
    }

    pub const fn intersects(self, cards: CardSet) -> bool {
        self.cards().intersects(cards)
    }

    /// All 1,326 combos in index order.
    pub fn all() -> impl Iterator<Item = Combo> {
        (0..NUM_COMBOS).map(Combo::from_index)
    }
}

/// Higher card first, such as `AsKs`.
impl fmt::Display for Combo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", self.hi, self.lo)
    }
}

impl fmt::Debug for Combo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl FromStr for Combo {
    type Err = ParseHandError;

    fn from_str(s: &str) -> Result<Combo, ParseHandError> {
        if s.chars().count() != 4 || !s.is_ascii() {
            return Err(ParseHandError::BadCombo(s.to_string()));
        }
        let (a, b): (Card, Card) = (s[..2].parse()?, s[2..].parse()?);
        if a == b {
            return Err(ParseHandError::BadCombo(s.to_string()));
        }
        Ok(Combo::new(a, b))
    }
}

/// One of the 169 preflop hand classes: a pair, a suited hand or an offsuit hand.
///
/// Classes are numbered in 13x13 grid order, `13 * row + column`, with rows and columns running
/// from ace to deuce. Pairs are on the diagonal, suited hands above it and offsuit hands below
/// it. So 0 is AA, 1 is AKs, 13 is AKo and 168 is 22.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct HandClass(u8);

impl HandClass {
    /// Builds a class from its two ranks. `suited` is ignored for a pair.
    pub const fn new(rank_a: u8, rank_b: u8, suited: bool) -> HandClass {
        let (high, low) = if rank_a >= rank_b {
            (rank_a, rank_b)
        } else {
            (rank_b, rank_a)
        };
        assert!(high < 13);
        let (high_line, low_line) = (12 - high, 12 - low);
        if suited || high == low {
            HandClass(13 * high_line + low_line)
        } else {
            HandClass(13 * low_line + high_line)
        }
    }

    pub const fn from_index(index: usize) -> HandClass {
        assert!(index < NUM_CLASSES);
        HandClass(index as u8)
    }

    pub const fn index(self) -> usize {
        self.0 as usize
    }

    /// The class a combo belongs to.
    pub const fn of(combo: Combo) -> HandClass {
        let (a, b) = (combo.lo(), combo.hi());
        HandClass::new(a.rank(), b.rank(), a.suit() == b.suit())
    }

    const fn row(self) -> u8 {
        self.0 / 13
    }

    const fn column(self) -> u8 {
        self.0 % 13
    }

    pub const fn high_rank(self) -> u8 {
        let (row, column) = (self.row(), self.column());
        12 - if row < column { row } else { column }
    }

    pub const fn low_rank(self) -> u8 {
        let (row, column) = (self.row(), self.column());
        12 - if row > column { row } else { column }
    }

    pub const fn is_pair(self) -> bool {
        self.row() == self.column()
    }

    pub const fn is_suited(self) -> bool {
        self.row() < self.column()
    }

    /// 6 for a pair, 4 for a suited hand and 12 for an offsuit hand.
    pub const fn num_combos(self) -> usize {
        if self.is_pair() {
            6
        } else if self.is_suited() {
            4
        } else {
            12
        }
    }

    /// The combos in this class.
    pub fn combos(self) -> impl Iterator<Item = Combo> {
        let (high, low) = (self.high_rank(), self.low_rank());
        let (pair, suited) = (self.is_pair(), self.is_suited());
        let suits = 0..NUM_SUITS as u8;
        suits
            .clone()
            .flat_map(move |a| suits.clone().map(move |b| (a, b)))
            .filter(move |&(a, b)| if pair { a < b } else { (a == b) == suited })
            .map(move |(a, b)| Combo::new(Card::new(high, a), Card::new(low, b)))
    }

    /// All 169 classes in grid order.
    pub fn all() -> impl Iterator<Item = HandClass> {
        (0..NUM_CLASSES as u8).map(HandClass)
    }
}

impl fmt::Display for HandClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}{}",
            rank_char(self.high_rank()),
            rank_char(self.low_rank())
        )?;
        match (self.is_pair(), self.is_suited()) {
            (true, _) => Ok(()),
            (false, true) => write!(f, "s"),
            (false, false) => write!(f, "o"),
        }
    }
}

impl fmt::Debug for HandClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl FromStr for HandClass {
    type Err = ParseHandError;

    fn from_str(s: &str) -> Result<HandClass, ParseHandError> {
        let bad = || ParseHandError::BadClass(s.to_string());
        let chars: Vec<char> = s.chars().collect();
        let (&a, &b) = (
            chars.first().ok_or_else(bad)?,
            chars.get(1).ok_or_else(bad)?,
        );
        let (a, b) = (
            parse_rank(a).ok_or_else(bad)?,
            parse_rank(b).ok_or_else(bad)?,
        );
        match (
            chars.get(2).map(char::to_ascii_lowercase),
            chars.len(),
            a == b,
        ) {
            (None, 2, true) => Ok(HandClass::new(a, b, false)),
            (Some('s'), 3, false) => Ok(HandClass::new(a, b, true)),
            (Some('o'), 3, false) => Ok(HandClass::new(a, b, false)),
            _ => Err(bad()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_combo_round_trips() {
        let mut seen = 0;
        for (index, combo) in Combo::all().enumerate() {
            assert_eq!(combo.index(), index);
            assert!(combo.lo() < combo.hi());
            assert_eq!(Combo::new(combo.hi(), combo.lo()), combo);
            assert_eq!(combo.to_string().parse::<Combo>(), Ok(combo));
            assert_eq!(combo.cards().len(), 2);
            seen += 1;
        }
        assert_eq!(seen, NUM_COMBOS);
    }

    #[test]
    fn combo_index_covers_every_pair_of_cards() {
        let mut hit = [false; NUM_COMBOS];
        for a in Card::all() {
            for b in Card::all().filter(|&b| b != a) {
                hit[Combo::new(a, b).index()] = true;
            }
        }
        assert!(hit.iter().all(|&h| h));
        assert_eq!(Combo::from_index(0).to_string(), "2d2c");
        assert_eq!(Combo::from_index(NUM_COMBOS - 1).to_string(), "AsAh");
    }

    #[test]
    fn combo_parsing() {
        let combo: Combo = "KsAs".parse().unwrap();
        assert_eq!(combo.to_string(), "AsKs");
        assert!(combo.intersects("Ks7d2c".parse().unwrap()));
        assert!(!combo.intersects("Kh7d2c".parse().unwrap()));
        for bad in ["", "As", "AsAs", "AsK", "AsKsQs", "AxKs"] {
            assert!(bad.parse::<Combo>().is_err(), "{bad:?} should not parse");
        }
    }

    #[test]
    fn every_class_round_trips() {
        for (index, class) in HandClass::all().enumerate() {
            assert_eq!(class.index(), index);
            assert_eq!(HandClass::from_index(index), class);
            assert_eq!(class.to_string().parse::<HandClass>(), Ok(class));
            let rebuilt = HandClass::new(class.low_rank(), class.high_rank(), class.is_suited());
            assert_eq!(rebuilt, class);
            assert!(class.high_rank() >= class.low_rank());
            assert_eq!(class.is_pair(), class.high_rank() == class.low_rank());
        }
        assert_eq!(HandClass::all().count(), NUM_CLASSES);
    }

    #[test]
    fn classes_follow_grid_order() {
        let name = |index: usize| HandClass::from_index(index).to_string();
        assert_eq!(name(0), "AA");
        assert_eq!(name(1), "AKs");
        assert_eq!(name(12), "A2s");
        assert_eq!(name(13), "AKo");
        assert_eq!(name(14), "KK");
        assert_eq!(name(15), "KQs");
        assert_eq!(name(156), "A2o");
        assert_eq!(name(168), "22");
        assert_eq!("kas".parse::<HandClass>().unwrap().to_string(), "AKs");
    }

    #[test]
    fn class_combos_partition_the_1326() {
        let (mut pairs, mut suited, mut offsuit) = (0, 0, 0);
        let mut owner = [None; NUM_COMBOS];
        for class in HandClass::all() {
            let combos: Vec<Combo> = class.combos().collect();
            assert_eq!(combos.len(), class.num_combos());
            match combos.len() {
                6 => pairs += 1,
                4 => suited += 1,
                12 => offsuit += 1,
                n => panic!("{class} has {n} combos"),
            }
            for combo in combos {
                assert_eq!(HandClass::of(combo), class);
                assert_eq!(
                    owner[combo.index()].replace(class),
                    None,
                    "{combo} is in two classes"
                );
            }
        }
        assert_eq!((pairs, suited, offsuit), (13, 78, 78));
        assert_eq!(13 * 6 + 78 * 4 + 78 * 12, NUM_COMBOS);
        assert!(owner.iter().all(Option::is_some));
    }

    #[test]
    fn bad_classes_are_rejected() {
        for bad in ["", "A", "AK", "AAs", "AKx", "AKso", "1Ks"] {
            assert!(
                bad.parse::<HandClass>().is_err(),
                "{bad:?} should not parse"
            );
        }
    }
}
