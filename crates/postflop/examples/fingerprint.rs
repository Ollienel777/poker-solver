//! Prints a fingerprint of a solve: hashes of the bits of everything it computes, so that a
//! change meant to leave every number alone, such as a performance change, can be checked to do
//! so. Run it before the change and after it, on the same spots and iteration counts, and
//! compare the two outputs line for line.
//!
//! ```text
//! cargo run --release -p pk-postflop --example fingerprint -- <spot file> <iterations> [--threads <n>] [--merged] [--compressed]
//! cargo run --release -p pk-postflop --example fingerprint -- crates/postflop/tests/data/turn_small.toml 30
//! cargo run --release -p pk-postflop --example fingerprint -- crates/postflop/tests/data/turn_small.toml 30 --threads 8
//! ```
//!
//! It runs `iterations` iterations of DCFR(1.5, 0, 2), the solve's default schedule, and prints
//! a 64-bit FNV-1a hash of the bits of each of these, one per line:
//!
//! - every stored regret and strategy sum, node by node;
//! - the average strategy;
//! - each player's best response to it, its value by hand and its pure strategy, and each
//!   player's expected value by hand;
//! - the EVs of [`node_values`] under it;
//!
//! and then the exploitability of the average strategy as the solve measures it, both values
//! and both best responses, printed in full, which is exact for an `f64`.
//!
//! A hash is equal only when every value is bit for bit the same. It is not portable: the DCFR
//! discount uses `powf`, whose last bit can differ between platforms, so compare outputs made
//! on one machine with one toolchain, and never record a hash in a test.
//!
//! With `--threads <n>`, everything runs in a rayon pool of `n` threads, so that the walks of
//! the iterations, the best responses and the expected values split chance nodes when `n` is
//! more than one. Without it, everything runs on the main thread, as before the walks could
//! split. The output does not name the thread count, so that the outputs for every count, and
//! those made before, compare line for line.
//!
//! With `--merged`, the iterations run on the spot's merged game
//! ([`pk_postflop::merged`]), as a solve with merging on does, when the spot has a symmetry to
//! merge: the first line says so, the regrets and strategy sums are the merged solver's, node by
//! node of the merged tree, and everything after is of its average strategy expanded onto the
//! full tree, measured on the full game, as the solve measures it. On a spot with nothing to
//! merge the output is the one without `--merged`. Outputs with `--merged` compare with each
//! other across thread counts and builds, not with those without it.
//!
//! With `--compressed`, the solver stores its regrets and strategy sums in 16 bits
//! ([`pk_cfr::CompressedStorage`]), as a solve with `SolveOptions::compressed_storage` does: the
//! first line says so, and the first hash is of every node's regret codes and their scale, then
//! its strategy-sum codes and theirs. It combines with `--merged`, and its outputs compare with
//! each other, across thread counts and builds.

use std::process::ExitCode;

use pk_cfr::{
    CompressedStorage, Dcfr, F32Storage, PublicGame, PublicProfile, Schedule, Storage, VectorCfr,
    vector_best_response, vector_expected_value,
};
use pk_postflop::{MergedGame, PostflopGame, Spot, exploitability, node_values};

const USAGE: &str =
    "usage: fingerprint <spot file> <iterations> [--threads <n>] [--merged] [--compressed]";

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("fingerprint: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let merged = args.iter().any(|arg| arg == "--merged");
    let compressed = args.iter().any(|arg| arg == "--compressed");
    args.retain(|arg| arg != "--merged" && arg != "--compressed");
    let (path, iterations, threads) = match args.as_slice() {
        [path, iterations] => (path, iterations, None),
        [path, iterations, flag, threads] if flag == "--threads" => {
            let threads: usize = threads
                .parse()
                .ok()
                .filter(|&threads| threads >= 1)
                .ok_or_else(|| format!("`{threads}` is not a number of threads\n{USAGE}"))?;
            (path, iterations, Some(threads))
        }
        _ => return Err(USAGE.to_string()),
    };
    let iterations: u64 = iterations
        .parse()
        .map_err(|_| format!("`{iterations}` is not a number of iterations\n{USAGE}"))?;
    match threads {
        Some(threads) => {
            let pool = rayon::ThreadPoolBuilder::new()
                .num_threads(threads)
                .build()
                .map_err(|e| e.to_string())?;
            pool.install(|| match compressed {
                false => fingerprint::<F32Storage>(path, iterations, merged),
                true => fingerprint::<CompressedStorage>(path, iterations, merged),
            })
        }
        None => match compressed {
            false => fingerprint::<F32Storage>(path, iterations, merged),
            true => fingerprint::<CompressedStorage>(path, iterations, merged),
        },
    }
}

/// Runs `iterations` iterations on the spot at `path`, on its merged game if `merged` and it has
/// one, with the storage `S`, and prints the fingerprint.
fn fingerprint<S: Hashed>(path: &str, iterations: u64, merged: bool) -> Result<(), String> {
    let spot = Spot::load(path).map_err(|e| e.to_string())?;
    let game = PostflopGame::from_spot(&spot).map_err(|e| e.to_string())?;
    let schedule = Schedule::Discounted(Dcfr::default());
    let merged_game = if merged { MergedGame::new(&game) } else { None };
    let storage = S::NAME.map_or(String::new(), |name| format!(", {name}"));
    let (stored, average) = match merged_game {
        Some(merged_game) => {
            let mut solver = VectorCfr::<_, S>::with_storage(merged_game, schedule);
            solver.run(iterations, |_| {});
            println!(
                "{} after {iterations} iterations, merged{storage}",
                spot.name
            );
            (stored(&solver), MergedGame::average_strategy(&solver))
        }
        None => {
            let mut solver = VectorCfr::<_, S>::with_storage(game.clone(), schedule);
            solver.run(iterations, |_| {});
            println!("{} after {iterations} iterations{storage}", spot.name);
            (stored(&solver), solver.average_strategy())
        }
    };
    let game = &game;
    println!("regrets and strategy sums  {}", stored.hex());
    println!("average strategy           {}", profile(&average).hex());

    for player in 0..2 {
        let response = vector_best_response(game, &average, player);
        let mut hash = profile(&response.strategy);
        hash.f64s(&response.hand_values);
        hash.f64s(&[response.value]);
        println!("best response, player {player}    {}", hash.hex());
    }
    for player in 0..2 {
        let expected = vector_expected_value(game, &average, player);
        let mut hash = Fnv::new();
        hash.f64s(&expected.hand_values);
        hash.f64s(&[expected.value]);
        println!("expected value, player {player}   {}", hash.hex());
    }

    let values = node_values(game, &average);
    let mut hash = Fnv::new();
    for node in 0..game.num_nodes() {
        if let Some(evs) = values.get(node as u32) {
            hash.index(node);
            for ev in evs {
                match ev {
                    Some(ev) => hash.f64s(&[*ev]),
                    None => hash.bytes(b"none"),
                }
            }
        }
    }
    println!("node values                {}", hash.hex());

    let measured = exploitability(game, &average);
    println!(
        "exploitability             {:?} chips, {:?}% of the pot",
        measured.chips, measured.percent_of_pot
    );
    println!("values                     {:?}", measured.values);
    println!("best responses             {:?}", measured.best_responses);
    Ok(())
}

/// A storage whose stored values the fingerprint hashes.
trait Hashed: Storage {
    /// What the first line calls the storage: nothing for `f32`, as before it had a name.
    const NAME: Option<&'static str>;

    /// Hashes the stored values of `node`, if it has a row, and says whether it has one.
    fn hash<G: PublicGame>(solver: &VectorCfr<G, Self>, node: usize, hash: &mut Fnv) -> bool;
}

impl Hashed for F32Storage {
    const NAME: Option<&'static str> = None;

    /// Its regrets, then its strategy sums.
    fn hash<G: PublicGame>(solver: &VectorCfr<G>, node: usize, hash: &mut Fnv) -> bool {
        let (Some(regrets), Some(sums)) = (solver.regrets(node), solver.strategy_sum(node)) else {
            return false;
        };
        hash.index(node);
        hash.f32s(regrets);
        hash.f32s(sums);
        true
    }
}

impl Hashed for CompressedStorage {
    const NAME: Option<&'static str> = Some("16-bit");

    /// Its regret codes and their scale, then its strategy-sum codes and theirs.
    fn hash<G: PublicGame>(
        solver: &VectorCfr<G, CompressedStorage>,
        node: usize,
        hash: &mut Fnv,
    ) -> bool {
        let (Some((regrets, regret_scale)), Some((sums, sum_scale))) =
            (solver.regret_codes(node), solver.strategy_sum_codes(node))
        else {
            return false;
        };
        hash.index(node);
        for code in regrets {
            hash.bytes(&code.to_le_bytes());
        }
        hash.f32s(&[regret_scale]);
        for code in sums {
            hash.bytes(&code.to_le_bytes());
        }
        hash.f32s(&[sum_scale]);
        true
    }
}

/// The hash of a solver's stored values: each node that has a row, with its regrets and
/// strategy sums.
fn stored<G: PublicGame, S: Hashed>(solver: &VectorCfr<G, S>) -> Fnv {
    let mut stored = Fnv::new();
    for node in 0..solver.game().num_nodes() {
        S::hash(solver, node, &mut stored);
    }
    stored
}

/// The hash of a profile: each node that has a strategy, with its probabilities.
fn profile(profile: &PublicProfile) -> Fnv {
    let mut hash = Fnv::new();
    for node in profile.nodes() {
        hash.index(node);
        hash.f32s(profile.get(node).expect("the node has a strategy"));
    }
    hash
}

/// The 64-bit FNV-1a hash of the bytes fed to it.
struct Fnv(u64);

impl Fnv {
    fn new() -> Fnv {
        Fnv(0xcbf2_9ce4_8422_2325)
    }

    fn bytes(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.0 ^= u64::from(byte);
            self.0 = self.0.wrapping_mul(0x0100_0000_01b3);
        }
    }

    /// A node index, so that the same values at other nodes hash differently.
    fn index(&mut self, index: usize) {
        self.bytes(&(index as u64).to_le_bytes());
    }

    fn f32s(&mut self, values: &[f32]) {
        for value in values {
            self.bytes(&value.to_bits().to_le_bytes());
        }
    }

    fn f64s(&mut self, values: &[f64]) {
        for value in values {
            self.bytes(&value.to_bits().to_le_bytes());
        }
    }

    fn hex(&self) -> String {
        format!("{:016x}", self.0)
    }
}
