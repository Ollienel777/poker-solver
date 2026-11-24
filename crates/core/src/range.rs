//! Weighted ranges of hole cards, and the range-string format.
//!
//! A range string is a comma-separated list of items. Each item is one of:
//!
//! - a hand class: `QQ`, `AKs`, `AKo`, or `AK` for both suited and offsuit
//! - a class and everything above it: `QQ+` (QQ to AA), `ATs+` (ATs to AKs)
//! - a span of classes: `77-TT`, `A2s-A5s`, or `54s-JTs` for hands with the same gap
//! - one combo: `AsKs`
//!
//! An item may end with `:weight`, a number from 0 to 1. Later items override earlier ones.

use std::fmt;
use std::str::FromStr;

use thiserror::Error;

use crate::card::{CardSet, parse_rank, rank_char};
use crate::combo::{Combo, HandClass, NUM_COMBOS};

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ParseRangeError {
    #[error("the range has an empty item; check for a stray comma")]
    EmptyItem,
    #[error(
        "`{0}` is not a hand: expected a class such as `QQ+` or `AKs`, or a combo such as `AsKs`"
    )]
    BadHand(String),
    #[error(
        "`{0}` is not a span of classes: both ends must be pairs, share a high card, or have the same gap"
    )]
    BadSpan(String),
    #[error("`{0}` is not a weight: expected a number from 0 to 1")]
    BadWeight(String),
}

/// A weight from 0 to 1 for each of the 1,326 combos.
#[derive(Clone, PartialEq)]
pub struct Range {
    weights: [f32; NUM_COMBOS],
}

impl Range {
    /// A range holding no hands.
    pub const fn empty() -> Range {
        Range {
            weights: [0.0; NUM_COMBOS],
        }
    }

    /// Every combo at full weight.
    pub const fn full() -> Range {
        Range {
            weights: [1.0; NUM_COMBOS],
        }
    }

    pub fn weight(&self, combo: Combo) -> f32 {
        self.weights[combo.index()]
    }

    /// The weights in combo-index order.
    pub fn weights(&self) -> &[f32; NUM_COMBOS] {
        &self.weights
    }

    pub fn set(&mut self, combo: Combo, weight: f32) {
        assert!(
            (0.0..=1.0).contains(&weight),
            "weight {weight} is outside 0 to 1"
        );
        self.weights[combo.index()] = weight;
    }

    pub fn set_class(&mut self, class: HandClass, weight: f32) {
        for combo in class.combos() {
            self.set(combo, weight);
        }
    }

    /// Sets the weight of every combo that shares a card with `cards` to zero.
    pub fn remove_blocked(&mut self, cards: CardSet) {
        for combo in Combo::all().filter(|combo| combo.intersects(cards)) {
            self.weights[combo.index()] = 0.0;
        }
    }

    /// The combos with a positive weight, in index order.
    pub fn iter(&self) -> impl Iterator<Item = (Combo, f32)> + '_ {
        Combo::all()
            .map(|combo| (combo, self.weight(combo)))
            .filter(|&(_, weight)| weight > 0.0)
    }

    /// The number of combos with a positive weight.
    pub fn num_combos(&self) -> usize {
        self.iter().count()
    }

    pub fn is_empty(&self) -> bool {
        self.num_combos() == 0
    }

    pub fn total_weight(&self) -> f64 {
        self.weights.iter().map(|&weight| f64::from(weight)).sum()
    }

    fn set_spec(&mut self, high: u8, low: u8, suits: Suits, weight: f32) {
        if high == low || suits != Suits::Offsuit {
            self.set_class(HandClass::new(high, low, true), weight);
        }
        if high != low && suits != Suits::Suited {
            self.set_class(HandClass::new(high, low, false), weight);
        }
    }

    fn parse_item(&mut self, item: &str) -> Result<(), ParseRangeError> {
        let (hand, weight) = match item.split_once(':') {
            Some((hand, weight)) => (hand.trim(), parse_weight(weight.trim())?),
            None => (item, 1.0),
        };
        let bad_hand = || ParseRangeError::BadHand(hand.to_string());
        let bad_span = || ParseRangeError::BadSpan(hand.to_string());

        if let Some((left, right)) = hand.split_once('-') {
            let a = ClassSpec::parse(left.trim()).ok_or_else(bad_hand)?;
            let b = ClassSpec::parse(right.trim()).ok_or_else(bad_hand)?;
            if a.suits != b.suits || a.is_pair() != b.is_pair() {
                return Err(bad_span());
            }
            if a.is_pair() {
                for rank in a.high.min(b.high)..=a.high.max(b.high) {
                    self.set_spec(rank, rank, a.suits, weight);
                }
            } else if a.high == b.high {
                for low in a.low.min(b.low)..=a.low.max(b.low) {
                    self.set_spec(a.high, low, a.suits, weight);
                }
            } else if a.gap() == b.gap() {
                for high in a.high.min(b.high)..=a.high.max(b.high) {
                    self.set_spec(high, high - a.gap(), a.suits, weight);
                }
            } else {
                return Err(bad_span());
            }
        } else if let Some(base) = hand.strip_suffix('+') {
            let spec = ClassSpec::parse(base.trim()).ok_or_else(bad_hand)?;
            if spec.is_pair() {
                for rank in spec.high..13 {
                    self.set_spec(rank, rank, spec.suits, weight);
                }
            } else {
                for low in spec.low..spec.high {
                    self.set_spec(spec.high, low, spec.suits, weight);
                }
            }
        } else if let Some(spec) = ClassSpec::parse(hand) {
            self.set_spec(spec.high, spec.low, spec.suits, weight);
        } else {
            let combo: Combo = hand.parse().map_err(|_| bad_hand())?;
            self.set(combo, weight);
        }
        Ok(())
    }
}

impl Default for Range {
    fn default() -> Range {
        Range::empty()
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Suits {
    Suited,
    Offsuit,
    Both,
}

/// A hand class as written in a range string, where `AK` means both AKs and AKo.
struct ClassSpec {
    high: u8,
    low: u8,
    suits: Suits,
}

impl ClassSpec {
    fn parse(s: &str) -> Option<ClassSpec> {
        let chars: Vec<char> = s.chars().collect();
        if !(2..=3).contains(&chars.len()) {
            return None;
        }
        let (a, b) = (parse_rank(chars[0])?, parse_rank(chars[1])?);
        let suits = match chars.get(2).map(char::to_ascii_lowercase) {
            None => Suits::Both,
            Some('s') if a != b => Suits::Suited,
            Some('o') if a != b => Suits::Offsuit,
            Some(_) => return None,
        };
        Some(ClassSpec {
            high: a.max(b),
            low: a.min(b),
            suits,
        })
    }

    fn is_pair(&self) -> bool {
        self.high == self.low
    }

    fn gap(&self) -> u8 {
        self.high - self.low
    }
}

fn parse_weight(s: &str) -> Result<f32, ParseRangeError> {
    match s.parse::<f32>() {
        Ok(weight) if (0.0..=1.0).contains(&weight) => Ok(weight),
        _ => Err(ParseRangeError::BadWeight(s.to_string())),
    }
}

impl FromStr for Range {
    type Err = ParseRangeError;

    fn from_str(s: &str) -> Result<Range, ParseRangeError> {
        let mut range = Range::empty();
        if s.trim().is_empty() {
            return Ok(range);
        }
        for item in s.split(',') {
            let item = item.trim();
            if item.is_empty() {
                return Err(ParseRangeError::EmptyItem);
            }
            range.parse_item(item)?;
        }
        Ok(range)
    }
}

/// Inclusive runs of consecutive `true` values, from the last run to the first.
fn runs_from_top(flags: &[bool]) -> Vec<(u8, u8)> {
    let mut runs = Vec::new();
    let mut end = flags.len();
    while end > 0 {
        if !flags[end - 1] {
            end -= 1;
            continue;
        }
        let mut start = end - 1;
        while start > 0 && flags[start - 1] {
            start -= 1;
        }
        runs.push((start as u8, end as u8 - 1));
        end = start;
    }
    runs
}

/// A short canonical range string. Parsing it gives back exactly this range.
///
/// Classes whose combos all share a weight are grouped by weight, highest first, and written as
/// pairs, then suited hands, then offsuit hands. Combos of the remaining classes follow one by one.
impl fmt::Display for Range {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let class_weights: Vec<Option<f32>> = HandClass::all()
            .map(|class| {
                let mut weights = class.combos().map(|combo| self.weight(combo));
                let first = weights.next()?;
                weights.all(|weight| weight == first).then_some(first)
            })
            .collect();

        let mut levels: Vec<f32> = class_weights
            .iter()
            .flatten()
            .copied()
            .filter(|&weight| weight > 0.0)
            .collect();
        levels.sort_by(|a, b| b.total_cmp(a));
        levels.dedup();

        let mut items: Vec<String> = Vec::new();
        for level in levels {
            let has = |high: u8, low: u8, suited: bool| {
                class_weights[HandClass::new(high, low, suited).index()] == Some(level)
            };
            let suffix = if level == 1.0 {
                String::new()
            } else {
                format!(":{level}")
            };

            let pairs: Vec<bool> = (0..13).map(|rank| has(rank, rank, false)).collect();
            for (start, end) in runs_from_top(&pairs) {
                let (a, b) = (rank_char(start), rank_char(end));
                items.push(match (start == end, end == 12) {
                    (true, _) => format!("{a}{a}{suffix}"),
                    (false, true) => format!("{a}{a}+{suffix}"),
                    (false, false) => format!("{a}{a}-{b}{b}{suffix}"),
                });
            }

            for (suited, tag) in [(true, 's'), (false, 'o')] {
                for high in (1..13).rev() {
                    let lows: Vec<bool> = (0..high).map(|low| has(high, low, suited)).collect();
                    for (start, end) in runs_from_top(&lows) {
                        let (h, a, b) = (rank_char(high), rank_char(start), rank_char(end));
                        items.push(match (start == end, end == high - 1) {
                            (true, _) => format!("{h}{a}{tag}{suffix}"),
                            (false, true) => format!("{h}{a}{tag}+{suffix}"),
                            (false, false) => format!("{h}{a}{tag}-{h}{b}{tag}{suffix}"),
                        });
                    }
                }
            }
        }

        for class in HandClass::all().filter(|class| class_weights[class.index()].is_none()) {
            for combo in class.combos() {
                match self.weight(combo) {
                    0.0 => {}
                    1.0 => items.push(combo.to_string()),
                    weight => items.push(format!("{combo}:{weight}")),
                }
            }
        }

        write!(f, "{}", items.join(","))
    }
}

impl fmt::Debug for Range {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Range({self})")
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;
    use crate::combo::NUM_CLASSES;

    fn range(s: &str) -> Range {
        s.parse()
            .unwrap_or_else(|err| panic!("{s:?} should parse: {err}"))
    }

    #[test]
    fn known_strings_have_the_right_combo_counts() {
        for (text, combos) in [
            ("", 0),
            ("QQ", 6),
            ("QQ+", 18),
            ("22+", 78),
            ("77-TT", 24),
            ("TT-77", 24),
            ("AKs", 4),
            ("AKo", 12),
            ("AK", 16),
            ("KA", 16),
            ("ATs+", 16),
            ("T6s+", 16),
            ("A2+", 12 * 16),
            ("A2s-A5s", 16),
            ("A5s-A2s", 16),
            ("54s-JTs", 28),
            ("K9o-KJo", 36),
            ("AsKs", 1),
            ("QQ+, AKs ,AKo", 34),
            ("AA,AA,AA", 6),
        ] {
            assert_eq!(range(text).num_combos(), combos, "{text:?}");
        }
        assert_eq!(Range::full().num_combos(), NUM_COMBOS);
        assert!(Range::empty().is_empty());
    }

    #[test]
    fn weights_apply_and_later_items_override() {
        let half = range("AA:0.5");
        assert_eq!(half.num_combos(), 6);
        assert_eq!(half.total_weight(), 3.0);

        let mixed = range("AA,AsAh:0.25,KK:0.5,KK");
        assert_eq!(mixed.weight("AsAh".parse().unwrap()), 0.25);
        assert_eq!(mixed.weight("AsAd".parse().unwrap()), 1.0);
        assert_eq!(mixed.weight("KsKh".parse().unwrap()), 1.0);
        assert_eq!(mixed.total_weight(), 11.25);

        let removed = range("AA,AA:0");
        assert!(removed.is_empty());
    }

    #[test]
    fn canonical_strings() {
        for (text, canonical) in [
            ("AKs,QQ,KK,AA", "QQ+,AKs"),
            ("22,33,44,55,66,77,88,99,TT,JJ,QQ,KK,AA", "22+"),
            ("99,88,77", "77-99"),
            ("A5s,A4s,A3s,A2s", "A2s-A5s"),
            ("AK,AQ", "AQs+,AQo+"),
            ("AA:0.5,KK", "KK,AA:0.5"),
            ("72o", "72o"),
            ("AKs,AsKs:0.5", "AcKc,AdKd,AhKh,AsKs:0.5"),
            ("", ""),
        ] {
            assert_eq!(range(text).to_string(), canonical, "{text:?}");
            assert_eq!(range(canonical), range(text), "{canonical:?}");
        }
    }

    #[test]
    fn architecture_examples_round_trip() {
        for text in [
            "22+,A2s+,K2s+,Q4s+,J6s+,T6s+,96s+,86s+,75s+,64s+,54s,A2o+,K8o+,Q9o+,J9o+,T8o+,98o",
            "22+,A2+,K2s+,K8o+,Q5s+,Q9o+,J7s+,J9o+,T7s+,T9o,97s+,86s+,75s+,65s,54s",
        ] {
            let parsed = range(text);
            assert!(parsed.num_combos() > 400);
            assert_eq!(range(&parsed.to_string()), parsed);
        }
    }

    #[test]
    fn remove_blocked_drops_combos_that_share_a_card() {
        let mut aces = range("AA,KK");
        aces.remove_blocked("AsKd2c".parse().unwrap());
        assert_eq!(aces.num_combos(), 6);
        assert_eq!(aces.weight("AhAd".parse().unwrap()), 1.0);
        assert_eq!(aces.weight("AsAh".parse().unwrap()), 0.0);
        assert_eq!(aces.weight("KdKc".parse().unwrap()), 0.0);
    }

    #[test]
    fn bad_strings_are_rejected() {
        use ParseRangeError::*;
        let error = |s: &str| s.parse::<Range>().unwrap_err();
        assert_eq!(error("AA,,KK"), EmptyItem);
        assert_eq!(error("AA,"), EmptyItem);
        assert!(matches!(error("AKx"), BadHand(_)));
        assert!(matches!(error("QQs"), BadHand(_)));
        assert!(matches!(error("AsAs"), BadHand(_)));
        assert!(matches!(error("A"), BadHand(_)));
        assert!(matches!(error("AKs-"), BadHand(_)));
        assert!(matches!(error("A2s-K5s"), BadSpan(_)));
        assert!(matches!(error("A2s-A5o"), BadSpan(_)));
        assert!(matches!(error("22-A5s"), BadSpan(_)));
        assert!(matches!(error("AA:1.5"), BadWeight(_)));
        assert!(matches!(error("AA:-0.1"), BadWeight(_)));
        assert!(matches!(error("AA:x"), BadWeight(_)));
        assert!(matches!(error("AA:NaN"), BadWeight(_)));
    }

    fn any_weight() -> impl Strategy<Value = f32> {
        prop_oneof![
            3 => Just(0.0f32),
            3 => Just(1.0f32),
            1 => Just(0.5f32),
            1 => 0.0f32..=1.0f32,
        ]
    }

    fn any_range() -> impl Strategy<Value = Range> {
        let classes = prop::collection::vec(any_weight(), NUM_CLASSES);
        let combos = prop::collection::vec((0..NUM_COMBOS, any_weight()), 0..8);
        (classes, combos).prop_map(|(classes, combos)| {
            let mut range = Range::empty();
            for (index, weight) in classes.into_iter().enumerate() {
                range.set_class(HandClass::from_index(index), weight);
            }
            for (index, weight) in combos {
                range.set(Combo::from_index(index), weight);
            }
            range
        })
    }

    proptest! {
        #[test]
        fn formatting_then_parsing_gives_the_same_range(range in any_range()) {
            let text = range.to_string();
            prop_assert_eq!(text.parse::<Range>(), Ok(range));
        }
    }
}
