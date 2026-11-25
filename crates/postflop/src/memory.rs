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
//!   at every player node of the public tree. The hands are all of the player's hands on the
//!   spot's board, on every street (DECISIONS P25).
//! - **Solver bookkeeping**: `pk_cfr::VectorCfr`'s index from every public node to its row,
//!   16 bytes a node, the row headers, and a `bool` per hand at each player node saying whether
//!   the hand is possible there.
//! - **Public tree**: a record for every node, and the children and cards of each player and
//!   chance node, each its own allocation.
//! - **Hands on each board**: a `BoardHands` for every board the tree reaches, with each
//!   player's hands, their weights, the cross index, the ranking on a river board and the index
//!   back to the spot's lists.
//! - **Betting tree, payoffs and deal**: the betting tree, a payoff per betting node, and the
//!   deal vectors.
//! - **Average strategy**: the `PublicProfile` each exploitability measurement builds, with an
//!   entry for every public node and an `f32` per action and hand at each player node. The solve
//!   keeps the last measurement's profile while it makes the next, so it holds two at once
//!   whenever it can measure twice, which is whenever `max_iterations` is more than
//!   `check_every`. When it stops after an iteration it did not measure, at an iteration cap
//!   that is not a multiple of `check_every` or at a time cap, it measures once more and keeps
//!   the old profile until it returns, so it finishes with two as well.
//! - **Best response**: the pure strategy a best-response walk builds and then drops, with an
//!   entry for every public node and an `f32` per action and hand at the responder's nodes, for
//!   the larger of the two players.
//! - **Traversal buffers**: the vectors a walk allocates at a node and holds while it walks
//!   below it, summed down the costliest line of the tree, for the CFR walk and for the
//!   exploitability walks. They are small, since a line holds a few vectors per node on it.
//! - **Copy of the game in the solution**: [`Solution`](crate::Solution) holds a copy of the
//!   game, made while the solver still holds the original.
//! - **Invariant check**: in a build with debug assertions, the check of the solution that the
//!   solve makes before it returns ([`crate::invariants`]). Zero in a release build.
//!
//! # When the most is held
//!
//! The solve passes through four phases, and the estimate is the largest of them:
//!
//! - **Building**: the game, then the solver's rows, with the transient copies their growth
//!   makes and the solver's check of the tree.
//! - **Iterating**: the game, the solver, the profile kept from the last measurement, and a
//!   walk's buffers.
//! - **Measuring**: the same with the new profile, and a best response or a walk's buffers.
//! - **Finishing**: the game, the solver, the profiles, and the solution's copy of the game,
//!   with the invariant check in a build with debug assertions.
//!
//! On the test and benchmark spots the measurement or the finish holds the most.
//!
//! # Past the bytes the solver asks for
//!
//! [`MemoryEstimate::heap_bytes`] is what the solve asks the allocator for at its peak, which a
//! counting allocator measures, and which `tests/memory_estimate.rs` checks against one. It is
//! the largest phase plus a [`margin`](MemoryEstimate::margin) of 1/256 and 4 KiB for the
//! allocations too small to follow one by one. Without the margin, the largest phase comes
//! within 0.1% of the measured peak on every test spot but the smallest, a river spot of half a
//! megabyte where the walks' buffers are a larger share and the bound on them puts it 2.5%
//! above, and it is never below. A process holds more than the heap: each allocation carries
//! the allocator's header and rounding, and the process has its code and stacks.
//! [`MemoryEstimate::allocator_overhead`] allows
//! 32 bytes per live allocation plus 1/16 of the bytes, and [`MemoryEstimate::process_bytes`] a
//! fixed [`PROCESS_BYTES`]. [`MemoryEstimate::total`] adds the three, and is what the solve
//! compares with its limit. Both allowances are on the high side, because the estimate's job is
//! to refuse a solve that would not fit.
//!
//! # What it depends on
//!
//! The figures follow the solver's code, not a model of a solver, so a change to what the solver
//! allocates must change this module too. Two layouts are private to `pk_cfr`, the solver's row
//! and a profile's entry, and are mirrored here by structs with the same fields. A `Vec` filled
//! by pushes is taken to grow as the standard library grows it, doubling from four elements. The
//! test against a counting allocator catches a change to any of these.

use std::fmt;
use std::mem::size_of;
use std::ops::{Add, AddAssign, Mul};

use pk_core::{CardSet, Combo, HandRank, NUM_CARDS};
use pk_game::{Action, Node, Player, PostflopTree, Street};
use serde::Serialize;

use crate::expected_rake::TerminalOutcome;
use crate::game::{BOARD_BYTES, PUBLIC_NODE_BYTES};
use crate::payoff::Payoff;
use crate::solve::{SolveError, SolveOptions};
use crate::spot::Spot;

/// What [`MemoryEstimate::process_bytes`] allows for the process apart from the solve: its code,
/// its stacks and the standard library's buffers. 16 MiB, about three times what the example
/// program holds before it solves.
pub const PROCESS_BYTES: u64 = 16 << 20;

/// The layout of the row `pk_cfr::VectorCfr` keeps for each player node, which is private to
/// `pk_cfr`: the acting player, whether each hand is possible, the regrets and the strategy sums.
#[allow(dead_code)]
struct NodeRowLayout {
    player: usize,
    possible: Vec<bool>,
    regrets: Vec<f32>,
    strategy_sum: Vec<f32>,
}

/// The layout of a node's entry in a `pk_cfr::PublicProfile`, which is private to `pk_cfr`.
#[allow(dead_code)]
struct NodeStrategyLayout {
    num_hands: usize,
    probabilities: Vec<f32>,
}

const ROW_BYTES: u64 = size_of::<NodeRowLayout>() as u64;
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
    /// The values in each of the solver's two arrays: actions times the acting player's hands,
    /// summed over the player nodes.
    pub stored_values: u64,

    pub regrets_and_sums: u64,
    /// The solver's index of rows, its row headers and its possible-hand flags.
    pub solver_bookkeeping: u64,
    pub public_tree: u64,
    /// The `BoardHands` of every board, and the vector that holds them.
    pub board_hands: u64,
    /// The betting tree, the payoffs and the deal.
    pub betting_tree: u64,
    /// One average-strategy profile.
    pub average_strategy: u64,
    /// The most average-strategy profiles the solve holds at once: 2 when it can measure
    /// twice, and 1 otherwise.
    pub profiles_held: u64,
    /// The larger player's best response, with its walk's buffers.
    pub best_response: u64,
    /// The larger of a CFR walk's buffers and an exploitability walk's.
    pub traversal: u64,
    /// The copy of the game that the solution holds.
    pub solution_game: u64,
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

/// One player's lists on a board where it has `n` hands. The game's own lists are filled by
/// pushes, as `HandList::new` fills them, and its copy's hold exactly `n`. A river board ranks
/// its hands, and a board other than the spot's indexes them back to the spot's lists.
fn board_lists(n: u64, copy: bool, river: bool, later: bool) -> Heap {
    let slots = if copy { n } else { grown(n, 8) };
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
    /// Each player's public player nodes.
    player_nodes_of: [u64; 2],
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
                census.player_nodes_of[player] += m;
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

/// Which walk a traversal bound is for.
#[derive(Clone, Copy, PartialEq)]
enum Walk {
    /// `VectorCfr::iterate`'s walk for one traverser.
    Cfr,
    /// `vector_expected_value`'s walk.
    Follow,
    /// `vector_best_response`'s walk, counted beyond the whole pure strategy it builds: below a
    /// node of the responder, that node's strategy is not made yet.
    Respond,
}

/// The most heap a walk holds at once for the player `me`: the buffers each node holds while
/// the walk is below it, summed down the costliest line, and what a node holds at itself. Every
/// public copy of a betting node holds the same, with at most the spot's hands.
fn walk_bound(tree: &PostflopTree, hands: [u64; 2], me: usize, walk: Walk) -> u64 {
    let [h, o] = [hands[me], hands[1 - me]].map(|n| n as i64);
    let nodes = tree.nodes();
    let mut above = vec![0i64; nodes.len()];
    above[0] = match walk {
        // The traverser's reach and possible hands.
        Walk::Cfr => 9 * h,
        // The possible hands.
        Walk::Follow | Walk::Respond => h,
    };
    let mut most = 0;
    for (index, node) in nodes.iter().enumerate() {
        let here = above[index];
        // What the node holds while it walks a child, and the most it holds at itself, a
        // child's returned values included.
        let (frame, local) = match node {
            // The values, and a later board's gathered reach and values.
            Node::Terminal { .. } => (0, 16 * h + 8 * o),
            Node::Chance { .. } => {
                let frame = match walk {
                    // The values, the possible hands after the card, and both reaches.
                    Walk::Cfr => 17 * h + 8 * o,
                    // The values, the possible hands after the card, and the reach.
                    Walk::Follow | Walk::Respond => 9 * h + 8 * o,
                };
                (frame, frame + 8 * h)
            }
            Node::Player {
                actor, children, ..
            } => {
                let a = children.len() as i64;
                match (walk, actor.index() == me) {
                    (Walk::Cfr, true) => {
                        // The strategy, the action values, and the reach of an action.
                        let frame = 12 * a * h + 8 * h;
                        // Then the node's values, the instantaneous regrets, grown by pushes
                        // with the old block and the new one held while they grow, and the
                        // reach as `f32`.
                        let regrets = 4 * grown((a * h) as u64, 4) as i64;
                        let after =
                            12 * a * h + 8 * h + (regrets + regrets / 2).max(regrets + 4 * h);
                        (frame, (frame + 8 * h).max(after))
                    }
                    // The strategy, the values, and the opponent's reach for an action.
                    (Walk::Cfr, false) => (4 * a * o + 8 * h + 8 * o, 4 * a * o + 16 * h + 8 * o),
                    // The values.
                    (Walk::Follow, true) => (8 * h, 16 * h),
                    // The best values and the choices, less the node's strategy, which is
                    // made once its children are done.
                    (Walk::Respond, true) => (16 * h - 4 * a * h, (24 * h - 4 * a * h).max(16 * h)),
                    // The values, and the opponent's reach for an action.
                    (Walk::Follow | Walk::Respond, false) => (8 * h + 8 * o, 16 * h + 8 * o),
                }
            }
        };
        most = most.max(here + local);
        for &child in node.children() {
            above[child as usize] = here + frame;
        }
    }
    most.max(0) as u64
}

/// Estimates what solving `spot` with `options` will need, as the [module documentation](self)
/// describes, without building the game. Of the options, `max_nodes`, `max_iterations`,
/// `check_every` and whether there is a `max_time` matter.
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

    // The boards: the spot's, then each turn and each river the tree reaches.
    let players = lists.each_ref().map(|list| PlayerHands::new(list));
    let river_spot = spot.board.len() == 5;
    let mut boards = Heap::default();
    let mut boards_copy = Heap::default();
    for n in hands {
        boards += board_lists(n, false, river_spot, false);
        boards_copy += board_lists(n, true, river_spot, false);
    }
    let mut num_boards = 1u64;
    let mut later = |cards: &[usize], river: bool| {
        num_boards += 1;
        for player in &players {
            let n = player.missing(cards);
            boards += board_lists(n, false, river, true);
            boards_copy += board_lists(n, true, river, true);
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
    let board_hands_copy = boards_copy + Heap::one(num_boards * BOARD);

    // The betting tree, its payoffs and the deal. The tree's nodes are pushed as it is built,
    // and a copy holds exactly as many.
    let betting_nodes = tree.nodes().len() as u64;
    let mut betting = Heap::one(grown(betting_nodes, BETTING_NODE) * BETTING_NODE);
    let mut betting_copy = Heap::one(betting_nodes * BETTING_NODE);
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
    betting_copy += inner;

    // The public tree: a record per node, and the children and cards of each node.
    let public_tree = Heap::one(census.public_nodes * PUBLIC_NODE)
        + Heap::spread(4 * census.actions, census.player_nodes)
        + Heap::spread(5 * census.outcomes, 2 * census.chance_nodes);

    let game = betting + public_tree + board_hands;
    let game_copy = betting_copy + public_tree + board_hands_copy;

    // The solver: its index of rows, the rows, and each row's flags, regrets and sums.
    let stored = census.stored[0] + census.stored[1];
    let rows = grown(census.player_nodes, ROW_BYTES);
    let regrets_and_sums = Heap::spread(8 * stored, 2 * census.player_nodes);
    let bookkeeping = Heap::one(census.public_nodes * ROW_INDEX_BYTES)
        + Heap::one(rows * ROW_BYTES)
        + Heap::spread(census.flags, census.player_nodes);
    let solver = regrets_and_sums + bookkeeping;

    // A profile: an entry per node, and a strategy per player node.
    let profile = Heap::one(census.public_nodes * PROFILE_ENTRY_BYTES)
        + Heap::spread(4 * stored, census.player_nodes);
    // The last measurement's profile is held while the next is made, and, when the solve can
    // stop after an iteration it did not measure, while it measures once more and returns.
    let measures_twice = options.max_iterations > options.check_every;
    let profiles_held = if measures_twice { 2 } else { 1 };
    let unmeasured_stop = !options
        .max_iterations
        .is_multiple_of(options.check_every.max(1))
        || options.max_time.is_some();
    let profiles_at_finish = if measures_twice && unmeasured_stop {
        2
    } else {
        1
    };

    // The measurement's largest transient: a best response, an entry per node and a strategy
    // per node of the responder, with its walk and its values; an expected value's walk; or the
    // check of the tree's shape before each walk, a flag per node and the possible hands.
    let best_response = [0, 1]
        .map(|p| {
            Heap::one(census.public_nodes * PROFILE_ENTRY_BYTES)
                + Heap::spread(4 * census.stored[p], census.player_nodes_of[p])
                + Heap::one(8 * hands[p])
                + Heap::one(walk_bound(&tree, hands, p, Walk::Respond))
        })
        .into_iter()
        .fold(Heap::default(), Heap::max);
    let follow = [0, 1]
        .map(|p| walk_bound(&tree, hands, p, Walk::Follow))
        .into_iter()
        .max()
        .unwrap_or(0);
    let shape_check = Heap::one(census.public_nodes) + Heap::one(3 * (n0 + n1));
    let measuring_extra = best_response.max(Heap::one(follow)).max(shape_check);
    let cfr_walk = [0, 1]
        .map(|p| walk_bound(&tree, hands, p, Walk::Cfr))
        .into_iter()
        .max()
        .unwrap_or(0);

    // The invariant check, in a build with debug assertions only.
    let check = if cfg!(debug_assertions) {
        invariant_check(&census, hands)
    } else {
        Heap::default()
    };

    // The phases. Building ends with the solver's rows, while its check of the tree holds a
    // flag per node and its vector of rows has just doubled.
    let building = game + solver + shape_check + Heap::one(rows / 2 * ROW_BYTES);
    let iterating = game + solver + profile * (profiles_held - 1) + Heap::one(cfr_walk);
    let measuring = game + solver + profile * profiles_held + measuring_extra;
    let finishing = game + solver + profile * profiles_at_finish + game_copy + check;
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
        stored_values: stored,
        regrets_and_sums: regrets_and_sums.bytes,
        solver_bookkeeping: bookkeeping.bytes,
        public_tree: public_tree.bytes,
        board_hands: board_hands.bytes,
        betting_tree: betting.bytes,
        average_strategy: profile.bytes,
        profiles_held,
        best_response: best_response.bytes,
        traversal: cfr_walk.max(follow),
        solution_game: game_copy.bytes,
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
        let profiles = format!("average strategy, {} held at once", self.profiles_held);
        let items = [
            ("regrets and strategy sums", self.regrets_and_sums),
            ("solver bookkeeping", self.solver_bookkeeping),
            ("public tree", self.public_tree),
            ("hands on each board", self.board_hands),
            ("betting tree, payoffs and deal", self.betting_tree),
            (
                profiles.as_str(),
                self.average_strategy * self.profiles_held,
            ),
            ("best response", self.best_response),
            ("traversal buffers", self.traversal),
            ("copy of the game in the solution", self.solution_game),
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

        // Every combo on both sides of the benchmark flop, with a bet and a raise on every
        // street: under the node limit, and many times the memory limit, refused from the
        // counts alone.
        let mut spot = load("benches/data/flop_srp");
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
