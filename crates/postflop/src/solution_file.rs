//! Solution files: a solved spot saved to disk and loaded back.
//!
//! A solution file holds the strategy of a [`Solution`], quantized to 16 bits, and the acting
//! player's EV with each hand at every player node, both laid out street by street, so that a
//! reader can load the flop and the turn of a large solution without the river, which is most
//! of it. It holds no tree: the header carries the spot, and a reader builds the game again
//! from it and checks it against a hash of the tree the writer had. The extension is `.pksol`.
//!
//! [`save`] writes a solution, [`read_header`] reads the header alone, and
//! [`SavedSolution::load`] reads a file back into a [`Solution`] with its EVs, or
//! [`SavedSolution::load_streets`] its first streets only. A file may leave out the river
//! ([`SaveOptions::last_street`]), and [`SavedSolution::river_spot`] gives a river that is not
//! in the file as a spot of its own, which [`solve`](crate::solve()) solves again.
//!
//! This documentation is the format's contract: a reader can be written from it alone.
//!
//! # Layout
//!
//! Every integer in the file is little-endian.
//!
//! | Offset | Bytes | Holds |
//! | --- | --- | --- |
//! | 0 | 5 | the magic `PKSOL`, in ASCII |
//! | 5 | 2 | the format version, a `u16`: 1 ([`FORMAT_VERSION`]) |
//! | 7 | 4 | the header's length in bytes, `H`, a `u32`, at most 1 MiB ([`MAX_HEADER_BYTES`]) |
//! | 11 | `H` | the header, a JSON object in UTF-8 |
//! | 11 + `H` | | the sections, one after another, in the order the header lists them |
//!
//! Each section is one zstd frame ([RFC 8878](https://www.rfc-editor.org/rfc/rfc8878)) that
//! records its content size and has its content checksum, with nothing before or after it. The
//! file ends where the last section does, so it is `11 + H` bytes plus the sections' lengths
//! long. The header gives every section's length, so a reader can seek to one section without
//! reading the others.
//!
//! # The header
//!
//! The header of `turn_small`'s file, solved to 0.3% of the pot and saved without the river, laid
//! out on several lines, with the spot cut short and the numbers to fewer digits than the file
//! writes:
//!
//! ```json
//! {
//!   "solver": {"name": "pk-postflop", "version": "0.1.0"},
//!   "spot": "format = 1\nname = \"turn_small\"\ndescription = …",
//!   "tree": {"hash": "dbb28683015afd79", "nodes": 2085, "player_nodes": 776},
//!   "hands": {"oop": 10, "ip": 9},
//!   "streets": ["turn"],
//!   "solve": {
//!     "stop": "target",
//!     "iterations": 60,
//!     "exploitability": {"values": [549.6176, 375.6086], "best_responses": [552.2209, 378.3854],
//!                        "chips": 2.690068, "percent_of_pot": 0.2690068}
//!   },
//!   "exploitability": {"values": [549.6183, 375.6080], "best_responses": [552.2205, 378.3851],
//!                      "chips": 2.689672, "percent_of_pot": 0.2689672},
//!   "sections": [
//!     {"name": "strategy.turn", "length": 394, "raw_length": 380},
//!     {"name": "ev.turn", "length": 318, "raw_length": 304}
//!   ]
//! }
//! ```
//!
//! The file writes the header on one line, with every number as the shortest decimal that reads
//! back as the same `f64`. This file is 2,051 bytes long; with its river it is 40 kB.
//!
//! The header holds exactly these keys, each required, in any order; no other key is allowed,
//! and no object may repeat a key.
//!
//! | Key | Holds |
//! | --- | --- |
//! | `solver` | `name` and `version`, strings: the program that wrote the file |
//! | `spot` | the spot, as the text of its spot file ([`crate::spot`]) |
//! | `tree` | `hash`, the tree hash below as 16 lowercase hexadecimal digits; `nodes`, the number of nodes of the game's public tree; and `player_nodes`, the number of its player nodes |
//! | `hands` | `oop` and `ip`: the number of each player's hands ([`crate::hands`]) |
//! | `streets` | the streets the file holds, `"flop"`, `"turn"` or `"river"`: the spot's first street and the streets after it up to the last one held, in order |
//! | `solve` | how the solve ended: `stop`, `"target"`, `"iteration_cap"` or `"time_cap"` ([`StopReason`]); `iterations`; and `exploitability`, what the solve measured for the strategy it found, before quantizing |
//! | `exploitability` | the exploitability of the strategy the file holds, measured when it was written, on the whole tree, before any street was left out |
//! | `sections` | each section, in file order: `name`, `length` in the file and `raw_length` once decompressed, in bytes |
//!
//! An exploitability is an object of `values`, each player's value, `best_responses`, each
//! player's best-response value, both as arrays of OOP's then IP's, `chips`, the exploitability
//! in chips, and `percent_of_pot`, as [`Exploitability`] defines them: chips per hand dealt,
//! counted from the start of the spot. Counts are integers and the rest are numbers, written so
//! that they read back as the same `f64`.
//!
//! # The tree
//!
//! The sections are arrays laid out by the spot's game ([`crate::game`]): the nodes of its public
//! tree in pre-order, each player node's actions in the betting tree's order, each player's hands
//! in hand order. The file does not hold the tree. A reader builds the game from the header's
//! spot, as [`PostflopGame::from_spot`] does, and computes the *tree hash* of what it built. If
//! it differs from the header's, the arrays would be read against another layout, and the file
//! is refused: its spot was edited, or the program that reads it builds another tree from the
//! same spot than the one that wrote it.
//!
//! The tree hash is FNV-1a, 64 bits (offset basis `0xcbf29ce484222325`, prime
//! `0x100000001b3`), of these bytes, with every integer little-endian:
//!
//! 1. the 30 bytes of ASCII `pk-postflop solution tree hash`, then a `u8`, 1, the hash's own
//!    version;
//! 2. the root pot, a `u32` of chips; the rake's percent as the bits of an `f64`, a `u64`; its
//!    cap, a `u32` of chips; and a `u8`, 1 if it has no flop no drop and 0 if not;
//! 3. the number of nodes of the public tree, a `u32`;
//! 4. each node, in pre-order:
//!    - a player node: a `u8` 1, the acting player, a `u8` (0 for OOP, 1 for IP), the number of
//!      actions, a `u32`, and each action in order as a `u8` kind (0 fold, 1 check, 2 call, 3
//!      bet, 4 raise, 5 all-in) and a `u32`, the street total it takes the player to, 0 for a
//!      fold, a check or a call;
//!    - a chance node: a `u8` 2, the number of cards it deals, a `u32`, and each card in order
//!      as a `u8`, its index `4 × rank + suit` ([`Card::index`](pk_core::Card::index));
//!    - a terminal: a `u8` 0, its outcome as a `u8` (0 a fold that OOP wins, 1 a fold that IP
//!      wins, 2 a showdown), and the chips each player put in from the start of the spot, two
//!      `u32`s, OOP's first;
//! 5. each player's hands, OOP's then IP's: their number, a `u32`, and each hand in hand order
//!    as its combo's index, a `u16` ([`Combo::index`]), and its range weight as the bits of an
//!    `f64`, a `u64`.
//!
//! The pre-order and the numbers of children fix the tree's shape. The hash covers everything
//! the layout of the arrays depends on, and the pot, the rake and the chips at each terminal,
//! which the EVs and the exploitability depend on as well. It is a check that the reader's game
//! is the writer's, not a security measure.
//!
//! # Sections
//!
//! For each street the file holds, in order, two sections: `strategy.<street>` and then
//! `ev.<street>`, with `<street>` one of `flop`, `turn` and `river`. A file that holds every
//! street of a flop spot has the six sections `strategy.flop`, `ev.flop`, `strategy.turn`,
//! `ev.turn`, `strategy.river` and `ev.river`, in that order; one that leaves out the river ends
//! after `ev.turn`. Each section covers the player nodes of its street, in node order. A
//! node's street is its betting node's ([`PostflopGame::street`]).
//!
//! **`strategy.<street>`**, raw: for each player node of the street, the acting player's
//! strategy codes, `actions × hands` of them, each a `u16`, laid out action-major as
//! [`PublicProfile`] lays out probabilities: the code of action `a` with hand `h` is the
//! `(a × hands + h)`-th of the node's. Its raw length is `2 × Σ actions × hands` bytes over the
//! street's player nodes. A hand's codes sum to exactly 65,535 at every node, and the
//! probability of action `a` is its code divided by 65,535, computed in `f32`
//! ([`dequantize`]). Every hand has a row at every node, including a hand that holds a card
//! dealt on the way there, which cannot be held at the node and whose row means nothing.
//!
//! **`ev.<street>`**, raw: for each player node of the street, the acting player's EV with each
//! of its hands, in hand order, each an `f32`: its expected payoff, in chips from the start of
//! the spot, given that play reached the node, under the strategy the file holds, as
//! [`node_values`](mod@crate::node_values) defines it. An undefined EV is a NaN, which the writer
//! writes as `0x7FC00000`. Its raw length is `4 × Σ hands` bytes over the street's player
//! nodes.
//!
//! # Strategies
//!
//! Each hand's probabilities at a node, `p_a` for its actions `a`, become codes `c_a` that sum
//! to exactly 65,535, by the largest-remainder method ([`quantize`]): each `c_a` starts at
//! `⌊65535 · p_a⌋`, computed exactly in `f64`, and the codes still missing to make 65,535 go one
//! each to the actions with the largest remainders `65535 · p_a − ⌊65535 · p_a⌋`, ties to the
//! lower action index. A row is quantized only if its probabilities are from 0 to 1 and sum to
//! 1 within [`STRATEGY_TOLERANCE`], so that one code at
//! most goes to each action. Every probability the file holds is then within 1/65,535 of the
//! solve's. Quantizing is idempotent: the codes of a row of decoded probabilities are the codes
//! they were decoded from, so a solution loaded from a file is saved again to the same codes.
//!
//! # Every number describes the stored strategy
//!
//! The EVs are those of the quantized strategy, not the solve's, and so is the header's
//! `exploitability`, measured on the whole tree when the file is written, before any street is
//! left out. A reader that measures the exploitability of a complete file's strategy, in the
//! game it built, gets the header's figure bit for bit. The solve's own measurement, of the
//! strategy before quantizing, is in `solve.exploitability`, so that what quantizing cost shows
//! in every file.
//!
//! # Reading
//!
//! A reader trusts nothing in the file. [`read_header`] and [`SavedSolution::load`] refuse, with a
//! [`SolutionFileError`] that says what is wrong:
//!
//! - a file that does not start with `PKSOL`, or another format version than 1;
//! - a file that ends early, in the preamble, the header or a section, or goes on after its last
//!   section;
//! - a header longer than [`MAX_HEADER_BYTES`], that is not JSON or not UTF-8, repeats a key,
//!   lacks one or has one that is not listed above, or holds a value of the wrong type or out of
//!   range, each named by its path, such as `solve.iterations`;
//! - a spot that is not a valid spot file;
//! - streets that do not start at the spot's first street or are not in order, and sections
//!   that are not the two of each street, in order;
//! - a game built from the spot whose tree hash, or numbers of nodes, player nodes and hands,
//!   are not the header's;
//!
//! and [`SavedSolution::load`] then refuses a section that is not one zstd frame with its
//! checksum and content size, that fails its checksum, that does not hold as many bytes as the
//! tree needs, a hand whose codes at a node do not sum to 65,535, and an EV that is infinite.
//! Nothing is allocated from a length the file gives before that length is checked against the
//! file's size or the tree's.
//!
//! # Leaving out the river, and solving it again
//!
//! A river is most of a flop solution: on a flop spot, every line that reaches the river is
//! copied once for each of 49 × 48 runouts. [`SaveOptions::last_street`] leaves the streets after
//! it out of the file, which leaves out their two sections, as the brief's storage rule asks:
//! keep the flop and turn strategies, and solve a river again when it is needed. A solution
//! loaded from such a file holds no strategy on the river (see
//! [partial solutions](Solution#partial-solutions)).
//!
//! [`SavedSolution::river_spot`] gives the river that starts at a node, the first decision after
//! a river card falls, as a [`RiverSpot`]: the five-card board, the pot and the stack left at
//! the node, each player's range as its range weight times its own reach of the node under the
//! stored strategy, rounded to `f32`, with the hands that hold a board card or never reach the
//! node left out, and the spot's river sizing. The all-in threshold is restated for the river's
//! own stack, so that the same bets and raises turn into all-in ([`pk_game::rules::commit_limit`]).
//! Solving the river spot gives the river's strategy. Its payoffs, EVs and values count from the
//! start of the river; [`RiverSpot::put_in`] is what each player had put in before it, which an
//! EV of the river spot less that amount counts from the start of the full spot.
//!
//! # Determinism
//!
//! The bytes of a file are a function of the solution's spot, strategy, stopping rule,
//! iterations and exploitability, and of the options: no time of writing and nothing from the
//! machine, the solve's time included, and the same on any platform and thread count, since a
//! solve gives the same solution on any thread count. A loaded solution saved again with the
//! same options gives the same bytes. A loaded solution's [`Solution::elapsed`] is zero.
//!
//! # Writing
//!
//! [`save`] writes to `<path>.part` and renames it to `<path>` once it is complete and flushed
//! to disk, so that a crash leaves no half-written file under the final name.
//!
//! Beside the solution it is given, a save holds the quantized strategy, the one copy of a
//! strategy it needs, since the exploitability and the EVs are measured on the whole of it, and
//! little else at once: each node's codes go to its street's zstd encoder as they are made, the
//! EVs go from their walk to each kept street's raw section as they are found, the quantized
//! strategy is dropped before the EV sections are compressed, and each raw EV section once it
//! is. [`estimate_save_memory`](crate::memory::estimate_save_memory) counts what it holds,
//! from the spot alone.

use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use pk_cfr::{PublicGame, PublicNode, PublicProfile};
use pk_core::{Combo, Range};
use pk_game::{Action, Chips, Node, Outcome, Player, Street, rules};
use serde::Serialize;
use thiserror::Error;

use crate::dump::json::Json;
use crate::exploitability::{Exploitability, exploitability};
use crate::game::PostflopGame;
use crate::invariants::STRATEGY_TOLERANCE;
use crate::node_values::node_values_into;
use crate::solve::{Solution, SolveError, StopReason};
use crate::spot::{Spot, SpotError, TreeSizing};

/// The first five bytes of every solution file.
pub const MAGIC: [u8; 5] = *b"PKSOL";

/// The version of the format that this module writes and reads.
pub const FORMAT_VERSION: u16 = 1;

/// The longest header a reader accepts, in bytes: 1 MiB. A header holds a spot file and a few
/// numbers, a few kilobytes at most.
pub const MAX_HEADER_BYTES: u32 = 1 << 20;

/// What a hand's strategy codes at a node sum to.
pub const CODE_TOTAL: u16 = u16::MAX;

/// The zstd level every section is compressed at.
pub(crate) const LEVEL: i32 = 3;

/// The bytes before the header: the magic, the version and the header's length.
const PREAMBLE_BYTES: u64 = 11;

/// The bits written for an undefined EV.
const NAN_BITS: u32 = 0x7FC0_0000;

/// The first four bytes of a zstd frame, `0xFD2FB528` little-endian.
const ZSTD_MAGIC: [u8; 4] = [0x28, 0xB5, 0x2F, 0xFD];

/// The program that writes files, as the header names it.
const SOLVER_NAME: &str = "pk-postflop";
const SOLVER_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Why a solution file could not be written or read, or a river spot made.
#[derive(Debug, Error)]
pub enum SolutionFileError {
    /// The file system refused.
    #[error("could not {action} {}: {source}", .path.display())]
    Io {
        action: &'static str,
        path: PathBuf,
        source: io::Error,
    },
    /// The file does not start with [`MAGIC`].
    #[error("{} is not a solution file: it does not start with `PKSOL`", .path.display())]
    NotASolutionFile { path: PathBuf },
    /// The file is of another format version.
    #[error(
        "{} is a solution file of format version {found}; this solver reads version \
         {supported} only",
        .path.display()
    )]
    Version {
        path: PathBuf,
        found: u16,
        supported: u16,
    },
    /// The file ends before what its preamble and header say it holds: `place` says where.
    #[error("{} is cut short: it ends {place}", .path.display())]
    Truncated { path: PathBuf, place: String },
    /// The file goes on after its last section.
    #[error(
        "{} has {extra} bytes after its last section; a solution file ends with its last section",
        .path.display()
    )]
    TrailingBytes { path: PathBuf, extra: u64 },
    /// The header is not a valid header: `field` names the key, and is empty for the header as
    /// a whole.
    #[error("{}: the header{} {reason}", .path.display(), field_name(.field))]
    Header {
        path: PathBuf,
        field: String,
        reason: String,
    },
    /// The header's spot is not a valid spot file.
    #[error("{}: the spot in the header is not valid: {source}", .path.display())]
    Spot {
        path: PathBuf,
        source: Box<SpotError>,
    },
    /// The game could not be built from the header's spot.
    #[error("{}: the game of the spot in the header could not be built: {source}", .path.display())]
    Game {
        path: PathBuf,
        source: Box<SolveError>,
    },
    /// The game built from the header's spot is not the one the file was written for.
    #[error(
        "{} was written by {written_by} for a tree that {reading} builds differently from the \
         spot in its header ({detail}): the spot was edited, or the tree or the game has changed \
         since the file was written. Solve the spot again",
        .path.display()
    )]
    TreeMismatch {
        path: PathBuf,
        written_by: String,
        reading: String,
        detail: String,
    },
    /// A section is corrupt or does not fit the tree.
    #[error("{}: section `{name}` {reason}", .path.display())]
    Section {
        path: PathBuf,
        name: String,
        reason: String,
    },
    /// [`save`] was given a solution that does not hold every street.
    #[error(
        "the solution holds the strategy up to the {last_street} only; a solution file is \
         written from a solution that holds every street, so that its EVs and exploitability \
         can be measured"
    )]
    Incomplete { last_street: Street },
    /// [`SaveOptions::last_street`] is before the spot's first street.
    #[error(
        "`last_street` is the {last_street}, but the spot starts on the {first}; a file holds \
         at least the spot's first street"
    )]
    Options { last_street: Street, first: Street },
    /// A hand's strategy at a node is not a probability distribution, so it cannot be quantized.
    #[error(
        "the strategy at node {node} (`{path}`) for {combo} is {probabilities:?}, which is not \
         probabilities from 0 to 1 that sum to 1"
    )]
    Strategy {
        node: u32,
        path: String,
        combo: Combo,
        probabilities: Vec<f32>,
    },
    /// The node is not one a river spot can start at.
    #[error("node {node} (`{path}`) {reason}")]
    NotRiverStart {
        node: u32,
        path: String,
        reason: String,
    },
}

/// What follows "the header" in a message about `field`: "'s `field`", or nothing for the
/// header as a whole.
fn field_name(field: &str) -> String {
    if field.is_empty() {
        String::new()
    } else {
        format!("'s `{field}`")
    }
}

// ---------------------------------------------------------------------------------------------
// Quantizing

/// The codes of one hand's probabilities at a node, as the
/// [module documentation](self#strategies) describes: `⌊65535 · p⌋` for each, and one more for
/// each of the largest remainders, ties to the lower index, until they sum to exactly 65,535.
///
/// `None` if `probabilities` is empty, holds a value that is not from 0 to 1, or does not sum
/// to 1 within [`STRATEGY_TOLERANCE`].
pub fn quantize(probabilities: &[f32]) -> Option<Vec<u16>> {
    let mut codes = vec![0; probabilities.len()];
    quantize_into(probabilities, &mut codes).then_some(codes)
}

/// The probability a code stands for: `code / 65535`, computed in `f32`.
pub fn dequantize(code: u16) -> f32 {
    f32::from(code) / f32::from(CODE_TOTAL)
}

/// [`quantize`] into `codes`, which has one entry per probability. False, and `codes` left
/// holding anything, if the probabilities are not a distribution.
fn quantize_into(probabilities: &[f32], codes: &mut [u16]) -> bool {
    debug_assert_eq!(probabilities.len(), codes.len());
    if probabilities.is_empty() {
        return false;
    }
    let mut sum = 0.0;
    for &p in probabilities {
        if !(0.0..=1.0).contains(&p) {
            return false;
        }
        sum += f64::from(p);
    }
    if (sum - 1.0).abs() > STRATEGY_TOLERANCE {
        return false;
    }
    let total = f64::from(CODE_TOTAL);
    // An f32 has 24 bits of mantissa and 65,535 needs 16, so each product is exact in f64.
    let mut floors = 0u32;
    for (code, &p) in codes.iter_mut().zip(probabilities) {
        let floor = (f64::from(p) * total).floor();
        *code = floor as u16;
        floors += floor as u32;
    }
    // The sum is within 1e-6 of 1, so the floors are at most 65,535 and fall short of it by
    // at most one per action: one code at most goes to each.
    let mut missing = u32::from(CODE_TOTAL)
        .checked_sub(floors)
        .expect("the floors of a distribution sum to at most 65,535");
    debug_assert!(missing as usize <= probabilities.len());
    while missing > 0 {
        // The action with the largest remainder that has not had its code yet, the first of
        // those with the largest. An action has had it when its code is above its product.
        let mut best: Option<(usize, f64)> = None;
        for (action, (&code, &p)) in codes.iter().zip(probabilities).enumerate() {
            let exact = f64::from(p) * total;
            if f64::from(code) > exact {
                continue;
            }
            let remainder = exact - f64::from(code);
            if best.is_none_or(|(_, largest)| remainder > largest) {
                best = Some((action, remainder));
            }
        }
        let (action, _) = best.expect("an action is left for every code missing");
        codes[action] += 1;
        missing -= 1;
    }
    true
}

/// A profile with `profile`'s strategy quantized and decoded at every player node of `game`:
/// the strategy a solution file of `profile` holds. Fails at the first hand whose strategy is
/// not a distribution, or at a player node with no strategy or one of the wrong size.
pub fn quantize_profile(
    game: &PostflopGame,
    profile: &PublicProfile,
) -> Result<PublicProfile, SolutionFileError> {
    let mut quantized = PublicProfile::empty(game.num_nodes());
    let mut codes = Vec::new();
    for node in 0..game.num_nodes() as u32 {
        let Some(actor) = game.actor(node) else {
            continue;
        };
        quantize_node(game, profile, node, actor, &mut codes)?;
        let decoded = codes.iter().map(|&code| dequantize(code)).collect();
        quantized.set(node as usize, game.hands(actor).len(), decoded);
    }
    Ok(quantized)
}

/// The codes of `profile`'s strategy at the player node `node`, laid out `[actions × hands]`,
/// into `codes`.
fn quantize_node(
    game: &PostflopGame,
    profile: &PublicProfile,
    node: u32,
    actor: Player,
    codes: &mut Vec<u16>,
) -> Result<(), SolutionFileError> {
    let num_hands = game.hands(actor).len();
    let num_actions = game.children(node).len();
    let strategy = profile
        .get(node as usize)
        .filter(|strategy| strategy.len() == num_actions * num_hands);
    codes.clear();
    codes.reserve_exact(num_actions * num_hands);
    codes.resize(num_actions * num_hands, 0);
    let mut row = vec![0.0f32; num_actions];
    let mut row_codes = vec![0u16; num_actions];
    for hand in 0..num_hands {
        let ok = match strategy {
            Some(strategy) => {
                for (action, p) in row.iter_mut().enumerate() {
                    *p = strategy[action * num_hands + hand];
                }
                quantize_into(&row, &mut row_codes)
            }
            None => false,
        };
        if !ok {
            return Err(SolutionFileError::Strategy {
                node,
                path: game.node_path(node),
                combo: game.hands(actor).combos()[hand],
                probabilities: if strategy.is_some() { row } else { Vec::new() },
            });
        }
        for (action, &code) in row_codes.iter().enumerate() {
            codes[action * num_hands + hand] = code;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// The tree hash

/// FNV-1a, 64 bits, over bytes written one value at a time.
struct Fnv(u64);

impl Fnv {
    fn new() -> Fnv {
        Fnv(0xcbf2_9ce4_8422_2325)
    }

    fn bytes(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.0 ^= u64::from(byte);
            self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }

    fn u8(&mut self, value: u8) {
        self.bytes(&[value]);
    }

    fn u16(&mut self, value: u16) {
        self.bytes(&value.to_le_bytes());
    }

    fn u32(&mut self, value: u32) {
        self.bytes(&value.to_le_bytes());
    }

    fn u64(&mut self, value: u64) {
        self.bytes(&value.to_le_bytes());
    }
}

/// The tree hash of `game`, as the [module documentation](self#the-tree) defines it.
pub fn tree_hash(game: &PostflopGame) -> u64 {
    let mut hash = Fnv::new();
    hash.bytes(b"pk-postflop solution tree hash");
    hash.u8(1);
    let rake = game.rake();
    hash.u32(game.pot());
    hash.u64(rake.percent.to_bits());
    hash.u32(rake.cap);
    hash.u8(u8::from(rake.no_flop_no_drop));
    let num_nodes = game.num_nodes() as u32;
    hash.u32(num_nodes);
    for node in 0..num_nodes {
        match game.node(node as usize) {
            PublicNode::Player { player, .. } => {
                hash.u8(1);
                hash.u8(player as u8);
                let actions = game.actions(node);
                hash.u32(actions.len() as u32);
                for action in actions {
                    let (kind, to) = match *action {
                        Action::Fold => (0, 0),
                        Action::Check => (1, 0),
                        Action::Call => (2, 0),
                        Action::Bet { to, .. } => (3, to),
                        Action::Raise { to, .. } => (4, to),
                        Action::AllIn { to } => (5, to),
                    };
                    hash.u8(kind);
                    hash.u32(to);
                }
            }
            PublicNode::Chance { .. } => {
                hash.u8(2);
                let cards = game.outcomes(node);
                hash.u32(cards.len() as u32);
                for card in cards {
                    hash.u8(card.index() as u8);
                }
            }
            PublicNode::Terminal => {
                let Node::Terminal {
                    outcome,
                    contributions,
                    ..
                } = game.tree().node(game.tree_node(node))
                else {
                    unreachable!("a public terminal is a betting terminal");
                };
                hash.u8(0);
                hash.u8(match outcome {
                    Outcome::Fold {
                        winner: Player::Oop,
                    } => 0,
                    Outcome::Fold { winner: Player::Ip } => 1,
                    Outcome::Showdown => 2,
                });
                hash.u32(contributions[0]);
                hash.u32(contributions[1]);
            }
        }
    }
    for player in [Player::Oop, Player::Ip] {
        let hands = game.hands(player);
        hash.u32(hands.len() as u32);
        for (combo, weight) in hands.combos().iter().zip(hands.weights()) {
            hash.u16(combo.index() as u16);
            hash.u64(weight.to_bits());
        }
    }
    hash.0
}

// ---------------------------------------------------------------------------------------------
// The header

/// One section, as the header lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SectionInfo {
    /// `strategy.<street>` or `ev.<street>`.
    pub name: String,
    /// Its length in the file, compressed, in bytes.
    pub length: u64,
    /// Its length once decompressed, in bytes.
    pub raw_length: u64,
}

/// A solution file's header, as the [module documentation](self#the-header) describes it, with
/// the format version from the preamble.
#[derive(Debug, Clone, PartialEq)]
pub struct Header {
    /// The format version: [`FORMAT_VERSION`].
    pub version: u16,
    /// The program that wrote the file, `pk-postflop` for this one.
    pub solver: String,
    /// Its version, such as `0.1.0`.
    pub solver_version: String,
    pub spot: Spot,
    /// The tree hash of the game the file was written for.
    pub tree_hash: u64,
    /// The number of nodes of the game's public tree.
    pub nodes: u32,
    /// The number of its player nodes.
    pub player_nodes: u32,
    /// The number of each player's hands, indexed by [`Player::index`].
    pub hands: [u32; 2],
    /// The streets the file holds, from the spot's first, in order.
    pub streets: Vec<Street>,
    pub stop: StopReason,
    pub iterations: u64,
    /// What the solve measured for the strategy it found, before quantizing.
    pub solve_exploitability: Exploitability,
    /// The exploitability of the strategy the file holds, measured on the whole tree when the
    /// file was written.
    pub exploitability: Exploitability,
    /// The sections, in file order.
    pub sections: Vec<SectionInfo>,
}

impl Header {
    /// The last street the file holds.
    pub fn last_street(&self) -> Street {
        *self.streets.last().expect("a header holds a street")
    }

    /// Whether the file holds every street.
    pub fn is_complete(&self) -> bool {
        self.last_street() == Street::River
    }

    /// The writer and its version, such as `pk-postflop 0.1.0`.
    pub fn written_by(&self) -> String {
        format!("{} {}", self.solver, self.solver_version)
    }

    /// The header's JSON text.
    fn to_json(&self) -> String {
        #[derive(Serialize)]
        struct Out<'a> {
            solver: SolverOut<'a>,
            spot: &'a str,
            tree: TreeOut,
            hands: HandsOut,
            streets: Vec<&'static str>,
            solve: SolveOut,
            exploitability: ExploitabilityOut,
            sections: Vec<SectionOut<'a>>,
        }
        #[derive(Serialize)]
        struct SolverOut<'a> {
            name: &'a str,
            version: &'a str,
        }
        #[derive(Serialize)]
        struct TreeOut {
            hash: String,
            nodes: u32,
            player_nodes: u32,
        }
        #[derive(Serialize)]
        struct HandsOut {
            oop: u32,
            ip: u32,
        }
        #[derive(Serialize)]
        struct SolveOut {
            stop: &'static str,
            iterations: u64,
            exploitability: ExploitabilityOut,
        }
        #[derive(Serialize)]
        struct ExploitabilityOut {
            values: [f64; 2],
            best_responses: [f64; 2],
            chips: f64,
            percent_of_pot: f64,
        }
        #[derive(Serialize)]
        struct SectionOut<'a> {
            name: &'a str,
            length: u64,
            raw_length: u64,
        }
        let exploitability = |e: &Exploitability| ExploitabilityOut {
            values: e.values,
            best_responses: e.best_responses,
            chips: e.chips,
            percent_of_pot: e.percent_of_pot,
        };
        let spot = self.spot.to_toml();
        let out = Out {
            solver: SolverOut {
                name: &self.solver,
                version: &self.solver_version,
            },
            spot: &spot,
            tree: TreeOut {
                hash: format!("{:016x}", self.tree_hash),
                nodes: self.nodes,
                player_nodes: self.player_nodes,
            },
            hands: HandsOut {
                oop: self.hands[0],
                ip: self.hands[1],
            },
            streets: self.streets.iter().map(|street| street.name()).collect(),
            solve: SolveOut {
                stop: stop_name(self.stop),
                iterations: self.iterations,
                exploitability: exploitability(&self.solve_exploitability),
            },
            exploitability: exploitability(&self.exploitability),
            sections: self
                .sections
                .iter()
                .map(|section| SectionOut {
                    name: &section.name,
                    length: section.length,
                    raw_length: section.raw_length,
                })
                .collect(),
        };
        serde_json::to_string(&out).expect("the header is plain data with finite numbers")
    }
}

fn stop_name(stop: StopReason) -> &'static str {
    match stop {
        StopReason::Target => "target",
        StopReason::IterationCap => "iteration_cap",
        StopReason::TimeCap => "time_cap",
    }
}

/// The names of the two sections of `street`, in file order.
fn section_names(street: Street) -> [String; 2] {
    [format!("strategy.{street}"), format!("ev.{street}")]
}

/// The raw lengths, in bytes, of the strategy section and of the EV section of each of
/// `streets`, from the tree: `2 × actions × hands` and `4 × hands` over the street's player
/// nodes.
fn raw_sizes(game: &PostflopGame, streets: &[Street]) -> (Vec<u64>, Vec<u64>) {
    let mut strategy = vec![0u64; streets.len()];
    let mut ev = vec![0u64; streets.len()];
    for node in 0..game.num_nodes() as u32 {
        let Some(actor) = game.actor(node) else {
            continue;
        };
        if let Some(kept) = streets.iter().position(|&s| s == game.street(node)) {
            let hands = game.hands(actor).len() as u64;
            strategy[kept] += 2 * game.children(node).len() as u64 * hands;
            ev[kept] += 4 * hands;
        }
    }
    (strategy, ev)
}

/// Reads a header's JSON text strictly. Errors name the field and leave the path out: the
/// caller fills it in.
mod read {
    use super::*;

    /// A header error with no path yet.
    pub(super) struct Invalid {
        pub field: String,
        pub reason: String,
    }

    type Result<T> = std::result::Result<T, Invalid>;

    fn invalid<T>(field: &str, reason: impl Into<String>) -> Result<T> {
        Err(Invalid {
            field: field.to_string(),
            reason: reason.into(),
        })
    }

    fn kind(value: &Json) -> &'static str {
        match value {
            Json::Null => "null",
            Json::Bool(_) => "a boolean",
            Json::Integer(_) | Json::Float(_) => "a number",
            Json::String(_) => "a string",
            Json::Array(_) => "an array",
            Json::Object(_) => "an object",
        }
    }

    fn join(path: &str, key: &str) -> String {
        if path.is_empty() {
            key.to_string()
        } else {
            format!("{path}.{key}")
        }
    }

    /// `a`, `b` and `c`.
    fn list(keys: &[&str]) -> String {
        let quoted: Vec<String> = keys.iter().map(|key| format!("`{key}`")).collect();
        match quoted.split_last() {
            Some((last, rest)) if !rest.is_empty() => format!("{} and {last}", rest.join(", ")),
            _ => quoted.concat(),
        }
    }

    /// The keys of one object, named by its path, which must hold exactly `keys`.
    struct Object<'a> {
        entries: &'a [(String, Json)],
        path: String,
    }

    impl<'a> Object<'a> {
        fn new(value: &'a Json, path: &str, keys: &[&str]) -> Result<Object<'a>> {
            let Json::Object(entries) = value else {
                let what = if path.is_empty() { "" } else { " of" };
                return invalid(
                    path,
                    format!(
                        "is {}; it must be an object{what} {}",
                        kind(value),
                        list(keys)
                    ),
                );
            };
            if let Some((key, _)) = entries
                .iter()
                .find(|(key, _)| !keys.contains(&key.as_str()))
            {
                let place = if path.is_empty() {
                    "the header".to_string()
                } else {
                    format!("`{path}`")
                };
                return invalid(
                    &join(path, key),
                    format!("is not a key of {place}; its keys are {}", list(keys)),
                );
            }
            Ok(Object {
                entries,
                path: path.to_string(),
            })
        }

        fn field(&self, key: &str) -> String {
            join(&self.path, key)
        }

        fn get(&self, key: &str) -> Result<(&'a Json, String)> {
            let field = self.field(key);
            match self.entries.iter().find(|(k, _)| k == key) {
                Some((_, value)) => Ok((value, field)),
                None => invalid(&field, "is missing"),
            }
        }

        fn object(&self, key: &str, keys: &[&str]) -> Result<Object<'a>> {
            let (value, field) = self.get(key)?;
            Object::new(value, &field, keys)
        }

        fn string(&self, key: &str) -> Result<&'a str> {
            let (value, field) = self.get(key)?;
            match value {
                Json::String(text) => Ok(text),
                other => invalid(&field, format!("is {}; it must be a string", kind(other))),
            }
        }

        /// A whole number from 0 to `max`.
        fn count(&self, key: &str, max: u64) -> Result<u64> {
            let (value, field) = self.get(key)?;
            match value {
                Json::Integer(n) if (0..=i128::from(max)).contains(n) => Ok(*n as u64),
                Json::Integer(n) => invalid(&field, format!("is {n}; it must be from 0 to {max}")),
                other => invalid(
                    &field,
                    format!("is {}; it must be a whole number", kind(other)),
                ),
            }
        }

        fn number(&self, key: &str) -> Result<f64> {
            let (value, field) = self.get(key)?;
            number(value, &field)
        }

        fn pair(&self, key: &str) -> Result<[f64; 2]> {
            let (value, field) = self.get(key)?;
            match value {
                Json::Array(items) if items.len() == 2 => Ok([
                    number(&items[0], &format!("{field}[0]"))?,
                    number(&items[1], &format!("{field}[1]"))?,
                ]),
                Json::Array(items) => invalid(
                    &field,
                    format!(
                        "has {} numbers; it must have 2, OOP's then IP's",
                        items.len()
                    ),
                ),
                other => invalid(
                    &field,
                    format!("is {}; it must be an array of 2 numbers", kind(other)),
                ),
            }
        }

        fn array(&self, key: &str) -> Result<(&'a [Json], String)> {
            let (value, field) = self.get(key)?;
            match value {
                Json::Array(items) => Ok((items, field)),
                other => invalid(&field, format!("is {}; it must be an array", kind(other))),
            }
        }
    }

    fn number(value: &Json, field: &str) -> Result<f64> {
        match value {
            Json::Float(x) => Ok(*x),
            // Exactly, below 2^53; larger whole numbers are no value the writer writes.
            Json::Integer(n) if n.unsigned_abs() <= 1 << 53 => Ok(*n as f64),
            Json::Integer(n) => invalid(field, format!("is {n}, too large to be read exactly")),
            other => invalid(field, format!("is {}; it must be a number", kind(other))),
        }
    }

    fn exploitability(object: Object<'_>) -> Result<Exploitability> {
        Ok(Exploitability {
            values: object.pair("values")?,
            best_responses: object.pair("best_responses")?,
            chips: object.number("chips")?,
            percent_of_pot: object.number("percent_of_pot")?,
        })
    }

    const EXPLOITABILITY_KEYS: [&str; 4] = ["values", "best_responses", "chips", "percent_of_pot"];

    fn street(value: &Json, field: &str) -> Result<Street> {
        let name = match value {
            Json::String(name) => name.as_str(),
            other => return invalid(field, format!("is {}; it must be a street", kind(other))),
        };
        Street::ALL
            .into_iter()
            .find(|street| street.name() == name)
            .map_or_else(
                || {
                    invalid(
                        field,
                        format!("is `{name}`; a street is `flop`, `turn` or `river`"),
                    )
                },
                Ok,
            )
    }

    /// The header of `text`, with `version` from the preamble and the spot read by `spot`,
    /// whose error goes back to the caller as it is.
    pub(super) fn header<E>(
        text: &str,
        version: u16,
        spot: impl FnOnce(&str) -> std::result::Result<Spot, E>,
    ) -> std::result::Result<Result<Header>, E> {
        let fields = match fields(text) {
            Ok(fields) => fields,
            Err(invalid) => return Ok(Err(invalid)),
        };
        let spot = spot(&fields.spot)?;
        Ok(Ok(Header {
            version,
            solver: fields.solver,
            solver_version: fields.solver_version,
            spot,
            tree_hash: fields.tree_hash,
            nodes: fields.nodes,
            player_nodes: fields.player_nodes,
            hands: fields.hands,
            streets: fields.streets,
            stop: fields.stop,
            iterations: fields.iterations,
            solve_exploitability: fields.solve_exploitability,
            exploitability: fields.exploitability,
            sections: fields.sections,
        }))
    }

    /// The header's fields, with the spot as its text.
    pub(super) struct Fields {
        pub solver: String,
        pub solver_version: String,
        pub spot: String,
        pub tree_hash: u64,
        pub nodes: u32,
        pub player_nodes: u32,
        pub hands: [u32; 2],
        pub streets: Vec<Street>,
        pub stop: StopReason,
        pub iterations: u64,
        pub solve_exploitability: Exploitability,
        pub exploitability: Exploitability,
        pub sections: Vec<SectionInfo>,
    }

    /// The header's fields from its JSON text, every one checked but the spot, which is left
    /// as its text.
    pub(super) fn fields(text: &str) -> Result<Fields> {
        let value: Json = serde_json::from_str(text).map_err(|error| Invalid {
            field: String::new(),
            reason: format!("is not valid JSON: {error}"),
        })?;
        let top = Object::new(
            &value,
            "",
            &[
                "solver",
                "spot",
                "tree",
                "hands",
                "streets",
                "solve",
                "exploitability",
                "sections",
            ],
        )?;
        let solver = top.object("solver", &["name", "version"])?;
        let name = solver.string("name")?.to_string();
        let solver_version = solver.string("version")?.to_string();
        let spot = top.string("spot")?.to_string();

        let tree = top.object("tree", &["hash", "nodes", "player_nodes"])?;
        let hash_text = tree.string("hash")?;
        let tree_hash = if hash_text.len() == 16
            && hash_text
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            u64::from_str_radix(hash_text, 16).expect("16 hexadecimal digits are a u64")
        } else {
            return invalid(
                "tree.hash",
                format!("is `{hash_text}`; it must be 16 lowercase hexadecimal digits"),
            );
        };
        let max = u64::from(u32::MAX);
        let nodes = tree.count("nodes", max)? as u32;
        let player_nodes = tree.count("player_nodes", max)? as u32;

        let hands = top.object("hands", &["oop", "ip"])?;
        let hands = [
            hands.count("oop", max)? as u32,
            hands.count("ip", max)? as u32,
        ];

        let (items, field) = top.array("streets")?;
        let streets = items
            .iter()
            .enumerate()
            .map(|(i, item)| street(item, &format!("{field}[{i}]")))
            .collect::<Result<Vec<Street>>>()?;
        if streets.is_empty() {
            return invalid("streets", "is empty; a file holds at least one street");
        }
        for (i, pair) in streets.windows(2).enumerate() {
            if pair[0].next() != Some(pair[1]) {
                return invalid(
                    &format!("streets[{}]", i + 1),
                    format!(
                        "is the {} after the {}; the streets are in order, with none left out",
                        pair[1], pair[0]
                    ),
                );
            }
        }

        let solve = top.object("solve", &["stop", "iterations", "exploitability"])?;
        let stop = match solve.string("stop")? {
            "target" => StopReason::Target,
            "iteration_cap" => StopReason::IterationCap,
            "time_cap" => StopReason::TimeCap,
            other => {
                return invalid(
                    "solve.stop",
                    format!("is `{other}`; it must be `target`, `iteration_cap` or `time_cap`"),
                );
            }
        };
        let iterations = solve.count("iterations", u64::MAX)?;
        let solve_exploitability =
            exploitability(solve.object("exploitability", &EXPLOITABILITY_KEYS)?)?;
        let stored = exploitability(top.object("exploitability", &EXPLOITABILITY_KEYS)?)?;

        let (items, field) = top.array("sections")?;
        let sections = items
            .iter()
            .enumerate()
            .map(|(i, item)| {
                let section = Object::new(
                    item,
                    &format!("{field}[{i}]"),
                    &["name", "length", "raw_length"],
                )?;
                Ok(SectionInfo {
                    name: section.string("name")?.to_string(),
                    length: section.count("length", u64::MAX)?,
                    raw_length: section.count("raw_length", u64::MAX)?,
                })
            })
            .collect::<Result<Vec<SectionInfo>>>()?;
        let expected: Vec<String> = streets.iter().flat_map(|&s| section_names(s)).collect();
        for (i, name) in expected.iter().enumerate() {
            match sections.get(i) {
                Some(section) if &section.name == name => {}
                Some(section) => {
                    return invalid(
                        &format!("sections[{i}].name"),
                        format!(
                            "is `{}`; for the streets {}, section {i} is `{name}`",
                            section.name,
                            street_list(&streets)
                        ),
                    );
                }
                None => {
                    return invalid(
                        "sections",
                        format!(
                            "lists {} sections; the streets {} have {}, the last `{}`",
                            sections.len(),
                            street_list(&streets),
                            expected.len(),
                            expected[expected.len() - 1]
                        ),
                    );
                }
            }
        }
        if sections.len() > expected.len() {
            return invalid(
                &format!("sections[{}]", expected.len()),
                format!(
                    "is `{}`, after the last section of the streets {}",
                    sections[expected.len()].name,
                    street_list(&streets)
                ),
            );
        }

        Ok(Fields {
            solver: name,
            solver_version,
            spot,
            tree_hash,
            nodes,
            player_nodes,
            hands,
            streets,
            stop,
            iterations,
            solve_exploitability,
            exploitability: stored,
            sections,
        })
    }

    fn street_list(streets: &[Street]) -> String {
        let names: Vec<&str> = streets.iter().map(|s| s.name()).collect();
        names.join(", ")
    }
}

// ---------------------------------------------------------------------------------------------
// Saving

/// How [`save`] writes a file. [`SaveOptions::default`] keeps every street.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SaveOptions {
    /// The last street the file holds: the river, the default, for every street, or the turn
    /// or the flop to leave the streets after it out. It may not be before the spot's first
    /// street.
    pub last_street: Street,
}

impl Default for SaveOptions {
    fn default() -> SaveOptions {
        SaveOptions {
            last_street: Street::River,
        }
    }
}

/// What [`save`] wrote.
#[derive(Debug, Clone, PartialEq)]
pub struct SaveReport {
    /// The file's length in bytes.
    pub bytes: u64,
    /// The header's length in bytes.
    pub header_bytes: u64,
    /// The sections, in file order.
    pub sections: Vec<SectionInfo>,
    /// The exploitability of the strategy the file holds, as its header records it.
    pub exploitability: Exploitability,
    /// What the solve measured for its strategy before quantizing.
    pub solve_exploitability: Exploitability,
}

/// Saves `solution` to `path` as a solution file, holding the streets from the spot's first to
/// `options.last_street`, as the [module documentation](self) describes. It writes
/// `<path>.part` and renames it to `path`, replacing any file there. It runs on the calling
/// thread, and holds what [`estimate_save_memory`](crate::memory::estimate_save_memory) counts.
///
/// Fails if the solution does not hold every street, if `options.last_street` is before the
/// spot's first street, if a hand's strategy at a node is not a distribution, or if the file
/// cannot be written.
pub fn save(
    solution: &Solution,
    path: impl AsRef<Path>,
    options: &SaveOptions,
) -> Result<SaveReport, SolutionFileError> {
    let path = path.as_ref();
    if !solution.is_complete() {
        return Err(SolutionFileError::Incomplete {
            last_street: solution.last_street(),
        });
    }
    let spot = solution.spot();
    let first = spot.street();
    if options.last_street < first {
        return Err(SolutionFileError::Options {
            last_street: options.last_street,
            first,
        });
    }
    let game = solution.game();
    let streets: Vec<Street> = Street::ALL
        .into_iter()
        .filter(|&street| first <= street && street <= options.last_street)
        .collect();

    // Each kept street's sections are sized from the tree.
    let (strategy_sizes, ev_sizes) = raw_sizes(game, &streets);
    let compress_error = |source| SolutionFileError::Io {
        action: "compress a section for",
        path: path.to_path_buf(),
        source,
    };

    // The quantized strategy on the whole tree, each kept node's codes written as they are
    // made to its street's encoder, so that no street's raw codes are held.
    let mut encoders = strategy_sizes
        .iter()
        .map(|&size| section_encoder(size))
        .collect::<io::Result<Vec<_>>>()
        .map_err(compress_error)?;
    let mut quantized = PublicProfile::empty(game.num_nodes());
    let mut codes = Vec::new();
    let mut bytes = Vec::new();
    for node in 0..game.num_nodes() as u32 {
        let Some(actor) = game.actor(node) else {
            continue;
        };
        quantize_node(game, solution.strategy(), node, actor, &mut codes)?;
        if let Some(kept) = streets.iter().position(|&s| s == game.street(node)) {
            bytes.clear();
            bytes.reserve_exact(2 * codes.len());
            for &code in &codes {
                bytes.extend_from_slice(&code.to_le_bytes());
            }
            encoders[kept].write_all(&bytes).map_err(compress_error)?;
        }
        let decoded = codes.iter().map(|&code| dequantize(code)).collect();
        quantized.set(node as usize, game.hands(actor).len(), decoded);
    }
    drop((codes, bytes));
    // Each street's strategy section, as its bytes, shrunk to them.
    let strategy_sections = encoders
        .into_iter()
        .map(|encoder| finish_section(encoder).map_err(compress_error))
        .collect::<Result<Vec<Vec<u8>>, SolutionFileError>>()?;

    // Every number describes the stored strategy, measured on the whole tree.
    let measured = exploitability(game, &quantized);
    // The EVs go from the walk to each kept street's raw section, at the node's place in it.
    let mut places = vec![u64::MAX; game.num_nodes()];
    let mut ends = vec![0u64; streets.len()];
    for node in 0..game.num_nodes() as u32 {
        let Some(actor) = game.actor(node) else {
            continue;
        };
        if let Some(kept) = streets.iter().position(|&s| s == game.street(node)) {
            places[node as usize] = ends[kept];
            ends[kept] += 4 * game.hands(actor).len() as u64;
        }
    }
    debug_assert_eq!(ends, ev_sizes);
    let mut ev_raw: Vec<Vec<u8>> = ev_sizes
        .iter()
        .map(|&size| vec![0; size as usize])
        .collect();
    node_values_into(game, &quantized, |node, evs| {
        let place = places[node as usize];
        if place == u64::MAX {
            return;
        }
        let kept = streets
            .iter()
            .position(|&s| s == game.street(node))
            .expect("a node with a place is on a kept street");
        let place = place as usize;
        let raw = &mut ev_raw[kept][place..place + 4 * evs.len()];
        for (slot, ev) in raw.as_chunks_mut::<4>().0.iter_mut().zip(&evs) {
            let bits = match ev {
                Some(ev) => (*ev as f32).to_bits(),
                None => NAN_BITS,
            };
            *slot = bits.to_le_bytes();
        }
    });
    drop((places, quantized));
    // Each street's EV section, its raw bytes dropped once they are compressed.
    let mut ev_sections = Vec::with_capacity(streets.len());
    for raw in ev_raw {
        let mut encoder = section_encoder(raw.len() as u64).map_err(compress_error)?;
        encoder.write_all(&raw).map_err(compress_error)?;
        drop(raw);
        ev_sections.push(finish_section(encoder).map_err(compress_error)?);
    }
    // Every section in file order, as its name, its raw length and its bytes.
    let mut compressed: Vec<(String, u64, Vec<u8>)> = Vec::new();
    for (index, (strategy_bytes, ev_bytes)) in
        strategy_sections.into_iter().zip(ev_sections).enumerate()
    {
        let [strategy_name, ev_name] = section_names(streets[index]);
        compressed.push((strategy_name, strategy_sizes[index], strategy_bytes));
        compressed.push((ev_name, ev_sizes[index], ev_bytes));
    }

    let header = Header {
        version: FORMAT_VERSION,
        solver: SOLVER_NAME.to_string(),
        solver_version: SOLVER_VERSION.to_string(),
        spot: spot.clone(),
        tree_hash: tree_hash(game),
        nodes: game.num_nodes() as u32,
        player_nodes: (0..game.num_nodes() as u32)
            .filter(|&node| game.actor(node).is_some())
            .count() as u32,
        hands: [Player::Oop, Player::Ip].map(|player| game.hands(player).len() as u32),
        streets,
        stop: solution.stop(),
        iterations: solution.iterations(),
        solve_exploitability: *solution.solve_exploitability(),
        exploitability: measured,
        sections: compressed
            .iter()
            .map(|(name, raw_length, bytes)| SectionInfo {
                name: name.clone(),
                length: bytes.len() as u64,
                raw_length: *raw_length,
            })
            .collect(),
    };
    // JSON has no NaN or infinity, and a reader takes no header longer than its limit.
    for (field, e) in [
        ("solve.exploitability", &header.solve_exploitability),
        ("exploitability", &header.exploitability),
    ] {
        let [v0, v1] = e.values;
        let [b0, b1] = e.best_responses;
        if ![v0, v1, b0, b1, e.chips, e.percent_of_pot]
            .iter()
            .all(|x| x.is_finite())
        {
            return Err(SolutionFileError::Header {
                path: path.to_path_buf(),
                field: field.to_string(),
                reason: format!("would hold a number that is not finite: {e:?}"),
            });
        }
    }
    let json = header.to_json();
    let header_bytes = json.len() as u64;
    if header_bytes > u64::from(MAX_HEADER_BYTES) {
        return Err(SolutionFileError::Header {
            path: path.to_path_buf(),
            field: String::new(),
            reason: format!(
                "would be {header_bytes} bytes long, more than the {MAX_HEADER_BYTES} a reader \
                 takes"
            ),
        });
    }
    write_atomically(path, |file| {
        file.write_all(&MAGIC)?;
        file.write_all(&FORMAT_VERSION.to_le_bytes())?;
        let length = u32::try_from(json.len()).expect("the header is at most 1 MiB");
        file.write_all(&length.to_le_bytes())?;
        file.write_all(json.as_bytes())?;
        for (_, _, bytes) in &compressed {
            file.write_all(bytes)?;
        }
        Ok(())
    })?;
    let bytes = PREAMBLE_BYTES
        + header_bytes
        + compressed
            .iter()
            .map(|(_, _, bytes)| bytes.len() as u64)
            .sum::<u64>();
    Ok(SaveReport {
        bytes,
        header_bytes,
        sections: header.sections,
        exploitability: header.exploitability,
        solve_exploitability: header.solve_exploitability,
    })
}

/// What a section's raw bytes are written to: a zstd frame at [`LEVEL`], with its content size
/// and checksum, made into room for the most zstd can make of `raw_length` bytes
/// ([`section_room`]), so that the room never grows as the frame is written. The bytes of the
/// frame do not depend on how its raw bytes are split between writes.
fn section_encoder(raw_length: u64) -> io::Result<SectionEncoder> {
    let room = Vec::with_capacity(section_room(raw_length) as usize);
    let mut encoder = zstd::stream::write::Encoder::new(room, LEVEL)?;
    encoder.include_checksum(true)?;
    encoder.include_contentsize(true)?;
    encoder.set_pledged_src_size(Some(raw_length))?;
    Ok(encoder)
}

pub(crate) type SectionEncoder = zstd::stream::write::Encoder<'static, Vec<u8>>;

/// The room a section of `raw_length` raw bytes is compressed into: zstd's bound on what it
/// makes of that many bytes, which [`crate::memory::estimate_save_memory`] counts as well.
pub(crate) fn section_room(raw_length: u64) -> u64 {
    zstd::zstd_safe::compress_bound(raw_length as usize) as u64
}

/// Ends a section's frame, and shrinks its bytes to their length.
fn finish_section(encoder: SectionEncoder) -> io::Result<Vec<u8>> {
    let mut bytes = encoder.finish()?;
    bytes.shrink_to_fit();
    Ok(bytes)
}

/// `<path>.part`.
fn part_path(path: &Path) -> PathBuf {
    let mut name = OsString::from(path.as_os_str());
    name.push(".part");
    PathBuf::from(name)
}

/// Writes `path` by writing `<path>.part` with `write`, flushing it to disk and renaming it.
/// On failure the part file is removed.
fn write_atomically(
    path: &Path,
    write: impl FnOnce(&mut File) -> io::Result<()>,
) -> Result<(), SolutionFileError> {
    let part = part_path(path);
    let result = File::create(&part)
        .map_err(|source| ("create", source))
        .and_then(|mut file| {
            write(&mut file).map_err(|source| ("write", source))?;
            file.sync_all().map_err(|source| ("flush", source))
        });
    if let Err((action, source)) = result {
        let _ = fs::remove_file(&part);
        return Err(SolutionFileError::Io {
            action,
            path: part,
            source,
        });
    }
    fs::rename(&part, path).map_err(|source| {
        let _ = fs::remove_file(&part);
        SolutionFileError::Io {
            action: "rename the finished file to",
            path: path.to_path_buf(),
            source,
        }
    })
}

// ---------------------------------------------------------------------------------------------
// Reading

/// An open file with its header read and checked against the file's length.
struct Opened {
    file: File,
    path: PathBuf,
    header: Header,
    /// Where the first section starts.
    sections_start: u64,
}

fn io_error(action: &'static str, path: &Path) -> impl FnOnce(io::Error) -> SolutionFileError {
    let path = path.to_path_buf();
    move |source| SolutionFileError::Io {
        action,
        path,
        source,
    }
}

/// Opens `path` and reads its preamble and header, checking everything that needs no game.
fn open(path: &Path) -> Result<Opened, SolutionFileError> {
    let mut file = File::open(path).map_err(io_error("open", path))?;
    let length = file.metadata().map_err(io_error("read", path))?.len();
    let mut preamble = Vec::with_capacity(PREAMBLE_BYTES as usize);
    (&mut file)
        .take(PREAMBLE_BYTES)
        .read_to_end(&mut preamble)
        .map_err(io_error("read", path))?;
    let truncated = |place: &str| SolutionFileError::Truncated {
        path: path.to_path_buf(),
        place: place.to_string(),
    };
    if preamble.len() < MAGIC.len() || preamble[..MAGIC.len()] != MAGIC {
        return Err(SolutionFileError::NotASolutionFile {
            path: path.to_path_buf(),
        });
    }
    if preamble.len() < 7 {
        return Err(truncated("in the format version, after the magic"));
    }
    let version = u16::from_le_bytes([preamble[5], preamble[6]]);
    if version != FORMAT_VERSION {
        return Err(SolutionFileError::Version {
            path: path.to_path_buf(),
            found: version,
            supported: FORMAT_VERSION,
        });
    }
    if preamble.len() < PREAMBLE_BYTES as usize {
        return Err(truncated(
            "in the header's length, after the format version",
        ));
    }
    let header_length = u32::from_le_bytes([preamble[7], preamble[8], preamble[9], preamble[10]]);
    let header_error = |field: &str, reason: String| SolutionFileError::Header {
        path: path.to_path_buf(),
        field: field.to_string(),
        reason,
    };
    if header_length > MAX_HEADER_BYTES {
        return Err(header_error(
            "",
            format!(
                "is {header_length} bytes long by the preamble, more than the {MAX_HEADER_BYTES} \
                 a header may have"
            ),
        ));
    }
    let sections_start = PREAMBLE_BYTES + u64::from(header_length);
    if length < sections_start {
        return Err(truncated(&format!(
            "in the header, {} bytes into its {header_length}",
            length - PREAMBLE_BYTES
        )));
    }
    let mut text = Vec::with_capacity(header_length as usize);
    (&mut file)
        .take(u64::from(header_length))
        .read_to_end(&mut text)
        .map_err(io_error("read", path))?;
    let text = String::from_utf8(text)
        .map_err(|error| header_error("", format!("is not UTF-8: {error}")))?;
    let header = read::header(&text, version, Spot::from_toml)
        .map_err(|source| SolutionFileError::Spot {
            path: path.to_path_buf(),
            source: Box::new(source),
        })?
        .map_err(|invalid| header_error(&invalid.field, invalid.reason))?;
    let first = header.spot.street();
    if header.streets[0] != first {
        return Err(header_error(
            "streets",
            format!(
                "starts with the {}, but the spot starts on the {first}; a file holds the \
                 spot's first street and the streets after it",
                header.streets[0]
            ),
        ));
    }
    // The file's length is the sum of its parts.
    let mut end = sections_start;
    for section in &header.sections {
        let Some(next) = end
            .checked_add(section.length)
            .filter(|&next| next <= length)
        else {
            return Err(truncated(&format!(
                "in section `{}`, {} bytes into its {}",
                section.name,
                length - end,
                section.length
            )));
        };
        end = next;
    }
    if length > end {
        return Err(SolutionFileError::TrailingBytes {
            path: path.to_path_buf(),
            extra: length - end,
        });
    }
    Ok(Opened {
        file,
        path: path.to_path_buf(),
        header,
        sections_start,
    })
}

/// Reads the header of the solution file at `path`, and checks it, without building the game
/// or reading a section: see [reading](self#reading) for what it refuses.
pub fn read_header(path: impl AsRef<Path>) -> Result<Header, SolutionFileError> {
    Ok(open(path.as_ref())?.header)
}

/// A solution read from a solution file: the [`Solution`], whose strategy is the file's, its
/// EVs and its header.
#[derive(Debug, Clone)]
pub struct SavedSolution {
    header: Header,
    solution: Solution,
    /// By node: the acting player's EVs by hand at a player node the solution holds.
    evs: Vec<Option<Box<[f32]>>>,
}

impl SavedSolution {
    /// Reads the solution file at `path`, every street it holds.
    ///
    /// Builds the game from the header's spot and refuses the file if its tree hash is not
    /// the header's; see [reading](self#reading) for every check.
    pub fn load(path: impl AsRef<Path>) -> Result<SavedSolution, SolutionFileError> {
        SavedSolution::load_streets(path, Street::River)
    }

    /// Reads the solution file at `path` up to `last_street`, and reads none of the sections
    /// after it: up to the last street the file holds if that is earlier, and the spot's first
    /// street at least. A solution loaded without its last streets holds no strategy and no EVs
    /// on them ([partial solutions](Solution#partial-solutions)).
    pub fn load_streets(
        path: impl AsRef<Path>,
        last_street: Street,
    ) -> Result<SavedSolution, SolutionFileError> {
        let mut opened = open(path.as_ref())?;
        let header = &opened.header;
        let path = opened.path.clone();
        let mismatch = |detail: String| SolutionFileError::TreeMismatch {
            path: path.clone(),
            written_by: header.written_by(),
            reading: format!("{SOLVER_NAME} {SOLVER_VERSION}"),
            detail,
        };
        // The default limit, as a solve has, or the file's tree if it is larger: the game is
        // then the one a solve of the spot builds, node limit included.
        let limit = PostflopGame::DEFAULT_MAX_NODES.max(u64::from(header.nodes));
        let game = match PostflopGame::with_node_limit(&header.spot, limit) {
            Ok(game) => game,
            Err(SolveError::PublicTreeTooLarge { nodes, .. }) => {
                return Err(mismatch(format!(
                    "the public tree has {nodes} nodes here, and {} in the file",
                    header.nodes
                )));
            }
            Err(SolveError::Tree(pk_game::TreeError::TooLarge { limit })) => {
                return Err(mismatch(format!(
                    "the betting tree has more than {limit} nodes here, and the file's public \
                     tree {}",
                    header.nodes
                )));
            }
            Err(source) => {
                return Err(SolutionFileError::Game {
                    path,
                    source: Box::new(source),
                });
            }
        };
        if game.num_nodes() as u64 != u64::from(header.nodes) {
            return Err(mismatch(format!(
                "the public tree has {} nodes here, and {} in the file",
                game.num_nodes(),
                header.nodes
            )));
        }
        let built = tree_hash(&game);
        if built != header.tree_hash {
            return Err(mismatch(format!(
                "tree hash {:016x} in the file and {built:016x} here",
                header.tree_hash
            )));
        }
        let player_nodes = (0..game.num_nodes() as u32)
            .filter(|&node| game.actor(node).is_some())
            .count() as u32;
        let hands = [Player::Oop, Player::Ip].map(|player| game.hands(player).len() as u32);
        let counts = (game.num_nodes() as u32, player_nodes, hands);
        if counts != (header.nodes, header.player_nodes, header.hands) {
            return Err(SolutionFileError::Header {
                path,
                field: "tree".to_string(),
                reason: format!(
                    "gives {} nodes, {} player nodes and {} and {} hands, but the game its tree \
                     hash matches has {} nodes, {} player nodes and {} and {} hands",
                    header.nodes,
                    header.player_nodes,
                    header.hands[0],
                    header.hands[1],
                    counts.0,
                    counts.1,
                    counts.2[0],
                    counts.2[1]
                ),
            });
        }

        let loaded_last = last_street.max(header.streets[0]).min(header.last_street());
        let mut strategy = PublicProfile::empty(game.num_nodes());
        let mut evs: Vec<Option<Box<[f32]>>> = vec![None; game.num_nodes()];
        let mut offset = opened.sections_start;
        let sections = header.sections.clone();
        let streets = header.streets.clone();
        let mut sections = sections.iter();
        let (strategy_sizes, ev_sizes) = raw_sizes(&game, &streets);
        for (index, &street) in streets.iter().enumerate() {
            if street > loaded_last {
                break;
            }
            let nodes: Vec<u32> = (0..game.num_nodes() as u32)
                .filter(|&node| game.actor(node).is_some() && game.street(node) == street)
                .collect();
            let strategy_section = sections.next().expect("the header lists every section");
            let expected = strategy_sizes[index];
            let raw = read_section(&mut opened, offset, strategy_section, expected)?;
            offset += strategy_section.length;
            decode_strategy(&game, &nodes, &raw, &mut strategy).map_err(|reason| {
                SolutionFileError::Section {
                    path: opened.path.clone(),
                    name: strategy_section.name.clone(),
                    reason,
                }
            })?;
            drop(raw);
            let ev_section = sections.next().expect("the header lists every section");
            let expected = ev_sizes[index];
            let raw = read_section(&mut opened, offset, ev_section, expected)?;
            offset += ev_section.length;
            decode_evs(&game, &nodes, &raw, &mut evs).map_err(|reason| {
                SolutionFileError::Section {
                    path: opened.path.clone(),
                    name: ev_section.name.clone(),
                    reason,
                }
            })?;
        }
        let header = opened.header;
        let solution = Solution::from_parts(
            header.spot.clone(),
            game,
            strategy,
            loaded_last,
            header.stop,
            header.iterations,
            Duration::ZERO,
            header.exploitability,
            header.solve_exploitability,
        );
        Ok(SavedSolution {
            header,
            solution,
            evs,
        })
    }

    /// The file's header.
    pub fn header(&self) -> &Header {
        &self.header
    }

    /// The solution, whose strategy is the file's, on the game built from the file's spot.
    pub fn solution(&self) -> &Solution {
        &self.solution
    }

    /// The solution, given up by the file it was read from.
    pub fn into_solution(self) -> Solution {
        self.solution
    }

    /// The last street the solution holds: the last the file holds, or an earlier one if it was
    /// loaded with [`SavedSolution::load_streets`].
    pub fn last_street(&self) -> Street {
        self.solution.last_street()
    }

    /// At the player node `node`, the acting player's EV with each of its hands, in chips from
    /// the start of the spot, under the file's strategy, as the file holds them: NaN where it
    /// is undefined. `None` at a node that is not a player node, is out of range, or is on a
    /// street the solution does not hold.
    pub fn ev(&self, node: u32) -> Option<&[f32]> {
        self.evs.get(node as usize)?.as_deref()
    }

    /// The river that starts at the player node `node` as a spot of its own, with what each
    /// player had put in before it, as the [module documentation](self#leaving-out-the-river-and-solving-it-again)
    /// describes.
    ///
    /// Fails if `node` is out of range, is not the first decision of a river (the root of a
    /// river spot, or the child of a chance node that deals the river), if the solution does
    /// not hold the strategy on the way to it, or if no pair of hands, one of each player's,
    /// both reach it.
    pub fn river_spot(&self, node: u32) -> Result<RiverSpot, SolutionFileError> {
        let solution = &self.solution;
        let game = solution.game();
        let spot = solution.spot();
        let refuse = |reason: String| SolutionFileError::NotRiverStart {
            node,
            path: if (node as usize) < game.num_nodes() {
                game.node_path(node)
            } else {
                String::new()
            },
            reason,
        };
        if node as usize >= game.num_nodes() {
            return Err(refuse(format!(
                "is out of range: the tree has {} nodes",
                game.num_nodes()
            )));
        }
        let path = game.path(node);
        let starts_river = game.street(node) == Street::River
            && game.actor(node).is_some()
            && path.last().is_none_or(|&(parent, _)| {
                matches!(game.node(parent as usize), PublicNode::Chance { .. })
            });
        if !starts_river {
            return Err(refuse(
                "is not the first decision of a river: a river spot starts at the node a river \
                 card leads to"
                    .to_string(),
            ));
        }
        if let Some(&(at, _)) = path
            .iter()
            .find(|&&(at, _)| game.actor(at).is_some() && !solution.holds(at))
        {
            return Err(refuse(format!(
                "is below node {at}, on the {}, which the solution does not hold",
                game.street(at)
            )));
        }
        let reach = game.reach(solution.strategy(), node);
        let put_in = put_in_before(game, node);
        let board = game.board(node);
        let ranges = [Player::Oop, Player::Ip].map(|player| {
            let hands = game.hands(player);
            let mut range = Range::empty();
            for ((&combo, &weight), &reach) in hands
                .combos()
                .iter()
                .zip(hands.weights())
                .zip(&reach[player.index()])
            {
                let weight = (weight * reach) as f32;
                if weight > 0.0 && !combo.intersects(board) {
                    range.set(combo, weight.min(1.0));
                }
            }
            range
        });
        let stack = spot.effective_stack - put_in[0];
        let limit = rules::commit_limit(spot.effective_stack, spot.tree.allin_threshold)
            .saturating_sub(put_in[0]);
        let threshold = if limit >= stack {
            1.0
        } else {
            (f64::from(limit) + 0.5) / f64::from(stack)
        };
        debug_assert_eq!(rules::commit_limit(stack, threshold), limit.min(stack));
        let path_text = game.node_path(node);
        let [oop_range, ip_range] = ranges;
        let river = Spot {
            name: format!("{}, river after `{path_text}`", spot.name),
            description: format!(
                "The river of `{}` after `{path_text}`, from its solution file: each range is the \
                 range weight times the player's own reach of that node.",
                spot.name
            ),
            board,
            pot: spot.pot + put_in[0] + put_in[1],
            effective_stack: stack,
            oop_range,
            ip_range,
            rake_percent: spot.rake_percent,
            rake_cap: spot.rake_cap,
            tree: TreeSizing {
                max_raises: spot.tree.max_raises,
                allin_threshold: threshold,
                flop: None,
                turn: None,
                river: spot.tree.river.clone(),
            },
        };
        for (player, range) in [
            (Player::Oop, &river.oop_range),
            (Player::Ip, &river.ip_range),
        ] {
            if range.is_empty() {
                return Err(refuse(format!(
                    "is never reached by {player}: no hand of its range plays to it, so it has \
                     no river spot"
                )));
            }
        }
        river.validate().map_err(|error| match error {
            SpotError::Invalid { field, .. } if field == "ip_range" => refuse(
                "is never reached by a pair of hands that can be dealt together, so it has no \
                 river spot"
                    .to_string(),
            ),
            error => refuse(format!("gives a river spot that is not valid: {error}")),
        })?;
        Ok(RiverSpot {
            spot: river,
            node,
            put_in,
        })
    }
}

/// A river as a spot of its own, from [`SavedSolution::river_spot`].
#[derive(Debug, Clone, PartialEq)]
pub struct RiverSpot {
    /// The river spot, which [`solve`](crate::solve()) solves.
    pub spot: Spot,
    /// The node of the full tree it starts at.
    pub node: u32,
    /// The chips each player had put in before the river, from the start of the full spot,
    /// indexed by [`Player::index`]. The river spot's payoffs, EVs and values count from the
    /// start of the river: less this, they count from the start of the full spot.
    pub put_in: [Chips; 2],
}

/// The chips each player has put in from the root to the start of the street of `node`, which
/// is the first node of its street.
fn put_in_before(game: &PostflopGame, node: u32) -> [Chips; 2] {
    let mut total = [0; 2];
    let mut street = [0; 2];
    for (at, index) in game.path(node) {
        match game.actor(at) {
            Some(actor) => {
                let (me, them) = (actor.index(), actor.opponent().index());
                match game.actions(at)[index] {
                    Action::Fold | Action::Check => {}
                    Action::Call => street[me] = street[them],
                    Action::Bet { to, .. } | Action::Raise { to, .. } | Action::AllIn { to } => {
                        street[me] = to;
                    }
                }
            }
            None => {
                for player in 0..2 {
                    total[player] += street[player];
                }
                street = [0; 2];
            }
        }
    }
    [total[0] + street[0], total[1] + street[1]]
}

/// Reads the section at `offset` described by `section`, checks that it is one zstd frame of
/// `expected` bytes with its checksum, and decompresses it.
fn read_section(
    opened: &mut Opened,
    offset: u64,
    section: &SectionInfo,
    expected: u64,
) -> Result<Vec<u8>, SolutionFileError> {
    let path = opened.path.clone();
    let corrupt = |reason: String| SolutionFileError::Section {
        path: path.clone(),
        name: section.name.clone(),
        reason,
    };
    if section.raw_length != expected {
        return Err(corrupt(format!(
            "holds {} bytes by the header, but the tree needs {expected}",
            section.raw_length
        )));
    }
    // The length was checked against the file's when the header was read.
    let mut bytes = Vec::with_capacity(section.length as usize);
    opened
        .file
        .seek(SeekFrom::Start(offset))
        .map_err(io_error("read", &path))?;
    (&mut opened.file)
        .take(section.length)
        .read_to_end(&mut bytes)
        .map_err(io_error("read", &path))?;
    if bytes.len() as u64 != section.length {
        return Err(SolutionFileError::Truncated {
            path: path.clone(),
            place: format!("in section `{}`", section.name),
        });
    }
    if !bytes.starts_with(&ZSTD_MAGIC) {
        return Err(corrupt(
            "does not start with a zstd frame's magic number".to_string(),
        ));
    }
    let frame = zstd::zstd_safe::find_frame_compressed_size(&bytes)
        .map_err(|code| corrupt(format!("is not a zstd frame: {}", zstd_error(code))))?;
    if frame != bytes.len() {
        return Err(corrupt(format!(
            "is a zstd frame of {frame} bytes followed by {} more; a section is one frame",
            bytes.len() - frame
        )));
    }
    // The frame header descriptor follows the 4-byte magic; bit 2 is the checksum flag.
    if bytes[4] & 0b100 == 0 {
        return Err(corrupt("is a zstd frame without its checksum".to_string()));
    }
    match zstd::zstd_safe::get_frame_content_size(&bytes) {
        Ok(Some(size)) if size == expected => {}
        Ok(Some(size)) => {
            return Err(corrupt(format!(
                "decompresses to {size} bytes by its frame, but the tree needs {expected}"
            )));
        }
        _ => {
            return Err(corrupt(
                "is a zstd frame that does not record its size".to_string(),
            ));
        }
    }
    let mut raw = Vec::new();
    raw.try_reserve_exact(expected as usize).map_err(|_| {
        corrupt(format!(
            "needs {expected} bytes, which could not be allocated"
        ))
    })?;
    let mut decompressor = zstd::bulk::Decompressor::new().map_err(io_error("read", &path))?;
    decompressor
        .decompress_to_buffer(&bytes, &mut raw)
        .map_err(|error| corrupt(format!("is corrupt: {error}")))?;
    if raw.len() as u64 != expected {
        return Err(corrupt(format!(
            "decompresses to {} bytes, but the tree needs {expected}",
            raw.len()
        )));
    }
    Ok(raw)
}

fn zstd_error(code: usize) -> &'static str {
    zstd::zstd_safe::get_error_name(code)
}

/// Fills `strategy` at `nodes` from the raw codes of a strategy section.
fn decode_strategy(
    game: &PostflopGame,
    nodes: &[u32],
    raw: &[u8],
    strategy: &mut PublicProfile,
) -> Result<(), String> {
    let mut codes = raw
        .as_chunks::<2>()
        .0
        .iter()
        .map(|&pair| u16::from_le_bytes(pair));
    for &node in nodes {
        let actor = game.actor(node).expect("a player node");
        let num_hands = game.hands(actor).len();
        let num_actions = game.children(node).len();
        let node_codes: Vec<u16> = codes.by_ref().take(num_actions * num_hands).collect();
        for hand in 0..num_hands {
            let sum: u32 = (0..num_actions)
                .map(|action| u32::from(node_codes[action * num_hands + hand]))
                .sum();
            if sum != u32::from(CODE_TOTAL) {
                return Err(format!(
                    "is corrupt: the codes of {} at node {node} (`{}`) sum to {sum}, not 65535",
                    game.hands(actor).combos()[hand],
                    game.node_path(node)
                ));
            }
        }
        let decoded = node_codes.iter().map(|&code| dequantize(code)).collect();
        strategy.set(node as usize, num_hands, decoded);
    }
    Ok(())
}

/// Fills `evs` at `nodes` from the raw EVs of an EV section.
fn decode_evs(
    game: &PostflopGame,
    nodes: &[u32],
    raw: &[u8],
    evs: &mut [Option<Box<[f32]>>],
) -> Result<(), String> {
    let mut values = raw
        .as_chunks::<4>()
        .0
        .iter()
        .map(|&bytes| f32::from_le_bytes(bytes));
    for &node in nodes {
        let actor = game.actor(node).expect("a player node");
        let num_hands = game.hands(actor).len();
        let node_evs: Box<[f32]> = values.by_ref().take(num_hands).collect();
        if let Some(hand) = node_evs.iter().position(|ev| ev.is_infinite()) {
            return Err(format!(
                "is corrupt: the EV of {} at node {node} (`{}`) is infinite",
                game.hands(actor).combos()[hand],
                game.node_path(node)
            ));
        }
        evs[node as usize] = Some(node_evs);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
