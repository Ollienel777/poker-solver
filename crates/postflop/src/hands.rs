//! The hands a player can hold on a board.
//!
//! A *hand list* holds the combos of one player's range that have a positive weight and share
//! no card with the board, in ascending combo index ([`Combo::index`]). A hand's position in
//! the list is its *hand index*: every per-hand vector the postflop solver passes around (reach
//! probabilities, counterfactual values, regrets) is laid out in this order. The two players'
//! lists are built from different ranges, so they generally differ in length and the same combo
//! sits at different indexes in each.

use pk_core::{CardSet, Combo, Range};

/// One player's hands on one board: the combos of a range with positive weight that share no
/// card with the board, in ascending combo index, with their range weights.
#[derive(Debug, Clone, PartialEq)]
pub struct HandList {
    board: CardSet,
    combos: Vec<Combo>,
    weights: Vec<f64>,
}

impl HandList {
    /// The hands of `range` that can be held on `board`. A combo is listed when its weight is
    /// positive and neither of its cards is on the board.
    pub fn new(range: &Range, board: CardSet) -> HandList {
        let (combos, weights) = range
            .iter()
            .filter(|(combo, _)| !combo.intersects(board))
            .map(|(combo, weight)| (combo, f64::from(weight)))
            .unzip();
        HandList {
            board,
            combos,
            weights,
        }
    }

    /// The board the list was built for.
    pub fn board(&self) -> CardSet {
        self.board
    }

    pub fn len(&self) -> usize {
        self.combos.len()
    }

    pub fn is_empty(&self) -> bool {
        self.combos.is_empty()
    }

    /// The combos, by hand index. They are in ascending combo index.
    pub fn combos(&self) -> &[Combo] {
        &self.combos
    }

    /// The range weight of each hand, by hand index, converted exactly from the range's `f32`.
    /// Every weight is positive.
    pub fn weights(&self) -> &[f64] {
        &self.weights
    }

    /// The hand index of `combo`, or `None` if it is not in the list.
    pub fn position(&self, combo: Combo) -> Option<usize> {
        self.combos
            .binary_search_by_key(&combo.index(), |c| c.index())
            .ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cards(s: &str) -> CardSet {
        s.parse().unwrap()
    }

    fn combo(s: &str) -> Combo {
        s.parse().unwrap()
    }

    #[test]
    fn a_hand_list_keeps_weighted_combos_off_the_board_in_combo_order() {
        let range: Range = "AA,KK:0.5,72o:0".parse().unwrap();
        let hands = HandList::new(&range, cards("As7d2c"));
        // Three aces are left once the ace of spades is on the board, and all six kings.
        assert_eq!(hands.len(), 3 + 6);
        assert_eq!(hands.board(), cards("As7d2c"));
        let indexes: Vec<usize> = hands.combos().iter().map(|c| c.index()).collect();
        assert!(indexes.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(hands.combos().iter().all(|c| !c.intersects(hands.board())));
        assert_eq!(hands.position(combo("AsAh")), None);
        assert_eq!(hands.position(combo("7h2d")), None);

        let kings = hands.position(combo("KsKh")).unwrap();
        let aces = hands.position(combo("AhAd")).unwrap();
        assert_eq!(hands.combos()[kings], combo("KsKh"));
        assert_eq!(hands.weights()[kings], 0.5);
        assert_eq!(hands.weights()[aces], 1.0);
        // Kings have lower combo indexes than aces, so they come first.
        assert!(kings < aces);
    }

    #[test]
    fn a_range_blocked_by_the_board_gives_an_empty_list() {
        let range: Range = "AsKs,AhAs".parse().unwrap();
        let hands = HandList::new(&range, cards("As7d2c"));
        assert!(hands.is_empty());
        assert_eq!(hands.len(), 0);
        assert!(HandList::new(&Range::empty(), cards("Ks7d2c")).is_empty());
    }

    #[test]
    fn a_full_range_holds_every_combo_off_the_board() {
        for (board, count) in [("Ks7d2c", 1176), ("Ks7d2c9h", 1128), ("Ks7d2c9h3s", 1081)] {
            assert_eq!(HandList::new(&Range::full(), cards(board)).len(), count);
        }
    }
}
