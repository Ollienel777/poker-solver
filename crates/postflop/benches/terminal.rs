//! Benchmarks for terminal evaluation at full range width: 1,081 hands a side on the river.

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use pk_core::{CardSet, Range};
use pk_game::Player;
use pk_postflop::{BoardHands, ShowdownPayoff};

/// A fixed spread of reach probabilities in `[0, 1)`, a quarter of them zero.
fn sample_reach(len: usize) -> Vec<f64> {
    let mut state = 0x9E37_79B9_7F4A_7C15u64;
    (0..len)
        .map(|_| {
            // SplitMix64 step.
            state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = state;
            z = (z ^ z >> 30).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ z >> 27).wrapping_mul(0x94D0_49BB_1331_11EB);
            z ^= z >> 31;
            if z.is_multiple_of(4) {
                0.0
            } else {
                (z >> 11) as f64 / (1u64 << 53) as f64
            }
        })
        .collect()
}

fn bench_terminal(c: &mut Criterion) {
    let board: CardSet = "Ks7d2c9h3s".parse().unwrap();
    let full = Range::full();
    let hands = BoardHands::new(board, &full, &full);
    let n = hands.hands(Player::Ip).len();
    assert_eq!(n, 1081);
    let reach = sample_reach(n);
    let mut values = vec![0.0; hands.hands(Player::Oop).len()];
    let payoff = ShowdownPayoff {
        win: 1400.0,
        tie: 450.0,
        lose: -500.0,
    };

    c.bench_function("river setup, full ranges", |b| {
        b.iter(|| BoardHands::new(black_box(board), &full, &full))
    });
    c.bench_function("fold, full ranges", |b| {
        b.iter(|| {
            hands.fold_values(Player::Oop, black_box(&reach), 1000.0, &mut values);
            values[0]
        })
    });
    c.bench_function("showdown, full ranges", |b| {
        b.iter(|| {
            hands.showdown_values(Player::Oop, black_box(&reach), payoff, &mut values);
            values[0]
        })
    });
}

criterion_group!(benches, bench_terminal);
criterion_main!(benches);
