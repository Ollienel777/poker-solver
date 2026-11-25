//! Benchmarks for terminal evaluation: at full range width, 1,081 hands a side on the river; at
//! the width of the benchmark spots of `benches/solve.rs`, about 300 hands a side on a river of
//! `turn_srp`; and through the game, where a terminal on a board after the spot's gathers the
//! opponent's reach into the board's hand order and writes the values back, in room the walks
//! keep from one terminal to the next.

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use pk_cfr::PublicGame;
use pk_core::{CardSet, Range};
use pk_game::Player;
use pk_postflop::{BoardHands, PostflopGame, ShowdownPayoff, Spot};

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

fn bench_spot_width(c: &mut Criterion) {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/benches/data/turn_srp.toml");
    let spot = Spot::load(path).unwrap();
    let river: CardSet = spot.board | "7h".parse::<CardSet>().unwrap();
    let hands = BoardHands::new(river, &spot.oop_range, &spot.ip_range);
    let widths = [Player::Oop, Player::Ip].map(|player| hands.hands(player).len());
    assert_eq!(widths, [301, 284], "the benchmark spot changed");
    let reach = sample_reach(widths[1]);
    let mut values = vec![0.0; widths[0]];
    let payoff = ShowdownPayoff {
        win: 1400.0,
        tie: 450.0,
        lose: -500.0,
    };
    c.bench_function("fold, turn_srp river", |b| {
        b.iter(|| {
            hands.fold_values(Player::Oop, black_box(&reach), 1000.0, &mut values);
            values[0]
        })
    });
    c.bench_function("showdown, turn_srp river", |b| {
        b.iter(|| {
            hands.showdown_values(Player::Oop, black_box(&reach), payoff, &mut values);
            values[0]
        })
    });

    // Through the game, with the reach and values indexed by the spot's hand lists, and the
    // room for the gather and the scatter kept from call to call, as the walks keep it.
    let game = PostflopGame::from_spot(&spot).unwrap();
    let reach = sample_reach(game.num_hands(1));
    let mut values = vec![0.0; game.num_hands(0)];
    let mut scratch = Vec::new();
    for (name, path) in [
        ("game fold, turn_srp spot's board", "b50 f"),
        ("game fold, turn_srp river", "x x 7h b50 f"),
        ("game showdown, turn_srp river", "x x 7h x x"),
    ] {
        let node = game.find(path).unwrap() as usize;
        c.bench_function(name, |b| {
            b.iter(|| {
                let reach = black_box(&reach);
                game.terminal_values_with(node, 0, reach, &mut values, &mut scratch);
                values[0]
            })
        });
    }
}

criterion_group!(benches, bench_terminal, bench_spot_width);
criterion_main!(benches);
