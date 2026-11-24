//! Suit isomorphism: relabelling the four suits.
//!
//! Hold'em treats the suits alike. Relabel them consistently, say by swapping hearts and spades
//! on the board and in every hand, and a situation turns into one that plays exactly the same.
//! This module is the bookkeeping that lets a solver solve one situation from each such class and
//! reuse the result for the others. It is combinatorics only, and knows nothing of betting.
//!
//! - [`SuitPermutation`] is one of the 24 relabellings. It maps cards, card sets, combos, ranges
//!   and boards.
//! - [`SuitGroup`] is a set of relabellings closed under composition and inverse, such as the
//!   symmetries of a board.
//! - [`canonical`] picks one fixed member of each class of card sets.
//! - [`DealtBoard`] is a board street by street. Its [`symmetries`](DealtBoard::symmetries) are
//!   the relabellings that keep every street as it was dealt, and
//!   [`next_cards`](DealtBoard::next_cards) sorts the cards that can come next into
//!   [`NextCards`] classes that lead to equivalent subgames.

use std::fmt;

use thiserror::Error;

use crate::card::{Card, CardSet, NUM_CARDS, NUM_SUITS};
use crate::combo::Combo;
use crate::range::Range;

/// The number of ways to relabel the four suits.
pub const NUM_SUIT_PERMUTATIONS: usize = 24;

/// The letter of each suit in suit order, as [`Card`] reads and writes them.
const SUIT_CHARS: [char; NUM_SUITS] = ['c', 'd', 'h', 's'];

/// The clubs, one bit for each rank: bit `4 * rank` of a [`CardSet`]. Shifted up by `suit`, it
/// gives the cards of that suit.
const CLUBS: u64 = 0x1_1111_1111_1111;

/// Whether `images` holds each of the suits 0 to 3 exactly once.
const fn is_permutation(images: [u8; NUM_SUITS]) -> bool {
    let mut seen = 0u8;
    let mut suit = 0;
    while suit < NUM_SUITS {
        if images[suit] >= NUM_SUITS as u8 {
            return false;
        }
        seen |= 1 << images[suit];
        suit += 1;
    }
    seen == 0b1111
}

/// The images of every permutation, in lexicographic order.
const PERMUTATIONS: [[u8; NUM_SUITS]; NUM_SUIT_PERMUTATIONS] = {
    let mut table = [[0; NUM_SUITS]; NUM_SUIT_PERMUTATIONS];
    let mut count = 0;
    // Read `code` as four base-4 digits, most significant first. Counting up visits every
    // sequence of four suits in lexicographic order; keep those that are permutations.
    let mut code = 0u32;
    while code < 256 {
        let images = [
            (code >> 6) as u8 & 3,
            (code >> 4) as u8 & 3,
            (code >> 2) as u8 & 3,
            code as u8 & 3,
        ];
        if is_permutation(images) {
            table[count] = images;
            count += 1;
        }
        code += 1;
    }
    assert!(count == NUM_SUIT_PERMUTATIONS);
    table
};

/// A relabelling of the four suits: suit `s` becomes suit `p.image(s)`.
///
/// A permutation maps a card by keeping its rank and relabelling its suit, and maps card sets,
/// combos, ranges and boards card by card. The 24 permutations are numbered 0 to 23 in
/// lexicographic order of their images `[p(c), p(d), p(h), p(s)]`, which is also the order of
/// `Ord`. So 0 is the identity and 23 reverses the suits.
///
/// They are written in cycle notation: `(h s)` swaps hearts and spades, `(c d h)` sends clubs to
/// diamonds, diamonds to hearts and hearts to clubs, and `()` is the identity. Each cycle starts
/// at its lowest suit, and the cycles are in order of that suit.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SuitPermutation([u8; NUM_SUITS]);

impl SuitPermutation {
    /// The permutation that keeps every suit.
    pub const IDENTITY: SuitPermutation = SuitPermutation([0, 1, 2, 3]);

    /// The permutation that sends each suit `s` to `images[s]`.
    ///
    /// # Panics
    ///
    /// If `images` does not hold each of the suits 0 to 3 exactly once.
    pub const fn new(images: [u8; NUM_SUITS]) -> SuitPermutation {
        assert!(
            is_permutation(images),
            "the images must hold each suit exactly once"
        );
        SuitPermutation(images)
    }

    /// The permutation that exchanges suits `a` and `b` and keeps the other two. It is the
    /// identity when `a == b`.
    pub const fn swap(a: u8, b: u8) -> SuitPermutation {
        assert!(a < NUM_SUITS as u8 && b < NUM_SUITS as u8);
        let mut images = [0, 1, 2, 3];
        images[a as usize] = b;
        images[b as usize] = a;
        SuitPermutation(images)
    }

    pub const fn from_index(index: usize) -> SuitPermutation {
        assert!(index < NUM_SUIT_PERMUTATIONS);
        SuitPermutation(PERMUTATIONS[index])
    }

    /// The position from 0 to 23 in lexicographic order of the images.
    pub const fn index(self) -> usize {
        // The Lehmer code: for each suit in turn, how many of the later suits have a smaller
        // image, read as a number with digits in base 4, 3, 2 and 1.
        let mut index = 0;
        let mut i = 0;
        while i < NUM_SUITS {
            let mut smaller = 0;
            let mut j = i + 1;
            while j < NUM_SUITS {
                if self.0[j] < self.0[i] {
                    smaller += 1;
                }
                j += 1;
            }
            index = index * (NUM_SUITS - i) + smaller;
            i += 1;
        }
        index
    }

    /// All 24 permutations in index order.
    pub fn all() -> impl Iterator<Item = SuitPermutation> {
        PERMUTATIONS.into_iter().map(SuitPermutation)
    }

    /// The suit that each suit becomes, in suit order.
    pub const fn images(self) -> [u8; NUM_SUITS] {
        self.0
    }

    /// The suit that `suit` becomes.
    pub const fn image(self, suit: u8) -> u8 {
        self.0[suit as usize]
    }

    pub const fn is_identity(self) -> bool {
        self.index() == 0
    }

    /// This permutation followed by `next`: suit `s` becomes `next.image(self.image(s))`.
    pub const fn then(self, next: SuitPermutation) -> SuitPermutation {
        let mut images = [0; NUM_SUITS];
        let mut suit = 0;
        while suit < NUM_SUITS {
            images[suit] = next.0[self.0[suit] as usize];
            suit += 1;
        }
        SuitPermutation(images)
    }

    /// The permutation that undoes this one.
    pub const fn inverse(self) -> SuitPermutation {
        let mut images = [0; NUM_SUITS];
        let mut suit = 0;
        while suit < NUM_SUITS {
            images[self.0[suit] as usize] = suit as u8;
            suit += 1;
        }
        SuitPermutation(images)
    }

    /// The card of the same rank in the relabelled suit.
    pub const fn apply_card(self, card: Card) -> Card {
        Card::new(card.rank(), self.image(card.suit()))
    }

    /// The set of the images of the cards in `cards`.
    pub const fn apply_set(self, cards: CardSet) -> CardSet {
        // Move each suit's cards, as a block, to the suit they become.
        let bits = cards.bits();
        let mut image = 0;
        let mut suit = 0;
        while suit < NUM_SUITS {
            image |= ((bits >> suit) & CLUBS) << self.0[suit];
            suit += 1;
        }
        CardSet::from_bits(image)
    }

    /// The combo of the images of the two cards.
    pub fn apply_combo(self, combo: Combo) -> Combo {
        Combo::new(self.apply_card(combo.lo()), self.apply_card(combo.hi()))
    }

    /// The range in which each combo's weight has moved to the combo's image. So the weight of
    /// `self.apply_combo(c)` in the result is the weight of `c` in `range`.
    pub fn apply_range(self, range: &Range) -> Range {
        let mut image = Range::empty();
        for combo in Combo::all() {
            image.set(self.apply_combo(combo), range.weight(combo));
        }
        image
    }

    /// The board with each street mapped: the starting cards as a set, then each later card.
    pub fn apply_board(self, board: DealtBoard) -> DealtBoard {
        DealtBoard {
            start: self.apply_set(board.start),
            later: board
                .later
                .map(|card| card.map(|card| self.apply_card(card))),
        }
    }

    /// Whether `range` is unchanged by this permutation: every combo has the weight of its
    /// image. This compares all 1,326 combos, including any the board blocks; see
    /// [`DealtBoard::symmetries_with_ranges`] for the comparison that leaves those out.
    pub fn fixes_range(self, range: &Range) -> bool {
        Combo::all().all(|combo| range.weight(self.apply_combo(combo)) == range.weight(combo))
    }
}

impl Default for SuitPermutation {
    fn default() -> SuitPermutation {
        SuitPermutation::IDENTITY
    }
}

impl fmt::Display for SuitPermutation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_identity() {
            return write!(f, "()");
        }
        let mut written = 0u8;
        for start in 0..NUM_SUITS as u8 {
            if written & (1 << start) != 0 || self.image(start) == start {
                continue;
            }
            write!(f, "(")?;
            let mut suit = start;
            loop {
                written |= 1 << suit;
                write!(f, "{}", SUIT_CHARS[suit as usize])?;
                suit = self.image(suit);
                if suit == start {
                    break;
                }
                write!(f, " ")?;
            }
            write!(f, ")")?;
        }
        Ok(())
    }
}

impl fmt::Debug for SuitPermutation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

const fn bit(permutation: SuitPermutation) -> u32 {
    1 << permutation.index()
}

/// A group of suit permutations: a set that holds the identity and is closed under composition
/// and inverse.
///
/// Every way to build one gives a group, so the cards a group can map onto one another always
/// fall into proper classes. It is stored as a 24-bit mask over the permutation indexes.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct SuitGroup(u32);

impl SuitGroup {
    /// All 24 permutations.
    pub const fn full() -> SuitGroup {
        SuitGroup((1 << NUM_SUIT_PERMUTATIONS) - 1)
    }

    /// The identity alone.
    pub const fn trivial() -> SuitGroup {
        SuitGroup(bit(SuitPermutation::IDENTITY))
    }

    /// The smallest group that holds every permutation in `generators`.
    pub fn generated_by(generators: impl IntoIterator<Item = SuitPermutation>) -> SuitGroup {
        let mut members = generators
            .into_iter()
            .fold(SuitGroup::trivial().0, |members, p| members | bit(p));
        // A finite set closed under composition also holds every inverse, since the inverse of
        // a permutation is one of its powers.
        loop {
            let group = SuitGroup(members);
            let mut closed = members;
            for a in group.iter() {
                for b in group.iter() {
                    closed |= bit(a.then(b));
                }
            }
            if closed == members {
                return group;
            }
            members = closed;
        }
    }

    /// The permutations that map `cards` onto itself.
    pub fn stabilizer(cards: CardSet) -> SuitGroup {
        SuitGroup::full().filter(|p| p.apply_set(cards) == cards)
    }

    /// The permutations in both groups.
    pub const fn intersection(self, other: SuitGroup) -> SuitGroup {
        SuitGroup(self.0 & other.0)
    }

    pub const fn contains(self, permutation: SuitPermutation) -> bool {
        self.0 & bit(permutation) != 0
    }

    /// The number of permutations in the group, from 1 to 24.
    pub const fn order(self) -> usize {
        self.0.count_ones() as usize
    }

    /// Whether the identity is the only member.
    pub const fn is_trivial(self) -> bool {
        self.0 == SuitGroup::trivial().0
    }

    /// The members in index order, so the identity comes first.
    pub fn iter(self) -> impl Iterator<Item = SuitPermutation> {
        (0..NUM_SUIT_PERMUTATIONS)
            .filter(move |&index| self.0 & (1 << index) != 0)
            .map(SuitPermutation::from_index)
    }

    /// The cards that the members map `card` to.
    pub fn orbit(self, card: Card) -> CardSet {
        self.iter().map(|p| p.apply_card(card)).collect()
    }

    /// The members for which `keep` holds. The condition must make the result a group, as
    /// "maps this to itself" does.
    fn filter(self, keep: impl Fn(SuitPermutation) -> bool) -> SuitGroup {
        let members = self
            .iter()
            .filter(|&p| keep(p))
            .fold(0, |members, p| members | bit(p));
        let group = SuitGroup(members);
        debug_assert!(group.is_group(), "{group} is not a group");
        group
    }

    /// Whether the members hold the identity and every composition of two members.
    fn is_group(self) -> bool {
        self.contains(SuitPermutation::IDENTITY)
            && self
                .iter()
                .all(|a| self.iter().all(|b| self.contains(a.then(b))))
    }
}

/// The members in index order, such as `{(), (h s)}`.
impl fmt::Display for SuitGroup {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let members: Vec<String> = self.iter().map(|p| p.to_string()).collect();
        write!(f, "{{{}}}", members.join(", "))
    }
}

impl fmt::Debug for SuitGroup {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SuitGroup{self}")
    }
}

/// The canonical form of a set of cards under suit relabelling, and a permutation that maps the
/// set onto it.
///
/// The canonical form is the image `p.apply_set(cards)`, over all 24 permutations `p`, whose
/// [`CardSet::bits`] is smallest as an unsigned integer. Card `4 * rank + suit` is bit
/// `4 * rank + suit`, so this is the image whose cards, listed from the highest index down, come
/// first in lexicographic order: the highest card is as low as it can be, then the next, and so
/// on. Clubs are suit 0, so the highest card of a canonical form is a club. Two sets have the
/// same canonical form exactly when some relabelling maps one onto the other.
///
/// The permutation returned is the first in index order that maps `cards` onto its canonical
/// form. More than one does when the set has symmetries of its own.
pub fn canonical(cards: CardSet) -> (CardSet, SuitPermutation) {
    let mut best = (cards, SuitPermutation::IDENTITY);
    for p in SuitPermutation::all().skip(1) {
        let image = p.apply_set(cards);
        if image.bits() < best.0.bits() {
            best = (image, p);
        }
    }
    best
}

/// Why a [`DealtBoard`] cannot be started or dealt to.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum BoardError {
    #[error("a board starts with 3 to 5 cards, not {0}")]
    StartSize(usize),
    #[error("card `{0}` is already on the board")]
    Duplicate(Card),
    #[error("the board already has five cards")]
    Complete,
}

/// A board as it was dealt: the cards on it when play began, then each later street's card in
/// order.
///
/// A flop spot starts with the three flop cards, a turn spot with four cards and a river spot
/// with five. The starting cards are a set, since no one acted between them. Each later card
/// comes after a round of betting, so the history tells it apart from the cards before it, and
/// the turn and the river of a flop spot are two separate streets.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct DealtBoard {
    start: CardSet,
    later: [Option<Card>; 2],
}

impl DealtBoard {
    /// The board when play begins: 3 to 5 cards.
    pub fn new(start: CardSet) -> Result<DealtBoard, BoardError> {
        if !(3..=5).contains(&start.len()) {
            return Err(BoardError::StartSize(start.len()));
        }
        Ok(DealtBoard {
            start,
            later: [None; 2],
        })
    }

    /// The board after the next street brings `card`.
    pub fn deal(self, card: Card) -> Result<DealtBoard, BoardError> {
        if self.is_complete() {
            return Err(BoardError::Complete);
        }
        if self.cards().contains(card) {
            return Err(BoardError::Duplicate(card));
        }
        let mut board = self;
        let slot = board
            .later
            .iter_mut()
            .find(|slot| slot.is_none())
            .expect("a board of fewer than five cards has room for another");
        *slot = Some(card);
        Ok(board)
    }

    /// The cards on the board when play began.
    pub fn start(self) -> CardSet {
        self.start
    }

    /// The cards dealt since play began, one per street, in order.
    pub fn later_cards(self) -> impl Iterator<Item = Card> {
        self.later.into_iter().flatten()
    }

    /// Every card on the board.
    pub fn cards(self) -> CardSet {
        self.later_cards()
            .fold(self.start, |cards, card| cards | CardSet::single(card))
    }

    /// Whether all five cards are out.
    pub fn is_complete(self) -> bool {
        self.cards().len() == 5
    }

    /// The relabellings that keep every street as it was dealt: those that map the starting
    /// cards onto themselves as a set and keep each later card.
    ///
    /// This can be smaller than the set of relabellings that map all the cards onto themselves,
    /// because the players acted on each street before the next card came. On a flop of
    /// 7s 7h 2c with a turn of 7d, swapping hearts and diamonds keeps the four cards but swaps
    /// the turn with a flop card, which gives a different history: a flop of 7s 7d 2c and a turn
    /// of 7h. Only the identity and swapping hearts with spades keep this one.
    ///
    /// Equivalently, these are the relabellings that map the cards on the board onto themselves
    /// after every street, since one that keeps the cards before a street and after it must
    /// keep that street's card.
    pub fn symmetries(self) -> SuitGroup {
        SuitGroup::full().filter(|p| p.apply_board(self) == self)
    }

    /// The symmetries of the board that also leave each of `ranges` unchanged on the combos
    /// that do not share a card with the board.
    ///
    /// Cards dealt next can only share a subgame when the ranges are symmetric too. A range
    /// typed as hand classes, such as `QQ` or `AKs`, is unchanged by every relabelling, but one
    /// that lists a specific combo, such as `AsKs:0.5`, is changed by every relabelling that
    /// moves spades. A combo that shares a card with the board is dead, and a symmetry of the
    /// board maps it to another dead combo, so the weights of dead combos are not compared.
    ///
    /// The result is a group: each range adds the condition that the board's group, which
    /// permutes the live combos, maps that range's weights on them to themselves.
    pub fn symmetries_with_ranges<'a>(
        self,
        ranges: impl IntoIterator<Item = &'a Range>,
    ) -> SuitGroup {
        let board = self.cards();
        let live: Vec<Combo> = Combo::all()
            .filter(|combo| !combo.intersects(board))
            .collect();
        let ranges: Vec<&Range> = ranges.into_iter().collect();
        self.symmetries().filter(|p| {
            ranges.iter().all(|range| {
                live.iter()
                    .all(|&combo| range.weight(p.apply_combo(combo)) == range.weight(combo))
            })
        })
    }

    /// The cards that can come next, in classes under the board's symmetries.
    ///
    /// # Panics
    ///
    /// If the board already has five cards.
    pub fn next_cards(self) -> NextCards {
        self.next_cards_within(SuitGroup::full())
    }

    /// The cards that can come next, in classes under the board's symmetries that are also in
    /// `allowed`, such as the group from [`symmetries_with_ranges`](Self::symmetries_with_ranges).
    ///
    /// # Panics
    ///
    /// If the board already has five cards.
    pub fn next_cards_within(self, allowed: SuitGroup) -> NextCards {
        assert!(!self.is_complete(), "no card comes after the river");
        NextCards::new(self.cards(), self.symmetries().intersection(allowed))
    }
}

/// The starting cards, then each later card, such as `7s7h2c 7d`.
impl fmt::Display for DealtBoard {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.start)?;
        for card in self.later_cards() {
            write!(f, " {card}")?;
        }
        Ok(())
    }
}

impl fmt::Debug for DealtBoard {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "DealtBoard({self})")
    }
}

/// Cards that a symmetry of the board maps onto one another, so that dealing any of them leads
/// to the same subgame up to relabelling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CardClass {
    representative: Card,
    members: CardSet,
}

impl CardClass {
    /// The member with the lowest index, which stands for the class.
    pub fn representative(&self) -> Card {
        self.representative
    }

    /// Every card in the class, the representative included.
    pub fn members(&self) -> CardSet {
        self.members
    }

    /// The number of cards in the class.
    pub fn multiplicity(&self) -> usize {
        self.members.len()
    }
}

/// The cards that can come next on a board, in classes.
///
/// Two live cards are in one class when a permutation in the group maps one to the other. So a
/// solver can deal each class's representative only, count it as many times as the class has
/// cards, and read a hand's result after any other card `c` from the representative's subgame,
/// at the hand mapped through [`to_representative(c)`](Self::to_representative).
#[derive(Clone, PartialEq, Eq)]
pub struct NextCards {
    group: SuitGroup,
    live: CardSet,
    classes: Vec<CardClass>,
    class_of: [Option<u8>; NUM_CARDS],
    to_representative: [Option<SuitPermutation>; NUM_CARDS],
}

impl NextCards {
    /// The cards not in `dead`, in classes under `group`, which must map `dead` onto itself.
    fn new(dead: CardSet, group: SuitGroup) -> NextCards {
        let live = CardSet::FULL - dead;
        let mut next = NextCards {
            group,
            live,
            classes: Vec::new(),
            class_of: [None; NUM_CARDS],
            to_representative: [None; NUM_CARDS],
        };
        // Cards come in index order, so the first card of a class to come is its lowest.
        for card in live {
            if next.class_of[card.index()].is_some() {
                continue;
            }
            let members = group.orbit(card);
            debug_assert!(!members.intersects(dead), "the group moves a dead card");
            let class = next.classes.len() as u8;
            for member in members {
                next.class_of[member.index()] = Some(class);
                next.to_representative[member.index()] =
                    group.iter().find(|p| p.apply_card(member) == card);
            }
            next.classes.push(CardClass {
                representative: card,
                members,
            });
        }
        next
    }

    /// The group the classes are taken under.
    pub fn group(&self) -> SuitGroup {
        self.group
    }

    /// The cards that are not on the board.
    pub fn live_cards(&self) -> CardSet {
        self.live
    }

    /// The classes in order of their representatives. Their multiplicities sum to the number of
    /// live cards.
    pub fn classes(&self) -> &[CardClass] {
        &self.classes
    }

    /// The class of a live card, or `None` for a card on the board.
    pub fn class_of(&self, card: Card) -> Option<&CardClass> {
        self.class_of[card.index()].map(|class| &self.classes[class as usize])
    }

    /// The permutation in the group that maps a live card to its class's representative, or
    /// `None` for a card on the board. It is the first such permutation in index order, so it is
    /// the identity for a representative. It is a symmetry of the board.
    pub fn to_representative(&self, card: Card) -> Option<SuitPermutation> {
        self.to_representative[card.index()]
    }
}

impl fmt::Debug for NextCards {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NextCards")
            .field("group", &self.group)
            .field("classes", &self.classes)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use proptest::prelude::*;

    use super::*;
    use crate::card::NUM_RANKS;
    use crate::combo::{HandClass, NUM_CLASSES, NUM_COMBOS};
    use crate::equity::{Equity, EquityError, range_vs_range};

    const CLUB: u8 = 0;
    const DIAMOND: u8 = 1;
    const HEART: u8 = 2;
    const SPADE: u8 = 3;

    fn card(s: &str) -> Card {
        s.parse().unwrap()
    }

    fn cards(s: &str) -> CardSet {
        s.parse().unwrap()
    }

    fn range(s: &str) -> Range {
        s.parse().unwrap()
    }

    fn combo(s: &str) -> Combo {
        s.parse().unwrap()
    }

    /// A board that starts with `start` and then deals each of `later` in order.
    fn board(start: &str, later: &[&str]) -> DealtBoard {
        later
            .iter()
            .fold(DealtBoard::new(cards(start)).unwrap(), |board, &c| {
                board.deal(card(c)).unwrap()
            })
    }

    fn binomial(n: u64, k: u64) -> u64 {
        (0..k).fold(1, |product, i| product * (n - i) / (i + 1))
    }

    /// Calls `visit` with every set of `size` cards, in increasing order of bit mask.
    ///
    /// It steps from one mask to the next with the same number of bits set: add the lowest set
    /// bit, which carries through the lowest run of ones, then put the bits of that run that
    /// the carry cleared, less one, back at the bottom.
    fn for_each_set(size: usize, mut visit: impl FnMut(CardSet)) {
        assert!(size > 0);
        let mut bits: u64 = (1 << size) - 1;
        let mut count = 0;
        while bits >> NUM_CARDS == 0 {
            visit(CardSet::from_bits(bits));
            count += 1;
            let lowest = bits.isolate_lowest_one();
            let carried = bits + lowest;
            let next = carried | (((carried ^ bits) >> 2) / lowest);
            assert!(next > bits && next.count_ones() as usize == size);
            bits = next;
        }
        assert_eq!(count, binomial(NUM_CARDS as u64, size as u64));
    }

    /// How many classes have each multiplicity from 1 to 4.
    fn class_sizes(next: &NextCards) -> [usize; 4] {
        let mut sizes = [0; 4];
        for class in next.classes() {
            sizes[class.multiplicity() - 1] += 1;
        }
        sizes
    }

    /// The members of the class of a card, written as a card set.
    fn class_of(next: &NextCards, c: &str) -> String {
        next.class_of(card(c)).unwrap().members().to_string()
    }

    /// Checks every promise `NextCards` makes about the cards that can follow `board` under
    /// `group`.
    fn check_next_cards(board: DealtBoard, next: &NextCards, group: SuitGroup) {
        assert_eq!(next.group(), group, "{board}");
        let live = CardSet::FULL - board.cards();
        assert_eq!(next.live_cards(), live);
        let (mut covered, mut total) = (CardSet::EMPTY, 0);
        for class in next.classes() {
            let representative = class.representative();
            assert!(
                !covered.intersects(class.members()),
                "{board}: classes overlap"
            );
            covered = covered | class.members();
            total += class.multiplicity();
            assert_eq!(class.multiplicity(), class.members().len());
            // The class is the orbit of its representative, which is its lowest card.
            let orbit: CardSet = group.iter().map(|p| p.apply_card(representative)).collect();
            assert_eq!(class.members(), orbit, "{board}: class of {representative}");
            assert_eq!(class.members().iter().next(), Some(representative));
            for member in class.members() {
                assert_eq!(next.class_of(member), Some(class));
                let p = next.to_representative(member).unwrap();
                assert_eq!(p.apply_card(member), representative, "{board}: {member}");
                assert!(group.contains(p));
                assert_eq!(
                    p.apply_board(board),
                    board,
                    "{p} is not a symmetry of {board}"
                );
                let first = SuitPermutation::all()
                    .find(|&q| group.contains(q) && q.apply_card(member) == representative);
                assert_eq!(Some(p), first);
            }
        }
        assert_eq!(covered, live, "{board}: a live card is in no class");
        assert_eq!(total, live.len(), "{board}: multiplicities");
        for dead in board.cards() {
            assert_eq!(next.class_of(dead), None);
            assert_eq!(next.to_representative(dead), None);
        }
        assert!(
            next.classes()
                .windows(2)
                .all(|pair| pair[0].representative() < pair[1].representative())
        );
    }

    #[test]
    fn there_are_24_permutations_numbered_in_lexicographic_order() {
        let all: Vec<SuitPermutation> = SuitPermutation::all().collect();
        assert_eq!(all.len(), NUM_SUIT_PERMUTATIONS);
        for (index, &p) in all.iter().enumerate() {
            assert_eq!(p.index(), index);
            assert_eq!(SuitPermutation::from_index(index), p);
            assert_eq!(SuitPermutation::new(p.images()), p);
        }
        assert!(
            all.windows(2)
                .all(|pair| pair[0].images() < pair[1].images())
        );
        assert_eq!(all[0], SuitPermutation::IDENTITY);
        assert!(all[0].is_identity() && !all[1].is_identity());
        assert_eq!(all[23].images(), [3, 2, 1, 0]);
        assert_eq!(SuitGroup::full().order(), 24);
    }

    #[test]
    fn permutations_are_written_in_cycle_notation() {
        for (images, written) in [
            ([0, 1, 2, 3], "()"),
            ([0, 1, 3, 2], "(h s)"),
            ([1, 0, 2, 3], "(c d)"),
            ([1, 0, 3, 2], "(c d)(h s)"),
            ([1, 2, 0, 3], "(c d h)"),
            ([2, 0, 1, 3], "(c h d)"),
            ([1, 2, 3, 0], "(c d h s)"),
            ([3, 2, 1, 0], "(c s)(d h)"),
        ] {
            assert_eq!(SuitPermutation::new(images).to_string(), written);
        }
        assert_eq!(SuitPermutation::swap(SPADE, HEART).to_string(), "(h s)");
        assert_eq!(SuitPermutation::swap(CLUB, CLUB), SuitPermutation::IDENTITY);
        let group = SuitGroup::generated_by([SuitPermutation::swap(HEART, SPADE)]);
        assert_eq!(group.to_string(), "{(), (h s)}");
    }

    #[test]
    #[should_panic(expected = "each suit exactly once")]
    fn images_that_repeat_a_suit_are_rejected() {
        SuitPermutation::new([0, 0, 2, 3]);
    }

    #[test]
    fn a_permutation_relabels_suits_and_keeps_ranks() {
        let p = SuitPermutation::new([DIAMOND, HEART, SPADE, CLUB]); // (c d h s)
        assert_eq!(p.apply_card(card("As")), card("Ac"));
        assert_eq!(p.apply_card(card("2c")), card("2d"));
        assert_eq!(p.apply_set(cards("Ks7d2c")).to_string(), "Kc7h2d");
        assert_eq!(p.apply_combo(combo("AsKs")), combo("AcKc"));
        assert_eq!(p.apply_combo(combo("Ah2c")), combo("As2d"));
        assert_eq!(p.apply_set(CardSet::FULL), CardSet::FULL);
        assert_eq!(p.apply_set(CardSet::EMPTY), CardSet::EMPTY);
        assert_eq!(
            p.apply_board(board("7s7h2c", &["7d"])),
            board("7c7s2d", &["7h"])
        );
    }

    #[test]
    fn a_mapped_range_moves_each_weight_to_its_image() {
        let swap = SuitPermutation::swap(HEART, SPADE);
        let specific = range("QQ,AsKs:0.5,7h6h");
        let mapped = swap.apply_range(&specific);
        assert_eq!(mapped, range("QQ,AhKh:0.5,7s6s"));
        assert_eq!(swap.apply_range(&mapped), specific);
        assert!(!swap.fixes_range(&specific));

        // A range of hand classes is unchanged by every relabelling.
        let classes = range("22+,A2s+,K9s+,QTs+,JTs,ATo+,KJo+,AK:0.5");
        assert!(SuitPermutation::all().all(|p| p.fixes_range(&classes)));
        assert!(SuitPermutation::all().all(|p| p.apply_range(&classes) == classes));

        // `AsKs:0.5` is changed by exactly the relabellings that move spades.
        let suited = range("AKs,AsKs:0.5");
        for p in SuitPermutation::all() {
            assert_eq!(p.fixes_range(&suited), p.image(SPADE) == SPADE, "{p}");
        }
    }

    #[test]
    fn groups_are_closed_under_composition() {
        let p = SuitPermutation::swap;
        for (generators, order) in [
            (vec![], 1),
            (vec![p(CLUB, DIAMOND)], 2),
            (vec![p(CLUB, DIAMOND), p(HEART, SPADE)], 4),
            (vec![p(CLUB, DIAMOND), p(DIAMOND, HEART)], 6),
            (vec![SuitPermutation::new([1, 2, 3, 0])], 4),
            (
                vec![p(CLUB, DIAMOND), SuitPermutation::new([1, 2, 3, 0])],
                24,
            ),
        ] {
            let group = SuitGroup::generated_by(generators.iter().copied());
            assert_eq!(group.order(), order, "{group}");
            assert!(group.is_group());
            assert!(generators.iter().all(|&g| group.contains(g)));
        }
        assert!(SuitGroup::trivial().is_trivial());
        assert!(SuitGroup::full().is_group());
        assert_eq!(SuitGroup::stabilizer(CardSet::EMPTY), SuitGroup::full());
        assert_eq!(SuitGroup::stabilizer(CardSet::FULL), SuitGroup::full());
        assert_eq!(
            SuitGroup::stabilizer(cards("AsAhAdAc")).order(),
            24,
            "four of a kind"
        );
        assert_eq!(SuitGroup::stabilizer(cards("Ks7d2c")), SuitGroup::trivial());
    }

    #[test]
    fn canonical_forms_of_some_flops() {
        for (flop, form) in [
            ("KsQs2s", "KcQc2c"),
            ("Ks7d2c", "Kc7d2h"),
            ("7s7h2c", "7d7c2h"),
            ("7s7h2s", "7d7c2c"),
            ("KsQs2d", "KcQc2d"),
            ("Ks7s2d", "Kc7c2d"),
            ("Ks7d2d", "Kc7d2d"),
            ("7s7h7d", "7h7d7c"),
        ] {
            let (canonical_form, p) = canonical(cards(flop));
            assert_eq!(canonical_form.to_string(), form, "{flop}");
            assert_eq!(p.apply_set(cards(flop)), canonical_form);
        }
        // A monotone flop in any suit has the same form.
        for suit in ['c', 'd', 'h', 's'] {
            let flop = cards(&format!("K{suit}Q{suit}2{suit}"));
            assert_eq!(canonical(flop).0, cards("KcQc2c"));
        }
    }

    /// Counts the classes of `size`-card sets under relabelling in two ways, and checks them
    /// against each other and against `expected`.
    ///
    /// The first way groups the sets by canonical form. Each group must have as many sets as
    /// the orbit of its canonical form, 24 divided by the order of its stabilizer, which is
    /// counted directly. The second way is Burnside's lemma: the number of orbits is the average,
    /// over the 24 permutations, of the number of sets each one maps onto itself.
    fn check_set_classes(size: usize, expected: usize) {
        let mut members: HashMap<u64, u64> = HashMap::new();
        let mut fixed_pairs = 0u64;
        let all: Vec<SuitPermutation> = SuitPermutation::all().collect();
        for_each_set(size, |set| {
            *members.entry(canonical(set).0.bits()).or_default() += 1;
            fixed_pairs += all.iter().filter(|p| p.apply_set(set) == set).count() as u64;
        });
        assert_eq!(
            members.len(),
            expected,
            "distinct {size}-card canonical forms"
        );
        assert_eq!(fixed_pairs % 24, 0);
        assert_eq!(
            fixed_pairs / 24,
            expected as u64,
            "Burnside for {size} cards"
        );
        for (&bits, &count) in &members {
            let form = CardSet::from_bits(bits);
            assert_eq!(canonical(form), (form, SuitPermutation::IDENTITY));
            let stabilizer = all.iter().filter(|p| p.apply_set(form) == form).count() as u64;
            assert_eq!(count, 24 / stabilizer, "the class of {form}");
        }
        let total: u64 = members.values().sum();
        assert_eq!(total, binomial(NUM_CARDS as u64, size as u64));
    }

    #[test]
    fn there_are_1755_distinct_flops() {
        check_set_classes(3, 1_755);
    }

    #[test]
    fn there_are_16432_distinct_four_card_boards() {
        check_set_classes(4, 16_432);
    }

    #[test]
    fn there_are_134459_distinct_five_card_boards() {
        check_set_classes(5, 134_459);
    }

    #[test]
    fn boards_must_be_dealt_in_legal_streets() {
        assert_eq!(
            DealtBoard::new(cards("Ks7d")),
            Err(BoardError::StartSize(2))
        );
        assert_eq!(
            DealtBoard::new(cards("Ks7d2c9h3s4d")),
            Err(BoardError::StartSize(6))
        );
        let flop = board("Ks7d2c", &[]);
        assert_eq!(
            flop.deal(card("7d")),
            Err(BoardError::Duplicate(card("7d")))
        );
        let river = board("Ks7d2c", &["9h", "3s"]);
        assert_eq!(river.deal(card("4d")), Err(BoardError::Complete));
        assert!(river.is_complete() && !flop.is_complete());
        assert_eq!(river.to_string(), "Ks7d2c 9h 3s");
        assert_eq!(river.cards(), cards("Ks7d2c9h3s"));
        assert_eq!(
            river.later_cards().collect::<Vec<_>>(),
            [card("9h"), card("3s")]
        );
        // The same cards with the turn and river the other way round are another history.
        assert_ne!(river, board("Ks7d2c", &["3s", "9h"]));
        assert_ne!(river, board("Ks7d2c9h3s", &[]));
    }

    /// Flop 7s 7h 2c, turn 7d. The four cards are kept by every relabelling that keeps clubs,
    /// since the 2c is the only deuce and the other three suits each hold one seven. Swapping
    /// hearts and diamonds is one of them, but it is not a symmetry of the flop, which it turns
    /// into 7s 7d 2c, and so it is not a symmetry of the board as dealt: it swaps the turn with
    /// a flop card. The flop alone is kept by the identity and by swapping hearts and spades,
    /// which exchanges its two sevens, and both keep the 7d, so those two are the symmetries.
    #[test]
    fn the_turn_card_stays_the_turn_card() {
        let swap_hd = SuitPermutation::swap(HEART, DIAMOND);
        let swap_hs = SuitPermutation::swap(HEART, SPADE);
        let flop = board("7s7h2c", &[]);
        let turn = flop.deal(card("7d")).unwrap();

        let four_cards = SuitGroup::stabilizer(turn.cards());
        assert_eq!(four_cards.order(), 6);
        assert!(four_cards.iter().all(|p| p.image(CLUB) == CLUB));
        assert!(four_cards.contains(swap_hd));
        assert!(!flop.symmetries().contains(swap_hd));
        assert!(!turn.symmetries().contains(swap_hd));

        assert_eq!(flop.symmetries().to_string(), "{(), (h s)}");
        assert_eq!(turn.symmetries().to_string(), "{(), (h s)}");
        assert!(turn.symmetries().contains(swap_hs));

        // So the river cards fall into 36 classes, not the 24 that the four cards as a set would
        // give: the 7c, the 2d and each club and diamond of the other ranks alone, and the heart
        // and the spade of each rank but seven together.
        let rivers = turn.next_cards();
        assert_eq!(rivers.classes().len(), 36);
        assert_eq!(class_sizes(&rivers), [24, 12, 0, 0]);
        assert_eq!(class_of(&rivers, "2h"), "2s2h");
        assert_eq!(class_of(&rivers, "2d"), "2d");
        assert_eq!(class_of(&rivers, "7c"), "7c");
        check_next_cards(turn, &rivers, turn.symmetries());

        // The same four cards as the start of a turn spot have no history, and all six
        // relabellings that keep clubs are symmetries.
        let turn_spot = board("7s7h7d2c", &[]);
        assert_eq!(turn_spot.symmetries(), four_cards);
        assert_eq!(turn_spot.next_cards().classes().len(), 24);
    }

    /// Two ways for a symmetry of the flop to stop being one once the turn is out.
    #[test]
    fn a_flop_symmetry_can_end_with_the_turn() {
        // On 7s 7h 2c, swapping hearts and spades exchanges the two sevens. A turn of 9h becomes
        // 9s under it, so after that turn only the identity is left.
        let swap_hs = SuitPermutation::swap(HEART, SPADE);
        let flop = board("7s7h2c", &[]);
        assert!(flop.symmetries().contains(swap_hs));
        let turn = flop.deal(card("9h")).unwrap();
        assert!(!turn.symmetries().contains(swap_hs));
        assert!(turn.symmetries().is_trivial());

        // On Ks Qs 2s every relabelling that keeps spades is a symmetry. A turn of 5h leaves
        // only those that also keep hearts: the identity and swapping clubs and diamonds.
        let flop = board("KsQs2s", &[]);
        assert_eq!(flop.symmetries().order(), 6);
        let turn = flop.deal(card("5h")).unwrap();
        assert_eq!(turn.symmetries().to_string(), "{(), (c d)}");

        // And a river of 5d leaves only the identity. The five cards are still kept by
        // swapping diamonds and hearts, which is a symmetry of the flop too, but it swaps the
        // turn with the river.
        let river = turn.deal(card("5d")).unwrap();
        assert!(river.symmetries().is_trivial());
        let five_cards = SuitGroup::stabilizer(river.cards());
        assert_eq!(five_cards.to_string(), "{(), (d h)}");
        assert!(
            flop.symmetries()
                .contains(SuitPermutation::swap(DIAMOND, HEART))
        );
    }

    /// Ks Qs 2s. The symmetries keep spades and permute the other three suits in any way. Each
    /// rank's club, diamond and heart form one class, 13 classes of 3, and the 10 live spades
    /// are each alone: 23 classes, 39 + 10 = 49 cards.
    #[test]
    fn on_a_monotone_flop_the_other_three_suits_are_interchangeable() {
        let flop = board("KsQs2s", &[]);
        let turns = flop.next_cards();
        assert_eq!(
            flop.symmetries().to_string(),
            "{(), (d h), (c d), (c d h), (c h d), (c h)}"
        );
        assert_eq!(turns.classes().len(), 23);
        assert_eq!(class_sizes(&turns), [10, 0, 13, 0]);
        assert_eq!(class_of(&turns, "5h"), "5h5d5c");
        assert_eq!(class_of(&turns, "Kd"), "KhKdKc");
        assert_eq!(class_of(&turns, "As"), "As");
        let to_club = turns.to_representative(card("5h")).unwrap();
        assert_eq!(to_club.apply_card(card("5h")), card("5c"));
        check_next_cards(flop, &turns, flop.symmetries());
    }

    /// Ks 7d 2c. Each card has a rank of its own, so a symmetry keeps each card and with it
    /// spades, diamonds and clubs, which leaves hearts where they are too. Only the identity is
    /// left, and each of the 49 cards is a class of its own: the fourth suit stands alone.
    #[test]
    fn on_a_rainbow_flop_no_two_suits_are_interchangeable() {
        let flop = board("Ks7d2c", &[]);
        let turns = flop.next_cards();
        assert!(flop.symmetries().is_trivial());
        assert_eq!(turns.classes().len(), 49);
        assert_eq!(class_sizes(&turns), [49, 0, 0, 0]);
        assert_eq!(class_of(&turns, "5h"), "5h");
        check_next_cards(flop, &turns, flop.symmetries());
    }

    /// Ks Qs 2d. Each card has a rank of its own, so spades and diamonds stay, and the
    /// symmetries are the identity and swapping the absent clubs and hearts. The K, Q and 2 each
    /// give a class of 2 and one of 1; the 10 other ranks each give a class of 2 and two of 1:
    /// 36 classes, 9 + 40 = 49 cards.
    #[test]
    fn on_a_two_tone_flop_the_two_absent_suits_are_interchangeable() {
        let flop = board("KsQs2d", &[]);
        let turns = flop.next_cards();
        assert_eq!(flop.symmetries().to_string(), "{(), (c h)}");
        assert_eq!(turns.classes().len(), 36);
        assert_eq!(class_sizes(&turns), [23, 13, 0, 0]);
        assert_eq!(class_of(&turns, "Kh"), "KhKc");
        assert_eq!(class_of(&turns, "Kd"), "Kd");
        assert_eq!(class_of(&turns, "2s"), "2s");
        assert_eq!(class_of(&turns, "9c"), "9h9c");
        check_next_cards(flop, &turns, flop.symmetries());
    }

    /// On a paired flop the two suits of the pair can be exchanged, if the third card allows.
    #[test]
    fn on_a_paired_flop_the_suits_of_the_pair_are_interchangeable() {
        // 7s 7h 2c: the 2c stays, and the sevens may swap. Each rank's heart and spade go
        // together, except the dead sevens: 37 classes, with the 7c, 7d and 2d alone.
        let flop = board("7s7h2c", &[]);
        let turns = flop.next_cards();
        assert_eq!(flop.symmetries().to_string(), "{(), (h s)}");
        assert_eq!(turns.classes().len(), 37);
        assert_eq!(class_sizes(&turns), [25, 12, 0, 0]);
        assert_eq!(class_of(&turns, "2h"), "2s2h");
        assert_eq!(class_of(&turns, "7c"), "7c");
        check_next_cards(flop, &turns, flop.symmetries());

        // 7s 7h 2s: the 2s keeps spades, so the 7s and then the 7h stay, and only the absent
        // clubs and diamonds swap. The repeated suit of the 2 and one seven matters.
        let flop = board("7s7h2s", &[]);
        let turns = flop.next_cards();
        assert_eq!(flop.symmetries().to_string(), "{(), (c d)}");
        assert_eq!(turns.classes().len(), 36);
        assert_eq!(class_sizes(&turns), [23, 13, 0, 0]);
        assert_eq!(class_of(&turns, "7c"), "7d7c");
        assert_eq!(class_of(&turns, "2h"), "2h");
        check_next_cards(flop, &turns, flop.symmetries());

        // 7s 7d 2c: the pair's suits are spades and diamonds, and they swap.
        let flop = board("7s7d2c", &[]);
        let turns = flop.next_cards();
        assert_eq!(flop.symmetries().to_string(), "{(), (d s)}");
        assert_eq!(turns.classes().len(), 37);
        assert_eq!(class_of(&turns, "Ad"), "AsAd");
        check_next_cards(flop, &turns, flop.symmetries());
    }

    /// 7s 7h 7d. Clubs stay, and the three suits of the trips permute in any way. The 7c is
    /// alone; each other rank gives the club alone and the other three together: 25 classes,
    /// 1 + 12 * 4 = 49 cards.
    #[test]
    fn on_a_trips_flop_the_suits_of_the_trips_are_interchangeable() {
        let flop = board("7s7h7d", &[]);
        let turns = flop.next_cards();
        assert_eq!(
            flop.symmetries().to_string(),
            "{(), (h s), (d h), (d h s), (d s h), (d s)}"
        );
        assert_eq!(turns.classes().len(), 25);
        assert_eq!(class_sizes(&turns), [13, 0, 12, 0]);
        assert_eq!(class_of(&turns, "Ah"), "AsAhAd");
        assert_eq!(class_of(&turns, "7c"), "7c");
        check_next_cards(flop, &turns, flop.symmetries());
    }

    /// Each of the 1,755 flops, through every promise of `NextCards`. The classes of turn cards,
    /// summed over the flops, count the (flop, turn) pairs up to relabelling, which Burnside's
    /// lemma counts independently as the average number of pairs a permutation keeps.
    #[test]
    fn turn_classes_are_right_on_every_flop() {
        let (mut flops, mut flop_turns) = (0, 0);
        for_each_set(3, |flop| {
            if canonical(flop).0 != flop {
                return;
            }
            flops += 1;
            let board = DealtBoard::new(flop).unwrap();
            let turns = board.next_cards();
            check_next_cards(board, &turns, board.symmetries());
            flop_turns += turns.classes().len();
        });
        assert_eq!(flops, 1_755);

        let mut kept = 0;
        for p in SuitPermutation::all() {
            let unmoved: CardSet = Card::all().filter(|&c| p.apply_card(c) == c).collect();
            for_each_set(3, |flop| {
                if p.apply_set(flop) == flop {
                    kept += (unmoved - flop).len();
                }
            });
        }
        assert_eq!(kept % 24, 0);
        assert_eq!(flop_turns, kept / 24);
        println!("{flops} flops, {flop_turns} (flop, turn) pairs up to relabelling");
    }

    /// The same check one street later: the river classes after each (flop, turn) pair, against
    /// Burnside's count of (flop, turn, river) histories.
    #[test]
    fn river_classes_are_right_after_every_flop_and_turn() {
        let mut histories = 0;
        for_each_set(3, |flop| {
            if canonical(flop).0 != flop {
                return;
            }
            let start = DealtBoard::new(flop).unwrap();
            for class in start.next_cards().classes() {
                let board = start.deal(class.representative()).unwrap();
                let rivers = board.next_cards();
                assert_eq!(rivers.live_cards().len(), 48);
                let total: usize = rivers.classes().iter().map(CardClass::multiplicity).sum();
                assert_eq!(total, 48);
                histories += rivers.classes().len();
            }
        });

        let mut kept = 0;
        for p in SuitPermutation::all() {
            let unmoved: CardSet = Card::all().filter(|&c| p.apply_card(c) == c).collect();
            for_each_set(3, |flop| {
                if p.apply_set(flop) == flop {
                    let n = (unmoved - flop).len();
                    kept += n * (n - 1);
                }
            });
        }
        assert_eq!(kept % 24, 0);
        assert_eq!(histories, kept / 24);
        println!("{histories} (flop, turn, river) histories up to relabelling");
    }

    /// Ranges symmetric under every symmetry of the board: random class weights, then random
    /// combos set to a random weight together with their images under `group`.
    fn symmetric_range(group: SuitGroup, class_weights: &[f32], combos: &[(usize, f32)]) -> Range {
        let mut range = Range::empty();
        for (index, &weight) in class_weights.iter().enumerate() {
            range.set_class(HandClass::from_index(index), weight);
        }
        for &(index, weight) in combos {
            for p in group.iter() {
                range.set(p.apply_combo(Combo::from_index(index)), weight);
            }
        }
        range
    }

    /// The largest gap in win, tie or loss between two results, which must agree on failing.
    fn gap(a: &Result<Equity, EquityError>, b: &Result<Equity, EquityError>) -> f64 {
        match (a, b) {
            (Ok(a), Ok(b)) => (a.win - b.win)
                .abs()
                .max((a.tie - b.tie).abs())
                .max((a.lose - b.lose).abs()),
            (Err(a), Err(b)) => {
                assert_eq!(a, b);
                0.0
            }
            _ => panic!("one card has matchups and the other does not: {a:?} {b:?}"),
        }
    }

    /// The largest equity gap between a class's representative and another member, over the
    /// classes for which `check` holds, with the board plus that card as the board.
    fn largest_class_gap(
        board: DealtBoard,
        next: &NextCards,
        hero: &Range,
        villain: &Range,
        check: impl Fn(&CardClass) -> bool,
    ) -> f64 {
        let mut largest = 0.0f64;
        for class in next.classes().iter().filter(|class| check(class)) {
            let on = |c: Card| range_vs_range(hero, villain, board.cards() | CardSet::single(c));
            let base = on(class.representative());
            for member in class.members() - CardSet::single(class.representative()) {
                largest = largest.max(gap(&base, &on(member)));
            }
        }
        largest
    }

    /// The largest gap between a hand's equity after a card and its image's equity after the
    /// card's representative, the image taken through `to_representative`. This is how a solver
    /// reads a hand's result off the representative's subgame.
    fn largest_hand_gap(board: DealtBoard, next: &NextCards, hero: &Range, villain: &Range) -> f64 {
        let single = |combo: Combo| {
            let mut range = Range::empty();
            range.set(combo, 1.0);
            range
        };
        let mut largest = 0.0f64;
        for class in next.classes() {
            let to_board = |c: Card| board.cards() | CardSet::single(c);
            let representative = to_board(class.representative());
            for member in class.members() - CardSet::single(class.representative()) {
                let p = next.to_representative(member).unwrap();
                for (hand, _) in hero
                    .iter()
                    .filter(|(hand, _)| !hand.intersects(to_board(member)))
                {
                    let here = range_vs_range(&single(hand), villain, to_board(member));
                    let there =
                        range_vs_range(&single(p.apply_combo(hand)), villain, representative);
                    largest = largest.max(gap(&here, &there));
                }
            }
        }
        largest
    }

    /// For boards of each kind and ranges symmetric under their symmetries, the cards in a
    /// class give the same equity, and so does each hand mapped to the representative.
    ///
    /// The weights are not sums of a few powers of two, so the products and sums in the equity
    /// round, and the summation order, which the relabelling changes, can show in the result.
    #[test]
    fn cards_in_one_class_give_the_same_equity() {
        let hero_classes = range("TT+,AQs+:0.35,KQs,JTs:0.6,98s,76s,AKo:0.15");
        let villain_classes = range("77-99:0.45,ATs-AJs,KJs:0.8,QJs,T9s,65s:0.55,AQo,KQo:0.2");
        let mut largest = 0.0f64;
        for (start, later) in [
            ("KsQs2s", &[][..]),
            ("KsQs2d", &[]),
            ("7s7h2c", &[]),
            ("7s7h2s", &[]),
            ("7s7h7d", &[]),
            ("KsQs2s", &["5h"]),
            ("7s7h2c", &["7d"]),
            ("KsQs2d", &["9s"]),
            ("7s7h7d2c", &[]),
        ] {
            let board = board(start, later);
            let group = board.symmetries();
            // Add specific combos that the board's symmetries map onto one another.
            let mut hero = hero_classes.clone();
            let mut villain = villain_classes.clone();
            for (range, extras) in [
                (&mut hero, [("AhTh", 0.3), ("Kd9c", 0.7)]),
                (&mut villain, [("QhJd", 0.1), ("5s4s", 0.9)]),
            ] {
                for (text, weight) in extras {
                    for p in group.iter() {
                        range.set(p.apply_combo(combo(text)), weight);
                    }
                }
            }
            // The ranges are symmetric under the board's symmetries but not under every
            // relabelling.
            assert!(!SuitPermutation::all().all(|p| p.fixes_range(&hero)));
            assert_eq!(board.symmetries_with_ranges([&hero, &villain]), group);

            let next = board.next_cards();
            let class_gap = largest_class_gap(board, &next, &hero, &villain, |_| true);
            largest = largest.max(class_gap);
            if later.len() + cards(start).len() == 4 {
                largest = largest.max(largest_hand_gap(board, &next, &hero, &villain));
            }
        }
        println!("largest equity gap within a class: {largest:e}");
        assert!(largest < 1e-12, "{largest}");
    }

    /// On 7s 7h 2c, the turns 5h and 5s are in one class of the board, which swapping hearts
    /// and spades makes. A range that holds AsKs but not AhKh breaks that symmetry: AsKs has a
    /// flush draw after the 5s and not after the 5h, so the two turns give different equities,
    /// and the classes taken with the ranges keep them apart.
    #[test]
    fn a_range_that_is_not_symmetric_keeps_apart_cards_the_board_alone_would_join() {
        let flop = board("7s7h2c", &[]);
        let hero = range("QQ,AsKs:0.5");
        let villain = range("88,JJ,AQo,KQs");
        let on = |c: &str| range_vs_range(&hero, &villain, flop.cards() | cards(c)).unwrap();
        let (hearts, spades) = (on("5h"), on("5s"));
        println!(
            "after 5h {:.6}, after 5s {:.6}",
            hearts.share(),
            spades.share()
        );
        assert!((hearts.share() - spades.share()).abs() > 1e-3);

        let board_only = flop.next_cards();
        assert_eq!(class_of(&board_only, "5h"), "5s5h");

        let group = flop.symmetries_with_ranges([&hero, &villain]);
        assert!(group.is_trivial());
        let with_ranges = flop.next_cards_within(group);
        assert_eq!(class_of(&with_ranges, "5h"), "5h");
        assert_eq!(class_of(&with_ranges, "5s"), "5s");
        assert_eq!(with_ranges.classes().len(), 49);
        check_next_cards(flop, &with_ranges, group);

        // With AhKh at the same weight the symmetry is back, and so is the equal equity.
        let hero = range("QQ,AsKs:0.5,AhKh:0.5");
        assert_eq!(
            flop.symmetries_with_ranges([&hero, &villain]),
            flop.symmetries()
        );
        let on = |c: &str| range_vs_range(&hero, &villain, flop.cards() | cards(c));
        assert!(gap(&on("5h"), &on("5s")) < 1e-12);
    }

    /// 7h6h shares the 7h with the board, and swapping hearts and spades maps it to 7s6s, which
    /// shares the 7s. Both are dead, so listing one of them does not break the symmetry, though
    /// the range as a whole is not unchanged.
    #[test]
    fn dead_combos_do_not_break_a_symmetry() {
        let flop = board("7s7h2c", &[]);
        let swap_hs = SuitPermutation::swap(HEART, SPADE);
        let with_dead = range("QQ,7h6h");
        assert!(!swap_hs.fixes_range(&with_dead));
        assert_eq!(
            flop.symmetries_with_ranges([&with_dead, &range("JJ")]),
            flop.symmetries()
        );
        let with_live = range("QQ,8h6h");
        assert!(
            flop.symmetries_with_ranges([&with_live, &range("JJ")])
                .is_trivial()
        );
    }

    fn any_permutation() -> impl Strategy<Value = SuitPermutation> {
        (0..NUM_SUIT_PERMUTATIONS).prop_map(SuitPermutation::from_index)
    }

    fn any_card() -> impl Strategy<Value = Card> {
        (0..NUM_CARDS).prop_map(Card::from_index)
    }

    /// Small sets such as boards, and sets of any size.
    fn any_card_set() -> impl Strategy<Value = CardSet> {
        prop_oneof![
            prop::sample::subsequence(Card::all().collect::<Vec<_>>(), 0..=7)
                .prop_map(|cards| cards.into_iter().collect()),
            any::<u64>().prop_map(|bits| CardSet::from_bits(bits & CardSet::FULL.bits())),
        ]
    }

    fn any_weight() -> impl Strategy<Value = f32> {
        prop_oneof![
            3 => Just(1.0f32),
            1 => Just(0.5f32),
            1 => 0.0f32..=1.0f32,
        ]
    }

    /// Weights for a few of the classes, the rest at zero.
    fn sparse_class_weights(one_in: u32) -> impl Strategy<Value = Vec<f32>> {
        let weight = prop_oneof![
            one_in - 1 => Just(0.0f32),
            1 => any_weight(),
        ];
        prop::collection::vec(weight, NUM_CLASSES)
    }

    fn any_combos() -> impl Strategy<Value = Vec<(usize, f32)>> {
        prop::collection::vec((0..NUM_COMBOS, any_weight()), 0..8)
    }

    /// A board of 3 to `max_cards` cards from the deck, or from the cards of a few ranks so
    /// that pairs, trips and flushes come often. It starts with 3 or more of them as a set and
    /// deals the rest one at a time.
    fn any_board(max_cards: usize) -> impl Strategy<Value = DealtBoard> {
        let ranks = prop::sample::subsequence((0..NUM_RANKS as u8).collect::<Vec<_>>(), 2..=4);
        let deck =
            prop_oneof![Just((0..NUM_RANKS as u8).collect::<Vec<_>>()), ranks,].prop_map(|ranks| {
                ranks
                    .iter()
                    .flat_map(|&rank| (0..NUM_SUITS as u8).map(move |suit| Card::new(rank, suit)))
                    .collect::<Vec<Card>>()
            });
        (deck, 3..=max_cards)
            .prop_flat_map(|(deck, total)| (Just(deck).prop_shuffle(), 3..=total, Just(total)))
            .prop_map(|(deck, start, total)| {
                deck[start..total].iter().fold(
                    DealtBoard::new(deck[..start].iter().copied().collect()).unwrap(),
                    |board, &card| board.deal(card).unwrap(),
                )
            })
    }

    fn any_range() -> impl Strategy<Value = Range> {
        (sparse_class_weights(3), any_combos())
            .prop_map(|(classes, combos)| symmetric_range(SuitGroup::trivial(), &classes, &combos))
    }

    proptest! {
        #[test]
        fn composition_applies_one_permutation_after_the_other(
            p in any_permutation(),
            q in any_permutation(),
            r in any_permutation(),
            card in any_card(),
        ) {
            prop_assert_eq!(p.then(q).apply_card(card), q.apply_card(p.apply_card(card)));
            prop_assert_eq!(p.then(q).then(r), p.then(q.then(r)));
            prop_assert_eq!(p.then(p.inverse()), SuitPermutation::IDENTITY);
            prop_assert_eq!(p.inverse().then(p), SuitPermutation::IDENTITY);
            prop_assert_eq!(p.inverse().apply_card(p.apply_card(card)), card);
            prop_assert_eq!(SuitPermutation::IDENTITY.then(p), p);
            for suit in 0..NUM_SUITS as u8 {
                prop_assert_eq!(p.then(q).image(suit), q.image(p.image(suit)));
            }
        }

        #[test]
        fn permutations_map_sets_combos_and_ranges_card_by_card(
            p in any_permutation(),
            q in any_permutation(),
            set in any_card_set(),
            combo_index in 0..NUM_COMBOS,
            range in any_range(),
        ) {
            let mapped: CardSet = set.iter().map(|card| p.apply_card(card)).collect();
            prop_assert_eq!(p.apply_set(set), mapped);
            let combo = Combo::from_index(combo_index);
            prop_assert_eq!(p.apply_combo(combo).cards(), p.apply_set(combo.cards()));

            let image = p.apply_range(&range);
            for combo in Combo::all() {
                prop_assert_eq!(image.weight(p.apply_combo(combo)), range.weight(combo));
            }
            prop_assert_eq!(p.then(q).apply_range(&range), q.apply_range(&image));
            prop_assert_eq!(p.inverse().apply_range(&image), range.clone());
            prop_assert_eq!(p.fixes_range(&range), image == range);
        }

        /// The issue's first condition, with the definition of the form checked too.
        #[test]
        fn canonical_of_a_permuted_board_is_canonical_of_the_board(
            set in any_card_set(),
            p in any_permutation(),
        ) {
            let (form, to_form) = canonical(set);
            prop_assert_eq!(canonical(p.apply_set(set)).0, form);
            prop_assert_eq!(to_form.apply_set(set), form);
            prop_assert_eq!(form.len(), set.len());
            // It is the smallest image, and the permutation the first that reaches it.
            for q in SuitPermutation::all() {
                prop_assert!(q.apply_set(set).bits() >= form.bits());
                if q < to_form {
                    prop_assert_ne!(q.apply_set(set), form);
                }
            }
            prop_assert_eq!(canonical(form), (form, SuitPermutation::IDENTITY));
        }

        #[test]
        fn the_symmetries_of_a_board_keep_every_street(
            board in any_board(5),
            p in any_permutation(),
        ) {
            let group = board.symmetries();
            for q in SuitPermutation::all() {
                let keeps = q.apply_set(board.start()) == board.start()
                    && board.later_cards().all(|card| q.apply_card(card) == card);
                prop_assert_eq!(group.contains(q), keeps, "{} on {}", q, board);
            }
            prop_assert!(group.is_group());
            // They are also the relabellings that keep the cards so far at every street.
            let mut so_far = board.start();
            let mut every_street = SuitGroup::stabilizer(so_far);
            for card in board.later_cards() {
                so_far = so_far | CardSet::single(card);
                every_street = every_street.intersection(SuitGroup::stabilizer(so_far));
            }
            prop_assert_eq!(every_street, group);
            // Mapping the board conjugates its symmetries.
            let mapped = p.apply_board(board).symmetries();
            for q in group.iter() {
                prop_assert!(mapped.contains(p.inverse().then(q).then(p)));
            }
            prop_assert_eq!(mapped.order(), group.order());
        }

        /// The issue's second condition, with every other promise of `NextCards`, under the
        /// board's symmetries and under a random group of them.
        #[test]
        fn multiplicities_sum_to_the_number_of_live_cards(
            board in any_board(4),
            generators in prop::collection::vec(any_permutation(), 0..3),
        ) {
            let next = board.next_cards();
            let live = NUM_CARDS - board.cards().len();
            let total: usize = next.classes().iter().map(CardClass::multiplicity).sum();
            prop_assert_eq!(total, live);
            check_next_cards(board, &next, board.symmetries());

            let allowed = SuitGroup::generated_by(generators);
            let finer = board.next_cards_within(allowed);
            check_next_cards(board, &finer, board.symmetries().intersection(allowed));
            for class in finer.classes() {
                let coarse = next.class_of(class.representative()).unwrap().members();
                prop_assert_eq!(class.members() & coarse, class.members());
            }
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(48))]

        /// Random boards before the river and random ranges symmetric under the board: every
        /// class's cards give the same equity. On a turn board every hand is checked too.
        #[test]
        fn random_symmetric_ranges_give_equal_equity_within_a_class(
            board in any_board(4),
            hero_classes in sparse_class_weights(16),
            villain_classes in sparse_class_weights(16),
            hero_combos in any_combos(),
            villain_combos in any_combos(),
            pick in 0..NUM_CARDS,
        ) {
            let group = board.symmetries();
            let hero = symmetric_range(group, &hero_classes, &hero_combos);
            let villain = symmetric_range(group, &villain_classes, &villain_combos);
            prop_assert_eq!(board.symmetries_with_ranges([&hero, &villain]), group);
            let next = board.next_cards();
            if board.cards().len() == 4 {
                prop_assert!(largest_class_gap(board, &next, &hero, &villain, |_| true) < 1e-12);
                prop_assert!(largest_hand_gap(board, &next, &hero, &villain) < 1e-12);
            } else {
                // A flop's turns each need a river enumeration, so check two classes.
                let classes = next.classes().len();
                let chosen = [pick % classes, (pick * 7 + 3) % classes];
                let check = |class: &CardClass| {
                    chosen.iter().any(|&i| next.classes()[i] == *class)
                };
                prop_assert!(largest_class_gap(board, &next, &hero, &villain, check) < 1e-12);
            }
        }
    }
}
