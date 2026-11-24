//! Comparing two solutions of the same river spot, each ours or another solver's, and a verdict
//! against a set of tolerances.
//!
//! [`compare`] takes the spot and two [dumps](crate::dump), maps each onto the spot's tree,
//! which checks first that both answer it ([`crate::import`]), and compares them.
//! [`compare_mapped`] compares two solutions already on the tree. The result is a
//! [`Comparison`]: data, with a `Display` for a command line. The library prints nothing.
//!
//! # What is compared
//!
//! Every number below is computed by our code, in our game, from the two strategies, except
//! the EVs and values a solver reports, which are compared both with the other solution's and
//! with what its own strategy gives here. Call the two solutions A and B.
//!
//! - **Exploitability.** Each strategy's exploitability, as
//!   [`exploitability`](mod@crate::exploitability) measures it. For a strategy from another
//!   solver this is the most trustworthy number: it does not depend on how that solver splits
//!   a mixed strategy among equivalent hands.
//! - **Values.** Each player's value at the root under each strategy, in chips per hand dealt,
//!   and the difference.
//! - **Nodes.** At every decision node: its reach, the probability that a hand dealt plays to
//!   it, under each strategy; and each action's frequency over the acting player's range there,
//!   under each. The frequencies are summed up by the largest difference over the nodes
//!   checked (below) and by the mean absolute difference over every node both strategies reach
//!   and every action there, each node weighted by the mean of its two reaches, so that nodes
//!   almost never reached count for little.
//! - **EVs.** Where a solution has EVs, at every decision node the acting player's EV over its
//!   range there: the mean of its hands' EVs, over the hands whose EV is defined.
//! - **Hand classes at the root.** For each of the 169 classes in the range of the player who
//!   acts at the root, its share of the range, each action's frequency over its combos, and its
//!   EV, under each strategy. This is what a person compares by eye in two range views.
//!
//! Every frequency, share and EV over a range or a class is weighted as a solver's range view
//! weighs it: each hand by its range weight times the acting player's own reach of the node,
//! with no card removal by the opponent's range ([`RiverGame::range_view_frequencies`]). That
//! is what a person sees in another solver. It is not the probability of each action once the
//! node is reached, [`RiverGame::action_frequencies`], which weighs each hand also by the
//! opponent's hands it can meet there, and which differs from it by up to several points (see
//! [`crate::river`]). Both sides are summed by the same functions, here, from their per-hand
//! strategies and EVs, so the convention can never differ between them: a dump carries no
//! aggregate of its own.
//!
//! # The verdict
//!
//! A [`Tolerances`] sets the limits, and the [`Comparison`] lists each [`Failure`]. A node is
//! *checked* when either strategy reaches it more often than [`Tolerances::min_reach`] and
//! both reach it at all. The checks are:
//!
//! - [`Check::Exploitability`]: each strategy's exploitability is at most
//!   [`Tolerances::exploitability`] percent of the pot.
//! - [`Check::Value`]: each player's two values at the root are within [`Tolerances::value`]
//!   percent of the pot.
//! - [`Check::Frequency`]: at every checked node, each action's two frequencies are within
//!   [`Tolerances::frequency`].
//! - [`Check::Ev`], when both solutions have EVs: at the root, the two EVs of each class that
//!   holds more than [`Tolerances::min_reach`] of the range are within [`Tolerances::ev`]
//!   percent of the pot.
//! - [`Check::ReportedEv`], for each solution that reports EVs or values: the EVs it reports
//!   are within [`Tolerances::reported_ev`] percent of the pot of those our code measures for
//!   its own strategy, over the range at each node it reaches more often than
//!   [`Tolerances::min_reach`] and for each class at the root, and so are the values it
//!   reports.
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
//! The defaults for exploitability, values, frequencies and the reach of a checked node are the
//! tolerance of the M3 gate (DECISIONS D20), which has both solvers at 0.03% of the pot or
//! better: an exploitability under 0.3% of the pot, frequencies within 2 points at every node
//! reached more than 1% of the time, and values within 0.2% of the pot. The gate sets no
//! limit for EVs. [`Tolerances::ev`] defaults to 5% of the pot and
//! [`Tolerances::reported_ev`] to 0.1%, from the measurements of the slow test
//! `honest_solvers_table` in `tests/compare.rs`.
//!
//! Single combos are not compared: where several equilibria exist, two correct solvers can
//! split a mixed strategy differently among equivalent hands.

use std::fmt;

use pk_cfr::PublicProfile;
use pk_core::HandClass;
use pk_game::{Chips, Player, PostflopTree};

use crate::dump::{DumpAction, PathText, SolutionDump, SolverInfo, decision_nodes};
use crate::exploitability::Exploitability;
use crate::import::{ImportError, MappedSolution};
use crate::node_values::NodeValues;
use crate::river::RiverGame;
use crate::spot::Spot;

/// The limits of a comparison's verdict. [`Tolerances::default`] gives those of the M3 gate:
/// see the [module documentation](self#the-verdict).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tolerances {
    /// The most exploitable either strategy may be, in percent of the pot. Defaults to 0.3.
    pub exploitability: f64,
    /// How far apart each player's values may be, in percent of the pot. Defaults to 0.2.
    pub value: f64,
    /// How far apart an action's frequencies may be at a checked node, as a probability.
    /// Defaults to 0.02, 2 percentage points.
    pub frequency: f64,
    /// A node is checked when either strategy reaches it with more than this probability, and
    /// a class at the root when it holds more than this share of the range. Defaults to 0.01.
    pub min_reach: f64,
    /// How far apart the two solutions' EVs of a class at the root may be, in percent of the
    /// pot. Defaults to 5.
    pub ev: f64,
    /// How far the EVs and values a solution reports may be from those our code measures for
    /// its strategy, in percent of the pot. Defaults to 0.1.
    pub reported_ev: f64,
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
    /// A strategy is more exploitable than allowed.
    Exploitability { side: Side, percent_of_pot: f64 },
    /// A player's values at the root under the two strategies are too far apart.
    Value {
        player: Player,
        values: [f64; 2],
        percent_of_pot: f64,
    },
    /// An action's frequencies at a checked node are too far apart.
    Frequency {
        node: u32,
        path: Vec<DumpAction>,
        action: DumpAction,
        frequencies: [f64; 2],
        reach: [f64; 2],
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
            Failure::Exploitability { .. } => Check::Exploitability,
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
            Failure::Value {
                player,
                values,
                percent_of_pot,
            } => write!(
                f,
                "{player}'s value is {:.2} under A and {:.2} under B, {percent_of_pot:.3}% of \
                 the pot apart",
                values[0], values[1]
            ),
            Failure::Frequency {
                path,
                action,
                frequencies,
                reach,
                ..
            } => write!(
                f,
                "at {} (reached {:.1}% / {:.1}%), {action} is played {:.2}% under A and {:.2}% \
                 under B, {:.2} points apart",
                PathText(path),
                100.0 * reach[0],
                100.0 * reach[1],
                100.0 * frequencies[0],
                100.0 * frequencies[1],
                100.0 * (frequencies[0] - frequencies[1]).abs()
            ),
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

/// One solution of a comparison: what its solver reported, and what our code measures.
#[derive(Debug, Clone, PartialEq)]
pub struct SolutionSummary {
    pub solver: SolverInfo,
    /// Each player's value as the solver reported it.
    pub reported_values: Option<[f64; 2]>,
    /// Whether the solution has EVs of its own.
    pub has_evs: bool,
    /// The exploitability and values of its strategy, measured by our code in our game.
    pub measured: Exploitability,
}

/// One decision node under the two strategies. Arrays are indexed by [`Side`].
#[derive(Debug, Clone, PartialEq)]
pub struct NodeComparison {
    /// The node of our tree.
    pub node: u32,
    pub path: Vec<DumpAction>,
    pub player: Player,
    pub actions: Vec<DumpAction>,
    /// The probability that a hand dealt plays to the node.
    pub reach: [f64; 2],
    /// Each action's frequency over the acting player's range as a range view shows it
    /// ([`RiverGame::range_view_frequencies`]), in the order of `actions`, or `None` where the
    /// acting player never plays to the node.
    pub frequencies: [Option<Vec<f64>>; 2],
    /// The acting player's EV over its range, weighted as a range view weighs it, from the EVs
    /// the solution reports, if it has EVs and plays to the node.
    pub ev: [Option<f64>; 2],
    /// The same EV from the EVs our code measures for the solution's strategy.
    pub measured_ev: [Option<f64>; 2],
}

impl NodeComparison {
    /// Whether the verdict checks the node: either strategy reaches it with more than
    /// `min_reach`, and both reach it at all.
    pub fn is_checked(&self, min_reach: f64) -> bool {
        self.reach.iter().any(|&r| r > min_reach)
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

/// The frequency differences summed up over the tree.
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
    /// The class's EV as the solution reports it, if it has EVs and one is defined.
    pub ev: [Option<f64>; 2],
    /// The same EV measured by our code for the solution's strategy.
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
    /// A and B.
    pub solutions: [SolutionSummary; 2],
    /// Every decision node, in our tree's order.
    pub nodes: Vec<NodeComparison>,
    pub frequencies: FrequencySummary,
    /// The player who acts at the root.
    pub root_player: Player,
    /// The root's actions.
    pub root_actions: Vec<DumpAction>,
    /// The classes in the root player's range, in grid order.
    pub classes: Vec<ClassComparison>,
    /// Every check that fails, in the order of the checks.
    pub failures: Vec<Failure>,
}

impl Comparison {
    /// Whether every check passes.
    pub fn passes(&self) -> bool {
        self.failures.is_empty()
    }

    /// The failures of one check.
    pub fn failures_of(&self, check: Check) -> impl Iterator<Item = &Failure> + '_ {
        self.failures.iter().filter(move |f| f.check() == check)
    }

    /// Each player's value at the root under A and under B, measured: `[side][player]`.
    pub fn values(&self) -> [[f64; 2]; 2] {
        [0, 1].map(|side| self.solutions[side].measured.values)
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

/// Compares two solutions on the tree of one spot.
///
/// Panics if they are not on the same game: the same tree, hands, pot and rake.
pub fn compare_mapped(
    a: &MappedSolution,
    b: &MappedSolution,
    tolerances: &Tolerances,
) -> Comparison {
    let game = a.game();
    let other = b.game();
    assert!(
        game.tree() == other.tree()
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

    let solutions = sides.map(|s| SolutionSummary {
        solver: s.solver().clone(),
        reported_values: s.reported_values(),
        has_evs: s.reported_evs().is_some(),
        measured: *s.exploitability(),
    });

    let nodes: Vec<NodeComparison> = decision_nodes(game.tree())
        .into_iter()
        .map(|decision| {
            let at = sides.map(|s| At::new(game, s.strategy(), decision.node));
            let frequencies = [0, 1].map(|i| at[i].frequencies.clone());
            let ev = [0, 1].map(|i| sides[i].reported_evs().and_then(|evs| at[i].range_ev(evs)));
            let measured_ev = [0, 1].map(|i| at[i].range_ev(sides[i].measured_evs()));
            NodeComparison {
                node: decision.node,
                path: decision.path,
                player: decision.player,
                actions: decision.actions,
                reach: [at[0].reach, at[1].reach],
                frequencies,
                ev,
                measured_ev,
            }
        })
        .collect();

    let frequencies = summarize(&nodes, tolerances.min_reach);
    let root = &nodes[0];
    debug_assert_eq!(root.node, PostflopTree::ROOT);
    let classes = root_classes(game, sides);

    let mut failures = Vec::new();
    for (side, solution) in solutions.iter().enumerate() {
        let measured = solution.measured.percent_of_pot;
        if !within(measured, tolerances.exploitability) {
            failures.push(Failure::Exploitability {
                side,
                percent_of_pot: measured,
            });
        }
    }
    for player in [Player::Oop, Player::Ip] {
        let values = [0, 1].map(|side| solutions[side].measured.values[player.index()]);
        let apart = percent((values[0] - values[1]).abs());
        if !within(apart, tolerances.value) {
            failures.push(Failure::Value {
                player,
                values,
                percent_of_pot: apart,
            });
        }
    }
    for node in nodes.iter().filter(|n| n.is_checked(tolerances.min_reach)) {
        let (Some(a), Some(b)) = (&node.frequencies[0], &node.frequencies[1]) else {
            unreachable!("a checked node is reached by both");
        };
        for (action, (&fa, &fb)) in a.iter().zip(b).enumerate() {
            if !within((fa - fb).abs(), tolerances.frequency) {
                failures.push(Failure::Frequency {
                    node: node.node,
                    path: node.path.clone(),
                    action: node.actions[action],
                    frequencies: [fa, fb],
                    reach: node.reach,
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
                .filter(|n| n.reach[side] > tolerances.min_reach);
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
        if let Some(reported) = solution.reported_values {
            for player in [Player::Oop, Player::Ip] {
                let (reported, measured) = (
                    reported[player.index()],
                    solution.measured.values[player.index()],
                );
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
        solutions,
        nodes,
        frequencies,
        root_player,
        root_actions,
        classes,
        failures,
    }
}

/// Whether `x` is at most `limit`. False for NaN, so that a number that is not one fails.
fn within(x: f64, limit: f64) -> bool {
    x <= limit
}

/// One strategy at one decision node: the node's reach, and the acting player's range there
/// as a range view weighs it.
struct At {
    node: u32,
    /// The probability that a hand dealt plays to the node.
    reach: f64,
    /// [`RiverGame::range_view_weights`], or `None` if the acting player never plays to the
    /// node.
    weights: Option<Vec<f64>>,
    /// [`RiverGame::range_view_frequencies`].
    frequencies: Option<Vec<f64>>,
}

impl At {
    fn new(game: &RiverGame, profile: &PublicProfile, node: u32) -> At {
        At {
            node,
            reach: game.reach_probability(profile, node),
            weights: game.range_view_weights(profile, node),
            frequencies: game.range_view_frequencies(profile, node),
        }
    }

    /// The EV over the range at the node, as a range view weighs it: the mean of the hands'
    /// EVs weighted by range weight and own reach, over the hands whose EV is defined.
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

fn summarize(nodes: &[NodeComparison], min_reach: f64) -> FrequencySummary {
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
fn root_classes(game: &RiverGame, sides: [&MappedSolution; 2]) -> Vec<ClassComparison> {
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

impl fmt::Display for Comparison {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let t = &self.tolerances;
        let pct = |chips: f64| self.percent(chips);
        let label = |side: Side| {
            let solver = &self.solutions[side].solver;
            match solver.iterations {
                Some(n) => format!("{} = {} ({n} iterations)", side_name(side), solver.name),
                None => format!("{} = {}", side_name(side), solver.name),
            }
        };
        write!(f, "Comparison of {} and {}: ", label(0), label(1))?;
        if self.passes() {
            writeln!(f, "every check passes")?;
        } else {
            let count = self.failures.len();
            writeln!(f, "{count} check{} fail", if count == 1 { "" } else { "s" })?;
        }

        writeln!(
            f,
            "\nExploitability, measured here (limit {}% of the pot):",
            t.exploitability
        )?;
        for (side, s) in self.solutions.iter().enumerate() {
            write!(
                f,
                "  {}: {:.4}% of the pot ({:.3} chips per hand dealt)",
                side_name(side),
                s.measured.percent_of_pot,
                s.measured.chips
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
        let values = self.values();
        for player in [Player::Oop, Player::Ip] {
            let (a, b) = (values[0][player.index()], values[1][player.index()]);
            writeln!(
                f,
                "  {player}: A {a:.2}, B {b:.2}: {:.4}% of the pot apart",
                pct((a - b).abs())
            )?;
        }
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
        if let Some(largest) = summary.largest {
            let node = self
                .nodes
                .iter()
                .find(|n| n.node == largest.node)
                .expect("the largest difference is at a node");
            writeln!(
                f,
                "  largest difference {:.3} points, {} at {}",
                100.0 * largest.difference,
                node.actions[largest.action],
                PathText(&node.path)
            )?;
        }
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
            write!(
                f,
                "  {} {}, reached {:.2} / {:.2}:",
                PathText(&node.path),
                node.player,
                100.0 * node.reach[0],
                100.0 * node.reach[1]
            )?;
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
            writeln!(f)?;
        }

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

        if !self.failures.is_empty() {
            writeln!(f, "\nFailures:")?;
            for failure in &self.failures {
                writeln!(f, "- {failure}")?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
