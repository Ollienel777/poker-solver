//! Benchmarks for the seven-card evaluator.

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use pk_core::{CardSet, evaluate};

/// A fixed, spread-out sample of seven-card hands.
fn sample_hands(count: usize) -> Vec<CardSet> {
    let mut hands = Vec::with_capacity(count);
    let mut state = 0x9E37_79B9_7F4A_7C15u64;
    while hands.len() < count {
        let mut bits = 0u64;
        while bits.count_ones() < 7 {
            // SplitMix64 step.
            state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = state;
            z = (z ^ z >> 30).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ z >> 27).wrapping_mul(0x94D0_49BB_1331_11EB);
            bits |= 1 << ((z ^ z >> 31) % 52);
        }
        hands.push(CardSet::from_bits(bits));
    }
    hands
}

fn bench_evaluate(c: &mut Criterion) {
    let hands = sample_hands(10_000);
    c.bench_function("evaluate 10k seven-card hands", |b| {
        b.iter(|| {
            let mut sum = 0u64;
            for &hand in &hands {
                sum += u64::from(evaluate(black_box(hand)).value());
            }
            sum
        })
    });
}

criterion_group!(benches, bench_evaluate);
criterion_main!(benches);
