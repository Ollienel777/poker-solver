//! The memory a solve needs, estimated from the spot before anything is allocated.
//!
//! [`estimate_memory`] counts the public tree from the betting tree, as the game's node limit
//! does, and the hands each player holds on every board the tree reaches, without building the
//! game. From those counts it adds up what [`solve`](crate::solve()) allocates, item by item,
//! as the code stands: the overheads that a tidier implementation would not have are counted,
//! not left out. [`solve`](crate::solve()) refuses a spot whose [`MemoryEstimate::total`] is over
//! [`SolveOptions::max_memory`].
//!
//! # What is counted
//!
//! Every figure is in bytes, from the sizes of the types the solver holds and the lengths and
//! capacities of its vectors, on the machine the code is built for.
//!
//! - **Regrets and strategy sums**: one `f32` of each per action and hand of the acting player
//!   at every player node of the tree the solver iterates on: the public tree, or the merged
//!   tree when the solve merges (see [A merged solve](#a-merged-solve)). The hands are all of the
//!   player's hands on the spot's board, on every street (DECISIONS P25). With
//!   [`SolveOptions::compressed_storage`], an `i16` and a `u16` in place of the two `f32`s, half
//!   the bytes.
//! - **Solver bookkeeping**: `pk_cfr::VectorCfr`'s index from every node of that tree to its
//!   row, 16 bytes a node, the row headers, and a `bool` per hand at each player node saying
//!   whether the hand is possible there. A row header is 80 bytes, and 88 with compressed
//!   storage, which keeps the two scales in it.
//! - **Public tree**: a record for every node, and the children and cards of each player and
//!   chance node, each its own allocation.
//! - **Hands on each board**: a `BoardHands` for every board the tree reaches, with each
//!   player's hands, their weights, the cross index, the ranking on a river board and the index
//!   back to the spot's lists, and for each player a list of the spot's hands that hold each
//!   card, the hands a chance node dealing it rules out.
//! - **Betting tree, payoffs and deal**: the betting tree, a payoff per betting node, and the
//!   deal vectors.
//! - **Average strategy**: the `PublicProfile` each exploitability measurement builds, with an
//!   entry for every public node and an `f32` per action and hand at each player node, on the
//!   full tree under either storage. The solve keeps the last measurement's profile while it
//!   iterates, and drops it before it makes the next, so it holds one at a time
//!   ([what is held](mod@crate::solve#what-is-held)).
//! - **Best response**: a best-response walk's working memory and its values at the root, for
//!   the larger of the two players. A measurement's best responses keep no strategy.
//! - **Traversal buffers**: a walk's working memory, as `pk_cfr::vector` describes it: a frame
//!   of vectors for each depth of the tree, each vector as large as the most a node at that
//!   depth needs, and the vectors kept once, the game's room at a terminal among them. The CFR
//!   walk's serves both players, and the solver keeps it from its first iteration to the end of
//!   the solve; each exploitability walk makes its own and drops it. They are small, since a
//!   frame holds a few vectors of the hands. On more than one thread, each walk also has a
//!   workspace for each outcome of the chance nodes it splits, and the frame of a split chance
//!   node holds the values of all its outcomes (see [On several threads](#on-several-threads)).
//! - **Thread pool**: on more than one thread, the solve's rayon pool, [`POOL_BYTES`] and
//!   [`POOL_THREAD_BYTES`] a thread.
//! - **Invariant check**: in a build with debug assertions, the check of the solution that the
//!   solve makes before it returns ([`crate::invariants`]). Zero in a release build.
//!
//! The [`Solution`](crate::Solution) holds the game itself, moved into it once the solver is
//! dropped, and the last profile, so it adds nothing to these.
//!
//! # A merged solve
//!
//! When the solve iterates on the [merged game](crate::merged), the estimate counts the merged
//! tree from the betting tree and the classes of cards at each chance node, which
//! [`NextCards`](pk_core::NextCards) gives for the spot's board and for each turn's
//! representative, without building either game. It counts:
//!
//! - the solver's rows, bookkeeping and CFR walk on the merged tree, whose split chance nodes
//!   have one outcome per class;
//! - **Merged game**: its nodes, its chance nodes' representatives, children and images, the
//!   counterpart of every node of the full tree, each player's hands relabelled by each member
//!   of `Γ`, and the stabilizers of its nodes with the classes of alike hands of each, held from
//!   the solver's start to the end of the solve, beside the full game;
//! - everything measured on the full tree as before: the profiles, the best response and the
//!   measurement walks, and the room in which the expansion of the average strategy onto the
//!   full tree holds one node's strategy.
//!
//! The boards and the public tree are the full game's, which the solve holds as it does without
//! merging.
//!
//! # When the most is held
//!
//! The solve passes through four phases, and the estimate is the largest of them:
//!
//! - **Building**: the game, then the solver's rows, with the transient copies their growth
//!   makes and the solver's check of the tree.
//! - **Iterating**: the game, the solver with its walk's working memory, the thread pool on
//!   several threads, and the profile kept from the last measurement, when the solve can
//!   iterate after one, which is whenever `max_iterations` is more than `check_every`.
//! - **Measuring**: the same with the new profile in place of the old one, and a best
//!   response's or an exploitability walk's working memory.
//! - **Finishing**: once the solver is dropped, the game, the profile and the thread pool, with
//!   the invariant check in a build with debug assertions.
//!
//! On the test and benchmark spots the measurement holds the most, in a test build as in a
//! release one.
//!
//! # Past the bytes the solver asks for
//!
//! [`MemoryEstimate::heap_bytes`] is what the solve asks the allocator for at its peak, which a
//! counting allocator measures, and which `tests/memory_estimate.rs` checks against one. It is
//! the largest phase plus a [`margin`](MemoryEstimate::margin) of 1/256 and 4 KiB for the
//! allocations too small to follow one by one. Without the margin, the largest phase comes
//! within 0.1% of the measured peak on every test spot, and it is never below. A process holds
//! more than the heap: each allocation carries the allocator's header and rounding, and the
//! process has its code and stacks. [`MemoryEstimate::allocator_overhead`] allows 32 bytes per
//! live allocation plus 1/16 of the bytes, and [`MemoryEstimate::process_bytes`] a fixed
//! [`PROCESS_BYTES`]. [`MemoryEstimate::total`] adds the three, and is what the solve
//! compares with its limit. Both allowances are on the high side, because the estimate's job is
//! to refuse a solve that would not fit.
//!
//! # On several threads
//!
//! A solve on more than one thread of a spot with a chance node ([`MemoryEstimate::threads`])
//! splits the first chance node on each line, as `pk_cfr`'s parallel walk does, and the
//! estimate counts what that holds, which is the same whatever the thread count above one:
//!
//! - each walk's own frames down to the split chance nodes, the frame of a split chance node
//!   holding `outcomes × hands` values;
//! - a workspace for each outcome of the largest chance node split, 49 on a flop spot and 48 on
//!   a turn spot, holding a frame for its child's inputs and one for each depth below, each
//!   buffer as large as the most the child of that index needs at that depth under any chance
//!   node split, with its room at a terminal; the CFR walk's are kept from the first iteration
//!   to the end of the solve, and an exploitability walk's while it runs;
//! - the solve's pool, which is the one figure that grows with the thread count.
//!
//! The walks' working memory is then a function of the tree, as on one thread. What the solve
//! holds at its peak still depends on how the pool's threads shared out the jobs: how much the
//! pool's threads have allocated for themselves, which the allowance takes at its most, and
//! whether the vectors of frames of several children's workspaces grow at once near the end of
//! an exploitability walk, which the estimate counts for every child. So the estimate is never
//! below the peak, and can be above it by those two, a few kilobytes a thread and a few hundred
//! bytes a child, on top of what it is above on one thread. The threads of an earlier solve's
//! pool can also still be freeing what they held while the next solve runs, which makes the
//! next solve look smaller. `tests/memory_estimate.rs` holds the estimate on several threads to
//! at most 5%, 16 KiB a thread and 64 KiB above the measured peak.
//!
//! # What it depends on
//!
//! The figures follow the solver's code, not a model of a solver, so a change to what the solver
//! allocates must change this module too. Seven layouts are private to `pk_cfr`, the solver's
//! row under each storage, a profile's entry, and the frames and the workspaces of the two kinds
//! of walk, and are mirrored here by structs with the same fields. A `Vec` filled by pushes is taken to grow as the standard
//! library grows it, doubling from four elements, and a walk's buffer to grow to exactly what
//! it is asked to hold. The test against a counting allocator catches a change to any of these.

use std::fmt;
use std::mem::size_of;
use std::ops::{Add, AddAssign, Mul};

use pk_core::{CardSet, Combo, DealtBoard, HandRank, NUM_CARDS, SuitGroup, SuitPermutation};
use pk_game::{Action, Node, Player, PostflopTree, Street};
use serde::Serialize;

use crate::expected_rake::TerminalOutcome;
use crate::game::{BOARD_BYTES, PUBLIC_NODE_BYTES};
use crate::merged::{
    COUNTERPART_BYTES, IMAGE_BYTES, MERGED_CHANCE_BYTES, MERGED_NODE_BYTES, STABILIZER_BYTES,
    merging_group,
};
use crate::payoff::Payoff;
use crate::solve::{SolveError, SolveOptions};
use crate::spot::Spot;

/// What [`MemoryEstimate::process_bytes`] allows for the process apart from the solve: its code,
/// its stacks and the standard library's buffers. 16 MiB, about three times what the example
/// program holds before it solves.
pub const PROCESS_BYTES: u64 = 16 << 20;

/// What [`MemoryEstimate::thread_pool`] allows for a solve's rayon pool apart from its threads:
/// 12 KiB, for its registry, its queue of jobs and the lock a thread outside it waits on.
pub const POOL_BYTES: u64 = 12 << 10;

/// What [`MemoryEstimate::thread_pool`] allows for each thread of a solve's rayon pool: 6 KiB,
/// for its queue of jobs, its sleep state, the epoch record its first steal registers and what
/// the standard library allocates to start it. Measured with a counting allocator, a pool held
/// about 4.9 kB a thread, and 9 kB more at most, once its threads had run jobs.
pub const POOL_THREAD_BYTES: u64 = 6 << 10;

/// The layout of the row `pk_cfr::VectorCfr` keeps for each player node under `f32` storage,
/// which is private to `pk_cfr`: the acting player, whether each hand is possible, the regrets
/// and the strategy sums.
#[allow(dead_code)]
struct NodeRowLayout {
    player: usize,
    possible: Vec<bool>,
    regrets: Vec<f32>,
    strategy_sum: Vec<f32>,
}

/// The layout of the row under compressed storage: the same with the codes and their scales.
#[allow(dead_code)]
struct CompressedNodeRowLayout {
    player: usize,
    possible: Vec<bool>,
    regrets: Vec<i16>,
    regret_scale: f32,
    strategy_sum: Vec<u16>,
    sum_scale: f32,
}

/// The layout of a node's entry in a `pk_cfr::PublicProfile`, which is private to `pk_cfr`.
#[allow(dead_code)]
struct NodeStrategyLayout {
    num_hands: usize,
    probabilities: Vec<f32>,
}

/// The layout of a frame of `pk_cfr::VectorCfr`'s walk, which is private to `pk_cfr`: the
/// strategy, the children's values, both reaches and the possible hands.
#[allow(dead_code)]
struct CfrFrameLayout {
    strategy: Vec<f32>,
    values: Vec<f64>,
    own_reach: Vec<f64>,
    opponent_reach: Vec<f64>,
    possible: Vec<bool>,
}

/// The layout of a frame of `pk_cfr::vector_best_response`'s walks, which is private to
/// `pk_cfr`: a child's values, the opponent's reach, the possible hands and the choices.
#[allow(dead_code)]
struct ResponseFrameLayout {
    values: Vec<f64>,
    opponent_reach: Vec<f64>,
    possible: Vec<bool>,
    choices: Vec<usize>,
}

/// The layout of the workspace of `pk_cfr::VectorCfr`'s walk, which is private to `pk_cfr`: the
/// frames, the instantaneous regrets, the reach as `f32` and the room at a terminal.
#[allow(dead_code)]
struct CfrWorkspaceLayout {
    frames: Vec<CfrFrameLayout>,
    regrets: Vec<f32>,
    reach: Vec<f32>,
    terminal: Vec<f64>,
}

/// The layout of the workspace of `pk_cfr::vector_best_response`'s walks, which is private to
/// `pk_cfr`: the frames and the room at a terminal.
#[allow(dead_code)]
struct ResponseWorkspaceLayout {
    frames: Vec<ResponseFrameLayout>,
    terminal: Vec<f64>,
}

/// The buffers of a walk's frame, in the order of [`FrameNeeds`], with the size of an entry
/// of each: the strategy (`f32`), the children's values, the traverser's reach and the
/// opponent's reach (`f64`), the possible hands (`bool`) and a best response's choices
/// (`usize`).
const FRAME_ENTRY_BYTES: [u64; 6] = [4, 8, 8, 8, 1, size_of::<usize>() as u64];

/// The most entries each buffer of one frame holds, in the order of [`FRAME_ENTRY_BYTES`].
type FrameNeeds = [u64; 6];

const ROW_BYTES: u64 = size_of::<NodeRowLayout>() as u64;
const COMPRESSED_ROW_BYTES: u64 = size_of::<CompressedNodeRowLayout>() as u64;
const CFR_FRAME_BYTES: u64 = size_of::<CfrFrameLayout>() as u64;
const RESPONSE_FRAME_BYTES: u64 = size_of::<ResponseFrameLayout>() as u64;
const CFR_WORKSPACE_BYTES: u64 = size_of::<CfrWorkspaceLayout>() as u64;
const RESPONSE_WORKSPACE_BYTES: u64 = size_of::<ResponseWorkspaceLayout>() as u64;
/// A frame of the check of the tree's shape: both players' possible hands.
const CHECK_FRAME_BYTES: u64 = size_of::<[Vec<bool>; 2]>() as u64;
const ROW_INDEX_BYTES: u64 = size_of::<Option<usize>>() as u64;
const PROFILE_ENTRY_BYTES: u64 = size_of::<Option<NodeStrategyLayout>>() as u64;
const PUBLIC_NODE: u64 = PUBLIC_NODE_BYTES as u64;
const BOARD: u64 = BOARD_BYTES as u64;
const BETTING_NODE: u64 = size_of::<Node>() as u64;
const ACTION: u64 = size_of::<Action>() as u64;
const PAYOFF: u64 = size_of::<Option<Payoff>>() as u64;
const COMBO: u64 = size_of::<Combo>() as u64;
const SAME: u64 = size_of::<Option<u32>>() as u64;
const RANKED: u64 = size_of::<(HandRank, u32)>() as u64;
/// An entry of the invariant check's stack: a node and both players' reaches.
const REACH_ENTRY: u64 = size_of::<(u32, [Vec<f64>; 2])>() as u64;
const TERMINAL: u64 = size_of::<TerminalOutcome>() as u64;
const MERGED_NODE: u64 = MERGED_NODE_BYTES as u64;
const MERGED_CHANCE: u64 = MERGED_CHANCE_BYTES as u64;
const IMAGE: u64 = IMAGE_BYTES as u64;
const COUNTERPART: u64 = COUNTERPART_BYTES as u64;
const STABILIZER: u64 = STABILIZER_BYTES as u64;
const MEMBER: u64 = size_of::<SuitPermutation>() as u64;
/// The vector of one player's hands relabelled by one member of `Γ`.
const RELABELLED: u64 = size_of::<Vec<u32>>() as u64;

/// The phase of a solve that holds the most memory: see the
/// [module documentation](self#when-the-most-is-held).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Building,
    Iterating,
    Measuring,
    Finishing,
}

impl fmt::Display for Phase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Phase::Building => "while building the game and the solver",
            Phase::Iterating => "while iterating",
            Phase::Measuring => "while measuring exploitability",
            Phase::Finishing => "while finishing",
        })
    }
}

/// What a solve of a spot will need, as the [module documentation](self) describes. Every size
/// is in bytes.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MemoryEstimate {
    /// Each player's hands on the spot's board, indexed by [`Player::index`].
    pub hands: [u64; 2],
    pub betting_nodes: u64,
    pub public_nodes: u64,
    pub player_nodes: u64,
    pub chance_nodes: u64,
    pub terminal_nodes: u64,
    /// The boards the public tree reaches, the spot's included.
    pub boards: u64,
    /// Whether the solve iterates on the [merged game](crate::merged).
    pub merged: bool,
    /// The nodes of the merged tree, and its player nodes: 0 when the solve does not merge.
    pub merged_nodes: u64,
    pub merged_player_nodes: u64,
    /// Whether the solver stores its regrets and strategy sums in 16 bits
    /// ([`SolveOptions::compressed_storage`]) rather than as `f32`.
    pub compressed: bool,
    /// The values in each of the solver's two arrays: actions times the acting player's hands,
    /// summed over the player nodes of the tree the solver iterates on, the merged one when the
    /// solve merges.
    pub stored_values: u64,

    /// The two arrays: 8 bytes a stored value as `f32`, 4 compressed.
    pub regrets_and_sums: u64,
    /// The solver's index of rows, its row headers and its possible-hand flags.
    pub solver_bookkeeping: u64,
    pub public_tree: u64,
    /// The `BoardHands` of every board, the vector that holds them, and the lists of the
    /// spot's hands that hold each card.
    pub board_hands: u64,
    /// The betting tree, the payoffs and the deal.
    pub betting_tree: u64,
    /// The merged game, held beside the full one: 0 when the solve does not merge.
    pub merged_game: u64,
    /// One average-strategy profile.
    pub average_strategy: u64,
    /// The most average-strategy profiles the solve holds at once: 1.
    pub profiles_held: u64,
    /// The larger player's best-response walk: its buffers and its values at the root.
    pub best_response: u64,
    /// The larger of a CFR walk's buffers and an exploitability walk's: their working memory,
    /// which for the CFR walk the solver keeps from its first iteration on, with, on more than
    /// one thread, the workspaces of the children of the chance nodes they split.
    pub traversal: u64,
    /// The threads the walks run on: [`SolveOptions::threads`] on a spot with a chance node,
    /// and 1 on a river spot, which has none to split.
    pub threads: u64,
    /// The solve's rayon pool, on more than one thread: [`POOL_BYTES`] and [`POOL_THREAD_BYTES`]
    /// a thread.
    pub thread_pool: u64,
    /// The invariant check of a build with debug assertions; 0 in a release build.
    pub invariant_check: u64,

    /// The heap the solve holds at most in each phase.
    pub building: u64,
    pub iterating: u64,
    pub measuring: u64,
    pub finishing: u64,
    /// The phase that holds the most.
    pub peak_phase: Phase,
    /// A margin for the allocations too small to follow one by one: 1/256 of the largest
    /// phase and 4 KiB.
    pub margin: u64,
    /// What the solve asks the allocator for at its peak: the largest phase and the margin.
    pub heap_bytes: u64,
    /// The allowance for the allocator's headers and rounding at the peak.
    pub allocator_overhead: u64,
    /// The allowance for the process itself, [`PROCESS_BYTES`].
    pub process_bytes: u64,
    /// `heap_bytes + allocator_overhead + process_bytes`: the estimate, which the solve
    /// compares with [`SolveOptions::max_memory`].
    pub total: u64,
}

/// Bytes and the allocations that hold them, added up together.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct Heap {
    bytes: u64,
    allocations: u64,
}

impl Heap {
    /// One allocation of `bytes`, or none if `bytes` is 0, as for an empty `Vec`.
    fn one(bytes: u64) -> Heap {
        Heap {
            bytes,
            allocations: u64::from(bytes > 0),
        }
    }

    /// `bytes` in `allocations` allocations.
    fn spread(bytes: u64, allocations: u64) -> Heap {
        Heap { bytes, allocations }
    }

    /// The larger by bytes.
    fn max(self, other: Heap) -> Heap {
        if other.bytes > self.bytes {
            other
        } else {
            self
        }
    }

    /// The allowance for the allocator's headers and rounding: 32 bytes an allocation, and 1/16
    /// of the bytes.
    fn overhead(self) -> u64 {
        32 * self.allocations + self.bytes / 16
    }
}

impl Add for Heap {
    type Output = Heap;
    fn add(self, other: Heap) -> Heap {
        Heap {
            bytes: self.bytes + other.bytes,
            allocations: self.allocations + other.allocations,
        }
    }
}

impl AddAssign for Heap {
    fn add_assign(&mut self, other: Heap) {
        *self = *self + other;
    }
}

impl Mul<u64> for Heap {
    type Output = Heap;
    fn mul(self, count: u64) -> Heap {
        Heap {
            bytes: self.bytes * count,
            allocations: self.allocations * count,
        }
    }
}

/// The capacity a `Vec` of `len` elements of `size` bytes ends with when it grows by pushes
/// from empty: the standard library starts at 4 elements (8 of one byte, 1 over 1 KiB) and
/// doubles.
fn grown(len: u64, size: u64) -> u64 {
    if len == 0 {
        return 0;
    }
    let least = match size {
        1 => 8,
        2..=1024 => 4,
        _ => 1,
    };
    len.next_power_of_two().max(least)
}

/// The number of outcomes of a chance node that deals `street`: the cards off its board.
fn outcomes(street: Street) -> u64 {
    match street {
        Street::Turn => 49,
        Street::River => 48,
        Street::Flop => unreachable!("no chance node deals the flop"),
    }
}

/// One player's hands on the spot's board, as the counts on the later boards need them.
struct PlayerHands {
    count: u64,
    /// For each card, how many of the hands hold it.
    holding: [u64; NUM_CARDS],
    /// For each pair of cards, whether one of the hands is that combo.
    holds: Vec<bool>,
}

impl PlayerHands {
    fn new(combos: &[Combo]) -> PlayerHands {
        let mut holding = [0u64; NUM_CARDS];
        let mut holds = vec![false; NUM_CARDS * NUM_CARDS];
        for combo in combos {
            let (lo, hi) = (combo.lo().index(), combo.hi().index());
            holding[lo] += 1;
            holding[hi] += 1;
            holds[lo * NUM_CARDS + hi] = true;
            holds[hi * NUM_CARDS + lo] = true;
        }
        PlayerHands {
            count: combos.len() as u64,
            holding,
            holds,
        }
    }

    /// The hands that hold none of `cards`, one or two cards off the spot's board.
    fn missing(&self, cards: &[usize]) -> u64 {
        let held: u64 = cards.iter().map(|&card| self.holding[card]).sum();
        let both = match *cards {
            [a, b] => u64::from(self.holds[a * NUM_CARDS + b]),
            _ => 0,
        };
        self.count - held + both
    }
}

/// One player's lists on a board where it has `n` hands, filled by pushes, as `HandList::new`
/// fills them. A river board ranks its hands, and a board other than the spot's indexes them
/// back to the spot's lists.
fn board_lists(n: u64, river: bool, later: bool) -> Heap {
    let slots = grown(n, 8);
    let mut heap = Heap::one(slots * COMBO) + Heap::one(slots * 8) + Heap::one(n * SAME);
    if river {
        heap += Heap::one(n * RANKED);
    }
    if later {
        heap += Heap::one(n * 4);
    }
    heap
}

/// Counts over the public tree, from the betting tree.
#[derive(Default)]
struct Census {
    public_nodes: u64,
    player_nodes: u64,
    chance_nodes: u64,
    terminal_nodes: u64,
    /// Σ actions over the public player nodes.
    actions: u64,
    /// Σ actions × the actor's hands over each player's public player nodes.
    stored: [u64; 2],
    /// Σ the actor's hands over the public player nodes.
    flags: u64,
    /// Σ outcomes over the public chance nodes.
    outcomes: u64,
    turn_chance: bool,
    river_chance: bool,
    /// Whether there is a terminal on the turn, and on the river.
    turn_terminals: bool,
    river_terminals: bool,
    /// The most nodes other than terminals on a line from the root.
    depth: u64,
    /// The most reaches waiting at once on the invariant check's stack.
    waiting: u64,
}

/// The public tree's counts, with each betting node standing for as many public nodes as the
/// chance nodes above it deal cards.
fn census(tree: &PostflopTree, hands: [u64; 2]) -> Census {
    let nodes = tree.nodes();
    let mut census = Census::default();
    // For each betting node: its public copies, the nodes other than terminals above it, and
    // the reaches waiting on the invariant check's stack when it is reached.
    let mut copies = vec![0u64; nodes.len()];
    let mut depth = vec![0u64; nodes.len()];
    let mut waiting = vec![0u64; nodes.len()];
    copies[0] = 1;
    // Parents come before their children in pre-order.
    for (index, node) in nodes.iter().enumerate() {
        let m = copies[index];
        census.public_nodes += m;
        let (fanout, below) = match node {
            Node::Player {
                actor, children, ..
            } => {
                let (player, a) = (actor.index(), children.len() as u64);
                census.player_nodes += m;
                census.actions += m * a;
                census.stored[player] += m * a * hands[player];
                census.flags += m * hands[player];
                (a, m)
            }
            Node::Chance { street, .. } => {
                let k = outcomes(*street);
                census.chance_nodes += m;
                census.outcomes += m * k;
                match street {
                    Street::Turn => census.turn_chance = true,
                    Street::River => census.river_chance = true,
                    Street::Flop => {}
                }
                (k, m * k)
            }
            Node::Terminal { .. } => {
                census.terminal_nodes += m;
                match node.street() {
                    Street::Turn => census.turn_terminals = true,
                    Street::River => census.river_terminals = true,
                    Street::Flop => {}
                }
                (0, 0)
            }
        };
        if fanout > 0 {
            census.depth = census.depth.max(depth[index] + 1);
            census.waiting = census.waiting.max(waiting[index] + fanout);
        }
        // The check pushes a node's children in order and takes the last first, so the
        // children before one wait while it is walked. A chance node's copies of its child are
        // all alike, and the most wait below the last.
        for (slot, &child) in node.children().iter().enumerate() {
            let child = child as usize;
            let before = match node {
                Node::Chance { .. } => fanout - 1,
                _ => slot as u64,
            };
            copies[child] = below;
            depth[child] = depth[index] + 1;
            waiting[child] = waiting[index] + before;
        }
    }
    census
}

/// Counts over the merged tree, from the betting tree and the classes of cards at each chance
/// node.
#[derive(Default)]
struct MergedCensus {
    public_nodes: u64,
    player_nodes: u64,
    chance_nodes: u64,
    /// Σ actions over the merged player nodes.
    actions: u64,
    /// Σ actions × the actor's hands over each player's merged player nodes.
    stored: [u64; 2],
    /// Σ the actor's hands over the merged player nodes.
    flags: u64,
    /// Σ outcomes, the classes, over the merged chance nodes.
    outcomes: u64,
    /// Σ images over the merged chance nodes, and the chance nodes that have any.
    images: u64,
    with_images: u64,
    /// By street: the outcomes of a merged chance node that deals the spot's next card, the
    /// node a walk splits.
    split_outcomes: [u64; 3],
    /// The stabilizers of the merged nodes, each once.
    stabilizers: Vec<SuitGroup>,
}

/// The merged tree's counts. Each betting node stands for as many merged nodes as there are
/// classes of the cards dealt on its line: on a flop spot one on the flop, the turn's classes on
/// the turn, and on the river the river's classes after each turn's representative, summed.
fn merged_census(
    tree: &PostflopTree,
    hands: [u64; 2],
    board: CardSet,
    group: SuitGroup,
) -> MergedCensus {
    let start = DealtBoard::new(board).expect("a spot's board has 3 to 5 cards");
    let index = |street: Street| street as usize;
    // By street: the merged copies of a betting node on it, and the copies of a chance node
    // dealing it that have images.
    let mut copies = [0u64; 3];
    let mut with_images = [0u64; 3];
    let mut split_outcomes = [0u64; 3];
    // The stabilizer of each board the merged tree reaches: the spot's, each turn's
    // representative, and on a flop spot whose tree reaches the river each river's after each.
    let mut stabilizers: Vec<SuitGroup> = Vec::new();
    let mut stabilizer_of = |board: DealtBoard| {
        let stabilizer = board.symmetries().intersection(group);
        if !stabilizers.contains(&stabilizer) {
            stabilizers.push(stabilizer);
        }
    };
    stabilizer_of(start);
    let reaches_river = tree.nodes().iter().any(|node| {
        matches!(
            node,
            Node::Chance {
                street: Street::River,
                ..
            }
        )
    });
    let first = index(tree.spec().street);
    copies[first] = 1;
    if first < 2 {
        let next = start.next_cards_within(group);
        let classes = next.classes().len() as u64;
        copies[first + 1] = classes;
        split_outcomes[first + 1] = classes;
        with_images[first + 1] = u64::from(classes < next.live_cards().len() as u64);
        for class in next.classes() {
            let dealt = start
                .deal(class.representative())
                .expect("a representative is off the board");
            stabilizer_of(dealt);
            if first == 0 {
                let rivers = dealt.next_cards_within(group);
                let classes = rivers.classes().len() as u64;
                copies[2] += classes;
                with_images[2] += u64::from(classes < rivers.live_cards().len() as u64);
                if reaches_river {
                    for river in rivers.classes() {
                        let dealt = dealt
                            .deal(river.representative())
                            .expect("a representative is off the board");
                        stabilizer_of(dealt);
                    }
                }
            }
        }
    }
    let mut census = MergedCensus {
        split_outcomes,
        stabilizers,
        ..MergedCensus::default()
    };
    for node in tree.nodes() {
        match node {
            Node::Player {
                street,
                actor,
                children,
                ..
            } => {
                let (m, p, a) = (copies[index(*street)], actor.index(), children.len() as u64);
                census.public_nodes += m;
                census.player_nodes += m;
                census.actions += m * a;
                census.stored[p] += m * a * hands[p];
                census.flags += m * hands[p];
            }
            Node::Chance { street, .. } => {
                let dealt = index(*street);
                let m = copies[dealt - 1];
                census.public_nodes += m;
                census.chance_nodes += m;
                census.outcomes += copies[dealt];
                census.images += m * outcomes(*street) - copies[dealt];
                census.with_images += with_images[dealt];
            }
            Node::Terminal { .. } => census.public_nodes += copies[index(node.street())],
        }
    }
    census
}

/// Which walk a workspace is for.
#[derive(Clone, Copy, PartialEq)]
enum Walk {
    /// `VectorCfr::iterate`'s walk, whose workspace serves both traversers.
    Cfr,
    /// `vector_expected_value`'s walk for a player.
    Follow(usize),
    /// `vector_best_response`'s walk for a responder.
    Respond(usize),
}

/// The most each buffer of one workspace's frames has held, frame by frame, and the most the
/// CFR walk's instantaneous regrets and reach as `f32`, which it keeps once, have held.
#[derive(Default)]
struct Needs {
    frames: Vec<FrameNeeds>,
    regrets: u64,
    reach: u64,
}

impl Needs {
    /// Frame `frame` needs at least `needs`.
    fn add(&mut self, frame: usize, needs: FrameNeeds) {
        if self.frames.len() <= frame {
            self.frames.resize(frame + 1, [0; 6]);
        }
        for (most, need) in self.frames[frame].iter_mut().zip(needs) {
            *most = (*most).max(need);
        }
    }

    /// The workspace's heap: the frames' buffers, the vector of the frames, of `frame_bytes`
    /// each, and the buffers kept once, the game's room at a terminal, `terminal` entries,
    /// among them. The vector of frames grows by pushes as nodes give their frames back, the
    /// last as the walk's first node does. With `ends_growing`, the walk holds the most it ever
    /// does then, as an exploitability walk does, so it holds the old block and the new one at
    /// once if that push is one that grows it.
    fn heap(&self, frame_bytes: u64, terminal: u64, ends_growing: bool) -> Heap {
        let slots = grown(self.frames.len() as u64, frame_bytes);
        let mut heap = Heap::one(slots * frame_bytes)
            + Heap::one(4 * self.regrets)
            + Heap::one(4 * self.reach)
            + Heap::one(8 * terminal);
        if ends_growing {
            heap += Heap::one(slots / 2 * frame_bytes);
        }
        for needs in &self.frames {
            for (&entries, bytes) in needs.iter().zip(FRAME_ENTRY_BYTES) {
                heap += Heap::one(entries * bytes);
            }
        }
        heap
    }
}

/// The working memory a walk holds once it has been over the whole tree, as `pk_cfr` keeps it:
/// a frame for each depth of the tree, each of its buffers as large as the most a node at that
/// depth has needed, the vector of the frames, and the buffers kept once, among them the
/// game's room at a terminal, `terminal` entries. The CFR walk holds one frame more, for its
/// inputs at the root. Every public copy of a betting node needs the same, with at most the
/// spot's hands.
///
/// With `split`, on several threads, the walk splits the first chance node on each line, as
/// `pk_cfr`'s parallel walk does: its own frames serve the nodes down to those chance nodes,
/// whose frames hold the values of every outcome, and a workspace for each outcome serves the
/// children's walks, with a frame for each child's inputs and one for each depth below it, and
/// the room at a terminal, which the walk's own then never needs. The children of index `o` of
/// every chance node split share workspace `o`, so each workspace is counted as large as the
/// most any child needs at each depth.
///
/// `split_outcomes(street)` is the number of outcomes of a split chance node that deals
/// `street`: the cards off its board on the full tree, and their classes on the merged one.
fn workspace(
    tree: &PostflopTree,
    hands: [u64; 2],
    walk: Walk,
    terminal: u64,
    split: bool,
    split_outcomes: impl Fn(Street) -> u64,
) -> Heap {
    let nodes = tree.nodes();
    let n = hands[0].max(hands[1]);
    let cfr = walk == Walk::Cfr;
    let (mut own, mut branch) = (Needs::default(), Needs::default());
    // The most outcomes of a chance node the walk splits, one workspace each.
    let mut branches = 0;
    // For each betting node, its depth, and the depth of the split chance node above it, if any.
    let mut depth = vec![0usize; nodes.len()];
    let mut below = vec![None; nodes.len()];
    if cfr {
        // The root's inputs: its values, the traverser's reach and its possible hands.
        own.add(0, [0, n, n, 0, n, 0]);
    }
    for (index, node) in nodes.iter().enumerate() {
        let splits = split && below[index].is_none() && matches!(node, Node::Chance { .. });
        for &child in node.children() {
            depth[child as usize] = depth[index] + 1;
            below[child as usize] = if splits {
                Some(depth[index])
            } else {
                below[index]
            };
        }
        let needs = match below[index] {
            Some(_) => &mut branch,
            None => &mut own,
        };
        let frame: FrameNeeds = match (node, walk) {
            (Node::Terminal { .. }, _) => continue,
            // Every outcome's values.
            (Node::Chance { street, .. }, _) if splits => {
                let outcomes = split_outcomes(*street);
                branches = branches.max(outcomes);
                let values = match walk {
                    Walk::Cfr => n,
                    Walk::Follow(me) | Walk::Respond(me) => hands[me],
                };
                [0, outcomes * values, 0, 0, 0, 0]
            }
            // The possible hands after the card, both reaches and the card's values.
            (Node::Chance { .. }, Walk::Cfr) => [0, n, n, n, n, 0],
            // The possible hands after the card, the opponent's reach and the card's values.
            (Node::Chance { .. }, Walk::Follow(me) | Walk::Respond(me)) => {
                [0, hands[me], 0, hands[1 - me], hands[me], 0]
            }
            (
                Node::Player {
                    actor, children, ..
                },
                _,
            ) => {
                let (p, a) = (actor.index(), children.len() as u64);
                match walk {
                    // The strategy; as the traverser's node, the action values and the reach
                    // after an action; as the opponent's, a child's values and the opponent's
                    // reach after an action.
                    Walk::Cfr => {
                        needs.regrets = needs.regrets.max(a * hands[p]);
                        needs.reach = needs.reach.max(hands[p]);
                        let values = (a * hands[p]).max(hands[1 - p]);
                        [a * hands[p], values, hands[p], hands[p], 0, 0]
                    }
                    // A child's values, and the opponent's reach after an action.
                    Walk::Follow(me) | Walk::Respond(me) if p != me => {
                        [0, hands[me], 0, hands[p], 0, 0]
                    }
                    // A child's values.
                    Walk::Follow(me) => [0, hands[me], 0, 0, 0, 0],
                    // A child's values and the choices.
                    Walk::Respond(me) => [0, hands[me], 0, 0, 0, hands[me]],
                }
            }
        };
        match below[index] {
            // Below a split, frame 0 holds the child's inputs.
            Some(split_depth) => branch.add(depth[index] - split_depth, frame),
            None => own.add(depth[index] + usize::from(cfr), frame),
        }
    }
    let (frame_bytes, workspace_bytes) = match walk {
        Walk::Cfr => (CFR_FRAME_BYTES, CFR_WORKSPACE_BYTES),
        Walk::Follow(_) | Walk::Respond(_) => (RESPONSE_FRAME_BYTES, RESPONSE_WORKSPACE_BYTES),
    };
    if branches == 0 {
        return own.heap(frame_bytes, terminal, !cfr);
    }
    // A child's inputs: the traverser's possible hands and both reaches, or the player's
    // possible hands and the opponent's reach.
    let inputs = match walk {
        Walk::Cfr => [0, 0, n, n, n, 0],
        Walk::Follow(me) | Walk::Respond(me) => [0, 0, 0, hands[1 - me], hands[me], 0],
    };
    branch.add(0, inputs);
    // The walk's own frames reach terminals on the spot's board only, which need no room.
    own.heap(frame_bytes, 0, !cfr)
        + branch.heap(frame_bytes, terminal, !cfr) * branches
        + Heap::one(branches * workspace_bytes)
}

/// Estimates what solving `spot` with `options` will need, as the [module documentation](self)
/// describes, without building the game. Of the options, `max_nodes`, `max_iterations`,
/// `check_every`, `threads`, `merge_isomorphic` and `compressed_storage` matter.
///
/// Fails as [`PostflopGame::with_node_limit`](crate::PostflopGame::with_node_limit) does with
/// `options.max_nodes`: on a spot that fails [`Spot::validate`], a betting tree over the limit,
/// or a public tree over it. It builds the betting tree, which is small next to the solve.
pub fn estimate_memory(spot: &Spot, options: &SolveOptions) -> Result<MemoryEstimate, SolveError> {
    spot.validate()?;
    let limit = options.max_nodes.min(u64::from(u32::MAX));
    let mut spec = spot.tree_spec();
    spec.max_nodes = u32::try_from(limit).expect("the limit is at most u32::MAX");
    let tree = PostflopTree::build(spec)?;
    let root = spot.board_hands();
    let lists = [Player::Oop, Player::Ip].map(|player| root.hands(player).combos().to_vec());
    let hands = lists.each_ref().map(|list| list.len() as u64);
    let census = census(&tree, hands);
    if census.public_nodes > limit {
        return Err(SolveError::PublicTreeTooLarge {
            nodes: census.public_nodes,
            limit,
        });
    }
    let [n0, n1] = hands;
    // Whether the solve iterates on the merged game, as `MergedGame::new` decides it.
    let group = merging_group(
        spot.board,
        [root.hands(Player::Oop), root.hands(Player::Ip)],
    );
    let merged = (options.merge_isomorphic && census.chance_nodes > 0 && !group.is_trivial())
        .then(|| merged_census(&tree, hands, spot.board, group));
    // The threads the walks run on: a spot with no chance node runs on the calling thread.
    let threads = if census.chance_nodes > 0 {
        (options.threads as u64).max(1)
    } else {
        1
    };
    let split = threads > 1;

    // The boards: the spot's, then each turn and each river the tree reaches.
    let players = lists.each_ref().map(|list| PlayerHands::new(list));
    let river_spot = spot.board.len() == 5;
    let mut boards = Heap::default();
    for n in hands {
        boards += board_lists(n, river_spot, false);
    }
    // The spot's hands that hold each card, a list per player.
    for n in hands {
        boards += Heap::one(8 * n);
    }
    let mut num_boards = 1u64;
    // The most hands, both players' together, on a later board with a terminal: the room a
    // walk keeps for gathering the reach into a board's order and its values in that order.
    let mut terminal_room = 0;
    let mut later = |cards: &[usize], river: bool| {
        num_boards += 1;
        let mut both = 0;
        for player in &players {
            let n = player.missing(cards);
            both += n;
            boards += board_lists(n, river, true);
        }
        let terminals = if river {
            census.river_terminals
        } else {
            census.turn_terminals
        };
        if terminals {
            terminal_room = terminal_room.max(both);
        }
    };
    let off: Vec<usize> = (CardSet::FULL - spot.board)
        .iter()
        .map(|card| card.index())
        .collect();
    if spot.board.len() == 3 && census.turn_chance {
        for &turn in &off {
            later(&[turn], false);
        }
    }
    if spot.board.len() == 3 && census.river_chance {
        for (i, &turn) in off.iter().enumerate() {
            for &river in &off[i + 1..] {
                later(&[turn, river], true);
            }
        }
    }
    if spot.board.len() == 4 && census.river_chance {
        for &river in &off {
            later(&[river], true);
        }
    }
    // The vector of boards starts with the spot's alone and grows by pushes.
    let slots = if num_boards == 1 {
        1
    } else {
        grown(num_boards, BOARD)
    };
    let board_hands = boards + Heap::one(slots * BOARD);

    // The betting tree, its payoffs and the deal. The tree's nodes are pushed as it is built.
    let betting_nodes = tree.nodes().len() as u64;
    let mut betting = Heap::one(grown(betting_nodes, BETTING_NODE) * BETTING_NODE);
    let mut inner = Heap::default();
    for node in tree.nodes() {
        if let Node::Player { actions, .. } = node {
            let a = actions.len() as u64;
            inner += Heap::one(a * ACTION) + Heap::one(a * 4);
        }
    }
    for street in [Street::Flop, Street::Turn, Street::River] {
        let sizing = tree.spec().sizing.street(street);
        inner +=
            Heap::one(sizing.bets.len() as u64 * 8) + Heap::one(sizing.raises.len() as u64 * 8);
    }
    inner += Heap::one(betting_nodes * PAYOFF) + Heap::one(8 * n0) + Heap::one(8 * n1);
    betting += inner;

    // The public tree: a record per node, and the children and cards of each node.
    let public_tree = Heap::one(census.public_nodes * PUBLIC_NODE)
        + Heap::spread(4 * census.actions, census.player_nodes)
        + Heap::spread(5 * census.outcomes, 2 * census.chance_nodes);

    let game = betting + public_tree + board_hands;

    // The tree the solver iterates on, the merged one when the solve merges: its nodes, player
    // nodes, stored values and flags.
    let (solver_nodes, solver_player_nodes, stored, flags) = match &merged {
        Some(merged) => (
            merged.public_nodes,
            merged.player_nodes,
            merged.stored[0] + merged.stored[1],
            merged.flags,
        ),
        None => (
            census.public_nodes,
            census.player_nodes,
            census.stored[0] + census.stored[1],
            census.flags,
        ),
    };
    // The solver: its index of rows, the rows, and each row's flags, regrets and sums, two
    // `f32`s a stored value or, compressed, an `i16` and a `u16`.
    let compressed = options.compressed_storage;
    let (row_bytes, value_bytes) = if compressed {
        (COMPRESSED_ROW_BYTES, 2 + 2)
    } else {
        (ROW_BYTES, 4 + 4)
    };
    let rows = grown(solver_player_nodes, row_bytes);
    let regrets_and_sums = Heap::spread(value_bytes * stored, 2 * solver_player_nodes);
    let bookkeeping = Heap::one(solver_nodes * ROW_INDEX_BYTES)
        + Heap::one(rows * row_bytes)
        + Heap::spread(flags, solver_player_nodes);
    let solver = regrets_and_sums + bookkeeping;

    // The merged game: its nodes, grown by pushes, and each player node's children; its chance
    // nodes, grown by pushes, each with its representatives, its children, where each
    // outcome's images start and the images; the counterpart of every node of the full tree;
    // the members of `Γ`, collected from a filter; each player's hands relabelled by each; and
    // the stabilizers of its nodes, grown by pushes, each that holds more than the identity
    // with each player's classes of alike hands.
    let merged_game = match &merged {
        Some(merged) => {
            let order = group.order() as u64;
            let mut heap = Heap::one(grown(merged.public_nodes, MERGED_NODE) * MERGED_NODE)
                + Heap::spread(4 * merged.actions, merged.player_nodes)
                + Heap::one(grown(merged.chance_nodes, MERGED_CHANCE) * MERGED_CHANCE)
                + Heap::spread(
                    9 * merged.outcomes + 4 * merged.chance_nodes,
                    3 * merged.chance_nodes,
                )
                + Heap::spread(IMAGE * merged.images, merged.with_images)
                + Heap::one(census.public_nodes * COUNTERPART)
                + Heap::one(grown(order, MEMBER) * MEMBER);
            for n in hands {
                heap += Heap::one(order * RELABELLED) + Heap::spread(order * 4 * n, order);
            }
            let stabilizers = merged.stabilizers.len() as u64;
            heap += Heap::one(grown(stabilizers, STABILIZER) * STABILIZER);
            for stabilizer in &merged.stabilizers {
                if !stabilizer.is_trivial() {
                    heap += Heap::one(4 * hands[0]) + Heap::one(4 * hands[1]);
                }
            }
            heap
        }
        None => Heap::default(),
    };

    // A profile: an entry per node, and a strategy per player node, on the full tree whether
    // the solve merges or not.
    let full_stored = census.stored[0] + census.stored[1];
    let profile = Heap::one(census.public_nodes * PROFILE_ENTRY_BYTES)
        + Heap::spread(4 * full_stored, census.player_nodes);
    // The last measurement's profile is held while the solve iterates after it, when it can,
    // and dropped before the next is made.
    let iterates_after_measuring = options.max_iterations > options.check_every;

    // The measurement's largest transient: a best response's walk, with its workspace, its
    // possible hands at the root and its values; an expected value's walk, the same with a
    // workspace of its own; or the check of the tree's shape before each walk, a flag per node,
    // both players' possible hands at the root and a frame of them for each chance node on a
    // line.
    let best_response = [0, 1]
        .map(|p| {
            Heap::one(8 * hands[p])
                + Heap::one(hands[p])
                + workspace(
                    &tree,
                    hands,
                    Walk::Respond(p),
                    terminal_room,
                    split,
                    outcomes,
                )
        })
        .into_iter()
        .fold(Heap::default(), Heap::max);
    let follow = [0, 1]
        .map(|p| {
            Heap::one(8 * hands[p])
                + Heap::one(hands[p])
                + workspace(
                    &tree,
                    hands,
                    Walk::Follow(p),
                    terminal_room,
                    split,
                    outcomes,
                )
        })
        .into_iter()
        .fold(Heap::default(), Heap::max);
    let chance_levels = u64::from(census.turn_chance) + u64::from(census.river_chance);
    let mut check_frames =
        Heap::one(n0) + Heap::one(n1) + Heap::spread(chance_levels * (n0 + n1), 2 * chance_levels);
    if chance_levels > 0 {
        check_frames += Heap::one(grown(chance_levels, CHECK_FRAME_BYTES) * CHECK_FRAME_BYTES);
    }
    // The check of the full tree before each walk of a measurement, and the check of the
    // solver's tree when the solver is built.
    let shape_check = Heap::one(census.public_nodes) + check_frames;
    let solver_shape_check = Heap::one(solver_nodes) + check_frames;
    // A merged solve expands its average strategy onto the full tree through the room of one
    // node's strategy, which grows to the largest.
    let expansion = if merged.is_some() {
        let largest = tree
            .nodes()
            .iter()
            .filter_map(|node| match node {
                Node::Player {
                    actor, children, ..
                } => Some(children.len() as u64 * hands[actor.index()]),
                _ => None,
            })
            .max()
            .unwrap_or(0);
        Heap::one(4 * largest)
    } else {
        Heap::default()
    };
    let measuring_extra = best_response.max(follow).max(shape_check).max(expansion);
    // The CFR walk's workspace, which the solver keeps from its first iteration on, on the
    // merged tree when the solve merges.
    let cfr_walk = match &merged {
        Some(merged) => {
            let split_outcomes = merged.split_outcomes;
            let classes = |street: Street| split_outcomes[street as usize];
            workspace(&tree, hands, Walk::Cfr, terminal_room, split, classes)
        }
        None => workspace(&tree, hands, Walk::Cfr, terminal_room, split, outcomes),
    };
    // The solve's own pool, made once the solver is built and dropped when the solve returns.
    let thread_pool = if split {
        Heap::spread(POOL_BYTES + threads * POOL_THREAD_BYTES, 8 + 12 * threads)
    } else {
        Heap::default()
    };

    // The invariant check, in a build with debug assertions only.
    let check = if cfg!(debug_assertions) {
        invariant_check(&census, hands)
    } else {
        Heap::default()
    };

    // The phases. Building ends with the solver's rows, while its check of the tree holds a
    // flag per node and its vector of rows has just doubled. From the first iteration on, the
    // solver holds its walk's workspace too, and the solve its pool. The finish comes once the
    // solver is dropped, with the game and the last profile moved into the solution.
    let building =
        game + merged_game + solver + solver_shape_check + Heap::one(rows / 2 * row_bytes);
    let held = game + merged_game + solver + cfr_walk + thread_pool;
    let iterating = if iterates_after_measuring {
        held + profile
    } else {
        held
    };
    let measuring = held + profile + measuring_extra;
    let finishing = game + profile + thread_pool + check;
    let (peak_phase, peak) = [
        (Phase::Iterating, iterating),
        (Phase::Measuring, measuring),
        (Phase::Finishing, finishing),
    ]
    .into_iter()
    .fold((Phase::Building, building), |most, phase| {
        if phase.1.bytes > most.1.bytes {
            phase
        } else {
            most
        }
    });
    // A margin for the allocations too small to follow one by one.
    let margin = peak.bytes / 256 + 4096;
    let heap_bytes = peak.bytes + margin;
    let allocator_overhead = peak.overhead();
    Ok(MemoryEstimate {
        hands,
        betting_nodes,
        public_nodes: census.public_nodes,
        player_nodes: census.player_nodes,
        chance_nodes: census.chance_nodes,
        terminal_nodes: census.terminal_nodes,
        boards: num_boards,
        merged: merged.is_some(),
        merged_nodes: merged.as_ref().map_or(0, |merged| merged.public_nodes),
        merged_player_nodes: merged.as_ref().map_or(0, |merged| merged.player_nodes),
        compressed,
        stored_values: stored,
        regrets_and_sums: regrets_and_sums.bytes,
        solver_bookkeeping: bookkeeping.bytes,
        public_tree: public_tree.bytes,
        board_hands: board_hands.bytes,
        betting_tree: betting.bytes,
        merged_game: merged_game.bytes,
        average_strategy: profile.bytes,
        profiles_held: 1,
        best_response: best_response.bytes,
        traversal: cfr_walk.max(follow).bytes,
        threads,
        thread_pool: thread_pool.bytes,
        invariant_check: check.bytes,
        building: building.bytes,
        iterating: iterating.bytes,
        measuring: measuring.bytes,
        finishing: finishing.bytes,
        peak_phase,
        margin,
        heap_bytes,
        allocator_overhead,
        process_bytes: PROCESS_BYTES,
        total: heap_bytes + allocator_overhead + PROCESS_BYTES,
    })
}

/// The invariant check's heap, the larger of its two walks: the reach check, with its stack of
/// both players' reaches, and the expected rake, with its terminals and its walk's copies of
/// the weights.
fn invariant_check(census: &Census, hands: [u64; 2]) -> Heap {
    let reaches = 8 * (hands[0] + hands[1]);
    // The stack, the root's reaches of 1, the reaches of the node taken off the stack, its
    // child's expected reaches and the ones found for it, and the path that finds them. The
    // stack and the path grow by pushes, the old block held while the new one is filled.
    let stack = grown(census.waiting + 1, REACH_ENTRY) * REACH_ENTRY;
    let path = grown(census.depth + 1, 16) * 16;
    let reach_check = Heap::spread((census.waiting + 3) * reaches, 2 * (census.waiting + 3))
        + Heap::spread(stack + stack / 2, 2)
        + Heap::spread(path + path / 2, 2);
    // Each hand's cards, the combos both players hold, the weights, a copy of them per node
    // on the way to a terminal, and the terminals: grown by pushes, with the old block and the
    // new held while they grow, and then sorted with a scratch buffer of the same size.
    let terminals = grown(census.terminal_nodes, TERMINAL) * TERMINAL;
    let walk = Heap::spread(census.depth * reaches, 2 * census.depth)
        + Heap::spread(terminals + terminals / 2, 2);
    let sort = Heap::one(terminals) + Heap::one(census.terminal_nodes * TERMINAL);
    let rake_check = Heap::one(16 * (hands[0] + hands[1]))
        + Heap::one(16 * grown(hands[0].min(hands[1]), 16))
        + Heap::one(reaches)
        + walk.max(sort);
    reach_check.max(rake_check)
}

/// Bytes in decimal units, as `1.23 GB`, `456.7 MB`, `12.3 kB` or `999 B`.
pub fn format_bytes(bytes: u64) -> String {
    let b = bytes as f64;
    if b >= 1e9 {
        format!("{:.2} GB", b / 1e9)
    } else if b >= 1e6 {
        format!("{:.1} MB", b / 1e6)
    } else if b >= 1e3 {
        format!("{:.1} kB", b / 1e3)
    } else {
        format!("{bytes} B")
    }
}

/// The sizes, the items, the allowances and the total, one per line.
impl fmt::Display for MemoryEstimate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "OOP has {} hands and IP {}; the public tree has {} nodes ({} player, {} chance, {} \
             terminal) from {} betting nodes, on {} boards",
            self.hands[0],
            self.hands[1],
            self.public_nodes,
            self.player_nodes,
            self.chance_nodes,
            self.terminal_nodes,
            self.betting_nodes,
            self.boards
        )?;
        if self.merged {
            writeln!(
                f,
                "The solver iterates on a merged tree of {} nodes ({} player), dealing once each \
                 class of cards that a symmetry of the suits makes alike",
                self.merged_nodes, self.merged_player_nodes
            )?;
        }
        let stored = if self.compressed {
            "regrets and strategy sums, 16-bit"
        } else {
            "regrets and strategy sums, f32"
        };
        let profiles = format!("average strategy, {} held at once", self.profiles_held);
        let pool = match self.threads {
            1 => "thread pool, none on one thread".to_string(),
            threads => format!("thread pool, {threads} threads"),
        };
        let items = [
            (stored, self.regrets_and_sums),
            ("solver bookkeeping", self.solver_bookkeeping),
            ("public tree", self.public_tree),
            ("hands on each board", self.board_hands),
            ("betting tree, payoffs and deal", self.betting_tree),
            ("merged game", self.merged_game),
            (
                profiles.as_str(),
                self.average_strategy * self.profiles_held,
            ),
            ("best response's walk", self.best_response),
            ("traversal buffers", self.traversal),
            (pool.as_str(), self.thread_pool),
            ("invariant check (debug builds)", self.invariant_check),
        ];
        for (name, bytes) in items {
            writeln!(f, "  {name:<36} {:>10}", format_bytes(bytes))?;
        }
        writeln!(
            f,
            "  {:<36} {:>10}",
            "margin for small allocations",
            format_bytes(self.margin)
        )?;
        writeln!(
            f,
            "  {:<36} {:>10}  {}",
            "heap at the peak",
            format_bytes(self.heap_bytes),
            self.peak_phase
        )?;
        writeln!(
            f,
            "  {:<36} {:>10}",
            "allocator overhead",
            format_bytes(self.allocator_overhead)
        )?;
        writeln!(
            f,
            "  {:<36} {:>10}",
            "the process itself",
            format_bytes(self.process_bytes)
        )?;
        write!(f, "  {:<36} {:>10}", "total", format_bytes(self.total))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_vec_filled_by_pushes_grows_as_the_standard_library_grows_it() {
        let filled = |len: usize| {
            let mut v: Vec<u64> = Vec::new();
            v.extend((0..len as u64).filter(|_| true));
            v.capacity() as u64
        };
        for len in [0, 1, 3, 4, 5, 300, 1025, 4096] {
            assert_eq!(grown(len as u64, 8), filled(len), "{len}");
        }
        let bytes = |len: usize| {
            let mut v: Vec<u8> = Vec::new();
            for byte in 0..len {
                v.push(byte as u8);
            }
            v.capacity() as u64
        };
        assert_eq!(grown(3, 1), bytes(3));
        assert_eq!(grown(9, 1), bytes(9));
    }

    #[test]
    fn the_hands_on_a_later_board_are_those_that_miss_its_cards() {
        let combos: Vec<Combo> = ["AsKs", "AsAh", "KdQd", "7c7d"]
            .iter()
            .map(|c| c.parse().unwrap())
            .collect();
        let hands = PlayerHands::new(&combos);
        let card = |c: &str| c.parse::<pk_core::Card>().unwrap().index();
        assert_eq!(hands.missing(&[card("As")]), 2);
        assert_eq!(hands.missing(&[card("2c")]), 4);
        // AsAh holds both cards, so it is taken off once.
        assert_eq!(hands.missing(&[card("As"), card("Ah")]), 2);
        assert_eq!(hands.missing(&[card("Ks"), card("Kd")]), 2);
    }

    fn load(path: &str) -> Spot {
        Spot::load(format!("{}/{path}.toml", env!("CARGO_MANIFEST_DIR"))).unwrap()
    }

    #[test]
    fn the_estimate_counts_the_tree_the_game_builds() {
        use crate::game::PostflopGame;
        use pk_cfr::{PublicGame, PublicNode};

        for path in [
            "tests/data/turn_small",
            "tests/data/flop_small",
            "../../reference/spots/river/deep_high_spr",
        ] {
            let spot = load(path);
            let estimate = estimate_memory(&spot, &SolveOptions::default()).unwrap();
            let game = PostflopGame::from_spot(&spot).unwrap();
            let mut counts = [0u64; 3];
            let mut stored = 0;
            for node in 0..game.num_nodes() {
                match game.node(node) {
                    PublicNode::Player {
                        player,
                        num_actions,
                    } => {
                        counts[0] += 1;
                        stored += (num_actions * game.num_hands(player)) as u64;
                    }
                    PublicNode::Chance { .. } => counts[1] += 1,
                    PublicNode::Terminal => counts[2] += 1,
                }
            }
            assert_eq!(estimate.public_nodes, game.num_nodes() as u64, "{path}");
            assert_eq!(
                [
                    estimate.player_nodes,
                    estimate.chance_nodes,
                    estimate.terminal_nodes
                ],
                counts,
                "{path}"
            );
            assert_eq!(estimate.stored_values, stored, "{path}");
            assert_eq!(estimate.regrets_and_sums, 8 * stored, "{path}");
            assert_eq!(estimate.betting_nodes, game.tree().nodes().len() as u64);
            let hands = [Player::Oop, Player::Ip].map(|p| game.hands(p).len() as u64);
            assert_eq!(estimate.hands, hands, "{path}");
        }
        // The flop spot reaches every turn and every river, the turn spot every river.
        let boards = |path| {
            estimate_memory(&load(path), &SolveOptions::default())
                .unwrap()
                .boards
        };
        assert_eq!(boards("tests/data/flop_small"), 1 + 49 + 1176);
        assert_eq!(boards("tests/data/turn_small"), 1 + 48);
    }

    #[test]
    fn the_estimate_counts_the_merged_tree_the_merged_game_builds() {
        use crate::game::PostflopGame;
        use crate::merged::MergedGame;
        use pk_cfr::{PublicGame, PublicNode};

        // The merged tree's nodes, player nodes and stored values, counted from the betting
        // tree and the classes of cards, are the merged game's, on a turn and four flop spots,
        // the benchmark's turn among them; with merging off the estimate counts the full tree.
        for path in [
            "benches/data/turn_srp",
            "tests/data/turn_four_flush",
            "tests/data/flop_monotone",
            "tests/data/flop_two_tone",
            "tests/data/flop_paired",
            "tests/data/flop_broken",
        ] {
            let spot = load(path);
            let estimate = estimate_memory(&spot, &SolveOptions::default()).unwrap();
            let full = PostflopGame::from_spot(&spot).unwrap();
            let game = MergedGame::new(&full).unwrap();
            let (mut player_nodes, mut stored) = (0, 0);
            for node in 0..game.num_nodes() {
                if let PublicNode::Player {
                    player,
                    num_actions,
                } = game.node(node)
                {
                    player_nodes += 1;
                    stored += (num_actions * game.num_hands(player)) as u64;
                }
            }
            assert!(estimate.merged, "{path}");
            assert_eq!(estimate.merged_nodes, game.num_nodes() as u64, "{path}");
            assert_eq!(estimate.merged_player_nodes, player_nodes, "{path}");
            assert_eq!(estimate.stored_values, stored, "{path}");
            assert_eq!(estimate.public_nodes, full.num_nodes() as u64, "{path}");
            let off = SolveOptions {
                merge_isomorphic: false,
                ..SolveOptions::default()
            };
            let unmerged = estimate_memory(&spot, &off).unwrap();
            assert!(!unmerged.merged && unmerged.merged_game == 0, "{path}");
            assert!(unmerged.stored_values > stored, "{path}");
        }
    }

    #[test]
    fn a_solve_over_the_memory_limit_is_refused_before_it_starts() {
        use crate::solve::{SolveError, solve};

        let spot = load("tests/data/turn_small");
        let options = SolveOptions {
            max_iterations: 20,
            ..SolveOptions::default()
        };
        let estimate = estimate_memory(&spot, &options).unwrap();
        assert!(estimate.total > estimate.heap_bytes && estimate.heap_bytes > 0);
        let tight = SolveOptions {
            max_memory: estimate.total - 1,
            ..options
        };
        let error = solve(&spot, &tight, |_| panic!("the solve should not start")).unwrap_err();
        let SolveError::MemoryTooLarge { estimate: e, limit } = error else {
            panic!("{error}");
        };
        assert_eq!((e, limit), (estimate.total, estimate.total - 1));
        let text = error.to_string();
        assert!(text.contains(&format_bytes(estimate.total)), "{text}");
        assert!(text.contains("more than the limit of"), "{text}");
        // At the limit, the solve runs.
        let exact = SolveOptions {
            max_memory: estimate.total,
            ..options
        };
        assert!(solve(&spot, &exact, |_| {}).is_ok());
    }

    #[test]
    fn the_default_limit_refuses_a_full_range_flop_solve_with_a_raise_a_street() {
        use crate::solve::{SolveError, solve};

        // Every combo on both sides of the benchmark spot on an unpaired rainbow flop, which no
        // relabelling of the suits keeps, with a bet and a raise on every street: under the node
        // limit, and over the memory limit, refused from the counts alone. (On the benchmark's
        // own flop, which merges, the solve fits since #41, at 9.87 GB by the estimate.)
        let mut spot = load("benches/data/flop_srp");
        spot.board = "Ks7d2c".parse().unwrap();
        spot.oop_range = pk_core::Range::full();
        spot.ip_range = pk_core::Range::full();
        for sizing in [
            &mut spot.tree.flop,
            &mut spot.tree.turn,
            &mut spot.tree.river,
        ] {
            sizing.as_mut().unwrap().raises = vec![3.0];
        }
        spot.tree.max_raises = 1;
        let options = SolveOptions::default();
        let estimate = estimate_memory(&spot, &options).unwrap();
        assert!(estimate.public_nodes <= options.max_nodes);
        assert!(estimate.total > options.max_memory, "{estimate}");
        let start = std::time::Instant::now();
        match solve(&spot, &options, |_| panic!("the solve should not start")) {
            Err(SolveError::MemoryTooLarge { estimate: e, limit }) => {
                assert_eq!((e, limit), (estimate.total, 10_000_000_000));
            }
            other => panic!("{other:?}"),
        }
        assert!(start.elapsed().as_secs_f64() < 1.0);
    }

    #[test]
    fn a_public_tree_over_the_node_limit_is_refused_as_the_game_refuses_it() {
        use crate::game::PostflopGame;

        let spot = load("tests/data/flop_small");
        let options = SolveOptions {
            max_nodes: 121_387,
            ..SolveOptions::default()
        };
        let ours = estimate_memory(&spot, &options).unwrap_err();
        let game = PostflopGame::with_node_limit(&spot, options.max_nodes).unwrap_err();
        assert_eq!(ours.to_string(), game.to_string());
        assert!(matches!(
            ours,
            SolveError::PublicTreeTooLarge {
                nodes: 121_388,
                limit: 121_387
            }
        ));
    }
}
