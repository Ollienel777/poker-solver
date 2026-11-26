//! Comparing two solutions of the same spot, each ours or another solver's, complete or
//! partial, and a verdict against a set of tolerances.
//!
//! [`compare`] takes the spot and two [dumps](crate::dump), maps each onto the spot's tree,
//! which checks first that both answer it ([`crate::import`]), and compares them.
//! [`compare_mapped`] compares two solutions already on the tree. The result is a
//! [`Comparison`]: data, with a `Display` for a command line. The library prints nothing.
//!
//! # What is compared
//!
//! Every number below is computed by our code, in our game, from the two strategies, except
//! the EVs, values and exploitabilities a solver reports, which are compared both with the
//! other solution's and, where the strategy allows, with what its own strategy gives here. Call
//! the two solutions A and B.
//!
//! - **Exploitability.** Each strategy's exploitability, as
//!   [`exploitability`](mod@crate::exploitability) measures it. For a strategy from another
//!   solver this is the most trustworthy number: it does not depend on how that solver splits
//!   a mixed strategy among equivalent hands. A partial solution, which holds only the first
//!   streets of a flop or turn spot, cannot be measured, since its strategy on the later
//!   streets is not known: its solver's own figure is shown in its place, as reported and not
//!   measured, and the verdict holds that figure to the limit.
//! - **Values.** Each player's value at the root under each strategy, in chips per hand dealt,
//!   and the difference: measured, or for a partial solution as its solver reports them.
//! - **Nodes.** At every decision node both solutions hold: its reach, the probability that a
//!   hand dealt plays to it, under each strategy; and each action's frequency over the acting
//!   player's range there, under each. The frequencies are summed up, on each street, by the
//!   largest difference over the nodes checked (below) and by the mean absolute difference over
//!   every node both strategies reach and every action there, each node weighted by the mean
//!   of its two reaches, so that nodes almost never reached count for little.
//! - **EVs.** Where a solution has EVs, at every decision node the acting player's EV over its
//!   range there: the mean of its hands' EVs, over the hands whose EV is given. An EV a dump
//!   writes as `null` is no EV, never zero.
//! - **Hand classes at the root.** For each of the 169 classes in the range of the player who
//!   acts at the root, its share of the range, each action's frequency over its combos, and its
//!   EV, under each strategy. This is what a person compares by eye in two range views.
//!
//! Every frequency, share and EV over a range or a class is weighted as a solver's range view
//! weighs it: each hand by its range weight times the acting player's own reach of the node,
//! with no card removal by the opponent's range ([`PostflopGame::range_view_frequencies`]).
//! That is what a person sees in another solver. It is not the probability of each action once
//! the node is reached, [`PostflopGame::action_frequencies`], which weighs each hand also by
//! the opponent's hands it can meet there, and which differs from it by up to several points
//! (see [`crate::game`]). Both sides are summed by the same functions, here, from their per-hand
//! strategies and EVs, so the convention can never differ between them: a dump carries no
//! aggregate of its own. Below a card, the hands that hold it are not in the range.
//!
//! # Which nodes are checked
//!
//! A node is *checked* when either strategy reaches it more often than
//! [`Tolerances::min_reach`] **given the cards dealt on its path**, and both reach it at all.
//! That is its reach divided by the probability that those cards fall
//! ([`NodeComparison::reach_given_cards`]): how often play takes the node's line on that
//! runout. On the spot's first street no card has been dealt, and it is the reach itself, so a
//! river spot is checked exactly as before. Below a deal the raw reach would not do: a node
//! after one particular turn card is reached at most 1/45 of the time, so a threshold of 1% on
//! it would check almost nothing, and a strategy that is right on the flop and wrong after
//! every turn card would pass. Given its cards, each runout's subtree is a game of its own,
//! whose nodes are weighed as a river spot's are, and a line taken a third of the time on that
//! runout is checked whatever the card.
//!
//! # The verdict
//!
//! A [`Tolerances`] sets the limits, and the [`Comparison`] lists each [`Failure`]. The checks
//! are:
//!
//! - [`Check::Exploitability`]: each strategy's exploitability is at most
//!   [`Tolerances::exploitability`] percent of the pot: measured, or for a partial solution as
//!   its solver reports it ([`Failure::ReportedExploitability`]).
//! - [`Check::Value`]: each player's two values at the root are within [`Tolerances::value`]
//!   percent of the pot.
//! - [`Check::Frequency`]: at every checked node, each action's two frequencies are within
//!   [`Tolerances::frequency`] on the spot's first street, and within
//!   [`Tolerances::frequency_below_deals`] below one or two deals.
//! - [`Check::Ev`], when both solutions have EVs: at the root, the two EVs of each class that
//!   holds more than [`Tolerances::min_reach`] of the range are within [`Tolerances::ev`]
//!   percent of the pot.
//! - [`Check::ReportedEv`], for each solution that reports EVs or values: the EVs it reports
//!   are within [`Tolerances::reported_ev`] percent of the pot of those our code measures for
//!   its own strategy, over the range at each node it reaches more often than
//!   [`Tolerances::min_reach`] given its cards, and for each class at the root, and so are the
//!   values it reports. For a partial solution this is made only where its strategy is known
//!   below the node, so that the EV can be measured.
//!
//! What cannot be checked is listed in [`Comparison::unchecked`], and the text says it plainly:
//! a partial solution's exploitability, and its values where it reports none.
//!
//! The two EV checks do different jobs. Reported against measured compares two sets of EVs
//! of one strategy, so it can be tight: it catches EVs on another baseline, in other units,
//! or of another strategy, wherever they show, even when the strategies agree. One solution's
//! EVs against the other's compare two strategies, and are only as close as the strategies'
//! convergence allows, so the check is loose and made where the weights are the same on both
//! sides: at the root, where every hand's own reach is 1. Over the range at a deeper node, the
//! two EVs are weighed by two different own reaches, and differ by several percent of the pot
//! between two sound solutions, so they are reported and not checked.
//!
//! The defaults for exploitability, values, frequencies on the first street and the reach of a
//! checked node are the tolerance of the M3 gate (DECISIONS D20), which has both solvers at
//! 0.03% of the pot or better: an exploitability under 0.3% of the pot, frequencies within 2
//! points at every node reached more than 1% of the time, and values within 0.2% of the pot.
//! The gate sets no limit for EVs. [`Tolerances::ev`] defaults to 5% of the pot and
//! [`Tolerances::reported_ev`] to 0.1%, from the measurements of the slow test
//! `honest_solvers_table` in `tests/compare.rs`. [`Tolerances::frequency_below_deals`], which a
//! river spot never uses, defaults to the limits of the M4 gate (DECISIONS D21), which were
//! set from the measurements of the slow test `honest_solvers_table_by_street` in
//! `tests/compare_streets.rs`: see its documentation.
//!
//! Single combos are not compared: where several equilibria exist, two correct solvers can
//! split a mixed strategy differently among equivalent hands.

use std::collections::HashMap;
use std::fmt;

use pk_cfr::{PublicGame, PublicProfile};
use pk_core::{CardSet, HandClass};
use pk_game::{Chips, Player, PostflopTree, Street};

use crate::dump::{DumpAction, PathText, SolutionDump, SolverInfo, decisions, streets_from};
use crate::exploitability::Exploitability;
use crate::game::PostflopGame;
use crate::import::{ImportError, MappedSolution};
use crate::node_values::NodeValues;
use crate::spot::Spot;

/// The limits of a comparison's verdict. [`Tolerances::default`] gives those of the M3 gate,
/// and for the nodes below a deal, which a river spot does not have, those of the M4 gate,
/// set from the honest solvers' measurements: see the [module documentation](self#the-verdict).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tolerances {
    /// The most exploitable either strategy may be, in percent of the pot. Defaults to 0.3.
    pub exploitability: f64,
    /// How far apart each player's values may be, in percent of the pot. Defaults to 0.2.
    pub value: f64,
    /// How far apart an action's frequencies may be at a checked node on the spot's first
    /// street, as a probability: on a river spot, at every checked node. Defaults to 0.02, 2
    /// percentage points.
    pub frequency: f64,
    /// A node is checked when either strategy reaches it with more than this probability,
    /// given the cards dealt on its path, and a class at the root when it holds more than this
    /// share of the range. Defaults to 0.01.
    pub min_reach: f64,
    /// How far apart the two solutions' EVs of a class at the root may be, in percent of the
    /// pot. Defaults to 5.
    pub ev: f64,
    /// How far the EVs and values a solution reports may be from those our code measures for
    /// its strategy, in percent of the pot. Defaults to 0.1.
    pub reported_ev: f64,
    /// How far apart an action's frequencies may be at a checked node below one deal (`[0]`:
    /// the turn of a flop spot, the river of a turn spot) and below two (`[1]`: the river of a
    /// flop spot), as a probability. Not part of the M3 gate, and never used on a river spot.
    /// Defaults to 0.10 and 0.25, 10 and 25 points: proposed from what two honest solves at
    /// 0.03% of the pot differ by, as the slow test `honest_solvers_table_by_street` measures
    /// it (DECISIONS P36), and confirmed at the M4 gate (D21).
    pub frequency_below_deals: [f64; 2],
}

impl Default for Tolerances {
    fn default() -> Tolerances {
        Tolerances {
            exploitability: 0.3,
            value: 0.2,
            frequency: 0.02,
            min_reach: 0.01,
            ev: 5.0,
            reported_ev: 0.1,
            frequency_below_deals: [0.10, 0.25],
        }
    }
}

impl Tolerances {
    /// The frequency limit at a node below `deals` deals: [`Tolerances::frequency`] on the
    /// spot's first street, and [`Tolerances::frequency_below_deals`] below.
    pub fn frequency_at(&self, deals: usize) -> f64 {
        match deals {
            0 => self.frequency,
            1 => self.frequency_below_deals[0],
            _ => self.frequency_below_deals[1],
        }
    }
}

/// The kinds of check of the [verdict](self#the-verdict).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Check {
    Exploitability,
    Value,
    Frequency,
    Ev,
    ReportedEv,
}

impl fmt::Display for Check {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Check::Exploitability => "exploitability",
            Check::Value => "value",
            Check::Frequency => "frequency",
            Check::Ev => "EV",
            Check::ReportedEv => "reported EV",
        })
    }
}

/// Solution A or B, as an index into a [`Comparison`]'s arrays.
pub type Side = usize;

fn side_name(side: Side) -> char {
    if side == 0 { 'A' } else { 'B' }
}

/// One check a comparison fails. Percentages are of the pot at the root; frequencies are
/// probabilities.
#[derive(Debug, Clone, PartialEq)]
pub enum Failure {
    /// A strategy is more exploitable than allowed, as our code measures it.
    Exploitability { side: Side, percent_of_pot: f64 },
    /// A partial solution's solver reports it more exploitable than allowed. Its strategy
    /// cannot be measured here.
    ReportedExploitability { side: Side, percent_of_pot: f64 },
    /// A player's values at the root under the two strategies are too far apart. `reported`
    /// says for each side whether its value is as its solver reports it, for a partial
    /// solution, rather than measured.
    Value {
        player: Player,
        values: [f64; 2],
        reported: [bool; 2],
        percent_of_pot: f64,
    },
    /// An action's frequencies at a checked node are too far apart.
    Frequency {
        node: u32,
        path: Vec<DumpAction>,
        action: DumpAction,
        frequencies: [f64; 2],
        reach: [f64; 2],
        /// The reach given the cards dealt on the path, which is `reach` on the first street.
        reach_given_cards: [f64; 2],
        /// The cards dealt on the path.
        deals: usize,
    },
    /// A class's EVs at the root are too far apart.
    ClassEv {
        class: HandClass,
        evs: [f64; 2],
        percent_of_pot: f64,
    },
    /// The EV a solution reports over the range at a node is too far from what its strategy
    /// gives in our game.
    ReportedNodeEv {
        side: Side,
        node: u32,
        path: Vec<DumpAction>,
        player: Player,
        reported: f64,
        measured: f64,
        percent_of_pot: f64,
    },
    /// The EV a solution reports for a class at the root is too far from what its strategy
    /// gives in our game.
    ReportedClassEv {
        side: Side,
        class: HandClass,
        reported: f64,
        measured: f64,
        percent_of_pot: f64,
    },
    /// The value a solution reports for a player is too far from what its strategy gives in
    /// our game.
    ReportedValue {
        side: Side,
        player: Player,
        reported: f64,
        measured: f64,
        percent_of_pot: f64,
    },
}

impl Failure {
    /// The check this fails.
    pub fn check(&self) -> Check {
        match self {
            Failure::Exploitability { .. } | Failure::ReportedExploitability { .. } => {
                Check::Exploitability
            }
            Failure::Value { .. } => Check::Value,
            Failure::Frequency { .. } => Check::Frequency,
            Failure::ClassEv { .. } => Check::Ev,
            Failure::ReportedNodeEv { .. }
            | Failure::ReportedClassEv { .. }
            | Failure::ReportedValue { .. } => Check::ReportedEv,
        }
    }

    /// The node it is at, if any.
    pub fn node(&self) -> Option<u32> {
        match self {
            Failure::Frequency { node, .. } | Failure::ReportedNodeEv { node, .. } => Some(*node),
            _ => None,
        }
    }
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: ", self.check())?;
        match self {
            Failure::Exploitability {
                side,
                percent_of_pot,
            } => write!(
                f,
                "{} is exploitable by {percent_of_pot:.3}% of the pot",
                side_name(*side)
            ),
            Failure::ReportedExploitability {
                side,
                percent_of_pot,
            } => write!(
                f,
                "{}'s solver reports it exploitable by {percent_of_pot:.3}% of the pot (a \
                 partial solution, which cannot be measured here)",
                side_name(*side)
            ),
            Failure::Value {
                player,
                values,
                reported,
                percent_of_pot,
            } => {
                let shown = |side: Side| {
                    let as_reported = if reported[side] { " as reported" } else { "" };
                    format!("{:.2} under {}{as_reported}", values[side], side_name(side))
                };
                write!(
                    f,
                    "{player}'s value is {} and {}, {percent_of_pot:.3}% of the pot apart",
                    shown(0),
                    shown(1)
                )
            }
            Failure::Frequency {
                path,
                action,
                frequencies,
                reach,
                reach_given_cards,
                deals,
                ..
            } => {
                write!(
                    f,
                    "at {} (reached {:.1}% / {:.1}%",
                    PathText(path),
                    100.0 * reach[0],
                    100.0 * reach[1]
                )?;
                if *deals > 0 {
                    write!(
                        f,
                        ", {:.1}% / {:.1}% given its cards",
                        100.0 * reach_given_cards[0],
                        100.0 * reach_given_cards[1]
                    )?;
                }
                write!(
                    f,
                    "), {action} is played {:.2}% under A and {:.2}% under B, {:.2} points apart",
                    100.0 * frequencies[0],
                    100.0 * frequencies[1],
                    100.0 * (frequencies[0] - frequencies[1]).abs()
                )
            }
            Failure::ClassEv {
                class,
                evs,
                percent_of_pot,
            } => write!(
                f,
                "at the root, {class}'s EV is {:.2} under A and {:.2} under B, \
                 {percent_of_pot:.3}% of the pot apart",
                evs[0], evs[1]
            ),
            Failure::ReportedNodeEv {
                side,
                path,
                player,
                reported,
                measured,
                percent_of_pot,
                ..
            } => write!(
                f,
                "at {}, {} reports {player}'s EV over its range as {reported:.2}, and its \
                 strategy gives {measured:.2} here, {percent_of_pot:.3}% of the pot apart",
                PathText(path),
                side_name(*side)
            ),
            Failure::ReportedClassEv {
                side,
                class,
                reported,
                measured,
                percent_of_pot,
            } => write!(
                f,
                "at the root, {} reports {class}'s EV as {reported:.2}, and its strategy gives \
                 {measured:.2} here, {percent_of_pot:.3}% of the pot apart",
                side_name(*side)
            ),
            Failure::ReportedValue {
                side,
                player,
                reported,
                measured,
                percent_of_pot,
            } => write!(
                f,
                "{} reports {player}'s value as {reported:.2}, and its strategy gives \
                 {measured:.2} here, {percent_of_pot:.3}% of the pot apart",
                side_name(*side)
            ),
        }
    }
}

/// A check the verdict could not make, or made only on a solver's own figure.
#[derive(Debug, Clone, PartialEq)]
pub enum Unchecked {
    /// A partial solution's exploitability, which cannot be measured without its strategy on
    /// the later streets. `reported` is its solver's figure in chips per hand dealt, which the
    /// verdict holds to the limit in its place, if there is one.
    Exploitability { side: Side, reported: Option<f64> },
    /// A partial solution's values at the root, which cannot be measured either. The verdict
    /// compares the values its solver reports, if it reports them; if not, the values are not
    /// compared.
    Values { side: Side, reported: bool },
}

/// One solution of a comparison: what its solver reported, and what our code measures.
#[derive(Debug, Clone, PartialEq)]
pub struct SolutionSummary {
    pub solver: SolverInfo,
    /// Each player's value as the solver reported it.
    pub reported_values: Option<[f64; 2]>,
    /// Whether the solution has EVs of its own.
    pub has_evs: bool,
    /// The exploitability and values of its strategy, measured by our code in our game, or
    /// `None` for a partial solution, which cannot be measured.
    pub measured: Option<Exploitability>,
    /// How many streets it holds, from the spot's first.
    pub streets: usize,
    /// Whether it holds every decision node.
    pub complete: bool,
}

impl SolutionSummary {
    /// The values the verdict compares: measured, or for a partial solution as its solver
    /// reports them, if it does.
    pub fn values(&self) -> Option<[f64; 2]> {
        match &self.measured {
            Some(measured) => Some(measured.values),
            None => self.reported_values,
        }
    }
}

/// One decision node under the two strategies. Arrays are indexed by [`Side`].
#[derive(Debug, Clone, PartialEq)]
pub struct NodeComparison {
    /// The node of our public tree.
    pub node: u32,
    pub path: Vec<DumpAction>,
    pub player: Player,
    pub actions: Vec<DumpAction>,
    /// The number of cards dealt on the path: 0 on the spot's first street.
    pub deals: usize,
    /// The probability that a hand dealt plays to the node, the chance of the cards dealt on
    /// the way included.
    pub reach: [f64; 2],
    /// The probability that a hand dealt plays to the node given that the cards dealt on its
    /// path fall: `reach` over the probability of those cards. On the first street it is
    /// `reach`; after a card that no pair of hands can see fall it is 0. This is what decides
    /// whether a node is [checked](self#which-nodes-are-checked).
    pub reach_given_cards: [f64; 2],
    /// Each action's frequency over the acting player's range as a range view shows it
    /// ([`PostflopGame::range_view_frequencies`]), in the order of `actions`, or `None` where
    /// the acting player never plays to the node.
    pub frequencies: [Option<Vec<f64>>; 2],
    /// The acting player's EV over its range, weighted as a range view weighs it, from the EVs
    /// the solution reports, if it has EVs and plays to the node.
    pub ev: [Option<f64>; 2],
    /// The same EV from the EVs our code measures for the solution's strategy, where it can be
    /// measured.
    pub measured_ev: [Option<f64>; 2],
}

impl NodeComparison {
    /// Whether the verdict checks the node: either strategy reaches it with more than
    /// `min_reach` given the cards dealt on its path, and both reach it at all.
    pub fn is_checked(&self, min_reach: f64) -> bool {
        self.reach_given_cards.iter().any(|&r| r > min_reach)
            && self.reach.iter().all(|&r| r > 0.0)
            && self.frequencies.iter().all(Option::is_some)
    }

    /// `|f_A(a) − f_B(a)|` for each action, if both strategies reach the node.
    pub fn differences(&self) -> Option<Vec<f64>> {
        let [Some(a), Some(b)] = &self.frequencies else {
            return None;
        };
        Some(a.iter().zip(b).map(|(x, y)| (x - y).abs()).collect())
    }

    /// The largest of [`NodeComparison::differences`], or 0.
    pub fn largest_difference(&self) -> f64 {
        self.differences()
            .map_or(0.0, |d| d.into_iter().fold(0.0, f64::max))
    }
}

/// The largest frequency difference over the checked nodes, and where it is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Largest {
    /// The node of our tree.
    pub node: u32,
    /// The index of the action in the node's actions.
    pub action: usize,
    pub difference: f64,
}

/// The frequency differences summed up over some nodes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrequencySummary {
    /// The number of nodes checked.
    pub checked: usize,
    /// The largest difference at a checked node, if any node is checked.
    pub largest: Option<Largest>,
    /// The mean of `|f_A(a) − f_B(a)|` over every node both strategies reach and every action
    /// there, each weighted by the mean of the node's two reaches.
    pub weighted_mean: f64,
}

/// The frequencies on one street, at the nodes both solutions hold there.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StreetSummary {
    pub street: Street,
    /// The cards dealt before it: 0 for the spot's first street.
    pub deals: usize,
    /// The decision nodes compared on it.
    pub nodes: usize,
    /// The frequency limit there.
    pub limit: f64,
    pub frequencies: FrequencySummary,
}

/// One hand class at the root under the two strategies. Arrays are indexed by [`Side`].
#[derive(Debug, Clone, PartialEq)]
pub struct ClassComparison {
    pub class: HandClass,
    /// The class's combos in the range on this board.
    pub combos: usize,
    /// The class's share of the range by weight, as a range view counts it.
    pub share: f64,
    /// Each action's frequency over the class's combos, weighted by range weight, in the order
    /// of the root's actions.
    pub frequencies: [Vec<f64>; 2],
    /// The class's EV as the solution reports it, if it has EVs and one is given.
    pub ev: [Option<f64>; 2],
    /// The same EV measured by our code for the solution's strategy, where it can be.
    pub measured_ev: [Option<f64>; 2],
}

impl ClassComparison {
    /// The largest `|f_A(a) − f_B(a)|` over the root's actions.
    pub fn largest_difference(&self) -> f64 {
        self.frequencies[0]
            .iter()
            .zip(&self.frequencies[1])
            .map(|(a, b)| (a - b).abs())
            .fold(0.0, f64::max)
    }
}

/// The comparison of two solutions of one spot: see the [module documentation](self).
#[derive(Debug, Clone, PartialEq)]
pub struct Comparison {
    /// The pot at the root, which percentages are of.
    pub pot: Chips,
    pub tolerances: Tolerances,
    /// The street the spot starts on.
    pub first_street: Street,
    /// A and B.
    pub solutions: [SolutionSummary; 2],
    /// Every decision node both solutions hold, in our tree's order.
    pub nodes: Vec<NodeComparison>,
    /// The frequencies over every node of `nodes`.
    pub frequencies: FrequencySummary,
    /// The frequencies on each street both solutions hold, the spot's first first.
    pub streets: Vec<StreetSummary>,
    /// The player who acts at the root.
    pub root_player: Player,
    /// The root's actions.
    pub root_actions: Vec<DumpAction>,
    /// The classes in the root player's range, in grid order.
    pub classes: Vec<ClassComparison>,
    /// Every check that fails, in the order of the checks.
    pub failures: Vec<Failure>,
    /// What the verdict could not check, or checked only on a solver's own figure.
    pub unchecked: Vec<Unchecked>,
}

impl Comparison {
    /// Whether every check passes. See [`Comparison::unchecked`] for what could not be checked.
    pub fn passes(&self) -> bool {
        self.failures.is_empty()
    }

    /// The failures of one check.
    pub fn failures_of(&self, check: Check) -> impl Iterator<Item = &Failure> + '_ {
        self.failures.iter().filter(move |f| f.check() == check)
    }

    /// Each player's value at the root under A and under B, `[side][player]`: measured, or for
    /// a partial solution as its solver reports them, if it does.
    pub fn values(&self) -> [Option<[f64; 2]>; 2] {
        [0, 1].map(|side| self.solutions[side].values())
    }

    /// `chips` as a percent of the pot.
    pub fn percent(&self, chips: f64) -> f64 {
        100.0 * chips / f64::from(self.pot)
    }
}

/// Maps `a` and `b` onto `spot`'s tree, checking that both answer it, and compares them.
pub fn compare(
    spot: &Spot,
    a: &SolutionDump,
    b: &SolutionDump,
    tolerances: &Tolerances,
) -> Result<Comparison, ImportError> {
    let a = MappedSolution::from_dump(a, spot)?;
    let b = MappedSolution::from_dump(b, spot)?;
    Ok(compare_mapped(&a, &b, tolerances))
}

/// Compares two solutions on the tree of one spot, at the nodes both hold.
///
/// Panics if they are not on the same game: the same tree, hands, pot and rake. Two trees are
/// the same when their nodes are, whatever node limit each was built under.
pub fn compare_mapped(
    a: &MappedSolution,
    b: &MappedSolution,
    tolerances: &Tolerances,
) -> Comparison {
    let game = a.game();
    let other = b.game();
    assert!(
        game.tree().nodes() == other.tree().nodes()
            && game.board_hands().board() == other.board_hands().board()
            && [Player::Oop, Player::Ip]
                .iter()
                .all(|&p| game.hands(p) == other.hands(p))
            && game.pot() == other.pot()
            && game.rake() == other.rake(),
        "the two solutions are not of the same spot"
    );
    let sides = [a, b];
    let pot = game.pot();
    let percent = |chips: f64| 100.0 * chips / f64::from(pot);
    let first_street = game.tree().spec().street;

    let solutions = sides.map(|s| SolutionSummary {
        solver: s.solver().clone(),
        reported_values: s.reported_values(),
        has_evs: s.reported_evs().is_some(),
        measured: s.exploitability().copied(),
        streets: s.streets(),
        complete: s.is_complete(),
    });

    let mut odds = CardOdds::new(game);
    let nodes: Vec<NodeComparison> = decisions(game)
        .into_iter()
        .filter(|decision| sides.iter().all(|s| s.holds(decision.node)))
        .map(|decision| {
            let at = sides.map(|s| At::new(game, s.strategy(), decision.node));
            let frequencies = [0, 1].map(|i| at[i].frequencies.clone());
            let ev = [0, 1].map(|i| sides[i].reported_evs().and_then(|evs| at[i].range_ev(evs)));
            let measured_ev = [0, 1].map(|i| at[i].range_ev(sides[i].measured_evs()));
            let reach = [at[0].reach, at[1].reach];
            let reach_given_cards = if decision.dealt.is_empty() {
                reach
            } else {
                let chance = odds.of(decision.node, decision.dealt);
                reach.map(|r| if chance > 0.0 { r / chance } else { 0.0 })
            };
            NodeComparison {
                node: decision.node,
                deals: decision.street(),
                path: decision.path,
                player: decision.player,
                actions: decision.actions,
                reach,
                reach_given_cards,
                frequencies,
                ev,
                measured_ev,
            }
        })
        .collect();

    let frequencies = summarize(nodes.iter(), tolerances.min_reach);
    let held = solutions.iter().map(|s| s.streets).min().unwrap_or(1);
    let streets = (0..held)
        .map(|deals| {
            let on: Vec<&NodeComparison> = nodes.iter().filter(|n| n.deals == deals).collect();
            StreetSummary {
                street: street_after(first_street, deals),
                deals,
                nodes: on.len(),
                limit: tolerances.frequency_at(deals),
                frequencies: summarize(on.into_iter(), tolerances.min_reach),
            }
        })
        .collect();
    let root = &nodes[0];
    debug_assert_eq!(root.node, PostflopTree::ROOT);
    let classes = root_classes(game, sides);

    let mut failures = Vec::new();
    let mut unchecked = Vec::new();
    for (side, solution) in solutions.iter().enumerate() {
        match &solution.measured {
            Some(measured) => {
                let measured = measured.percent_of_pot;
                if !within(measured, tolerances.exploitability) {
                    failures.push(Failure::Exploitability {
                        side,
                        percent_of_pot: measured,
                    });
                }
            }
            None => {
                let reported = solution.solver.exploitability;
                unchecked.push(Unchecked::Exploitability { side, reported });
                if let Some(chips) = reported {
                    let reported = percent(chips);
                    if !within(reported, tolerances.exploitability) {
                        failures.push(Failure::ReportedExploitability {
                            side,
                            percent_of_pot: reported,
                        });
                    }
                }
            }
        }
    }
    for (side, solution) in solutions.iter().enumerate() {
        if solution.measured.is_none() {
            unchecked.push(Unchecked::Values {
                side,
                reported: solution.reported_values.is_some(),
            });
        }
    }
    if let [Some(a_values), Some(b_values)] = [0, 1].map(|side| solutions[side].values()) {
        let reported = [0, 1].map(|side| solutions[side].measured.is_none());
        for player in [Player::Oop, Player::Ip] {
            let values = [a_values[player.index()], b_values[player.index()]];
            let apart = percent((values[0] - values[1]).abs());
            if !within(apart, tolerances.value) {
                failures.push(Failure::Value {
                    player,
                    values,
                    reported,
                    percent_of_pot: apart,
                });
            }
        }
    }
    for node in nodes.iter().filter(|n| n.is_checked(tolerances.min_reach)) {
        let (Some(a), Some(b)) = (&node.frequencies[0], &node.frequencies[1]) else {
            unreachable!("a checked node is reached by both");
        };
        let limit = tolerances.frequency_at(node.deals);
        for (action, (&fa, &fb)) in a.iter().zip(b).enumerate() {
            if !within((fa - fb).abs(), limit) {
                failures.push(Failure::Frequency {
                    node: node.node,
                    path: node.path.clone(),
                    action: node.actions[action],
                    frequencies: [fa, fb],
                    reach: node.reach,
                    reach_given_cards: node.reach_given_cards,
                    deals: node.deals,
                });
            }
        }
    }
    if solutions.iter().all(|s| s.has_evs) {
        for class in classes.iter().filter(|c| c.share > tolerances.min_reach) {
            if let [Some(ea), Some(eb)] = class.ev {
                let apart = percent((ea - eb).abs());
                if !within(apart, tolerances.ev) {
                    failures.push(Failure::ClassEv {
                        class: class.class,
                        evs: [ea, eb],
                        percent_of_pot: apart,
                    });
                }
            }
        }
    }
    for (side, solution) in solutions.iter().enumerate() {
        if solution.has_evs {
            let reached = nodes
                .iter()
                .filter(|n| n.reach_given_cards[side] > tolerances.min_reach);
            for node in reached {
                if let (Some(reported), Some(measured)) = (node.ev[side], node.measured_ev[side]) {
                    let apart = percent((reported - measured).abs());
                    if !within(apart, tolerances.reported_ev) {
                        failures.push(Failure::ReportedNodeEv {
                            side,
                            node: node.node,
                            path: node.path.clone(),
                            player: node.player,
                            reported,
                            measured,
                            percent_of_pot: apart,
                        });
                    }
                }
            }
            for class in &classes {
                if let (Some(reported), Some(measured)) = (class.ev[side], class.measured_ev[side])
                {
                    let apart = percent((reported - measured).abs());
                    if !within(apart, tolerances.reported_ev) {
                        failures.push(Failure::ReportedClassEv {
                            side,
                            class: class.class,
                            reported,
                            measured,
                            percent_of_pot: apart,
                        });
                    }
                }
            }
        }
        if let (Some(reported), Some(measured)) = (solution.reported_values, &solution.measured) {
            for player in [Player::Oop, Player::Ip] {
                let (reported, measured) =
                    (reported[player.index()], measured.values[player.index()]);
                let apart = percent((reported - measured).abs());
                if !within(apart, tolerances.reported_ev) {
                    failures.push(Failure::ReportedValue {
                        side,
                        player,
                        reported,
                        measured,
                        percent_of_pot: apart,
                    });
                }
            }
        }
    }

    let root_player = root.player;
    let root_actions = root.actions.clone();
    Comparison {
        pot,
        tolerances: *tolerances,
        first_street,
        solutions,
        nodes,
        frequencies,
        streets,
        root_player,
        root_actions,
        classes,
        failures,
        unchecked,
    }
}

/// The street `deals` cards after `first`.
fn street_after(first: Street, deals: usize) -> Street {
    (0..deals).fold(first, |street, _| street.next().unwrap_or(street))
}

/// Whether `x` is at most `limit`. False for NaN, so that a number that is not one fails.
fn within(x: f64, limit: f64) -> bool {
    x <= limit
}

/// The probability that the cards dealt on a node's path fall, which the reach of a node
/// below them is divided by to give its reach given the cards.
struct CardOdds<'a> {
    game: &'a PostflopGame,
    /// By the set of cards dealt: the deal probability of the pairs of hands that miss them.
    pairs: HashMap<CardSet, f64>,
}

impl CardOdds<'_> {
    fn new(game: &PostflopGame) -> CardOdds<'_> {
        CardOdds {
            game,
            pairs: HashMap::new(),
        }
    }

    /// The probability that the cards `dealt` on the way to `node` fall: the deal probability
    /// of the pairs of hands that miss them all, times the chance of each card at its chance
    /// node.
    fn of(&mut self, node: u32, dealt: CardSet) -> f64 {
        let game = self.game;
        let pairs = *self.pairs.entry(dealt).or_insert_with(|| {
            let missing = |player: Player| -> Vec<f64> {
                game.hands(player)
                    .combos()
                    .iter()
                    .zip(game.deal(player.index()))
                    .map(|(combo, &d)| if combo.intersects(dealt) { 0.0 } else { d })
                    .collect()
            };
            let (oop, ip) = (missing(Player::Oop), missing(Player::Ip));
            let mut met = vec![0.0; oop.len()];
            game.board_hands()
                .fold_values(Player::Oop, &ip, 1.0, &mut met);
            oop.iter().zip(&met).map(|(d, m)| d * m).sum()
        });
        let chance: f64 = game
            .path(node)
            .into_iter()
            .filter(|&(at, _)| !game.outcomes(at).is_empty())
            .map(|(at, outcome)| game.outcome_probability(at as usize, outcome))
            .product();
        pairs * chance
    }
}

/// One strategy at one decision node: the node's reach, and the acting player's range there
/// as a range view weighs it.
struct At {
    node: u32,
    /// The probability that a hand dealt plays to the node.
    reach: f64,
    /// [`PostflopGame::range_view_weights`], or `None` if the acting player never plays to the
    /// node.
    weights: Option<Vec<f64>>,
    /// [`PostflopGame::range_view_frequencies`].
    frequencies: Option<Vec<f64>>,
}

impl At {
    fn new(game: &PostflopGame, profile: &PublicProfile, node: u32) -> At {
        At {
            node,
            reach: game.reach_probability(profile, node),
            weights: game.range_view_weights(profile, node),
            frequencies: game.range_view_frequencies(profile, node),
        }
    }

    /// The EV over the range at the node, as a range view weighs it: the mean of the hands'
    /// EVs weighted by range weight and own reach, over the hands whose EV is given.
    fn range_ev(&self, evs: &NodeValues) -> Option<f64> {
        let weights = self.weights.as_ref()?;
        let evs = evs.get(self.node)?;
        let (mut total, mut weight) = (0.0, 0.0);
        for (w, ev) in weights.iter().zip(evs) {
            if let Some(ev) = ev {
                total += w * ev;
                weight += w;
            }
        }
        (weight > 0.0).then(|| total / weight)
    }
}

fn summarize<'a>(
    nodes: impl Iterator<Item = &'a NodeComparison>,
    min_reach: f64,
) -> FrequencySummary {
    let mut checked = 0;
    let mut largest: Option<Largest> = None;
    let (mut total, mut weight) = (0.0, 0.0);
    for node in nodes {
        let Some(differences) = node.differences() else {
            continue;
        };
        let w = (node.reach[0] + node.reach[1]) / 2.0;
        for &difference in &differences {
            total += w * difference;
            weight += w;
        }
        if node.is_checked(min_reach) {
            checked += 1;
            for (action, &difference) in differences.iter().enumerate() {
                if largest.is_none_or(|l| difference > l.difference) {
                    largest = Some(Largest {
                        node: node.node,
                        action,
                        difference,
                    });
                }
            }
        }
    }
    FrequencySummary {
        checked,
        largest,
        weighted_mean: if weight > 0.0 { total / weight } else { 0.0 },
    }
}

/// The classes of the root player's range at the root, under both strategies.
fn root_classes(game: &PostflopGame, sides: [&MappedSolution; 2]) -> Vec<ClassComparison> {
    let root = PostflopTree::ROOT;
    // At the root every hand's own reach is 1, so a range view weighs each hand by its range
    // weight alone, and the classes and their shares are the same under both strategies.
    let reached = "the root is always reached";
    let weights = game
        .range_view_weights(sides[0].strategy(), root)
        .expect(reached);
    let [a, b] = sides.map(|s| game.range_view_classes(s.strategy(), root).expect(reached));
    let class_ev = |evs: &NodeValues, hands: &[usize]| -> Option<f64> {
        let evs = evs.get(root)?;
        let (mut total, mut weight) = (0.0, 0.0);
        for &h in hands {
            if let Some(ev) = evs[h] {
                total += weights[h] * ev;
                weight += weights[h];
            }
        }
        (weight > 0.0).then(|| total / weight)
    };
    a.into_iter()
        .zip(b)
        .map(|(a, b)| {
            debug_assert_eq!((a.class, a.share), (b.class, b.share));
            ClassComparison {
                class: a.class,
                combos: a.hands.len(),
                share: a.share,
                ev: sides.map(|s| s.reported_evs().and_then(|evs| class_ev(evs, &a.hands))),
                measured_ev: sides.map(|s| class_ev(s.measured_evs(), &a.hands)),
                frequencies: [a.frequencies, b.frequencies],
            }
        })
        .collect()
}

/// The most classes the text of a comparison lists.
const CLASSES_SHOWN: usize = 12;

/// The most nodes below a deal the text of a comparison lists.
const NODES_SHOWN: usize = 12;

/// The most failures the text of a flop or turn comparison lists.
const FAILURES_SHOWN: usize = 40;

impl fmt::Display for Comparison {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.first_street == Street::River {
            self.fmt_river(f)
        } else {
            self.fmt_streets(f)
        }
    }
}

impl Comparison {
    fn label(&self, side: Side) -> String {
        let solver = &self.solutions[side].solver;
        match solver.iterations {
            Some(n) => format!("{} = {} ({n} iterations)", side_name(side), solver.name),
            None => format!("{} = {}", side_name(side), solver.name),
        }
    }

    fn verdict(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Comparison of {} and {}: ", self.label(0), self.label(1))?;
        if self.passes() {
            writeln!(f, "every check passes")
        } else {
            let count = self.failures.len();
            writeln!(f, "{count} check{} fail", if count == 1 { "" } else { "s" })
        }
    }

    /// The comparison of a river spot, as M3 wrote it.
    fn fmt_river(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let t = &self.tolerances;
        let pct = |chips: f64| self.percent(chips);
        self.verdict(f)?;

        writeln!(
            f,
            "\nExploitability, measured here (limit {}% of the pot):",
            t.exploitability
        )?;
        for (side, s) in self.solutions.iter().enumerate() {
            let measured = s.measured.expect("a river solution is complete");
            write!(
                f,
                "  {}: {:.4}% of the pot ({:.3} chips per hand dealt)",
                side_name(side),
                measured.percent_of_pot,
                measured.chips
            )?;
            if let Some(own) = s.solver.exploitability {
                write!(f, "; its solver reports {own:.3} chips")?;
            }
            writeln!(f)?;
        }

        writeln!(
            f,
            "\nValues at the root, in chips per hand dealt (limit {}% of the pot apart):",
            t.value
        )?;
        let values = self
            .values()
            .map(|v| v.expect("a river solution is complete"));
        for player in [Player::Oop, Player::Ip] {
            let (a, b) = (values[0][player.index()], values[1][player.index()]);
            writeln!(
                f,
                "  {player}: A {a:.2}, B {b:.2}: {:.4}% of the pot apart",
                pct((a - b).abs())
            )?;
        }
        self.fmt_reported_values(f)?;

        let summary = &self.frequencies;
        writeln!(
            f,
            "\nFrequencies at the {} of {} decision nodes reached more than {}% of the time \
             (limit {} points apart):",
            summary.checked,
            self.nodes.len(),
            100.0 * t.min_reach,
            100.0 * t.frequency
        )?;
        self.fmt_largest(f, summary)?;
        writeln!(
            f,
            "  mean difference over every node, weighted by reach: {:.3} points",
            100.0 * summary.weighted_mean
        )?;

        writeln!(
            f,
            "\nNodes: reach, then each action's frequency as a range view shows it (own combos, \
             no card removal by the opponent), under A / B, in percent; EV over the range in \
             chips:"
        )?;
        for node in &self.nodes {
            self.fmt_node(f, node)?;
        }

        self.fmt_classes(f)?;

        if !self.failures.is_empty() {
            writeln!(f, "\nFailures:")?;
            for failure in &self.failures {
                writeln!(f, "- {failure}")?;
            }
        }
        Ok(())
    }

    /// The comparison of a flop or turn spot: the first street node by node, and the streets
    /// below a deal summed up, with the nodes that differ most.
    fn fmt_streets(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let t = &self.tolerances;
        let pct = |chips: f64| self.percent(chips);
        let total = streets_from(match self.first_street {
            Street::Flop => 3,
            Street::Turn => 4,
            Street::River => 5,
        });
        let held = |side: Side| {
            let s = &self.solutions[side];
            let streets: Vec<String> = (0..s.streets)
                .map(|deals| street_after(self.first_street, deals).to_string())
                .collect();
            if s.complete {
                format!("{} holds every street", side_name(side))
            } else {
                format!(
                    "{} holds only the {}, of the {total} streets",
                    side_name(side),
                    streets.join(" and ")
                )
            }
        };
        self.verdict(f)?;
        writeln!(
            f,
            "A {} spot. {}; {}. The strategies are compared where both hold them.",
            self.first_street,
            held(0),
            held(1)
        )?;

        writeln!(
            f,
            "\nExploitability (limit {}% of the pot):",
            t.exploitability
        )?;
        for (side, s) in self.solutions.iter().enumerate() {
            match &s.measured {
                Some(measured) => {
                    write!(
                        f,
                        "  {}: {:.4}% of the pot ({:.3} chips per hand dealt), measured here",
                        side_name(side),
                        measured.percent_of_pot,
                        measured.chips
                    )?;
                    if let Some(own) = s.solver.exploitability {
                        write!(f, "; its solver reports {own:.3} chips")?;
                    }
                    writeln!(f)?;
                }
                None => {
                    write!(
                        f,
                        "  {}: NOT MEASURED HERE. {} holds the first {} of the {total} streets, \
                         and a strategy cannot be measured without the rest of it. ",
                        side_name(side),
                        side_name(side),
                        s.streets
                    )?;
                    match s.solver.exploitability {
                        Some(own) => writeln!(
                            f,
                            "Its solver reports {own:.3} chips per hand dealt, {:.4}% of the \
                             pot, which could not be checked here; the verdict holds that \
                             figure to the limit",
                            pct(own)
                        )?,
                        None => writeln!(
                            f,
                            "Its solver reports none either, so its exploitability is not \
                             checked at all"
                        )?,
                    }
                }
            }
        }

        writeln!(
            f,
            "\nValues at the root, in chips per hand dealt (limit {}% of the pot apart):",
            t.value
        )?;
        let values = self.values();
        let shown = |side: Side, player: Player| match values[side] {
            Some(v) => {
                let how = if self.solutions[side].measured.is_some() {
                    ""
                } else {
                    " (as reported, not measured)"
                };
                format!("{:.2}{how}", v[player.index()])
            }
            None => "not known".to_string(),
        };
        for player in [Player::Oop, Player::Ip] {
            write!(
                f,
                "  {player}: A {}, B {}",
                shown(0, player),
                shown(1, player)
            )?;
            if let [Some(a), Some(b)] = values {
                let i = player.index();
                write!(f, ": {:.4}% of the pot apart", pct((a[i] - b[i]).abs()))?;
            }
            writeln!(f)?;
        }
        self.fmt_reported_values(f)?;
        for unchecked in &self.unchecked {
            if let Unchecked::Values {
                side,
                reported: false,
            } = unchecked
            {
                writeln!(
                    f,
                    "  {} is partial and reports no values, so the values are not compared",
                    side_name(*side)
                )?;
            }
        }

        writeln!(
            f,
            "\nFrequencies as a range view shows them, at the decision nodes both hold; a node \
             is checked when reached more than {}% of the time given the cards dealt on its \
             path:",
            100.0 * t.min_reach
        )?;
        for street in &self.streets {
            let on = if street.deals == 0 {
                format!("{}", street.street)
            } else {
                format!(
                    "{}, below {} deal{}",
                    street.street,
                    street.deals,
                    if street.deals == 1 { "" } else { "s" }
                )
            };
            writeln!(
                f,
                "  {on}: {} of {} nodes checked (limit {} points apart)",
                street.frequencies.checked,
                street.nodes,
                100.0 * street.limit
            )?;
            self.fmt_largest(f, &street.frequencies)?;
            writeln!(
                f,
                "    mean difference over every node, weighted by reach: {:.3} points",
                100.0 * street.frequencies.weighted_mean
            )?;
        }
        let compared = self.streets.len();
        if compared < total {
            let missing: Vec<String> = (compared..total)
                .map(|deals| street_after(self.first_street, deals).to_string())
                .collect();
            writeln!(
                f,
                "  {}: not compared, since one solution does not hold it",
                missing.join(" and ")
            )?;
        }

        writeln!(
            f,
            "\nNodes on the {}: reach, then each action's frequency as a range view shows it \
             (own combos, no card removal by the opponent), under A / B, in percent; EV over \
             the range in chips:",
            self.first_street
        )?;
        for node in self.nodes.iter().filter(|n| n.deals == 0) {
            self.fmt_node(f, node)?;
        }
        let mut below: Vec<&NodeComparison> = self
            .nodes
            .iter()
            .filter(|n| n.deals > 0 && n.is_checked(t.min_reach))
            .collect();
        if !below.is_empty() {
            below.sort_by(|a, b| b.largest_difference().total_cmp(&a.largest_difference()));
            writeln!(
                f,
                "\nThe {} checked nodes below a deal whose frequencies differ most, of {}: reach, \
                 and the reach given the cards dealt on the way, then the frequencies under A / \
                 B, in percent:",
                below.len().min(NODES_SHOWN),
                below.len()
            )?;
            for node in below.iter().take(NODES_SHOWN) {
                self.fmt_node(f, node)?;
            }
        }

        self.fmt_reported_evs(f)?;
        self.fmt_classes(f)?;

        if !self.failures.is_empty() {
            writeln!(f, "\nFailures:")?;
            for failure in self.failures.iter().take(FAILURES_SHOWN) {
                writeln!(f, "- {failure}")?;
            }
            if self.failures.len() > FAILURES_SHOWN {
                writeln!(f, "- and {} more", self.failures.len() - FAILURES_SHOWN)?;
            }
        }
        Ok(())
    }

    fn fmt_reported_values(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (side, s) in self.solutions.iter().enumerate() {
            if let Some(reported) = s.reported_values {
                writeln!(
                    f,
                    "  {} reports OOP {:.2}, IP {:.2}",
                    side_name(side),
                    reported[0],
                    reported[1]
                )?;
            }
        }
        Ok(())
    }

    fn fmt_largest(&self, f: &mut fmt::Formatter<'_>, summary: &FrequencySummary) -> fmt::Result {
        let Some(largest) = summary.largest else {
            return Ok(());
        };
        let node = self
            .nodes
            .iter()
            .find(|n| n.node == largest.node)
            .expect("the largest difference is at a node");
        let indent = if self.first_street == Street::River {
            "  "
        } else {
            "    "
        };
        writeln!(
            f,
            "{indent}largest difference {:.3} points, {} at {}",
            100.0 * largest.difference,
            node.actions[largest.action],
            PathText(&node.path)
        )
    }

    /// One node's line: its path, player and reach, and each action's frequency under both.
    fn fmt_node(&self, f: &mut fmt::Formatter<'_>, node: &NodeComparison) -> fmt::Result {
        write!(
            f,
            "  {} {}, reached {:.2} / {:.2}",
            PathText(&node.path),
            node.player,
            100.0 * node.reach[0],
            100.0 * node.reach[1]
        )?;
        if node.deals > 0 {
            write!(
                f,
                " ({:.2} / {:.2} given its cards)",
                100.0 * node.reach_given_cards[0],
                100.0 * node.reach_given_cards[1]
            )?;
        }
        f.write_str(":")?;
        for (i, action) in node.actions.iter().enumerate() {
            let shown = |side: Side| match &node.frequencies[side] {
                Some(freq) => format!("{:.2}", 100.0 * freq[i]),
                None => "-".to_string(),
            };
            let comma = if i == 0 { "" } else { "," };
            write!(f, "{comma} {action} {} / {}", shown(0), shown(1))?;
        }
        if node.ev.iter().any(Option::is_some) {
            let shown = |ev: Option<f64>| ev.map_or("-".to_string(), |ev| format!("{ev:.2}"));
            write!(f, "; EV {} / {}", shown(node.ev[0]), shown(node.ev[1]))?;
        }
        writeln!(f)
    }

    /// For a flop or turn spot, where each solution's reported EVs were held to what its
    /// strategy gives, and where they could not be.
    fn fmt_reported_evs(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let t = &self.tolerances;
        writeln!(
            f,
            "\nReported EVs over the range, against what our code measures for the same \
             strategy (limit {}% of the pot), at the nodes reached more than {}% of the time \
             given their cards:",
            t.reported_ev,
            100.0 * t.min_reach
        )?;
        for (side, s) in self.solutions.iter().enumerate() {
            if !s.has_evs {
                writeln!(f, "  {}: reports no EVs", side_name(side))?;
                continue;
            }
            let reached: Vec<&NodeComparison> = self
                .nodes
                .iter()
                .filter(|n| n.reach_given_cards[side] > t.min_reach && n.ev[side].is_some())
                .collect();
            let checked: Vec<(&NodeComparison, f64)> = reached
                .iter()
                .filter_map(|n| {
                    let apart = (n.ev[side]? - n.measured_ev[side]?).abs();
                    Some((*n, self.percent(apart)))
                })
                .collect();
            write!(
                f,
                "  {}: checked at {} of the {} nodes with EVs",
                side_name(side),
                checked.len(),
                reached.len()
            )?;
            if let Some((node, apart)) = checked
                .iter()
                .max_by(|a, b| a.1.total_cmp(&b.1))
                .map(|&(n, a)| (n, a))
            {
                write!(
                    f,
                    ", largest {apart:.4}% of the pot apart, at {}",
                    PathText(&node.path)
                )?;
            }
            if checked.len() < reached.len() {
                write!(
                    f,
                    "; at the other {} its strategy below the node is not known, so they could \
                     not be checked here",
                    reached.len() - checked.len()
                )?;
            }
            writeln!(f)?;
        }
        Ok(())
    }

    fn fmt_classes(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let t = &self.tolerances;
        let mut classes: Vec<&ClassComparison> = self.classes.iter().collect();
        classes.sort_by(|a, b| b.largest_difference().total_cmp(&a.largest_difference()));
        writeln!(
            f,
            "\nThe root, {}'s {} classes, those whose frequencies differ most first: share of \
             the range, then each action's frequency under A / B in percent, and the EV (limit \
             {}% of the pot apart for a class holding more than {}% of the range; reported EVs \
             and values within {}% of the pot of their strategy's):",
            self.root_player,
            classes.len(),
            t.ev,
            100.0 * t.min_reach,
            t.reported_ev
        )?;
        for class in classes.iter().take(CLASSES_SHOWN) {
            write!(
                f,
                "  {} ({} combos, {:.2}%):",
                class.class,
                class.combos,
                100.0 * class.share
            )?;
            for (i, action) in self.root_actions.iter().enumerate() {
                let comma = if i == 0 { "" } else { "," };
                write!(
                    f,
                    "{comma} {action} {:.2} / {:.2}",
                    100.0 * class.frequencies[0][i],
                    100.0 * class.frequencies[1][i]
                )?;
            }
            if class.ev.iter().any(Option::is_some) {
                let shown = |ev: Option<f64>| ev.map_or("-".to_string(), |ev| format!("{ev:.2}"));
                write!(f, "; EV {} / {}", shown(class.ev[0]), shown(class.ev[1]))?;
            }
            writeln!(f)?;
        }
        if classes.len() > CLASSES_SHOWN {
            writeln!(f, "  and {} more", classes.len() - CLASSES_SHOWN)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
