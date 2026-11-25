//! The memory estimate against what a solve allocates, measured by a counting allocator.
//!
//! The allocator counts the bytes the program asks for, as `MemoryEstimate::heap_bytes` does,
//! and keeps the most held at once. Each test solves spots one at a time, holding a lock so that
//! no other test of this file allocates meanwhile, and compares the most the solve held above
//! what was held before it with the estimate.
//!
//! The estimate must never be below the measured peak, since its job is to refuse a solve that
//! would not fit, and must be within 5% above it, so that it does not refuse solves that would.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering::SeqCst};

use pk_cfr::{Dcfr, Schedule, VectorCfr};
use pk_postflop::{MemoryEstimate, PostflopGame, SolveOptions, Spot, estimate_memory, solve};

/// The system allocator, counting the bytes held and the most held since the last reset.
struct Counting;

static HELD: AtomicU64 = AtomicU64::new(0);
static MOST: AtomicU64 = AtomicU64::new(0);

fn take(bytes: usize) {
    let held = HELD.fetch_add(bytes as u64, SeqCst) + bytes as u64;
    MOST.fetch_max(held, SeqCst);
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
    assert!(
        ratio <= 1.05,
        "{name}: the estimate, {} bytes, is more than 5% above the measured peak, \
         {measured}\n{estimate}",
        estimate.heap_bytes
    );
}

/// Two measurements and a third at the end, so that the solve holds two profiles at once.
fn capped(iterations: u64) -> SolveOptions {
    SolveOptions {
        target: None,
        max_iterations: iterations,
        check_every: 10,
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
    check("turn_small, to its target", &spot, &SolveOptions::default());
}

#[test]
fn after_the_first_iteration_an_iteration_asks_for_no_memory() {
    // The estimate counts the CFR walk's working memory as kept by the solver from its first
    // iteration on. So, once the first iteration has sized it, an iteration takes nothing
    // beyond what the solver already holds: an allocation anywhere in the walk, made while it
    // holds what it held before, would raise the most held above that.
    let _one = ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    for path in ["tests/data/turn_small", "tests/data/flop_small"] {
        let game = PostflopGame::from_spot(&load(path)).unwrap();
        let mut solver = VectorCfr::new(game, Schedule::Discounted(Dcfr::default()));
        solver.iterate();
        for _ in 0..3 {
            let before = HELD.load(SeqCst);
            MOST.store(before, SeqCst);
            solver.iterate();
            assert_eq!(MOST.load(SeqCst), before, "{path}");
            assert_eq!(HELD.load(SeqCst), before, "{path}");
        }
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
