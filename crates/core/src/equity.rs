//! Showdown equity: exact enumeration and seeded Monte Carlo.

use std::cmp::Ordering;

use rand_chacha::ChaCha8Rng;
use rand_chacha::rand_core::{Rng, SeedableRng};
use thiserror::Error;

use crate::card::{Card, CardSet, NUM_CARDS};
use crate::combo::Combo;
use crate::eval::{HandRank, evaluate};
use crate::range::Range;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum EquityError {
    #[error("the board has {0} cards; at most 5 are allowed")]
    BoardTooLarge(usize),
    #[error("a card appears in both hands, or in a hand and on the board")]
    SharedCard,
    #[error("no hand from one range can be held together with a hand from the other")]
    NoMatchups,
}

/// How often a hand or range wins, ties and loses at showdown. The three sum to 1.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Equity {
    pub win: f64,
    pub tie: f64,
    pub lose: f64,
}

impl Equity {
    /// The average share of the pot: wins plus half of ties.
    pub fn share(&self) -> f64 {
        self.win + 0.5 * self.tie
    }
}

/// A Monte Carlo estimate of the pot share, with its standard error.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Estimate {
    pub share: f64,
    pub std_err: f64,
    pub samples: u64,
}

#[derive(Default)]
struct Tally {
    win: f64,
    tie: f64,
    lose: f64,
}

impl Tally {
    fn add(&mut self, hero: HandRank, villain: HandRank, weight: f64) {
        match hero.cmp(&villain) {
            Ordering::Greater => self.win += weight,
            Ordering::Equal => self.tie += weight,
            Ordering::Less => self.lose += weight,
        }
    }

    fn finish(self) -> Result<Equity, EquityError> {
        let total = self.win + self.tie + self.lose;
        if total <= 0.0 {
            return Err(EquityError::NoMatchups);
        }
        Ok(Equity {
            win: self.win / total,
            tie: self.tie / total,
            lose: self.lose / total,
        })
    }
}

fn check_board(board: CardSet) -> Result<(), EquityError> {
    if board.len() > 5 {
        Err(EquityError::BoardTooLarge(board.len()))
    } else {
        Ok(())
    }
}

/// Calls `visit` with every five-card board that extends `board` without using a `dead` card.
fn for_each_runout(board: CardSet, dead: CardSet, mut visit: impl FnMut(CardSet)) {
    let missing = 5 - board.len();
    if missing == 0 {
        visit(board);
        return;
    }
    let live: Vec<u64> = (CardSet::FULL - board - dead)
        .iter()
        .map(|card| CardSet::single(card).bits())
        .collect();
    if live.len() < missing {
        return;
    }

    // Walk the `missing`-card subsets of `live` in lexicographic order.
    let mut picks = [0, 1, 2, 3, 4];
    loop {
        let extra = picks[..missing]
            .iter()
            .fold(0, |bits, &pick| bits | live[pick]);
        visit(CardSet::from_bits(board.bits() | extra));

        let mut slot = missing;
        loop {
            if slot == 0 {
                return;
            }
            slot -= 1;
            if picks[slot] != slot + live.len() - missing {
                break;
            }
        }
        picks[slot] += 1;
        for next in slot + 1..missing {
            picks[next] = picks[next - 1] + 1;
        }
    }
}

/// Exact equity of one hand against another, over every way to complete the board.
pub fn hand_vs_hand(hero: Combo, villain: Combo, board: CardSet) -> Result<Equity, EquityError> {
    check_board(board)?;
    let (hero, villain) = (hero.cards(), villain.cards());
    if hero.intersects(villain) || board.intersects(hero | villain) {
        return Err(EquityError::SharedCard);
    }
    let mut tally = Tally::default();
    for_each_runout(board, hero | villain, |full| {
        tally.add(evaluate(full | hero), evaluate(full | villain), 1.0);
    });
    tally.finish()
}

/// The combos of a range that have weight and do not share a card with the board.
fn live_combos(range: &Range, board: CardSet) -> Vec<(Combo, f64)> {
    range
        .iter()
        .filter(|(combo, _)| !combo.intersects(board))
        .map(|(combo, weight)| (combo, f64::from(weight)))
        .collect()
}

/// Exact equity of one weighted range against another, with card removal.
///
/// Each pair of hands that can be held together counts with the product of its two weights.
///
/// The cost is the number of runouts times the product of the range sizes. That is fine on the
/// turn and river, slow on the flop with wide ranges, and impractical before the flop, where
/// [`monte_carlo`] is the tool.
pub fn range_vs_range(
    hero: &Range,
    villain: &Range,
    board: CardSet,
) -> Result<Equity, EquityError> {
    check_board(board)?;
    let heroes = live_combos(hero, board);
    let villains = live_combos(villain, board);

    let rank_all =
        |combos: &[(Combo, f64)], full: CardSet, ranks: &mut Vec<Option<HandRank>>| {
            ranks.clear();
            ranks.extend(combos.iter().map(|&(combo, _)| {
                (!combo.intersects(full)).then(|| evaluate(full | combo.cards()))
            }));
        };

    let mut tally = Tally::default();
    let (mut hero_ranks, mut villain_ranks) = (Vec::new(), Vec::new());
    for_each_runout(board, CardSet::EMPTY, |full| {
        rank_all(&heroes, full, &mut hero_ranks);
        rank_all(&villains, full, &mut villain_ranks);
        for (&(hero, hero_weight), hero_rank) in heroes.iter().zip(&hero_ranks) {
            let Some(hero_rank) = *hero_rank else {
                continue;
            };
            for (&(villain, villain_weight), villain_rank) in villains.iter().zip(&villain_ranks) {
                let Some(villain_rank) = *villain_rank else {
                    continue;
                };
                if !hero.intersects(villain.cards()) {
                    tally.add(hero_rank, villain_rank, hero_weight * villain_weight);
                }
            }
        }
    });
    tally.finish()
}

/// A uniform integer in `0..bound`.
fn below(rng: &mut ChaCha8Rng, bound: usize) -> usize {
    ((u128::from(rng.next_u64()) * bound as u128) >> 64) as usize
}

/// A uniform number in `[0, 1)`.
fn unit(rng: &mut ChaCha8Rng) -> f64 {
    (rng.next_u64() >> 11) as f64 / (1u64 << 53) as f64
}

/// Draws combos from a range in proportion to their weights.
struct Sampler {
    combos: Vec<Combo>,
    cumulative: Vec<f64>,
}

impl Sampler {
    fn new(range: &Range, board: CardSet) -> Sampler {
        let mut total = 0.0;
        let (combos, cumulative) = live_combos(range, board)
            .into_iter()
            .map(|(combo, weight)| {
                total += weight;
                (combo, total)
            })
            .unzip();
        Sampler { combos, cumulative }
    }

    fn draw(&self, rng: &mut ChaCha8Rng) -> Combo {
        let target = unit(rng) * self.cumulative[self.cumulative.len() - 1];
        let index = self.cumulative.partition_point(|&sum| sum <= target);
        self.combos[index.min(self.combos.len() - 1)]
    }
}

/// Estimates the equity of one weighted range against another by sampling.
///
/// Each sample draws a hand from each range by weight, redraws both if they share a card, then
/// deals the rest of the board at random. The same seed always gives the same estimate.
pub fn monte_carlo(
    hero: &Range,
    villain: &Range,
    board: CardSet,
    samples: u64,
    seed: u64,
) -> Result<Estimate, EquityError> {
    check_board(board)?;
    assert!(samples > 0, "monte_carlo needs at least one sample");
    let heroes = Sampler::new(hero, board);
    let villains = Sampler::new(villain, board);
    let fits = |hero: &Combo| {
        villains
            .combos
            .iter()
            .any(|villain| !hero.intersects(villain.cards()))
    };
    if !heroes.combos.iter().any(fits) {
        return Err(EquityError::NoMatchups);
    }

    let mut rng = ChaCha8Rng::seed_from_u64(seed);
    let (mut sum, mut sum_of_squares) = (0.0, 0.0);
    for _ in 0..samples {
        let (hero, villain) = loop {
            let (hero, villain) = (
                heroes.draw(&mut rng).cards(),
                villains.draw(&mut rng).cards(),
            );
            if !hero.intersects(villain) {
                break (hero, villain);
            }
        };
        let mut full = board;
        while full.len() < 5 {
            let card = Card::from_index(below(&mut rng, NUM_CARDS));
            if !hero.contains(card) && !villain.contains(card) {
                full.insert(card);
            }
        }
        let share = match evaluate(full | hero).cmp(&evaluate(full | villain)) {
            Ordering::Greater => 1.0,
            Ordering::Equal => 0.5,
            Ordering::Less => 0.0,
        };
        sum += share;
        sum_of_squares += share * share;
    }

    let n = samples as f64;
    let mean = sum / n;
    let variance = (sum_of_squares / n - mean * mean).max(0.0);
    Ok(Estimate {
        share: mean,
        std_err: (variance / n).sqrt(),
        samples,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn combo(s: &str) -> Combo {
        s.parse().unwrap()
    }

    fn cards(s: &str) -> CardSet {
        s.parse().unwrap()
    }

    fn range(s: &str) -> Range {
        s.parse().unwrap()
    }

    fn assert_close(a: f64, b: f64) {
        assert!((a - b).abs() < 1e-12, "{a} != {b}");
    }

    fn count_runouts(board: &str, dead: &str) -> usize {
        let mut count = 0;
        for_each_runout(cards(board), cards(dead), |full| {
            assert_eq!(full.len(), 5);
            assert!(!full.intersects(cards(dead)));
            count += 1;
        });
        count
    }

    #[test]
    fn runout_counts_are_binomials() {
        assert_eq!(count_runouts("", "AsAh KdKc"), 1_712_304); // C(48, 5)
        assert_eq!(count_runouts("Ks7d2c", "AsAh QdQc"), 990); // C(45, 2)
        assert_eq!(count_runouts("Ks7d2c9h", "AsAh QdQc"), 44);
        assert_eq!(count_runouts("Ks7d2c9h3s", "AsAh QdQc"), 1);
        assert_eq!(count_runouts("Ks7d2c", ""), 1176); // C(49, 2)
    }

    /// Aces against kings is about 82%. The exact figure depends on how many suits the two
    /// pairs share, so there is one matchup for each of zero, one and two shared suits.
    #[test]
    fn aces_against_kings_before_the_flop() {
        for (aces, kings) in [("AsAh", "KdKc"), ("AsAh", "KsKc"), ("AsAh", "KsKh")] {
            let hero = hand_vs_hand(combo(aces), combo(kings), CardSet::EMPTY).unwrap();
            let villain = hand_vs_hand(combo(kings), combo(aces), CardSet::EMPTY).unwrap();
            println!(
                "{aces} vs {kings}: {:.4}% (tie {:.4}%)",
                100.0 * hero.share(),
                100.0 * hero.tie
            );
            assert!(
                (0.81..0.83).contains(&hero.share()),
                "{aces} vs {kings}: {}",
                hero.share()
            );
            assert_close(hero.win + hero.tie + hero.lose, 1.0);
            assert_close(villain.win + villain.tie + villain.lose, 1.0);
            assert_close(hero.win, villain.lose);
            assert_close(hero.tie, villain.tie);
            assert_close(hero.share() + villain.share(), 1.0);
        }
    }

    #[test]
    #[ignore = "slow"]
    fn aces_against_kings_for_every_suit_combination() {
        let pairs = |class: &str| {
            range(class)
                .iter()
                .map(|(combo, _)| combo)
                .collect::<Vec<_>>()
        };
        let mut shares: Vec<f64> = Vec::new();
        for &aces in &pairs("AA") {
            for &kings in &pairs("KK") {
                let share = hand_vs_hand(aces, kings, CardSet::EMPTY).unwrap().share();
                assert!((0.81..0.83).contains(&share), "{aces} vs {kings}: {share}");
                shares.push(share);
            }
        }
        assert_eq!(shares.len(), 36);
        shares.sort_by(f64::total_cmp);
        shares.dedup();
        println!("distinct AA vs KK equities: {shares:?}");
        assert_eq!(
            shares.len(),
            3,
            "one value for each of zero, one and two shared suits"
        );
    }

    #[test]
    fn a_finished_board_is_decided() {
        let board = cards("Ks7d2c9h3s");
        let set = hand_vs_hand(combo("KdKc"), combo("AsAh"), board).unwrap();
        assert_eq!(
            set,
            Equity {
                win: 1.0,
                tie: 0.0,
                lose: 0.0
            }
        );
        let lost = hand_vs_hand(combo("AsAh"), combo("KdKc"), board).unwrap();
        assert_eq!(lost.share(), 0.0);
        let chop = hand_vs_hand(combo("AsQh"), combo("AdQc"), board).unwrap();
        assert_eq!(
            chop,
            Equity {
                win: 0.0,
                tie: 1.0,
                lose: 0.0
            }
        );
    }

    /// Checks the runout walk against two plain nested loops on a flop.
    #[test]
    fn flop_equity_matches_a_plain_double_loop() {
        let (hero, villain, board) = (combo("AhKh"), combo("QsQd"), cards("Qh7h2c"));
        let dead = hero.cards() | villain.cards() | board;
        let live: Vec<Card> = (CardSet::FULL - dead).iter().collect();
        let (mut win, mut tie, mut total) = (0.0, 0.0, 0.0);
        for (i, &turn) in live.iter().enumerate() {
            for &river in &live[i + 1..] {
                let full = board | CardSet::single(turn) | CardSet::single(river);
                match evaluate(full | hero.cards()).cmp(&evaluate(full | villain.cards())) {
                    Ordering::Greater => win += 1.0,
                    Ordering::Equal => tie += 1.0,
                    Ordering::Less => {}
                }
                total += 1.0;
            }
        }
        let equity = hand_vs_hand(hero, villain, board).unwrap();
        assert_eq!(total, 990.0);
        assert_close(equity.win, win / total);
        assert_close(equity.tie, tie / total);
        // A flush draw against top set is a clear underdog but far from dead.
        assert!((0.2..0.35).contains(&equity.share()), "{}", equity.share());
    }

    #[test]
    fn range_against_range_is_the_weighted_average_of_its_matchups() {
        let board = cards("Ah7d2c9s");
        let (hero, villain) = (range("AA,KK:0.5,7s6s"), range("AKs,AA:0.25,QQ,7h7c:0.75"));
        let (mut win, mut tie, mut lose) = (0.0, 0.0, 0.0);
        for (h, hero_weight) in hero.iter().filter(|(h, _)| !h.intersects(board)) {
            for (v, villain_weight) in villain.iter().filter(|(v, _)| !v.intersects(board)) {
                if h.intersects(v.cards()) {
                    continue;
                }
                let weight = f64::from(hero_weight) * f64::from(villain_weight);
                let matchup = hand_vs_hand(h, v, board).unwrap();
                win += weight * matchup.win;
                tie += weight * matchup.tie;
                lose += weight * matchup.lose;
            }
        }
        let total = win + tie + lose;
        let equity = range_vs_range(&hero, &villain, board).unwrap();
        assert_close(equity.win, win / total);
        assert_close(equity.tie, tie / total);
        assert_close(equity.lose, lose / total);
        assert_close(equity.win + equity.tie + equity.lose, 1.0);

        let reversed = range_vs_range(&villain, &hero, board).unwrap();
        assert_close(reversed.win, equity.lose);
        assert_close(reversed.share() + equity.share(), 1.0);
    }

    #[test]
    fn single_hand_ranges_match_hand_against_hand() {
        let board = cards("Qh7h2c");
        let exact = hand_vs_hand(combo("AhKh"), combo("QsQd"), board).unwrap();
        let ranged = range_vs_range(&range("AhKh"), &range("QsQd"), board).unwrap();
        assert_close(exact.win, ranged.win);
        assert_close(exact.tie, ranged.tie);
    }

    #[test]
    fn bad_inputs_are_rejected() {
        let six = cards("Ks7d2c9h3s4d");
        assert_eq!(
            hand_vs_hand(combo("AsAh"), combo("QdQc"), six),
            Err(EquityError::BoardTooLarge(6))
        );
        let board = cards("Ks7d2c");
        assert_eq!(
            hand_vs_hand(combo("AsAh"), combo("AsAd"), board),
            Err(EquityError::SharedCard)
        );
        assert_eq!(
            hand_vs_hand(combo("KsKh"), combo("QdQc"), board),
            Err(EquityError::SharedCard)
        );
        assert_eq!(
            range_vs_range(&range("AsAh"), &range("AsAd"), board),
            Err(EquityError::NoMatchups)
        );
        assert_eq!(
            monte_carlo(&range("AsAh"), &range("AsAd"), board, 100, 1),
            Err(EquityError::NoMatchups)
        );
        assert_eq!(
            range_vs_range(&range("KsKh"), &range("QQ"), board),
            Err(EquityError::NoMatchups)
        );
    }

    #[test]
    fn monte_carlo_agrees_with_exact_on_twenty_random_matchups() {
        let mut rng = ChaCha8Rng::seed_from_u64(2026);
        for matchup in 0..20 {
            // Deal two hands and a flop or turn from a shuffled deck.
            let mut deck: Vec<Card> = Card::all().collect();
            for i in (1..deck.len()).rev() {
                deck.swap(i, below(&mut rng, i + 1));
            }
            let (hero, villain) = (Combo::new(deck[0], deck[1]), Combo::new(deck[2], deck[3]));
            let board: CardSet = deck[4..7 + matchup % 2].iter().copied().collect();

            let exact = hand_vs_hand(hero, villain, board).unwrap().share();
            let single = |combo: Combo| {
                let mut range = Range::empty();
                range.set(combo, 1.0);
                range
            };
            let estimate = monte_carlo(
                &single(hero),
                &single(villain),
                board,
                20_000,
                matchup as u64,
            )
            .unwrap();
            let error = (estimate.share - exact).abs();
            println!(
                "{hero} vs {villain} on {board}: exact {exact:.4}, estimate {:.4}, {:.2} std errs",
                estimate.share,
                error / estimate.std_err.max(1e-12)
            );
            assert_eq!(estimate.samples, 20_000);
            assert!(
                error <= 3.0 * estimate.std_err + 1e-12,
                "{hero} vs {villain} on {board}"
            );
        }
    }

    #[test]
    fn monte_carlo_handles_weights_and_card_removal() {
        let board = cards("Ah7d2c9s");
        let (hero, villain) = (range("AA,KK:0.5,7s6s"), range("AKs,AA:0.25,QQ,7h7c:0.75"));
        let exact = range_vs_range(&hero, &villain, board).unwrap().share();
        let estimate = monte_carlo(&hero, &villain, board, 200_000, 7).unwrap();
        println!(
            "exact {exact:.5}, estimate {:.5} +- {:.5}",
            estimate.share, estimate.std_err
        );
        assert!((estimate.share - exact).abs() <= 3.0 * estimate.std_err);
    }

    #[test]
    fn monte_carlo_is_repeatable() {
        let (hero, villain) = (range("QQ+,AKs"), range("22+,ATs+,KQs"));
        let first = monte_carlo(&hero, &villain, CardSet::EMPTY, 5_000, 42).unwrap();
        let again = monte_carlo(&hero, &villain, CardSet::EMPTY, 5_000, 42).unwrap();
        let other = monte_carlo(&hero, &villain, CardSet::EMPTY, 5_000, 43).unwrap();
        assert_eq!(first, again);
        assert_ne!(first, other);
        assert!(first.std_err > 0.0 && first.std_err < 0.01);
        assert!((0.5..0.8).contains(&first.share), "{}", first.share);
    }
}
