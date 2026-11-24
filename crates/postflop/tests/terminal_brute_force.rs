//! The O(n) fold and showdown evaluations against a direct O(n²) double loop, on 1,000 seeded
//! random cases.
//!
//! Each case deals a random river board and draws a random weighted range for each player, from
//! a few combos to all 1,326, sometimes the same range for both. It checks the fold evaluation
//! on the flop, the turn or the river of that board, the three in turn from case to case, and
//! the showdown evaluation on the river. Each check is made with each player as the traverser,
//! against a random reach vector for the opponent with many zeros, and with random payoffs.
//!
//! The double loop here is written from the definition and shares nothing with the library but
//! the evaluator and the card types. It also builds its own hand lists, and checks that the
//! library's lists hold the same combos in the same order.

use std::cmp::Ordering;
use std::panic;
use std::thread;

use pk_core::{
    Card, CardSet, Combo, HandClass, HandRank, NUM_CLASSES, NUM_COMBOS, Range, evaluate,
};
use pk_game::Player;
use pk_postflop::{BoardHands, ShowdownPayoff};
use rand_chacha::ChaCha8Rng;
use rand_chacha::rand_core::{Rng, SeedableRng};

const CASES: usize = 1000;

/// Case `i` draws from a generator seeded with `SEED + i`, so a failing case can be replayed
/// alone and the result does not depend on how the cases are split across threads.
const SEED: u64 = 2600;

/// The largest error allowed, as a fraction of the largest value a hand could have: the
/// opponent's total reach times the largest payoff. The largest error measured over the 1,000
/// cases is under 7e-15 of that (run with `--nocapture` to see it).
const TOLERANCE: f64 = 1e-13;

/// A uniform integer in `0..bound`.
fn below(rng: &mut ChaCha8Rng, bound: usize) -> usize {
    ((u128::from(rng.next_u64()) * bound as u128) >> 64) as usize
}

/// A uniform number in `[0, 1)`.
fn unit(rng: &mut ChaCha8Rng) -> f64 {
    (rng.next_u64() >> 11) as f64 / (1u64 << 53) as f64
}

/// A range weight in `(0, 1]`: often exactly 1, sometimes 0.5, otherwise uniform.
fn weight(rng: &mut ChaCha8Rng) -> f32 {
    match below(rng, 4) {
        0 | 1 => 1.0,
        2 => 0.5,
        _ => ((1.0 - unit(rng)) as f32).max(f32::MIN_POSITIVE),
    }
}

/// A random range of varied width. `other` is the opponent's range, if drawn already.
fn random_range(rng: &mut ChaCha8Rng, other: Option<&Range>) -> Range {
    let mut range = Range::empty();
    match below(rng, 8) {
        // Every combo at full weight.
        0 => range = Range::full(),
        // Every combo at a random weight.
        1 => {
            for index in 0..NUM_COMBOS {
                range.set(Combo::from_index(index), weight(rng));
            }
        }
        // A narrow range of up to eight combos, which the board may block entirely.
        2 => {
            for _ in 0..1 + below(rng, 8) {
                range.set(Combo::from_index(below(rng, NUM_COMBOS)), weight(rng));
            }
        }
        // Whole hand classes, as range strings give them.
        3 | 4 => {
            let density = unit(rng);
            for index in 0..NUM_CLASSES {
                if unit(rng) < density {
                    range.set_class(HandClass::from_index(index), weight(rng));
                }
            }
        }
        // Single combos at some density.
        5 | 6 => {
            let density = unit(rng);
            for index in 0..NUM_COMBOS {
                if unit(rng) < density {
                    range.set(Combo::from_index(index), weight(rng));
                }
            }
        }
        // The opponent's range, so that every combo is in both.
        _ => range = other.cloned().unwrap_or_else(Range::full),
    }
    range
}

/// Five distinct random cards.
fn random_board(rng: &mut ChaCha8Rng) -> [Card; 5] {
    let mut deck: Vec<Card> = Card::all().collect();
    for i in 0..5 {
        let j = i + below(rng, deck.len() - i);
        deck.swap(i, j);
    }
    [deck[0], deck[1], deck[2], deck[3], deck[4]]
}

/// A reach vector: a quarter zeros, an eighth ones and the rest uniform. One vector in fifty is
/// all zeros.
fn random_reach(rng: &mut ChaCha8Rng, len: usize) -> Vec<f64> {
    let all_zero = below(rng, 50) == 0;
    (0..len)
        .map(|_| match below(rng, 8) {
            _ if all_zero => 0.0,
            0 | 1 => 0.0,
            2 => 1.0,
            _ => unit(rng),
        })
        .collect()
}

/// A payoff of random sign and magnitude, from about a chip to a million.
fn random_payoff(rng: &mut ChaCha8Rng) -> f64 {
    let magnitude = 10f64.powi(below(rng, 7) as i32);
    (2.0 * unit(rng) - 1.0) * magnitude
}

/// The hands of `range` that can be held on `board`, in combo index order, with their weights.
fn live_hands(range: &Range, board: CardSet) -> Vec<(Combo, f64)> {
    (0..NUM_COMBOS)
        .map(Combo::from_index)
        .filter(|combo| range.weight(*combo) > 0.0 && !combo.intersects(board))
        .map(|combo| (combo, f64::from(range.weight(combo))))
        .collect()
}

/// What the double loop saw, to show that the cases cover what they should.
#[derive(Debug, Default)]
struct Coverage {
    /// Pairs of hands that share a card and are skipped, where the opponent's reach is positive.
    shared_card: u64,
    /// Pairs of the same combo, where the opponent's reach is positive.
    same_combo: u64,
    /// Pairs that tie at showdown, where the opponent's reach is positive.
    ties: u64,
    /// Opponent hands with zero reach.
    zero_reach: u64,
    /// Evaluations where one of the two lists was empty.
    empty_lists: u64,
    evaluations: u64,
}

impl Coverage {
    fn merge(&mut self, other: &Coverage) {
        self.shared_card += other.shared_card;
        self.same_combo += other.same_combo;
        self.ties += other.ties;
        self.zero_reach += other.zero_reach;
        self.empty_lists += other.empty_lists;
        self.evaluations += other.evaluations;
    }
}

/// The direct sum `value[h] = Σ reach[o] · u(h, o)` over the opponent's hands `o` that share
/// no card with `h`, and beside it `Σ reach[o] · |u(h, o)|`. `payoff(h, o)` gives `u(h, o)`
/// and whether the two hands tie.
fn brute_force(
    mine: &[Combo],
    theirs: &[Combo],
    reach: &[f64],
    payoff: impl Fn(usize, usize) -> (f64, bool),
    coverage: &mut Coverage,
) -> (Vec<f64>, Vec<f64>) {
    let their_cards: Vec<u64> = theirs.iter().map(|combo| combo.cards().bits()).collect();
    let mut values = vec![0.0; mine.len()];
    let mut magnitudes = vec![0.0; mine.len()];
    for (h, my_combo) in mine.iter().enumerate() {
        let my_cards = my_combo.cards().bits();
        let (mut value, mut magnitude) = (0.0, 0.0);
        for (o, (&cards, &r)) in their_cards.iter().zip(reach).enumerate() {
            if my_cards & cards != 0 {
                if r > 0.0 {
                    coverage.shared_card += 1;
                    coverage.same_combo += u64::from(my_cards == cards);
                }
                continue;
            }
            let (u, tie) = payoff(h, o);
            value += r * u;
            magnitude += r * u.abs();
            coverage.ties += u64::from(tie && r > 0.0);
        }
        values[h] = value;
        magnitudes[h] = magnitude;
    }
    coverage.zero_reach += reach.iter().filter(|&&r| r == 0.0).count() as u64;
    coverage.empty_lists += u64::from(mine.is_empty() || theirs.is_empty());
    coverage.evaluations += 1;
    (values, magnitudes)
}

/// The largest errors seen, and a check of each against the tolerance.
#[derive(Debug, Default)]
struct Errors {
    /// The largest absolute error, in chips.
    absolute: f64,
    /// The largest error as a fraction of the opponent's total reach times the largest payoff.
    of_scale: f64,
    /// The largest error as a fraction of the hand's own `Σ reach[o] · |u(h, o)|`.
    of_magnitude: f64,
    hands: u64,
}

impl Errors {
    fn check(
        &mut self,
        got: &[f64],
        expected: &[f64],
        magnitudes: &[f64],
        scale: f64,
        what: &dyn Fn() -> String,
    ) {
        assert_eq!(got.len(), expected.len());
        for h in 0..got.len() {
            let error = (got[h] - expected[h]).abs();
            assert!(
                error <= TOLERANCE * scale,
                "{}, hand {h}: got {}, expected {}, error {error:e} against a scale of {scale:e}",
                what(),
                got[h],
                expected[h]
            );
            self.absolute = self.absolute.max(error);
            if scale > 0.0 {
                self.of_scale = self.of_scale.max(error / scale);
            }
            if magnitudes[h] > 0.0 {
                self.of_magnitude = self.of_magnitude.max(error / magnitudes[h]);
            }
            self.hands += 1;
        }
    }

    fn merge(&mut self, other: &Errors) {
        self.absolute = self.absolute.max(other.absolute);
        self.of_scale = self.of_scale.max(other.of_scale);
        self.of_magnitude = self.of_magnitude.max(other.of_magnitude);
        self.hands += other.hands;
    }
}

/// Everything a run of cases measured.
#[derive(Debug, Default)]
struct Tally {
    coverage: Coverage,
    fold: Errors,
    showdown: Errors,
    /// Fold checks on the flop, turn and river.
    fold_boards: [usize; 3],
}

/// Builds the hands for a board and checks the library's hand lists against `live_hands`.
fn board_hands(board: CardSet, ranges: [&Range; 2], case: usize) -> BoardHands {
    let hands = BoardHands::new(board, ranges[0], ranges[1]);
    for player in [Player::Oop, Player::Ip] {
        let list = hands.hands(player);
        let (combos, weights): (Vec<Combo>, Vec<f64>) = live_hands(ranges[player.index()], board)
            .into_iter()
            .unzip();
        assert_eq!(list.combos(), combos, "case {case}: {player} hand list");
        assert_eq!(list.weights(), weights, "case {case}: {player} weights");
        assert_eq!(list.board(), board);
    }
    hands
}

/// Draws case `case` and checks both evaluations on it.
fn run_case(case: usize, tally: &mut Tally) {
    let mut rng = ChaCha8Rng::seed_from_u64(SEED + case as u64);
    let cards = random_board(&mut rng);
    let oop_range = random_range(&mut rng, None);
    let ip_range = random_range(&mut rng, Some(&oop_range));
    let ranges = [&oop_range, &ip_range];

    // A fold on the flop, the turn or the river.
    let street = case % 3;
    tally.fold_boards[street] += 1;
    let board: CardSet = cards[..3 + street].iter().copied().collect();
    let hands = board_hands(board, ranges, case);
    for traverser in [Player::Oop, Player::Ip] {
        let mine = hands.hands(traverser).combos();
        let theirs = hands.hands(traverser.opponent()).combos();
        let reach = random_reach(&mut rng, theirs.len());
        let payoff = random_payoff(&mut rng);

        let mut values = vec![f64::NAN; mine.len()];
        hands.fold_values(traverser, &reach, payoff, &mut values);
        let always = |_, _| (payoff, false);
        let (expected, magnitudes) = brute_force(mine, theirs, &reach, always, &mut tally.coverage);
        let scale = reach.iter().sum::<f64>() * payoff.abs();
        let what = || format!("case {case}: fold on {board}, {traverser} traversing");
        tally
            .fold
            .check(&values, &expected, &magnitudes, scale, &what);
    }

    // A showdown on the river.
    let board: CardSet = cards.iter().copied().collect();
    let hands = board_hands(board, ranges, case);
    for traverser in [Player::Oop, Player::Ip] {
        let mine = hands.hands(traverser).combos();
        let theirs = hands.hands(traverser.opponent()).combos();
        let reach = random_reach(&mut rng, theirs.len());
        let payoff = ShowdownPayoff {
            win: random_payoff(&mut rng),
            tie: random_payoff(&mut rng),
            lose: random_payoff(&mut rng),
        };

        let strength = |combo: &Combo| evaluate(board | combo.cards());
        let my_ranks: Vec<HandRank> = mine.iter().map(strength).collect();
        let their_ranks: Vec<HandRank> = theirs.iter().map(strength).collect();
        let by_result = |h: usize, o: usize| match my_ranks[h].cmp(&their_ranks[o]) {
            Ordering::Greater => (payoff.win, false),
            Ordering::Equal => (payoff.tie, true),
            Ordering::Less => (payoff.lose, false),
        };

        let mut values = vec![f64::NAN; mine.len()];
        hands.showdown_values(traverser, &reach, payoff, &mut values);
        let (expected, magnitudes) =
            brute_force(mine, theirs, &reach, by_result, &mut tally.coverage);
        let largest = payoff
            .win
            .abs()
            .max(payoff.tie.abs())
            .max(payoff.lose.abs());
        let scale = reach.iter().sum::<f64>() * largest;
        let what = || format!("case {case}: showdown on {board}, {traverser} traversing");
        tally
            .showdown
            .check(&values, &expected, &magnitudes, scale, &what);
    }
}

#[test]
fn fold_and_showdown_sweeps_equal_the_double_loop_on_1000_random_cases() {
    // A few threads keep the double loops to a few seconds. Every case seeds its own
    // generator and the tallies combine by sums and maxima, so the split changes nothing.
    let threads = thread::available_parallelism().map_or(1, |n| n.get().min(4));
    let tallies: Vec<Tally> = thread::scope(|scope| {
        let workers: Vec<_> = (0..threads)
            .map(|first| {
                scope.spawn(move || {
                    let mut tally = Tally::default();
                    for case in (first..CASES).step_by(threads) {
                        run_case(case, &mut tally);
                    }
                    tally
                })
            })
            .collect();
        workers
            .into_iter()
            .map(|worker| {
                worker
                    .join()
                    .unwrap_or_else(|panic| panic::resume_unwind(panic))
            })
            .collect()
    });

    let mut total = Tally::default();
    for tally in &tallies {
        total.coverage.merge(&tally.coverage);
        total.fold.merge(&tally.fold);
        total.showdown.merge(&tally.showdown);
        for (sum, count) in total.fold_boards.iter_mut().zip(tally.fold_boards) {
            *sum += count;
        }
    }

    println!(
        "fold checks on (flop, turn, river): {:?}",
        total.fold_boards
    );
    println!("fold errors: {:?}", total.fold);
    println!("showdown errors: {:?}", total.showdown);
    println!("coverage: {:?}", total.coverage);
    assert_eq!(total.fold_boards.iter().sum::<usize>(), CASES);
    assert_eq!(total.coverage.evaluations, 4 * CASES as u64);
    let coverage = &total.coverage;
    assert!(coverage.shared_card > 0 && coverage.same_combo > 0 && coverage.ties > 0);
    assert!(coverage.zero_reach > 0 && coverage.empty_lists > 0);
}
