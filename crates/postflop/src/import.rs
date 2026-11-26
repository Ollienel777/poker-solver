//! Mapping a [solution dump](crate::dump) onto our tree for the same spot, so that its strategy
//! can be measured with our code and compared with ours.
//!
//! # Checks
//!
//! A reference solver's tree being subtly different from ours is the likeliest way for a
//! comparison to go wrong, so [`MappedSolution::from_dump`] checks that the dump answers the
//! spot before it reads a single probability, in this order, and stops at the first stage
//! that fails:
//!
//! 1. **The dump itself**, as [`SolutionDump::validate`] checks it. In version 2 that covers
//!    its paths too: where the deals fall, the betting, the cards that can fall, the rows of
//!    the hands that hold a dealt card, and whether `complete` agrees with the streets it holds.
//! 2. **The spot**: the board, the pot, the effective stack and the rake must be the spot's.
//!    The rake's cap is compared only when the percent is positive, since without a percent it
//!    takes nothing.
//! 3. **The hands**: each player's hands must be those of its range on the board, each with
//!    the same weight within [`WEIGHT_TOLERANCE`]. A dump whose two players' hands are ours
//!    the other way round is reported as that.
//! 4. **The tree**: walking our public tree from the root, the dump must have a node at the
//!    path of each of our decision nodes on the streets it holds, with the same acting player
//!    and the same actions, matched by kind and amount whatever their order. Where the actions
//!    differ, the walk goes on below the actions both have, so that every node where the trees
//!    part is reported, and not the nodes below an action only one side has. A node of the
//!    dump that is not one of ours is reported too, once, at the shallowest such path.
//!
//! Each stage reports every difference it finds, as a [`Mismatch`], so that one error shows
//! what is wrong. Once all pass, every probability is taken from the dump's row for the same
//! two cards, in our order of the actions, and rounded to `f32` as our strategies are stored.
//!
//! # Streets and cards
//!
//! A dump holds the decision nodes of its first [`SolutionDump::streets`] streets: every street
//! when it is complete, and on a flop or turn spot possibly only the first one or two. The tree
//! stage expects exactly those: on each street the dump holds, every decision node of ours,
//! after every card that can fall. Where our tree deals a card and the dump has no node after
//! some of the cards that can fall, the stage reports them together, once for the deal, as
//! [`Mismatch::MissingCards`]: a dump that leaves out a card, or lists a street for some lines
//! or cards and not others, is caught there, at the deal where it first shows.
//!
//! Our public tree has a subtree after every card off the board, including the rare card that
//! cannot fall because every pair of hands that can be held there holds it. No hand dealt ever
//! plays to such a subtree, a dump has no node there, and the stage expects none: it is not a
//! mismatch. Its strategy, which no hand dealt can reach, is filled in as uniform.
//!
//! # A partial solution
//!
//! The strategy of a partial dump is known on the streets it holds and nowhere below them,
//! where it is filled in as uniform so that our code can walk the tree. A [`MappedSolution`]
//! says which nodes it holds ([`MappedSolution::holds`]). Its exploitability cannot be measured
//! without the rest of the strategy, so [`MappedSolution::exploitability`] is `None`, and an EV
//! is measured only at a node whose whole subtree it holds, such as a node facing an all-in on
//! the first street, whose call runs the cards out with no more decisions. A hand that holds a
//! card dealt on the way to a node has no row there, and plays uniformly, as in our solves.

use std::collections::{HashMap, HashSet};
use std::fmt;

use pk_cfr::{PublicGame, PublicProfile};
use pk_core::{Card, Combo};
use pk_game::{Player, PostflopTree, Street};
use thiserror::Error;

use crate::dump::{
    Decision, DumpAction, DumpError, DumpNode, PathText, SOLVER_NAME, SolutionDump, SolverInfo,
    cards_that_can_fall, decisions, streets_from,
};
use crate::exploitability::{Exploitability, exploitability};
use crate::game::PostflopGame;
use crate::node_values::{NodeValues, node_values};
use crate::solution_file::SavedSolution;
use crate::solve::{Solution, SolveError};
use crate::spot::Spot;

/// How far a hand's weight in a dump may be from its weight in our range.
pub const WEIGHT_TOLERANCE: f64 = 1e-6;

/// The most differences an error's text lists.
const LISTED: usize = 30;

/// The most hands a [`Mismatch`]'s text names.
const NAMED: usize = 8;

/// Why a dump could not be mapped onto a spot's tree.
#[derive(Debug, Error)]
pub enum ImportError {
    /// The dump breaks the format.
    #[error("invalid dump: {0}")]
    Dump(#[from] DumpError),
    /// The spot's game cannot be built.
    #[error("{0}")]
    Spot(#[from] SolveError),
    /// The dump is valid, but answers another spot or another tree: every difference found at
    /// the first stage that has any.
    #[error("{}", MismatchList(.0))]
    Mismatch(Vec<Mismatch>),
}

impl ImportError {
    /// The differences, if the dump answers another spot or tree.
    pub fn mismatches(&self) -> &[Mismatch] {
        match self {
            ImportError::Mismatch(list) => list,
            _ => &[],
        }
    }
}

struct MismatchList<'a>(&'a [Mismatch]);

impl fmt::Display for MismatchList<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let count = self.0.len();
        let plural = if count == 1 { "" } else { "s" };
        write!(
            f,
            "the dump does not answer this spot: {count} difference{plural}"
        )?;
        for mismatch in self.0.iter().take(LISTED) {
            write!(f, "\n- {mismatch}")?;
        }
        if count > LISTED {
            write!(f, "\n- and {} more", count - LISTED)?;
        }
        Ok(())
    }
}

/// One way a valid dump differs from the spot it is mapped onto.
#[derive(Debug, Clone, PartialEq)]
pub enum Mismatch {
    /// A number of the spot differs: `field` is the dump's, such as `spot.pot`.
    Spot {
        field: &'static str,
        ours: String,
        theirs: String,
    },
    /// Each player's hands in the dump are the other player's hands of ours.
    SwappedPlayers,
    /// A player's hands differ: the hands of ours the dump lacks, and the dump's that are not
    /// ours, each with its weight.
    Hands {
        player: Player,
        missing: Vec<(Combo, f64)>,
        extra: Vec<(Combo, f64)>,
    },
    /// Hands both have, with different weights: ours, then the dump's.
    Weights {
        player: Player,
        hands: Vec<(Combo, f64, f64)>,
    },
    /// At a decision node of both, a different player acts.
    Player {
        path: Vec<DumpAction>,
        ours: Player,
        theirs: Player,
    },
    /// At a decision node of both, the actions differ.
    Actions {
        path: Vec<DumpAction>,
        player: Player,
        ours: Vec<DumpAction>,
        theirs: Vec<DumpAction>,
    },
    /// The dump has no node where we have a decision node.
    MissingNode {
        path: Vec<DumpAction>,
        player: Player,
    },
    /// Where our tree deals `street` at the end of `path`, the dump has no node after `cards`,
    /// in card order, of the `of` cards that can fall there, after each of which our tree has a
    /// decision.
    MissingCards {
        path: Vec<DumpAction>,
        street: Street,
        cards: Vec<Card>,
        of: usize,
    },
    /// The dump has a node where we have none. `ends_hand` is set when our tree has a
    /// terminal at that path, a fold or a showdown, rather than no node at all.
    ExtraNode {
        path: Vec<DumpAction>,
        player: Player,
        ends_hand: bool,
    },
}

fn hand_list(hands: &[(Combo, f64)]) -> String {
    let shown: Vec<String> = hands
        .iter()
        .take(NAMED)
        .map(|(combo, weight)| format!("{combo} ({weight})"))
        .collect();
    let more = if hands.len() > NAMED {
        format!(" and {} more", hands.len() - NAMED)
    } else {
        String::new()
    };
    format!("{}{more}", shown.join(", "))
}

fn action_list(actions: &[DumpAction]) -> String {
    let names: Vec<String> = actions.iter().map(DumpAction::to_string).collect();
    names.join(", ")
}

impl fmt::Display for Mismatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Mismatch::Spot {
                field,
                ours,
                theirs,
            } => write!(
                f,
                "`{field}` is {theirs} in the dump, and {ours} in our spot"
            ),
            Mismatch::SwappedPlayers => f.write_str(
                "the dump's OOP hands are our IP range and its IP hands our OOP range: the \
                 players are the wrong way round (OOP is the player who acts first)",
            ),
            Mismatch::Hands {
                player,
                missing,
                extra,
            } => {
                write!(f, "{player}'s hands differ:")?;
                if !missing.is_empty() {
                    write!(
                        f,
                        " the dump lacks {} of ours: {};",
                        missing.len(),
                        hand_list(missing)
                    )?;
                }
                if !extra.is_empty() {
                    write!(
                        f,
                        " the dump has {} that are not in our range on this board: {};",
                        extra.len(),
                        hand_list(extra)
                    )?;
                }
                write!(
                    f,
                    " a dump lists the hands of the range with a positive weight that miss the \
                     board"
                )
            }
            Mismatch::Weights { player, hands } => {
                let shown: Vec<String> = hands
                    .iter()
                    .take(NAMED)
                    .map(|(combo, ours, theirs)| format!("{combo} {theirs} (ours {ours})"))
                    .collect();
                let more = if hands.len() > NAMED {
                    format!(" and {} more", hands.len() - NAMED)
                } else {
                    String::new()
                };
                write!(
                    f,
                    "{} of {player}'s hands have other weights in the dump: {}{more}",
                    hands.len(),
                    shown.join(", ")
                )
            }
            Mismatch::Player { path, ours, theirs } => write!(
                f,
                "at {}, {theirs} acts in the dump and {ours} in our tree",
                PathText(path)
            ),
            Mismatch::Actions {
                path,
                player,
                ours,
                theirs,
            } => {
                let only = |a: &[DumpAction], b: &[DumpAction]| -> Vec<DumpAction> {
                    a.iter().filter(|x| !b.contains(x)).copied().collect()
                };
                let (ours_only, theirs_only) = (only(ours, theirs), only(theirs, ours));
                write!(
                    f,
                    "at {} ({player} to act), our tree offers {} and the dump offers {}",
                    PathText(path),
                    action_list(ours),
                    action_list(theirs)
                )?;
                if !ours_only.is_empty() {
                    write!(f, "; only ours has {}", action_list(&ours_only))?;
                }
                if !theirs_only.is_empty() {
                    write!(f, "; only the dump has {}", action_list(&theirs_only))?;
                }
                Ok(())
            }
            Mismatch::MissingNode { path, player } => write!(
                f,
                "at {}, our tree has a decision of {player}'s and the dump has no node",
                PathText(path)
            ),
            Mismatch::MissingCards {
                path,
                street,
                cards,
                of,
            } => {
                write!(
                    f,
                    "after {}, our tree deals the {street} and has a decision after each of the \
                     {of} cards that can fall there; the dump has no node after ",
                    PathText(path)
                )?;
                if cards.len() == *of {
                    write!(f, "any of them")?;
                } else {
                    let shown: Vec<String> =
                        cards.iter().take(NAMED).map(Card::to_string).collect();
                    let more = if cards.len() > NAMED {
                        format!(" and {} more", cards.len() - NAMED)
                    } else {
                        String::new()
                    };
                    write!(f, "{} of them: {}{more}", cards.len(), shown.join(", "))?;
                }
                f.write_str(
                    "; a dump covers each street it holds in full, after every card that can \
                     fall",
                )
            }
            Mismatch::ExtraNode {
                path,
                player,
                ends_hand,
            } => {
                write!(
                    f,
                    "at {}, the dump has a decision of {player}'s, and our tree ",
                    PathText(path)
                )?;
                if *ends_hand {
                    f.write_str("ends the hand there, with a fold or a showdown")
                } else {
                    f.write_str("has no node there")
                }
            }
        }
    }
}

/// A solution on our tree for a spot, ours or mapped from a dump, with what our code measures
/// of its strategy and what its solver reported. A [comparison](mod@crate::compare) is made
/// between two of these.
#[derive(Debug, Clone)]
pub struct MappedSolution {
    game: PostflopGame,
    strategy: PublicProfile,
    /// By public node: whether the solution holds the strategy there. Always false off a
    /// player node.
    held: Vec<bool>,
    /// How many streets it holds, from the spot's first.
    streets: usize,
    complete: bool,
    solver: SolverInfo,
    reported_values: Option<[f64; 2]>,
    reported_evs: Option<NodeValues>,
    exploitability: Option<Exploitability>,
    measured_evs: NodeValues,
}

impl MappedSolution {
    /// One of our solutions, which needs no mapping. A solution from a solve is complete and
    /// holds every player node. Its reported numbers are those the solve gives, and its
    /// reported EVs are the EVs our code measures, as its dump would hold.
    ///
    /// A solution loaded from a [solution file](crate::solution_file) without its last streets
    /// is mapped as a partial dump is: it holds the player nodes of its streets, its strategy is
    /// uniform below them, its exploitability is not measured, its reported values are those
    /// its file recorded, it reports no EVs, and its EVs are measured only at the nodes whose
    /// whole subtree it holds. [`MappedSolution::from_saved`] maps it with the EVs its file
    /// holds.
    pub fn from_solution(solution: &Solution) -> MappedSolution {
        MappedSolution::from_ours(solution, None)
    }

    /// One of our solutions read from a [solution file](crate::solution_file), whose reported
    /// EVs are the EVs the file holds, at every player node the solution holds, `None` where
    /// the file holds a NaN. Otherwise as [`MappedSolution::from_solution`].
    ///
    /// So a file without its river reports EVs on the streets it holds, as a partial dump does,
    /// and they are compared with another solution's there; they are the EVs of its whole
    /// strategy, river included, measured when the file was written. Its exploitability and
    /// values are still not measured here: those it reports are its file's, measured on the
    /// whole strategy when it was written. A complete file reports its file's EVs, which are
    /// those `from_solution` measures again, rounded to `f32`.
    pub fn from_saved(saved: &SavedSolution) -> MappedSolution {
        let solution = saved.solution();
        let evs = (0..solution.game().num_nodes() as u32)
            .map(|node| {
                let evs = saved.ev(node)?;
                Some(
                    evs.iter()
                        .map(|&ev| (!ev.is_nan()).then_some(f64::from(ev)))
                        .collect(),
                )
            })
            .collect();
        MappedSolution::from_ours(solution, Some(NodeValues::from_nodes(evs)))
    }

    /// One of our solutions, reporting `reported_evs`, or for a complete solution with none
    /// given the EVs our code measures, as [`MappedSolution::from_solution`] describes.
    fn from_ours(solution: &Solution, reported_evs: Option<NodeValues>) -> MappedSolution {
        let solver = SolverInfo {
            name: SOLVER_NAME.to_string(),
            iterations: Some(solution.iterations()),
            exploitability: Some(solution.exploitability().chips),
        };
        if !solution.is_complete() {
            return MappedSolution::from_partial(solution, solver, reported_evs);
        }
        let game = solution.game().clone();
        let strategy = solution.strategy().clone();
        let measured_evs = node_values(&game, &strategy);
        let held = (0..game.num_nodes())
            .map(|node| game.actor(node as u32).is_some())
            .collect();
        MappedSolution {
            exploitability: Some(*solution.exploitability()),
            reported_values: Some(solution.values()),
            reported_evs: Some(reported_evs.unwrap_or_else(|| measured_evs.clone())),
            measured_evs,
            held,
            streets: streets_from(game.board(PostflopTree::ROOT).len()),
            complete: true,
            solver,
            game,
            strategy,
        }
    }

    /// [`MappedSolution::from_solution`] for a solution that does not hold every street, which
    /// reports `reported_evs`, if any.
    fn from_partial(
        solution: &Solution,
        solver: SolverInfo,
        reported_evs: Option<NodeValues>,
    ) -> MappedSolution {
        let game = solution.game().clone();
        let held: Vec<bool> = (0..game.num_nodes() as u32)
            .map(|node| game.actor(node).is_some() && solution.holds(node))
            .collect();
        let strategy = PublicProfile::from_fn(&game, |node, hand| {
            solution
                .hand_strategy(node as u32, hand)
                .unwrap_or_else(|| {
                    let width = game.children(node as u32).len();
                    vec![1.0 / width as f32; width]
                })
        });
        let needed: Vec<bool> = (0..game.num_nodes() as u32)
            .map(|node| game.actor(node).is_some())
            .collect();
        let whole = measurable(&game, &held, &needed);
        let measured = node_values(&game, &strategy);
        let measured_evs = NodeValues::from_nodes(
            (0..game.num_nodes() as u32)
                .map(|node| {
                    let evs = measured.get(node)?;
                    whole[node as usize].then(|| evs.to_vec())
                })
                .collect(),
        );
        let first = solution.spot().street();
        let streets = Street::ALL
            .iter()
            .filter(|&&street| first <= street && street <= solution.last_street())
            .count();
        MappedSolution {
            held,
            streets,
            complete: false,
            solver,
            reported_values: Some(solution.values()),
            reported_evs,
            exploitability: None,
            measured_evs,
            strategy,
            game,
        }
    }

    /// Checks `dump` against `spot` as the [module documentation](self) describes, and maps it
    /// onto the spot's tree.
    pub fn from_dump(dump: &SolutionDump, spot: &Spot) -> Result<MappedSolution, ImportError> {
        dump.validate()?;
        let game = PostflopGame::from_spot(spot)?;
        for stage in [check_spot, check_hands] {
            let mismatches = stage(dump, &game);
            if !mismatches.is_empty() {
                return Err(ImportError::Mismatch(mismatches));
            }
        }
        let ours = decisions(&game);
        let mismatches = check_tree(dump, &game, &ours);
        if !mismatches.is_empty() {
            return Err(ImportError::Mismatch(mismatches));
        }
        Ok(map(dump, game, &ours))
    }

    /// The game the strategy is on: the spot's.
    pub fn game(&self) -> &PostflopGame {
        &self.game
    }

    /// The strategy, on our tree and hand indices: the solution's at every node it holds, and
    /// uniform elsewhere.
    pub fn strategy(&self) -> &PublicProfile {
        &self.strategy
    }

    /// Whether the solution holds the strategy at the public node `node`: at every player node
    /// of ours, and of a dump at each node it lists. False off a player node, and false out of
    /// range.
    pub fn holds(&self, node: u32) -> bool {
        self.held.get(node as usize).copied().unwrap_or(false)
    }

    /// How many streets the solution holds, counted from the spot's first.
    pub fn streets(&self) -> usize {
        self.streets
    }

    /// Whether the solution holds every decision node of the game: always for one of ours, and
    /// for a dump that says it is complete.
    pub fn is_complete(&self) -> bool {
        self.complete
    }

    /// The solver that made the strategy, with its own iterations and exploitability.
    pub fn solver(&self) -> &SolverInfo {
        &self.solver
    }

    /// Each player's value as the solver reported it, if it did.
    pub fn reported_values(&self) -> Option<[f64; 2]> {
        self.reported_values
    }

    /// The EVs as the solver reported them, if it did, at the nodes it holds.
    pub fn reported_evs(&self) -> Option<&NodeValues> {
        self.reported_evs.as_ref()
    }

    /// The strategy's exploitability and values, measured by our code in our game, or `None`
    /// for a partial solution, whose strategy below its last street is not known.
    pub fn exploitability(&self) -> Option<&Exploitability> {
        self.exploitability.as_ref()
    }

    /// The EVs of the strategy, measured by our code in our game
    /// ([`node_values`](mod@crate::node_values)), at the nodes whose whole subtree the solution
    /// holds: every player node of a complete solution.
    pub fn measured_evs(&self) -> &NodeValues {
        &self.measured_evs
    }
}

/// Stage 2: the board, the pot, the stack and the rake.
fn check_spot(dump: &SolutionDump, game: &PostflopGame) -> Vec<Mismatch> {
    let mut mismatches = Vec::new();
    let theirs = &dump.spot;
    let board = game.board_hands().board();
    if theirs.board != board {
        mismatches.push(Mismatch::Spot {
            field: "spot.board",
            ours: board.to_string(),
            theirs: theirs.board.to_string(),
        });
    }
    let chips = |field: &'static str, ours: u32, theirs: u32, list: &mut Vec<Mismatch>| {
        if ours == theirs {
            return;
        }
        let hint = if u64::from(theirs) * 100 == u64::from(ours) {
            "; a dump's amounts are in chips, with 1bb = 100 chips"
        } else {
            ""
        };
        list.push(Mismatch::Spot {
            field,
            ours: format!("{ours} chips"),
            theirs: format!("{theirs} chips{hint}"),
        });
    };
    chips("spot.pot", game.pot(), theirs.pot, &mut mismatches);
    let stack = game.tree().spec().stack;
    chips(
        "spot.effective_stack",
        stack,
        theirs.effective_stack,
        &mut mismatches,
    );
    let rake = game.rake();
    if (rake.percent - theirs.rake_percent).abs() > 1e-9 {
        let hint = if (theirs.rake_percent * 100.0 - rake.percent).abs() < 1e-9 {
            "; it is a percent, 5 for 5%"
        } else {
            ""
        };
        mismatches.push(Mismatch::Spot {
            field: "spot.rake.percent",
            ours: rake.percent.to_string(),
            theirs: format!("{}{hint}", theirs.rake_percent),
        });
    }
    if rake.percent > 0.0 {
        chips("spot.rake.cap", rake.cap, theirs.rake_cap, &mut mismatches);
    }
    mismatches
}

/// Stage 3: each player's hands and weights.
fn check_hands(dump: &SolutionDump, game: &PostflopGame) -> Vec<Mismatch> {
    let ours = [Player::Oop, Player::Ip].map(|player| {
        let list = game.hands(player);
        list.combos()
            .iter()
            .copied()
            .zip(list.weights().iter().copied())
            .collect::<Vec<_>>()
    });
    let set = |hands: &[(Combo, f64)]| -> HashSet<Combo> { hands.iter().map(|h| h.0).collect() };
    let (our_sets, their_sets) = (
        [set(&ours[0]), set(&ours[1])],
        [set(&dump.spot.hands[0]), set(&dump.spot.hands[1])],
    );
    if our_sets[0] != our_sets[1] && their_sets[0] == our_sets[1] && their_sets[1] == our_sets[0] {
        return vec![Mismatch::SwappedPlayers];
    }
    let mut mismatches = Vec::new();
    for player in [Player::Oop, Player::Ip] {
        let i = player.index();
        let theirs: HashMap<Combo, f64> = dump.spot.hands[i].iter().copied().collect();
        let mut missing = Vec::new();
        let mut weights = Vec::new();
        for &(combo, weight) in &ours[i] {
            match theirs.get(&combo) {
                None => missing.push((combo, weight)),
                Some(&other) if (other - weight).abs() > WEIGHT_TOLERANCE => {
                    weights.push((combo, weight, other));
                }
                Some(_) => {}
            }
        }
        let mut extra: Vec<(Combo, f64)> = dump.spot.hands[i]
            .iter()
            .filter(|(combo, _)| !our_sets[i].contains(combo))
            .copied()
            .collect();
        extra.sort_by_key(|(combo, _)| *combo);
        if !missing.is_empty() || !extra.is_empty() {
            mismatches.push(Mismatch::Hands {
                player,
                missing,
                extra,
            });
        }
        if !weights.is_empty() {
            mismatches.push(Mismatch::Weights {
                player,
                hands: weights,
            });
        }
    }
    mismatches
}

/// Whether the dump is expected to list our decision node: one on a street it holds, below no
/// card that cannot fall.
fn expected(decision: &Decision, streets: usize) -> bool {
    decision.street() < streets && !decision.dead
}

/// The decision nodes that come next below the public node `node`: itself if it is one, and
/// below a deal the first decision after each card. None below a terminal, or below an all-in
/// called before the river, whose cards run out with no decision.
fn next_decisions(game: &PostflopGame, node: u32, out: &mut Vec<u32>) {
    if game.actor(node).is_some() {
        out.push(node);
        return;
    }
    for &child in game.children(node) {
        next_decisions(game, child, out);
    }
}

/// Stage 4: the tree, walked from the root. `ours` is every decision node of our public tree.
fn check_tree(dump: &SolutionDump, game: &PostflopGame, ours: &[Decision]) -> Vec<Mismatch> {
    let streets = dump.streets();
    let by_path: HashMap<&[DumpAction], &Decision> =
        ours.iter().map(|d| (d.path.as_slice(), d)).collect();
    let theirs: HashMap<&[DumpAction], usize> = dump
        .nodes
        .iter()
        .enumerate()
        .map(|(i, node)| (node.path.as_slice(), i))
        .collect();
    let mut mismatches = Vec::new();
    let mut visited = vec![false; dump.nodes.len()];
    // Paths below which the dump's nodes are not reported again: those under an action only
    // the dump has, under a node where another player acts, and under a node it lacks.
    let mut covered: Vec<Vec<DumpAction>> = Vec::new();
    // Our decision nodes the walk reaches: the root, and those below an action both have.
    let mut reached: HashSet<u32> = HashSet::from([PostflopTree::ROOT]);
    // For each deal where the dump lacks the node after some cards, the index of its mismatch.
    let mut deals: HashMap<Vec<DumpAction>, usize> = HashMap::new();
    let mut below = Vec::new();
    for decision in ours {
        if !reached.contains(&decision.node) || !expected(decision, streets) {
            continue;
        }
        let Some(&index) = theirs.get(decision.path.as_slice()) else {
            covered.push(decision.path.clone());
            let Some((&DumpAction::Deal(card), line)) = decision.path.split_last() else {
                mismatches.push(Mismatch::MissingNode {
                    path: decision.path.clone(),
                    player: decision.player,
                });
                continue;
            };
            match deals.get(line) {
                Some(&at) => {
                    if let Mismatch::MissingCards { cards, .. } = &mut mismatches[at] {
                        cards.push(card);
                    }
                }
                None => {
                    let (chance, _) = *game
                        .path(decision.node)
                        .last()
                        .expect("a node after a deal has a chance node above it");
                    let board = game.board(chance);
                    let hands = [Player::Oop, Player::Ip].map(|p| game.hands(p).combos());
                    deals.insert(line.to_vec(), mismatches.len());
                    mismatches.push(Mismatch::MissingCards {
                        path: line.to_vec(),
                        street: game.street(chance),
                        cards: vec![card],
                        of: cards_that_can_fall(hands, board).len(),
                    });
                }
            }
            continue;
        };
        visited[index] = true;
        let node = &dump.nodes[index];
        if node.player != decision.player {
            mismatches.push(Mismatch::Player {
                path: decision.path.clone(),
                ours: decision.player,
                theirs: node.player,
            });
            covered.push(decision.path.clone());
            continue;
        }
        let same = decision.actions.len() == node.actions.len()
            && decision.actions.iter().all(|a| node.actions.contains(a));
        if !same {
            mismatches.push(Mismatch::Actions {
                path: decision.path.clone(),
                player: decision.player,
                ours: decision.actions.clone(),
                theirs: node.actions.clone(),
            });
            for action in node
                .actions
                .iter()
                .filter(|a| !decision.actions.contains(a))
            {
                let mut below = decision.path.clone();
                below.push(*action);
                covered.push(below);
            }
        }
        let children = game.children(decision.node);
        for (action, &child) in decision.actions.iter().zip(children) {
            if node.actions.contains(action) {
                below.clear();
                next_decisions(game, child, &mut below);
                reached.extend(below.iter().copied());
            }
        }
    }
    // The dump's nodes the walk did not reach, other than those under a difference already
    // reported, each at the shallowest path. A path is under a covered one when one of its
    // prefixes, itself included, is covered.
    let covered: HashSet<&[DumpAction]> = covered.iter().map(Vec::as_slice).collect();
    let unreached: Vec<&DumpNode> = dump
        .nodes
        .iter()
        .zip(&visited)
        .filter(|&(node, &seen)| {
            !seen && (0..=node.path.len()).all(|len| !covered.contains(&node.path[..len]))
        })
        .map(|(node, _)| node)
        .collect();
    let paths: HashSet<&[DumpAction]> = unreached.iter().map(|n| n.path.as_slice()).collect();
    let mut extra: Vec<&DumpNode> = unreached
        .into_iter()
        .filter(|node| (0..node.path.len()).all(|len| !paths.contains(&node.path[..len])))
        .collect();
    extra.sort_by(|a, b| a.path.cmp(&b.path));
    for node in extra {
        // Our tree ends the hand there if the parent is one of our decision nodes, the last
        // action is one of its actions, and what follows it is not a decision.
        let ends_hand = !by_path.contains_key(node.path.as_slice())
            && node.path.split_last().is_some_and(|(last, parent)| {
                by_path
                    .get(parent)
                    .is_some_and(|decision| decision.actions.contains(last))
            });
        mismatches.push(Mismatch::ExtraNode {
            path: node.path.clone(),
            player: node.player,
            ends_hand,
        });
    }
    mismatches
}

/// Maps a dump that has passed every check onto the game's tree.
fn map(dump: &SolutionDump, game: PostflopGame, ours: &[Decision]) -> MappedSolution {
    let streets = dump.streets();
    let theirs: HashMap<&[DumpAction], &DumpNode> = dump
        .nodes
        .iter()
        .map(|node| (node.path.as_slice(), node))
        .collect();
    // For each of our player nodes the dump lists: the dump's node, and for each of our
    // actions the index of the same action in the dump's.
    let mut matched: Vec<Option<(&DumpNode, Vec<usize>)>> = vec![None; game.num_nodes()];
    // Whether each public node is a player node that some hand dealt can reach: one below no
    // card that cannot fall, whose strategy a measurement needs.
    let mut needed = vec![false; game.num_nodes()];
    for decision in ours {
        needed[decision.node as usize] = !decision.dead;
        if !expected(decision, streets) {
            continue;
        }
        let node = theirs[decision.path.as_slice()];
        let order = decision
            .actions
            .iter()
            .map(|action| {
                node.actions
                    .iter()
                    .position(|a| a == action)
                    .expect("the tree check matched every action")
            })
            .collect();
        matched[decision.node as usize] = Some((node, order));
    }
    let rows: Vec<Option<HashMap<Combo, &[f64]>>> = matched
        .iter()
        .map(|m| {
            m.as_ref().map(|(node, _)| {
                node.strategy
                    .iter()
                    .map(|(combo, row)| (*combo, row.as_slice()))
                    .collect()
            })
        })
        .collect();
    let strategy = PublicProfile::from_fn(&game, |node, hand| {
        let width = game.children(node as u32).len();
        // As a solve leaves a hand no one plays to the node with: see `pk_cfr::regret`.
        let uniform = || vec![1.0 / width as f32; width];
        let Some((dump_node, order)) = matched[node].as_ref() else {
            return uniform();
        };
        let combo = game.hands(dump_node.player).combos()[hand];
        match rows[node]
            .as_ref()
            .expect("a matched node has rows")
            .get(&combo)
        {
            Some(row) => order.iter().map(|&i| row[i] as f32).collect(),
            // A hand that holds a card dealt on the way.
            None => uniform(),
        }
    });
    let held: Vec<bool> = matched.iter().map(Option::is_some).collect();
    let reported_evs = dump.nodes[0].ev.is_some().then(|| {
        let nodes = matched
            .iter()
            .map(|m| {
                m.as_ref().map(|(node, _)| {
                    let evs: HashMap<Combo, Option<f64>> = node
                        .ev
                        .as_ref()
                        .expect("every node has EVs")
                        .iter()
                        .copied()
                        .collect();
                    game.hands(node.player)
                        .combos()
                        .iter()
                        .map(|combo| evs.get(combo).copied().flatten())
                        .collect()
                })
            })
            .collect();
        NodeValues::from_nodes(nodes)
    });
    let measured = dump.complete.then(|| exploitability(&game, &strategy));
    let measured_evs = node_values(&game, &strategy);
    let measured_evs = if dump.complete {
        measured_evs
    } else {
        let whole = measurable(&game, &held, &needed);
        let nodes = (0..game.num_nodes() as u32)
            .map(|node| {
                let evs = measured_evs.get(node)?;
                whole[node as usize].then(|| evs.to_vec())
            })
            .collect();
        NodeValues::from_nodes(nodes)
    };
    MappedSolution {
        held,
        streets,
        complete: dump.complete,
        solver: dump.solver.clone(),
        reported_values: dump.values,
        reported_evs,
        exploitability: measured,
        measured_evs,
        strategy,
        game,
    }
}

/// By public node: whether the strategy is known at every player node of its subtree that
/// can be reached, so that the EVs there can be measured.
fn measurable(game: &PostflopGame, held: &[bool], needed: &[bool]) -> Vec<bool> {
    let mut whole = vec![true; game.num_nodes()];
    // Children come after their parent in pre-order, so a reverse pass sees them first.
    for node in (0..game.num_nodes()).rev() {
        let own = held[node] || !needed[node];
        whole[node] = own
            && game
                .children(node as u32)
                .iter()
                .all(|&child| whole[child as usize]);
    }
    whole
}

#[cfg(test)]
mod tests;
