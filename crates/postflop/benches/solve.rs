//! Benchmarks for the solve's hot paths, on two fixed spots in `benches/data`: `turn_srp`, a turn
//! spot with about 300 hands a side and two bet sizes, a raise and all-in on both streets, and
//! `flop_srp`, a flop spot with about 300 hands a side and one half-pot bet per street.
//!
//! For each spot: building the betting tree, building the game (the public tree and the hands on
//! every board), building the solver, one DCFR iteration from the state after a few iterations,
//! the average strategy, one best response, and one exploitability measurement as the solve makes
//! it (both players' values and best responses). `docs/BENCHMARKS.md` records the numbers and
//! how to compare them; the spots must not change, and the sizes are checked before anything is
//! timed so that a change to the tree builder cannot change the benchmark unnoticed.
//!
//! ```text
//! cargo bench -p pk-postflop --bench solve
//! cargo bench -p pk-postflop --bench solve -- turn_srp
//! ```

use std::cell::OnceCell;
use std::hint::black_box;
use std::time::Duration;

use criterion::{BatchSize, Criterion, SamplingMode, criterion_group, criterion_main};
use pk_cfr::{Dcfr, PublicGame, Schedule, VectorCfr, vector_best_response};
use pk_game::Player;
use pk_postflop::{PostflopGame, Spot, exploitability};

/// The iterations run before the timed one, so that the strategies are no longer uniform.
const WARMUP: u64 = 5;

/// A benchmark spot and the sizes it must have.
struct Fixture {
    name: &'static str,
    hands: [usize; 2],
    public_nodes: usize,
    /// The time to give each benchmark that runs an iteration or a measurement.
    seconds: [u64; 2],
}

const FIXTURES: [Fixture; 2] = [
    Fixture {
        name: "turn_srp",
        hands: [313, 291],
        public_nodes: 16_317,
        seconds: [10, 15],
    },
    Fixture {
        name: "flop_srp",
        hands: [318, 295],
        public_nodes: 191_844,
        seconds: [40, 60],
    },
];

fn load(name: &str) -> Spot {
    let path = format!("{}/benches/data/{name}.toml", env!("CARGO_MANIFEST_DIR"));
    Spot::load(path).unwrap()
}

fn bench_spot(c: &mut Criterion, fixture: &Fixture) {
    let spot = load(fixture.name);
    let game = PostflopGame::from_spot(&spot).unwrap();
    let hands = [Player::Oop, Player::Ip].map(|player| game.hands(player).len());
    assert_eq!(hands, fixture.hands, "{}: the hands changed", fixture.name);
    assert_eq!(
        game.num_nodes(),
        fixture.public_nodes,
        "{}: the public tree changed",
        fixture.name
    );
    let schedule = Schedule::Discounted(Dcfr::default());
    // The solver after the warm-up, and its average strategy, made when a benchmark that needs
    // them first runs, so that a filtered run of the other spot does not pay for them.
    let cell = OnceCell::new();
    let warm = || {
        cell.get_or_init(|| {
            let mut solver = VectorCfr::new(game.clone(), schedule);
            solver.run(WARMUP, |_| {});
            let average = solver.average_strategy();
            (solver, average)
        })
    };

    let mut group = c.benchmark_group(fixture.name);
    group.sample_size(10);
    group.bench_function("betting tree", |b| b.iter(|| spot.build_tree().unwrap()));
    group.bench_function("game", |b| {
        b.iter_with_large_drop(|| PostflopGame::from_spot(black_box(&spot)).unwrap())
    });
    group.bench_function("solver", |b| {
        b.iter_batched(
            || game.clone(),
            |game| VectorCfr::new(game, schedule),
            BatchSize::PerIteration,
        )
    });
    group.bench_function("average strategy", |b| {
        let (solver, _) = warm();
        b.iter_with_large_drop(|| solver.average_strategy())
    });

    // The long ones: one run per sample.
    group.sampling_mode(SamplingMode::Flat);
    group.measurement_time(Duration::from_secs(fixture.seconds[0]));
    group.bench_function("iteration", |b| {
        let (solver, _) = warm();
        b.iter_batched(
            || solver.clone(),
            |mut solver| {
                solver.iterate();
                solver
            },
            BatchSize::PerIteration,
        )
    });
    group.bench_function("best response", |b| {
        let (solver, average) = warm();
        b.iter_with_large_drop(|| vector_best_response(solver.game(), black_box(average), 0))
    });
    group.measurement_time(Duration::from_secs(fixture.seconds[1]));
    group.bench_function("exploitability", |b| {
        let (solver, average) = warm();
        b.iter(|| exploitability(solver.game(), black_box(average)))
    });
    group.finish();
}

fn bench_solve(c: &mut Criterion) {
    for fixture in &FIXTURES {
        bench_spot(c, fixture);
    }
}

criterion_group!(benches, bench_solve);
criterion_main!(benches);
