//! Counts the category of every seven-card hand and checks the totals against the known values.
//!
//! Run with `cargo test --release -- --ignored`.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::Instant;

use pk_core::{CardSet, Category, NUM_CARDS, evaluate};

const EXPECTED: [(Category, u64); 9] = [
    (Category::HighCard, 23_294_460),
    (Category::Pair, 58_627_800),
    (Category::TwoPair, 31_433_400),
    (Category::ThreeOfAKind, 6_461_620),
    (Category::Straight, 6_180_020),
    (Category::Flush, 4_047_644),
    (Category::FullHouse, 3_473_184),
    (Category::FourOfAKind, 224_848),
    (Category::StraightFlush, 41_584),
];
const TOTAL_HANDS: u64 = 133_784_560;

/// Counts the hands whose lowest card has index `first`.
fn count_with_lowest_card(first: usize) -> [u64; 9] {
    let n = NUM_CARDS;
    let mut counts = [0u64; 9];
    let m1 = 1u64 << first;
    for b in first + 1..n {
        let m2 = m1 | 1 << b;
        for c in b + 1..n {
            let m3 = m2 | 1 << c;
            for d in c + 1..n {
                let m4 = m3 | 1 << d;
                for e in d + 1..n {
                    let m5 = m4 | 1 << e;
                    for f in e + 1..n {
                        let m6 = m5 | 1 << f;
                        for g in f + 1..n {
                            let hand = CardSet::from_bits(m6 | 1 << g);
                            counts[evaluate(hand).category() as usize] += 1;
                        }
                    }
                }
            }
        }
    }
    counts
}

#[test]
#[ignore = "slow"]
fn seven_card_category_counts_are_exact() {
    let started = Instant::now();
    let next = AtomicUsize::new(0);
    let threads = thread::available_parallelism().map_or(1, |n| n.get());

    let mut counts = [0u64; 9];
    thread::scope(|scope| {
        let workers: Vec<_> = (0..threads)
            .map(|_| {
                scope.spawn(|| {
                    let mut local = [0u64; 9];
                    loop {
                        let first = next.fetch_add(1, Ordering::Relaxed);
                        if first >= NUM_CARDS {
                            return local;
                        }
                        let part = count_with_lowest_card(first);
                        for (total, n) in local.iter_mut().zip(part) {
                            *total += n;
                        }
                    }
                })
            })
            .collect();
        for worker in workers {
            for (total, n) in counts.iter_mut().zip(worker.join().unwrap()) {
                *total += n;
            }
        }
    });

    let elapsed = started.elapsed().as_secs_f64();
    let total: u64 = counts.iter().sum();
    println!("{total} hands in {elapsed:.1}s on {threads} threads");
    for (category, expected) in EXPECTED {
        let counted = counts[category as usize];
        println!("{category:>16}: {counted:>11} (expected {expected:>11})");
    }

    assert_eq!(total, TOTAL_HANDS);
    for (category, expected) in EXPECTED {
        assert_eq!(counts[category as usize], expected, "{category}");
    }
}
