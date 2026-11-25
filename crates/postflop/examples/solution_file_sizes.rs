//! Measures solution files: what each test spot's file takes on disk, with and without the
//! river, how long it takes to save and to load, the most memory either holds, and what
//! quantizing the strategy costs in exploitability. It prints the rows of the table in
//! `docs/BENCHMARKS.md` ("Solution files").
//!
//! ```text
//! cargo run --release -p pk-postflop --example solution_file_sizes
//! cargo run --release -p pk-postflop --example solution_file_sizes -- reference/spots/flop/dry_rainbow.toml
//! cargo run --release -p pk-postflop --example solution_file_sizes -- --compression
//! ```
//!
//! With no spot named it measures every spot under `reference/spots` and the two benchmark
//! spots. Each spot is solved with the default options, to 0.3% of the pot on all the machine's
//! threads, and its files are written to `solutions/sizes/`, which git ignores. The memory is
//! the heap, counted by an allocator that keeps the most held at once: for the solve, the save
//! and the load, the most held above what was held before it began, so a save's figure does not
//! count the solution it is given, which is listed beside it. Beside the measured peak of a
//! save, the solution and the save together, is `estimate_save_memory`'s heap for it.
//!
//! `--compression` measures instead what the sections would take at other zstd levels, and with
//! the bytes of each number written apart (the low bytes of every code, then the high bytes),
//! on the files just written.

use std::alloc::{GlobalAlloc, Layout, System};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicU64, Ordering::SeqCst};
use std::time::{Duration, Instant};

use pk_cfr::PublicGame;
use pk_game::Street;
use pk_postflop::memory::format_bytes;
use pk_postflop::solution_file::{SectionInfo, quantize_profile};
use pk_postflop::{
    SaveOptions, SavedSolution, SolveOptions, Spot, estimate_save_memory, exploitability,
    node_values, read_header, save, solve,
};

/// The system allocator, counting the bytes held and the most held since the last reset.
struct Counting;

static HELD: AtomicU64 = AtomicU64::new(0);
static MOST: AtomicU64 = AtomicU64::new(0);

// SAFETY: every call goes to the system allocator with the same arguments; the counting only
// adds and subtracts sizes.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let held = HELD.fetch_add(layout.size() as u64, SeqCst) + layout.size() as u64;
        MOST.fetch_max(held, SeqCst);
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let held = HELD.fetch_add(layout.size() as u64, SeqCst) + layout.size() as u64;
        MOST.fetch_max(held, SeqCst);
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        HELD.fetch_sub(layout.size() as u64, SeqCst);
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let held = HELD.fetch_add(new_size as u64, SeqCst) + new_size as u64;
        MOST.fetch_max(held, SeqCst);
        let moved = unsafe { System.realloc(ptr, layout, new_size) };
        HELD.fetch_sub(layout.size() as u64, SeqCst);
        moved
    }
}

#[global_allocator]
static COUNTING: Counting = Counting;

/// Runs `f`, and gives its result, its time and the most it held above what was held before.
fn measured<T>(f: impl FnOnce() -> T) -> (T, Duration, u64) {
    let before = HELD.load(SeqCst);
    MOST.store(before, SeqCst);
    let start = Instant::now();
    let result = f();
    let time = start.elapsed();
    (result, time, MOST.load(SeqCst) - before)
}

const OUT: &str = "solutions/sizes";

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("solution_file_sizes: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let compression = args.iter().any(|a| a == "--compression");
    args.retain(|a| a != "--compression");
    let spots: Vec<PathBuf> = if args.is_empty() {
        default_spots()?
    } else {
        args.into_iter().map(PathBuf::from).collect()
    };
    fs::create_dir_all(OUT).map_err(|e| format!("{OUT}: {e}"))?;
    if compression {
        println!(
            "| Spot | Section | Raw | Level 1 | Level 3 | Level 9 | Level 19 | Level 3, bytes apart \
             | Level 19, bytes apart | Time at 1, 3, 9, 19 |"
        );
        println!("| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |");
    } else {
        println!(
            "| Spot | Public nodes | Strategies, raw | EVs, raw | File | File without the river \
             | Save | Load | Load to the turn | Heap: solve's peak; solution; save's peak above it; \
             load's peak | Save's peak in all: measured, estimated (ratio) | Exploitability: \
             solved, stored |"
        );
        println!("| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |");
    }
    for spot in spots {
        if compression {
            compression_rows(&spot)?;
        } else {
            size_row(&spot)?;
        }
    }
    Ok(())
}

/// Every spot under `reference/spots`, river, turn, flop, and the benchmark spots.
fn default_spots() -> Result<Vec<PathBuf>, String> {
    let mut spots = Vec::new();
    for street in ["river", "turn", "flop"] {
        let dir = format!("reference/spots/{street}");
        let mut files: Vec<PathBuf> = fs::read_dir(&dir)
            .map_err(|e| format!("{dir}: {e}; run from the repository's root"))?
            .filter_map(|entry| entry.ok().map(|e| e.path()))
            .filter(|path| path.extension().is_some_and(|e| e == "toml"))
            .collect();
        files.sort();
        spots.extend(files);
    }
    spots.push("crates/postflop/benches/data/turn_srp.toml".into());
    spots.push("crates/postflop/benches/data/flop_srp.toml".into());
    Ok(spots)
}

fn name_of(path: &Path) -> String {
    path.file_stem().unwrap().to_string_lossy().into_owned()
}

fn seconds(time: Duration) -> String {
    let s = time.as_secs_f64();
    if s >= 10.0 {
        format!("{s:.1} s")
    } else if s >= 0.01 {
        format!("{s:.2} s")
    } else {
        format!("{:.1} ms", s * 1e3)
    }
}

/// The raw lengths of the strategy sections and of the EV sections.
fn raw_lengths(sections: &[SectionInfo]) -> (u64, u64) {
    let sum = |prefix: &str| {
        sections
            .iter()
            .filter(|s| s.name.starts_with(prefix))
            .map(|s| s.raw_length)
            .sum::<u64>()
    };
    (sum("strategy."), sum("ev."))
}

fn size_row(path: &Path) -> Result<(), String> {
    let name = name_of(path);
    let spot = Spot::load(path).map_err(|e| e.to_string())?;
    let base = HELD.load(SeqCst);
    let (solution, solve_time, solve_heap) =
        measured(|| solve(&spot, &SolveOptions::default(), |_| {}));
    let solution = solution.map_err(|e| e.to_string())?;
    // What the solution holds, which a save is given and does not count.
    let solution_heap = HELD.load(SeqCst).saturating_sub(base);
    let complete = PathBuf::from(format!("{OUT}/{name}.pksol"));
    let (report, save_time, save_heap) =
        measured(|| save(&solution, &complete, &SaveOptions::default()));
    let report = report.map_err(|e| e.to_string())?;
    let estimate = estimate_save_memory(&spot, &SolveOptions::default(), &SaveOptions::default())
        .map_err(|e| e.to_string())?;
    let (without_river, turn_save) = if spot.street() < Street::River {
        let path = PathBuf::from(format!("{OUT}/{name}.turn.pksol"));
        let options = SaveOptions {
            last_street: Street::Turn,
        };
        let (report, _, heap) = measured(|| save(&solution, &path, &options));
        let report = report.map_err(|e| e.to_string())?;
        let estimate = estimate_save_memory(&spot, &SolveOptions::default(), &options)
            .map_err(|e| e.to_string())?;
        let turn_save = format!(
            "; without the river, measured {} and estimated {}",
            format_bytes(solution_heap + heap),
            format_bytes(estimate.heap_bytes)
        );
        (format_bytes(report.bytes), turn_save)
    } else {
        ("the same".to_string(), String::new())
    };
    let nodes = solution.game().num_nodes();
    let before = solution.solve_exploitability().percent_of_pot;
    // Where a save's time goes, measured apart: quantizing, the exploitability and the EVs.
    let (quantized, quantize_time, _) =
        measured(|| quantize_profile(solution.game(), solution.strategy()).unwrap());
    let (_, exploitability_time, _) = measured(|| exploitability(solution.game(), &quantized));
    let (values, values_time, values_heap) = measured(|| node_values(solution.game(), &quantized));
    drop(values);
    drop(quantized);
    drop(solution);
    let (loaded, load_time, load_heap) = measured(|| SavedSolution::load(&complete));
    let loaded = loaded.map_err(|e| e.to_string())?;
    let after = loaded.header().exploitability.percent_of_pot;
    drop(loaded);
    let (first, first_time, _) = measured(|| SavedSolution::load_streets(&complete, Street::Turn));
    drop(first.map_err(|e| e.to_string())?);
    let header = read_header(&complete).map_err(|e| e.to_string())?;
    let (strategies, evs) = raw_lengths(&header.sections);
    let load_turn = if spot.street() < Street::River {
        seconds(first_time)
    } else {
        "".to_string()
    };
    println!(
        "| `{name}` | {nodes} | {} | {} | {} | {without_river} | {} | {} | {load_turn} | {}; {}; \
         {}; {} | {}, {} ({:.3}) | {before:.6}%, {after:.6}% |",
        format_bytes(strategies),
        format_bytes(evs),
        format_bytes(report.bytes),
        seconds(save_time),
        seconds(load_time),
        format_bytes(solve_heap),
        format_bytes(solution_heap),
        format_bytes(save_heap),
        format_bytes(load_heap),
        format_bytes(solution_heap + save_heap),
        format_bytes(estimate.heap_bytes),
        estimate.heap_bytes as f64 / (solution_heap + save_heap) as f64,
    );
    eprintln!(
        "{name}: solve {} in {} iterations; save {} = quantize {} + exploitability {} + EVs {} \
         (node_values would hold {}) + compressing and writing; save's peak {}, estimated {} \
         {} (total {}){turn_save}; sections {:?}",
        seconds(solve_time),
        header.iterations,
        seconds(save_time),
        seconds(quantize_time),
        seconds(exploitability_time),
        seconds(values_time),
        format_bytes(values_heap),
        format_bytes(solution_heap + save_heap),
        format_bytes(estimate.heap_bytes),
        estimate.peak_phase,
        format_bytes(estimate.total),
        header
            .sections
            .iter()
            .map(|s| format!("{} {} of {}", s.name, s.length, s.raw_length))
            .collect::<Vec<_>>()
    );
    Ok(())
}

/// A section's raw bytes from a file written by [`size_row`].
fn sections(path: &Path) -> Result<Vec<(SectionInfo, Vec<u8>)>, String> {
    let header = read_header(path).map_err(|e| e.to_string())?;
    let bytes = fs::read(path).map_err(|e| e.to_string())?;
    let length = u32::from_le_bytes(bytes[7..11].try_into().unwrap()) as usize;
    let mut at = 11 + length;
    header
        .sections
        .into_iter()
        .map(|section| {
            let frame = &bytes[at..at + section.length as usize];
            at += section.length as usize;
            let raw = zstd::bulk::decompress(frame, section.raw_length as usize)
                .map_err(|e| e.to_string())?;
            Ok((section, raw))
        })
        .collect()
}

/// The bytes of `raw`, numbers of `width` bytes, written byte plane by byte plane: every
/// number's first byte, then every number's second, and so on.
fn planes(raw: &[u8], width: usize) -> Vec<u8> {
    (0..width)
        .flat_map(|plane| raw.iter().skip(plane).step_by(width).copied())
        .collect()
}

/// The length of `raw` compressed at `level` with a checksum, and the time it took.
fn compressed(raw: &[u8], level: i32) -> (usize, Duration) {
    let start = Instant::now();
    let mut compressor = zstd::bulk::Compressor::new(level).unwrap();
    compressor
        .set_parameter(zstd::zstd_safe::CParameter::ChecksumFlag(true))
        .unwrap();
    let length = compressor.compress(raw).unwrap().len();
    (length, start.elapsed())
}

fn compression_rows(path: &Path) -> Result<(), String> {
    let name = name_of(path);
    let file = PathBuf::from(format!("{OUT}/{name}.pksol"));
    if !file.exists() {
        let spot = Spot::load(path).map_err(|e| e.to_string())?;
        let solution = solve(&spot, &SolveOptions::default(), |_| {}).map_err(|e| e.to_string())?;
        save(&solution, &file, &SaveOptions::default()).map_err(|e| e.to_string())?;
    }
    for (section, raw) in sections(&file)? {
        let width = if section.name.starts_with("strategy.") {
            2
        } else {
            4
        };
        let levels = [1, 3, 9, 19].map(|level| compressed(&raw, level));
        let apart = planes(&raw, width);
        let (apart_3, _) = compressed(&apart, 3);
        let (apart_19, _) = compressed(&apart, 19);
        println!(
            "| `{name}` | `{}` | {} | {} | {} | {} | {} | {} | {} | {} |",
            section.name,
            format_bytes(raw.len() as u64),
            format_bytes(levels[0].0 as u64),
            format_bytes(levels[1].0 as u64),
            format_bytes(levels[2].0 as u64),
            format_bytes(levels[3].0 as u64),
            format_bytes(apart_3 as u64),
            format_bytes(apart_19 as u64),
            levels
                .iter()
                .map(|(_, time)| seconds(*time))
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    Ok(())
}
