//! The memory estimate against what a solve allocates, measured by a counting allocator.
//!
//! The allocator counts the bytes the program asks for, as `MemoryEstimate::heap_bytes` does,
//! and keeps the most held at once. Each test solves spots one at a time, holding a lock so that
//! no other test of this file allocates meanwhile, and compares the most the solve held above
//! what was held before it with the estimate.
//!
//! The estimate must never be below the measured peak, since its job is to refuse a solve that
//! would not fit, and must be within 5% above it, so that it does not refuse solves that would.
//!
//! Those are the bounds on one thread, where the solve holds the same at every run. On several
//! threads the solve holds its pool's own memory, which depends on how the pool's threads have
//! shared out the jobs, and the estimate counts the growth of every child's vector of frames as
//! if it came at the peak; the threads of an earlier pool, the solve's or a test's, may also
//! still be freeing theirs while the next solve is measured. So the estimate must still never be
//! below the measured peak, and is held to 5% above it plus 16 KiB a thread and 64 KiB.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering::SeqCst};

use pk_cfr::{Dcfr, Schedule, VectorCfr};
use pk_postflop::memory::POOL_THREAD_BYTES;
use pk_postflop::{MemoryEstimate, PostflopGame, SolveOptions, Spot, estimate_memory, solve};
use rayon::ThreadPoolBuilder;

/// The system allocator, counting the bytes held and the most held since the last reset.
struct Counting;

static HELD: AtomicU64 = AtomicU64::new(0);
static MOST: AtomicU64 = AtomicU64::new(0);
/// The blocks asked for so far, by every thread.
static TAKEN: AtomicU64 = AtomicU64::new(0);

thread_local! {
    /// The blocks this thread has asked for so far.
    static MINE: Cell<u64> = const { Cell::new(0) };
}

fn take(bytes: usize) {
    TAKEN.fetch_add(1, SeqCst);
    // A thread that is ending may have no thread-local left to count in.
    let _ = MINE.try_with(|mine| mine.set(mine.get() + 1));
    let held = HELD.fetch_add(bytes as u64, SeqCst) + bytes as u64;
    MOST.fetch_max(held, SeqCst);
}

/// The blocks the calling thread has asked for so far.
fn mine() -> u64 {
    MINE.with(Cell::get)
}

fn give(bytes: usize) {
    HELD.fetch_sub(bytes as u64, SeqCst);
}

// SAFETY: every call goes to the system allocator with the same arguments; the counting only
// adds and subtracts sizes.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        take(layout.size());
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        take(layout.size());
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        give(layout.size());
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // Counted as a new block taken before the old one is given back, as a reallocation
        // that cannot grow in place copies.
        take(new_size);
        let moved = unsafe { System.realloc(ptr, layout, new_size) };
        give(layout.size());
        moved
    }
}

#[global_allocator]
static COUNTING: Counting = Counting;

/// Held while a test measures, so that the tests of this file measure one at a time.
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

fn load(path: &str) -> Spot {
    Spot::load(format!("{}/{path}.toml", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

/// Solves `spot` and returns the estimate and the most the solve held above what was held
/// before it, the solution included.
fn measure(spot: &Spot, options: &SolveOptions) -> (MemoryEstimate, u64) {
    let estimate = estimate_memory(spot, options).unwrap();
    let before = HELD.load(SeqCst);
    MOST.store(before, SeqCst);
    let solution = solve(spot, options, |_| {}).unwrap();
    let most = MOST.load(SeqCst) - before;
    drop(solution);
    (estimate, most)
}

/// Checks the estimate against the measured peak, and prints both.
fn check(name: &str, spot: &Spot, options: &SolveOptions) {
    let (estimate, measured) = measure(spot, options);
    print_and_check_below(name, &estimate, measured);
    let ratio = estimate.heap_bytes as f64 / measured as f64;
    assert!(
        ratio <= 1.05,
        "{name}: the estimate, {} bytes, is more than 5% above the measured peak, \
         {measured}\n{estimate}",
        estimate.heap_bytes
    );
}

/// Checks the estimate of a solve on `options.threads` threads against the measured peak, as
/// the [file's documentation](self) says, and prints both.
fn check_threads(name: &str, spot: &Spot, options: &SolveOptions) {
    let (estimate, measured) = measure(spot, options);
    print_and_check_below(name, &estimate, measured);
    let allowed = measured + measured / 20 + 16_384 * options.threads as u64 + 65_536;
    assert!(
        estimate.heap_bytes <= allowed,
        "{name}: the estimate, {} bytes, is more than 5%, 16 KiB a thread and 64 KiB above the \
         measured peak, {measured}\n{estimate}",
        estimate.heap_bytes
    );
}

/// Prints the estimate and the measured peak, and checks that the estimate is not below it.
fn print_and_check_below(name: &str, estimate: &MemoryEstimate, measured: u64) {
    let ratio = estimate.heap_bytes as f64 / measured as f64;
    println!(
        "{name}: estimated {} bytes {}, measured {measured}, ratio {ratio:.4}; total with \
         allowances {}; by phase {}, {}, {}, {}",
        estimate.heap_bytes,
        estimate.peak_phase,
        estimate.total,
        estimate.building,
        estimate.iterating,
        estimate.measuring,
        estimate.finishing
    );
    assert!(
        estimate.heap_bytes >= measured,
        "{name}: the estimate, {} bytes, is below the measured peak, {measured}\n{estimate}",
        estimate.heap_bytes
    );
}

/// Two measurements and a third at the end, so that the solve holds two profiles at once, on one
/// thread.
fn capped(iterations: u64) -> SolveOptions {
    SolveOptions {
        target: None,
        max_iterations: iterations,
        check_every: 10,
        threads: 1,
        ..SolveOptions::default()
    }
}

#[test]
fn the_estimate_covers_what_a_solve_allocates_and_is_within_five_percent() {
    let _one = ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let spots = [
        (
            "river deep_high_spr",
            "../../reference/spots/river/deep_high_spr",
            25,
        ),
        ("turn_small", "tests/data/turn_small", 25),
        ("turn_srp", "benches/data/turn_srp", 12),
        ("flop_small", "tests/data/flop_small", 12),
    ];
    for (name, path, iterations) in spots {
        check(name, &load(path), &capped(iterations));
    }
    // One measurement only: the estimate counts one profile.
    let spot = load("tests/data/turn_small");
    let (estimate, _) = measure(&spot, &capped(10));
    assert_eq!(estimate.profiles_held, 1);
    check("turn_small, measured once", &spot, &capped(10));
    // To the default target, as the solve runs by default.
    let to_target = SolveOptions {
        threads: 1,
        ..SolveOptions::default()
    };
    check("turn_small, to its target", &spot, &to_target);
}

#[test]
fn on_several_threads_the_estimate_covers_what_a_solve_allocates() {
    // More threads than the CI's cores, so that the threads interleave there too.
    let _one = ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let on = |threads: usize, iterations: u64| SolveOptions {
        threads,
        ..capped(iterations)
    };
    for threads in [2, 4, 16] {
        for (name, path, iterations) in [
            ("turn_small", "tests/data/turn_small", 25),
            ("flop_small", "tests/data/flop_small", 12),
        ] {
            let name = format!("{name}, {threads} threads");
            check_threads(&name, &load(path), &on(threads, iterations));
        }
    }
    check_threads(
        "turn_srp, 16 threads",
        &load("benches/data/turn_srp"),
        &on(16, 12),
    );
    // A river spot has no chance node to split, so it makes no pool and holds what it holds on
    // one thread, within the bounds of one thread.
    let river = load("../../reference/spots/river/deep_high_spr");
    let one = estimate_memory(&river, &capped(25)).unwrap();
    let many = estimate_memory(&river, &on(16, 25)).unwrap();
    assert_eq!((many.threads, many.thread_pool), (1, 0));
    assert_eq!(many.heap_bytes, one.heap_bytes);
    check("river deep_high_spr, 16 threads", &river, &on(16, 25));
}

#[test]
fn after_the_first_iteration_an_iteration_asks_for_no_memory() {
    // The estimate counts the CFR walk's working memory as kept by the solver from its first
    // iteration on. So, once the first iteration has sized it, an iteration takes nothing
    // beyond what the solver already holds: the thread that walks asks for no block at all,
    // not even one it gives straight back. The count is this thread's own, because other
    // threads of the test's process may allocate meanwhile, which the lock cannot stop: the
    // harness when another test ends, and the threads of an earlier test's pool as they end.
    let _one = ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    for path in ["tests/data/turn_small", "tests/data/flop_small"] {
        let game = PostflopGame::from_spot(&load(path)).unwrap();
        let mut solver = VectorCfr::new(game, Schedule::Discounted(Dcfr::default()));
        solver.iterate();
        let before = mine();
        for _ in 0..3 {
            solver.iterate();
        }
        assert_eq!(mine(), before, "{path}");
    }
}

#[test]
fn after_the_first_iteration_the_walks_on_several_threads_ask_for_no_memory() {
    // On several threads the solver keeps a workspace for each outcome of the chance nodes it
    // splits, which the first iteration sizes, and rayon holds each task of a split on the
    // stack of the thread that forks it. So once the first iteration in a pool has sized the
    // workspaces, the walks take nothing.
    //
    // Here the walks run on the pool's threads, so the count has to be the whole process's,
    // and the process is not held to nothing: a thread of the pool registers an epoch record
    // with rayon's queues the first time it steals a task, in whichever iteration the machine
    // lets it, which the estimate counts under the pool (`POOL_THREAD_BYTES`); and, as on one
    // thread, the harness and the threads of an earlier test's pool may allocate meanwhile. On
    // a machine with fewer cores than the pool has threads, an iteration held to nothing at
    // all failed for 1,564 bytes. What holds on any machine is that, over many iterations,
    // the blocks asked for are at most a few per thread of the pool, however many the
    // iterations, and what is held grows by no more than the pool's allowance for its threads:
    // a walk that asked for one block an iteration would ask for more. On the flop spot, whose
    // iterations are slower and so fewer here, a block per child of a split would.
    //
    // The iterations run in one call into the pool. The solve calls into its pool once per
    // iteration, and that call queues a job, which takes a block of rayon's queue every few
    // dozen calls.
    const THREADS: u64 = 8;
    /// More than the process asks for per thread of the pool, apart from the walks.
    const BLOCKS_PER_THREAD: u64 = 4;
    let _one = ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let pool = ThreadPoolBuilder::new()
        .num_threads(THREADS as usize)
        .build()
        .unwrap();
    for (path, iterations) in [("tests/data/turn_small", 200), ("tests/data/flop_small", 5)] {
        let game = PostflopGame::from_spot(&load(path)).unwrap();
        let mut solver = VectorCfr::new(game, Schedule::Discounted(Dcfr::default()));
        pool.install(|| {
            solver.iterate();
            let before = HELD.load(SeqCst);
            MOST.store(before, SeqCst);
            let taken = TAKEN.load(SeqCst);
            for _ in 0..iterations {
                solver.iterate();
            }
            let blocks = TAKEN.load(SeqCst) - taken;
            let grown = MOST.load(SeqCst) - before;
            println!(
                "{path}, {THREADS} threads: {blocks} blocks asked for and {grown} bytes more \
                 held in {iterations} iterations after the first"
            );
            assert!(
                blocks <= THREADS * BLOCKS_PER_THREAD,
                "{path}: {blocks} blocks asked for in {iterations} iterations"
            );
            assert!(
                grown <= THREADS * POOL_THREAD_BYTES,
                "{path}: the most held grew by {grown} bytes in {iterations} iterations"
            );
        });
    }
}

#[test]
#[ignore = "slow"]
fn the_estimate_covers_what_the_flop_benchmark_spot_allocates() {
    // About 1 GB, and a few minutes in a test build.
    let _one = ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    check("flop_srp", &load("benches/data/flop_srp"), &capped(12));
}

#[test]
#[ignore = "slow"]
fn on_16_threads_the_estimate_covers_what_the_flop_benchmark_spot_allocates() {
    let _one = ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let options = SolveOptions {
        threads: 16,
        ..capped(12)
    };
    let spot = load("benches/data/flop_srp");
    check_threads("flop_srp, 16 threads", &spot, &options);
    let one = estimate_memory(&spot, &capped(12)).unwrap();
    let many = estimate_memory(&spot, &options).unwrap();
    println!(
        "flop_srp: estimated {} bytes more on 16 threads than on one",
        many.heap_bytes - one.heap_bytes
    );
}

#[test]
fn the_estimate_covers_what_a_merged_solve_allocates_and_is_within_five_percent() {
    // Spots that a relabelling of the suits keeps, which the solve iterates on their merged
    // games, with the bounds of one thread; and the benchmark's turn spot with merging off,
    // which the default now merges.
    let _one = ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    for (name, iterations) in [
        ("flop_monotone", 12),
        ("flop_two_tone", 12),
        ("turn_four_flush", 25),
    ] {
        let spot = load(&format!("tests/data/{name}"));
        let estimate = estimate_memory(&spot, &capped(iterations)).unwrap();
        assert!(estimate.merged, "{name}");
        check(name, &spot, &capped(iterations));
    }
    let off = SolveOptions {
        merge_isomorphic: false,
        ..capped(12)
    };
    let spot = load("benches/data/turn_srp");
    assert!(!estimate_memory(&spot, &off).unwrap().merged);
    check("turn_srp, merging off", &spot, &off);
}

#[test]
fn on_several_threads_the_estimate_covers_what_a_merged_solve_allocates() {
    let _one = ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    for threads in [2, 16] {
        for (name, iterations) in [("flop_monotone", 12), ("turn_four_flush", 25)] {
            let options = SolveOptions {
                threads,
                ..capped(iterations)
            };
            let spot = load(&format!("tests/data/{name}"));
            check_threads(&format!("{name}, {threads} threads"), &spot, &options);
        }
    }
}
