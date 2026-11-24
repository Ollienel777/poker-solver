//! Range-against-range evaluation of terminal nodes in O(n), with card removal.
//!
//! # What is computed
//!
//! At a terminal node the traversing player's counterfactual value for each of their hands `h`
//! is
//!
//! ```text
//! value[h] = Σ reach[o] · u(h, o)    over the opponent's hands o that share no card with h
//! ```
//!
//! `reach[o]` is the opponent's reach probability for their hand `o`, indexed by the opponent's
//! hand list, and `value[h]` is indexed by the traverser's (see [`crate::hands`] for hand lists
//! and their order). `u(h, o)` is the traverser's payoff, in chips relative to the root of the
//! tree (see [`crate::payoff`]):
//!
//! - after a fold, one number for every pair of hands;
//! - at a showdown, `win`, `tie` or `lose` as `h` is stronger than, as strong as, or weaker than
//!   `o` with the five board cards.
//!
//! Nothing is normalised: the values scale with the reach given. Done directly, the sum costs
//! O(n²) per terminal, with n up to 1,081 hands on the river. The two evaluations here cost
//! O(n) plus a pass over the 52 cards. They sum in a different order from the double loop, so
//! they agree with it to rounding, not bit for bit.
//!
//! # Fold
//!
//! Write `R` for the opponent's total reach and `R_c` for the reach of the opponent's hands
//! that hold the card `c`. For a hand `h` with cards `a` and `b`, the hands that share a card
//! with `h` are those holding `a` or `b`, so by inclusion and exclusion
//!
//! ```text
//! D(h) = Σ reach[o] over o sharing no card with h  =  R − R_a − R_b + reach[h']
//! ```
//!
//! where `h'` is the same combo as `h` in the opponent's list, if the opponent holds it: it
//! holds both cards, so `R_a` and `R_b` both subtracted it. Then `value[h] = u · D(h)`.
//!
//! # Showdown
//!
//! On a river board every hand's strength is evaluated once, when the [`BoardHands`] is built,
//! and each player's hands are sorted from weakest to strongest. Hands of equal strength form a
//! *tie block*. The opponent reach that `h` meets splits into `W(h)`, `T(h)` and `L(h)`, the
//! disjoint reach of the opponent's weaker hands, equal hands and stronger hands, and
//!
//! ```text
//! value[h] = win · W(h) + tie · T(h) + lose · L(h)
//! ```
//!
//! One sweep walks the traverser's tie blocks from weakest to strongest while adding the
//! opponent's hands, in strength order, to a running reach sum `S` and running sums `S_c` for
//! each card. For a block of strength `s`:
//!
//! 1. Add the opponent's hands weaker than `s`. For each `h` in the block, `W(h) = S − S_a − S_b`.
//!    No correction is needed: `h'` has the strength of `h`, so it is not in the sum yet.
//! 2. Add the opponent's hands of strength `s`, the opponent's tie block. Now
//!    `W(h) + T(h) = S − S_a − S_b + reach[h']`, with `h'` as for a fold.
//! 3. `L(h) = D(h) − (W(h) + T(h))`, with `D(h)` as for a fold.
//!
//! # References
//!
//! - Johanson, Waugh, Bowling and Zinkevich, *Accelerating Best Response Calculation in Large
//!   Extensive Games*, IJCAI (2011): evaluating a terminal for every hand of a range at once in
//!   O(n), by sorting hands by strength and correcting for card removal by inclusion and
//!   exclusion over the cards each hand holds.

use pk_core::{CardSet, Combo, HandRank, NUM_CARDS, Range, evaluate};
use pk_game::Player;

use crate::hands::HandList;
use crate::payoff::ShowdownPayoff;

/// Both players' hands on one board, with what the terminal evaluations need. Built once per
/// board and read by every terminal node on it.
///
/// It holds each player's [`HandList`], indexed by [`Player::index`], the cross index from
/// each player's hands to the same combo in the other player's list, and on a river board each
/// player's hands sorted by strength. It is plain data, rebuilt from the ranges for each board.
#[derive(Debug, Clone)]
pub struct BoardHands {
    board: CardSet,
    hands: [HandList; 2],
    /// `same[p][h]` is the index in the other player's list of the combo `hands[p][h]`, if the
    /// other player holds it too.
    same: [Vec<Option<u32>>; 2],
    /// On a river board, each player's `(strength, hand index)` pairs from weakest to
    /// strongest. Equal strengths are in hand order.
    ranked: Option<[Vec<(HandRank, u32)>; 2]>,
}

impl BoardHands {
    /// The hands of OOP's range `oop` and IP's range `ip` on `board`, which may be a flop, turn
    /// or river. On a river the hands are also evaluated and sorted for
    /// [`showdown_values`](BoardHands::showdown_values).
    ///
    /// Panics unless the board has 3, 4 or 5 cards.
    pub fn new(board: CardSet, oop: &Range, ip: &Range) -> BoardHands {
        assert!(
            (3..=5).contains(&board.len()),
            "a postflop board has 3 to 5 cards, not {}",
            board.len()
        );
        let hands = [HandList::new(oop, board), HandList::new(ip, board)];
        let same = [
            same_combos(&hands[0], &hands[1]),
            same_combos(&hands[1], &hands[0]),
        ];
        let ranked = (board.len() == 5).then(|| [ranked_hands(&hands[0]), ranked_hands(&hands[1])]);
        BoardHands {
            board,
            hands,
            same,
            ranked,
        }
    }

    pub fn board(&self) -> CardSet {
        self.board
    }

    /// One player's hands. Their order is the hand order of every per-hand vector for that
    /// player.
    pub fn hands(&self, player: Player) -> &HandList {
        &self.hands[player.index()]
    }

    /// Writes the traverser's value for each of their hands at a fold:
    ///
    /// ```text
    /// values[h] = payoff · Σ reach[o]    over the opponent's hands o that share no card with h
    /// ```
    ///
    /// `payoff` is the traverser's payoff after the fold: positive when the opponent folded,
    /// and minus what the traverser put in when the traverser folded (see
    /// [`fold_payoffs`](crate::payoff::fold_payoffs)). `reach` is the opponent's reach by
    /// opponent hand index and `values` is written by traverser hand index. Works on a flop,
    /// turn or river board. Takes O(n) time and allocates nothing.
    ///
    /// Panics unless `reach` has one entry per opponent hand and `values` one per traverser
    /// hand.
    pub fn fold_values(&self, traverser: Player, reach: &[f64], payoff: f64, values: &mut [f64]) {
        let (mine, theirs) = self.check_lengths(traverser, reach, values);
        let all = ReachSum::of(theirs, reach);
        let same = &self.same[traverser.index()];
        for ((value, &combo), same) in values.iter_mut().zip(mine.combos()).zip(same) {
            *value = payoff * (all.without_cards_of(combo) + same_reach(*same, reach));
        }
    }

    /// Writes the traverser's value for each of their hands at a showdown:
    ///
    /// ```text
    /// values[h] = Σ reach[o] · u(h, o)    over the opponent's hands o that share no card with h
    /// ```
    ///
    /// where `u(h, o)` is `payoff.win`, `payoff.tie` or `payoff.lose` as `h` makes a stronger,
    /// equal or weaker hand than `o` with the board. `reach` is the opponent's reach by opponent
    /// hand index and `values` is written by traverser hand index. Takes O(n) time and allocates
    /// nothing.
    ///
    /// Panics unless the board is a river of five cards, `reach` has one entry per opponent hand
    /// and `values` one per traverser hand.
    pub fn showdown_values(
        &self,
        traverser: Player,
        reach: &[f64],
        payoff: ShowdownPayoff,
        values: &mut [f64],
    ) {
        let ranked = self
            .ranked
            .as_ref()
            .expect("a showdown needs a river board of five cards");
        let (mine, theirs) = self.check_lengths(traverser, reach, values);
        let (me, them) = (traverser.index(), traverser.opponent().index());
        let same = &self.same[me];
        let all = ReachSum::of(theirs, reach);

        // The opponent's hands added so far: those weaker than the current block, and then
        // also those that tie it.
        let mut below = ReachSum::default();
        let their_order = &ranked[them];
        let mut next = 0;
        for block in ranked[me].chunk_by(|a, b| a.0 == b.0) {
            let strength = block[0].0;
            while let Some(&(s, o)) = their_order.get(next)
                && s < strength
            {
                below.add(theirs.combos()[o as usize], reach[o as usize]);
                next += 1;
            }
            // `values` holds W(h), the disjoint reach of weaker hands, until the block is done.
            for &(_, h) in block {
                let h = h as usize;
                values[h] = below.without_cards_of(mine.combos()[h]);
            }
            while let Some(&(s, o)) = their_order.get(next)
                && s == strength
            {
                below.add(theirs.combos()[o as usize], reach[o as usize]);
                next += 1;
            }
            for &(_, h) in block {
                let h = h as usize;
                let combo = mine.combos()[h];
                let identical = same_reach(same[h], reach);
                let weaker = values[h];
                let not_stronger = below.without_cards_of(combo) + identical;
                let disjoint = all.without_cards_of(combo) + identical;
                values[h] = payoff.win * weaker
                    + payoff.tie * (not_stronger - weaker)
                    + payoff.lose * (disjoint - not_stronger);
            }
        }
    }

    /// The traverser's and the opponent's hand lists, after checking the slice lengths.
    fn check_lengths(
        &self,
        traverser: Player,
        reach: &[f64],
        values: &[f64],
    ) -> (&HandList, &HandList) {
        let mine = &self.hands[traverser.index()];
        let theirs = &self.hands[traverser.opponent().index()];
        assert_eq!(
            reach.len(),
            theirs.len(),
            "the reach needs one entry per opponent hand"
        );
        assert_eq!(
            values.len(),
            mine.len(),
            "the values need one entry per traverser hand"
        );
        (mine, theirs)
    }
}

/// The reach of the opponent's copy of a traverser hand, or 0 if the opponent does not hold it.
fn same_reach(same: Option<u32>, reach: &[f64]) -> f64 {
    same.map_or(0.0, |o| reach[o as usize])
}

/// Reach summed over a set of hands, in total and for each card.
#[derive(Clone)]
struct ReachSum {
    total: f64,
    /// `by_card[c]` sums the reach of the hands that hold card `c`.
    by_card: [f64; NUM_CARDS],
}

impl Default for ReachSum {
    fn default() -> ReachSum {
        ReachSum {
            total: 0.0,
            by_card: [0.0; NUM_CARDS],
        }
    }
}

impl ReachSum {
    /// The sums over every hand of a list.
    fn of(hands: &HandList, reach: &[f64]) -> ReachSum {
        let mut sum = ReachSum::default();
        for (&combo, &r) in hands.combos().iter().zip(reach) {
            sum.add(combo, r);
        }
        sum
    }

    fn add(&mut self, combo: Combo, reach: f64) {
        self.total += reach;
        self.by_card[combo.lo().index()] += reach;
        self.by_card[combo.hi().index()] += reach;
    }

    /// The total less the reach of every hand that holds either card of `combo`. A hand that
    /// holds both, which can only be `combo` itself, is taken off twice.
    fn without_cards_of(&self, combo: Combo) -> f64 {
        self.total - self.by_card[combo.lo().index()] - self.by_card[combo.hi().index()]
    }
}

/// For each hand of `from`, the index in `to` of the same combo, if `to` holds it.
fn same_combos(from: &HandList, to: &HandList) -> Vec<Option<u32>> {
    from.combos()
        .iter()
        .map(|&combo| to.position(combo).map(|o| o as u32))
        .collect()
}

/// The `(strength, hand index)` pairs of a list on a river board, from weakest to strongest.
/// Pairs are distinct, so the order is fully determined: equal strengths stay in hand order.
fn ranked_hands(hands: &HandList) -> Vec<(HandRank, u32)> {
    let mut ranked: Vec<(HandRank, u32)> = hands
        .combos()
        .iter()
        .enumerate()
        .map(|(h, combo)| (evaluate(hands.board() | combo.cards()), h as u32))
        .collect();
    ranked.sort_unstable();
    ranked
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

    fn range(s: &str) -> Range {
        s.parse().unwrap()
    }

    /// A reach vector over a hand list, zero except for the combos named.
    fn reach_of(hands: &HandList, entries: &[(&str, f64)]) -> Vec<f64> {
        let mut reach = vec![0.0; hands.len()];
        for &(name, r) in entries {
            let hand = hands
                .position(combo(name))
                .unwrap_or_else(|| panic!("{name} is not in the list"));
            reach[hand] = r;
        }
        reach
    }

    /// The value written for one combo.
    fn value_of(hands: &HandList, values: &[f64], name: &str) -> f64 {
        values[hands.position(combo(name)).unwrap()]
    }

    fn fold(board: &BoardHands, traverser: Player, reach: &[f64], payoff: f64) -> Vec<f64> {
        let mut values = vec![f64::NAN; board.hands(traverser).len()];
        board.fold_values(traverser, reach, payoff, &mut values);
        values
    }

    fn showdown(
        board: &BoardHands,
        traverser: Player,
        reach: &[f64],
        payoff: ShowdownPayoff,
    ) -> Vec<f64> {
        let mut values = vec![f64::NAN; board.hands(traverser).len()];
        board.showdown_values(traverser, reach, payoff, &mut values);
        values
    }

    /// Distinct payoffs, so that a hand counted as the wrong result shows.
    const PAYOFF: ShowdownPayoff = ShowdownPayoff {
        win: 100.0,
        tie: 7.0,
        lose: -50.0,
    };

    #[test]
    fn every_pair_ties_on_a_board_that_plays_for_everyone() {
        // A royal flush on the board: every hand plays it.
        let board = BoardHands::new(cards("AsKsQsJsTs"), &range("2d2c,KK"), &range("22,3d3c"));
        let (oop, ip) = (board.hands(Player::Oop), board.hands(Player::Ip));
        let reach = reach_of(ip, &[("2h2c", 0.5), ("2s2h", 0.25), ("3d3c", 0.125)]);
        let values = showdown(&board, Player::Oop, &reach, PAYOFF);
        // 2d2c meets 2s2h and 3d3c; 2h2c shares the 2c.
        assert_eq!(value_of(oop, &values, "2d2c"), 7.0 * 0.375);
        // KhKd meets all three.
        assert_eq!(value_of(oop, &values, "KhKd"), 7.0 * 0.875);

        let reach = reach_of(oop, &[("2d2c", 1.0), ("KhKd", 0.5)]);
        let values = showdown(&board, Player::Ip, &reach, PAYOFF);
        for (name, expected) in [("2h2c", 0.5), ("2s2h", 1.5), ("2d2c", 0.5), ("3d3c", 1.5)] {
            assert_eq!(value_of(ip, &values, name), 7.0 * expected, "{name}");
        }
    }

    #[test]
    fn a_showdown_counts_wins_ties_and_losses_by_eye() {
        let board = BoardHands::new(cards("Ks Th 8h 3d 2c"), &range("AcAd"), &range("AA,KK,QQ"));
        let ip = board.hands(Player::Ip);
        let reach = reach_of(ip, &[("AsAh", 0.5), ("KdKc", 0.25), ("QdQc", 1.0)]);
        let values = showdown(&board, Player::Oop, &reach, PAYOFF);
        // Aces beat queens, tie the other aces and lose to a set of kings.
        assert_eq!(values, vec![100.0 * 1.0 + 7.0 * 0.5 - 50.0 * 0.25]);
    }

    #[test]
    fn hands_that_share_a_card_do_not_meet() {
        let board = BoardHands::new(
            cards("Kc8h5d3s2c"),
            &range("AhAd"),
            &range("AhKh,QdQc,8d8c"),
        );
        let ip = board.hands(Player::Ip);
        let reach = reach_of(ip, &[("AhKh", 1.0), ("QdQc", 0.5), ("8d8c", 0.25)]);
        // AhKh shares the Ah, so only the queens (beaten) and the set of eights (winning) count.
        assert_eq!(
            showdown(&board, Player::Oop, &reach, PAYOFF),
            vec![100.0 * 0.5 - 50.0 * 0.25]
        );
        assert_eq!(fold(&board, Player::Oop, &reach, 30.0), vec![30.0 * 0.75]);

        let oop_reach = vec![0.5];
        let values = showdown(&board, Player::Ip, &oop_reach, PAYOFF);
        assert_eq!(value_of(ip, &values, "AhKh"), 0.0);
        assert_eq!(value_of(ip, &values, "QdQc"), -50.0 * 0.5);
        assert_eq!(value_of(ip, &values, "8d8c"), 100.0 * 0.5);
    }

    #[test]
    fn the_same_combo_in_both_ranges_is_blocked_by_itself() {
        let board = BoardHands::new(
            cards("Qs9h6d4c2h"),
            &range("AdAc"),
            &range("AdAc,AsAh,KdKc"),
        );
        let ip = board.hands(Player::Ip);
        let reach = reach_of(ip, &[("AdAc", 0.5), ("AsAh", 0.125), ("KdKc", 0.25)]);
        // IP's AdAc is OOP's own hand. It ties, but it cannot meet itself.
        assert_eq!(
            showdown(&board, Player::Oop, &reach, PAYOFF),
            vec![100.0 * 0.25 + 7.0 * 0.125]
        );
        assert_eq!(
            fold(&board, Player::Oop, &reach, -10.0),
            vec![-10.0 * 0.375]
        );

        let values = showdown(&board, Player::Ip, &[1.0], PAYOFF);
        assert_eq!(value_of(ip, &values, "AdAc"), 0.0);
        assert_eq!(value_of(ip, &values, "AsAh"), 7.0);
        assert_eq!(value_of(ip, &values, "KdKc"), -50.0);
        let values = fold(&board, Player::Ip, &[1.0], 10.0);
        assert_eq!(value_of(ip, &values, "AdAc"), 0.0);
        assert_eq!(value_of(ip, &values, "AsAh"), 10.0);
    }

    #[test]
    fn folds_work_on_flop_and_turn_boards() {
        let oop_range = range("AhAd,KhQh");
        let ip_range = range("AhKd,QhJh,7h7c");
        let flop = BoardHands::new(cards("Ks7d2c"), &oop_range, &ip_range);
        let reach = reach_of(
            flop.hands(Player::Ip),
            &[("AhKd", 0.5), ("QhJh", 0.25), ("7h7c", 1.0)],
        );
        let oop = flop.hands(Player::Oop);
        let values = fold(&flop, Player::Oop, &reach, -200.0);
        // AhAd is blocked from AhKd, and KhQh from QhJh.
        assert_eq!(value_of(oop, &values, "AhAd"), -200.0 * 1.25);
        assert_eq!(value_of(oop, &values, "KhQh"), -200.0 * 1.5);

        // The 7h on the turn takes 7h7c out of IP's range.
        let turn = BoardHands::new(cards("Ks7d2c7h"), &oop_range, &ip_range);
        assert_eq!(turn.hands(Player::Ip).len(), 2);
        let reach = reach_of(turn.hands(Player::Ip), &[("AhKd", 0.5), ("QhJh", 0.25)]);
        let oop = turn.hands(Player::Oop);
        let values = fold(&turn, Player::Oop, &reach, -200.0);
        assert_eq!(value_of(oop, &values, "AhAd"), -200.0 * 0.25);
        assert_eq!(value_of(oop, &values, "KhQh"), -200.0 * 0.5);
    }

    #[test]
    fn an_empty_range_gives_no_values_or_zero_values() {
        // Every OOP combo holds the As, which is on the board.
        let board = BoardHands::new(cards("As7d2c9h3s"), &range("AsKs,AsQs"), &range("QQ"));
        assert!(board.hands(Player::Oop).is_empty());
        let mut none: [f64; 0] = [];
        board.fold_values(Player::Oop, &[1.0; 6], 5.0, &mut none);
        board.showdown_values(Player::Oop, &[1.0; 6], PAYOFF, &mut none);
        assert_eq!(fold(&board, Player::Ip, &[], 5.0), vec![0.0; 6]);
        assert_eq!(showdown(&board, Player::Ip, &[], PAYOFF), vec![0.0; 6]);
    }

    #[test]
    fn the_cross_index_finds_the_same_combo_in_the_other_list() {
        let board = BoardHands::new(cards("Ks7d2c"), &range("AA,KK"), &range("AKs,AA:0.5"));
        for player in [Player::Oop, Player::Ip] {
            let (mine, theirs) = (board.hands(player), board.hands(player.opponent()));
            for (h, &combo) in mine.combos().iter().enumerate() {
                let same = board.same[player.index()][h].map(|o| o as usize);
                assert_eq!(same, theirs.position(combo), "{combo}");
                if let Some(o) = same {
                    assert_eq!(theirs.combos()[o], combo);
                }
            }
        }
        // The six aces are the only combos both players hold.
        assert_eq!(board.same[0].iter().flatten().count(), 6);
    }

    #[test]
    fn river_hands_are_sorted_from_weakest_to_strongest() {
        let board = BoardHands::new(cards("Ks7d2c9h3s"), &Range::full(), &range("AA,72o,KsQs"));
        let ranked = board.ranked.as_ref().unwrap();
        for player in [Player::Oop, Player::Ip] {
            let hands = board.hands(player);
            let order = &ranked[player.index()];
            assert_eq!(order.len(), hands.len());
            assert!(order.windows(2).all(|pair| pair[0] < pair[1]));
            for &(strength, h) in order {
                let combo = hands.combos()[h as usize];
                assert_eq!(strength, evaluate(board.board() | combo.cards()));
            }
        }
        let turn = BoardHands::new(cards("Ks7d2c9h"), &Range::full(), &Range::full());
        assert!(turn.ranked.is_none());
    }

    #[test]
    #[should_panic(expected = "river board")]
    fn a_showdown_on_the_turn_is_rejected() {
        let board = BoardHands::new(cards("Ks7d2c9h"), &range("AA"), &range("KK"));
        let reach = vec![1.0; board.hands(Player::Ip).len()];
        showdown(&board, Player::Oop, &reach, PAYOFF);
    }

    #[test]
    #[should_panic(expected = "one entry per opponent hand")]
    fn a_reach_of_the_wrong_length_is_rejected() {
        let board = BoardHands::new(cards("Ks7d2c"), &range("AA"), &range("QQ"));
        fold(&board, Player::Oop, &[1.0; 5], 1.0);
    }

    #[test]
    #[should_panic(expected = "3 to 5 cards")]
    fn a_board_of_six_cards_is_rejected() {
        BoardHands::new(cards("Ks7d2c9h3s4s"), &range("AA"), &range("QQ"));
    }
}
