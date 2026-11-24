//! The invariants every solution keeps, whatever the solver does inside, and a check that
//! reports each one a solution breaks.
//!
//! # The invariants
//!
//! [`check_solution`] reads a [`Solution`] through its accessors only, and checks:
//!
//! 1. **Strategies.** At every player node the strategy has one probability per action for
//!    each hand of the acting player. Each probability is from 0 to 1, and each hand's sum to 1
//!    within [`STRATEGY_TOLERANCE`].
//! 2. **Reach.** Each player's reach of the root ([`Solution::reach`]) is 1 with every hand. At
//!    every other node, the reach of the player who acted at the parent is its reach of the
//!    parent times the probability of the action that leads here, and the other player's is
//!    its reach of the parent, within [`REACH_TOLERANCE`].
//! 3. **Values.** The two players' values ([`Solution::values`]) sum to the pot less the
//!    expected rake. The expected rake comes from [`expected_rake`], which works it out from the
//!    chips and the strategy, apart from the payoffs and the expected values it is compared
//!    with. The comparison is with `pot · M − E[rake]`, where `M` is the probability of ending
//!    at some terminal. `M` would be 1 if the strategies summed to 1 exactly, and their `f32`
//!    rounding puts it up to 1.9e-8 off on the test spots, which comparing with `pot − E[rake]`
//!    would add to the difference (see [the module](mod@crate::expected_rake)). The sum must
//!    hold within [`VALUE_TOLERANCE`] times the largest pot raked at any terminal, the size of
//!    the largest payoff.
//! 4. **Finite.** No number is NaN or infinite: no probability of a strategy, no reach, and
//!    none of the values, best-response values and exploitability the solution reports.
//!
//! The fifth invariant of the brief and of issue #30, identical results on suit-isomorphic
//! boards, is not checked yet. It needs suit isomorphism, which comes in M4 (#35), and two
//! solves: a board and a suit permutation of it, whose strategies and values must match hand
//! for hand once the permutation maps the combos.
//!
//! # Reporting
//!
//! The [`Report`] lists every violation, with which invariant it breaks, where (node, player,
//! hand and combo) and the numbers, and the largest deviation each check found
//! ([`Report::worst`]), so that tolerances can be measured. Each fault is reported once, where
//! it arises, rather than again by every check downstream of it:
//!
//! - A probability that is not finite is reported as such, and its hand's sum is not checked.
//! - A reach is compared with its parent's only where what it should be is finite, so a NaN
//!   probability does not show up again at every node below it. A reach is compared with the
//!   reach the solution reports for its parent, so a wrong reach whose subtree follows from it
//!   is reported at the one node where it goes wrong.
//! - The value sum is checked only when every strategy passes (1) and both values are finite.
//!   Otherwise the fault is reported already, and a sum that went wrong with it would only
//!   report it again.
//! - The reach and the value sum need a strategy of the right size at every player node. If
//!   one is missing, only that is reported.
//!
//! # Tolerances
//!
//! Measured by the slow test `invariant_margins` on the nine river test spots and on a spot
//! with every combo in both ranges (1,081 hands a side), each solved by the default solve, by
//! DCFR capped at 0, 1, 10, 100 and 1,000 iterations, and by vanilla CFR, CFR+ and linear CFR
//! capped at 10 and 300:
//!
//! | Check | Worst measured | On the test spots | Tolerance |
//! |---|---|---|---|
//! | strategy sums, from 1 | 2.1e-7 | 1.9e-7 | [`STRATEGY_TOLERANCE`] = 1e-6 |
//! | reach | 0 | 0 | [`REACH_TOLERANCE`] = 1e-14 |
//! | value sum, of the largest pot | 2.3e-15 | 6.1e-16 | [`VALUE_TOLERANCE`] = 1e-13 |
//!
//! - **Strategy sums.** The average strategy divides each action's strategy sum by their
//!   total, summed in `f32`, so `k` probabilities sum to within about `k · 2^-24` of 1: 4.8e-7
//!   for 8 actions. The tolerance is about twice that, and 5 times the worst measured.
//!   Storing strategies in 16 bits will need a tolerance of its own.
//! - **Reach.** [`Solution::reach`] multiplies the same `f64` probabilities in the same order
//!   as the check, so they agree exactly. The tolerance allows the product to be taken in
//!   another order, which moves it by about an ulp (1.1e-16) per factor, and little more.
//! - **Value sum.** Both sides are `f64` sums of the same `f32` strategies, so they differ
//!   only by the rounding of those sums, which grows with the hands and terminals. The
//!   tolerance is 44 times the worst measured. On the test spots, whose largest pot is at most
//!   20,050 chips, it allows 2e-9 chips, while a payoff off by one chip at a terminal reached a
//!   millionth of the time moves the sum by 1e-6.
//!
//! # Cost
//!
//! A check reads every probability once and walks the tree twice, once through
//! [`Solution::reach`] and once for the expected rake. Measured in release, it takes 0.02 to
//! 1.3 ms on the test spots and 5.3 ms with every combo in both ranges, 1.1 to 1.4 times one
//! exploitability measurement, which the solve makes every 10 iterations. That is under 1% of
//! the default solve on the largest spots and up to 6% on the smallest, which solve in a few
//! milliseconds.
//!
//! In a build with debug assertions, which the tests are, [`solve`](crate::solve()) checks
//! every solution it returns and panics with the report if one breaks an invariant.

use std::fmt;

use pk_cfr::PublicProfile;
use pk_core::Combo;
use pk_game::{Chips, Node, Player, PostflopTree};

use crate::expected_rake::expected_rake;
use crate::exploitability::Exploitability;
use crate::river::{RiverGame, player_of};
use crate::solve::Solution;

/// How far each hand's probabilities at a node may sum from 1. Measured, at most 2.1e-7: see
/// the [module documentation](self#tolerances).
pub const STRATEGY_TOLERANCE: f64 = 1e-6;

/// How far a reach may be from its parent's times the probability of the action taken, in
/// absolute terms, since reaches are from 0 to 1. Measured, 0: see the
/// [module documentation](self#tolerances).
pub const REACH_TOLERANCE: f64 = 1e-14;

/// How far the two players' values may sum from `pot · M − E[rake]`, as a fraction of the
/// largest pot raked at any terminal. Measured, at most 2.3e-15: see the
/// [module documentation](self#tolerances).
pub const VALUE_TOLERANCE: f64 = 1e-13;

/// The invariants [`check_solution`] checks, numbered as in the [module documentation](self).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Invariant {
    /// (1) Each hand's probabilities at each node are a distribution.
    Strategy,
    /// (2) Reaches follow from the parent's and the strategy.
    Reach,
    /// (3) The values sum to the pot less the expected rake.
    ValueSum,
    /// (4) No number is NaN or infinite.
    Finite,
}

impl fmt::Display for Invariant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Invariant::Strategy => "strategy",
            Invariant::Reach => "reach",
            Invariant::ValueSum => "value sum",
            Invariant::Finite => "finite",
        })
    }
}

/// A number of a solution, as a [`Violation::NotFinite`] names it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Quantity {
    /// The probability of `action` at the player node `node` with the acting player's hand.
    Strategy {
        node: u32,
        player: Player,
        hand: usize,
        combo: Combo,
        action: usize,
    },
    /// A player's reach of `node` with one of its hands.
    Reach {
        node: u32,
        player: Player,
        hand: usize,
        combo: Combo,
    },
    /// A player's expected value.
    Value(Player),
    /// A player's best-response value.
    BestResponse(Player),
    /// The exploitability in chips.
    Exploitability,
    /// The exploitability as a percent of the pot.
    PercentOfPot,
}

impl fmt::Display for Quantity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Quantity::Strategy {
                node,
                player,
                hand,
                combo,
                action,
            } => write!(
                f,
                "the probability of action {action} at node {node} with {player} hand {hand} \
                 ({combo})"
            ),
            Quantity::Reach {
                node,
                player,
                hand,
                combo,
            } => write!(
                f,
                "{player}'s reach of node {node} with hand {hand} ({combo})"
            ),
            Quantity::Value(player) => write!(f, "{player}'s value"),
            Quantity::BestResponse(player) => write!(f, "{player}'s best-response value"),
            Quantity::Exploitability => f.write_str("the exploitability in chips"),
            Quantity::PercentOfPot => f.write_str("the exploitability as a percent of the pot"),
        }
    }
}

/// The numbers of the value-sum check (3).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ValueSum {
    /// `v_0` and `v_1`, as the solution reports them, indexed by [`Player::index`].
    pub values: [f64; 2],
    /// The pot at the root.
    pub pot: Chips,
    /// `M`, the probability of ending at some terminal, from [`expected_rake`].
    pub probability: f64,
    /// `E[rake]`, from [`expected_rake`].
    pub rake: f64,
    /// The largest pot raked at any terminal, which the tolerance is a fraction of.
    pub largest_pot: f64,
}

impl ValueSum {
    /// `pot · M − E[rake]`, what the values must sum to.
    pub fn expected(&self) -> f64 {
        f64::from(self.pot) * self.probability - self.rake
    }

    /// `v_0 + v_1 − (pot · M − E[rake])`, in chips.
    pub fn difference(&self) -> f64 {
        self.values[0] + self.values[1] - self.expected()
    }

    /// The difference as a fraction of the largest pot, which [`VALUE_TOLERANCE`] bounds.
    pub fn relative(&self) -> f64 {
        self.difference().abs() / self.largest_pot
    }
}

/// One way a solution breaks an invariant.
#[derive(Debug, Clone, PartialEq)]
pub enum Violation {
    /// (1) A player node with no strategy, or with one of the wrong size: `expected`
    /// probabilities, one per action and hand of the acting player.
    StrategyShape {
        node: u32,
        expected: usize,
        found: Option<usize>,
    },
    /// (1) A hand whose probabilities at a node, all finite, are not a distribution: one is
    /// below 0 or above 1, or they sum to more than [`STRATEGY_TOLERANCE`] from 1.
    Strategy {
        node: u32,
        player: Player,
        hand: usize,
        combo: Combo,
        /// By action.
        probabilities: Vec<f32>,
        /// Their sum, in `f64`.
        sum: f64,
    },
    /// (2) A player's reach of `node` with a hand is more than [`REACH_TOLERANCE`] from what
    /// it should be: 1 at the root, and elsewhere what the parent's reach and the strategy
    /// give.
    Reach {
        node: u32,
        /// The parent and the index of the action that leads to `node`, or `None` at the root.
        parent: Option<(u32, usize)>,
        player: Player,
        hand: usize,
        combo: Combo,
        expected: f64,
        found: f64,
    },
    /// (3) The values sum to more than [`VALUE_TOLERANCE`] of the largest pot from the pot
    /// less the expected rake.
    ValueSum(ValueSum),
    /// (4) A number that is NaN or infinite.
    NotFinite { quantity: Quantity, value: f64 },
}

impl Violation {
    /// The invariant this breaks.
    pub fn invariant(&self) -> Invariant {
        match self {
            Violation::StrategyShape { .. } | Violation::Strategy { .. } => Invariant::Strategy,
            Violation::Reach { .. } => Invariant::Reach,
            Violation::ValueSum(_) => Invariant::ValueSum,
            Violation::NotFinite { .. } => Invariant::Finite,
        }
    }
}

impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: ", self.invariant())?;
        match self {
            Violation::StrategyShape {
                node,
                expected,
                found: None,
            } => write!(
                f,
                "node {node} has no strategy, and needs {expected} probabilities"
            ),
            Violation::StrategyShape {
                node,
                expected,
                found: Some(found),
            } => write!(
                f,
                "node {node} has {found} probabilities, and needs {expected}"
            ),
            Violation::Strategy {
                node,
                player,
                hand,
                combo,
                probabilities,
                sum,
            } => write!(
                f,
                "at node {node}, {player} hand {hand} ({combo}) plays {probabilities:?}, which \
                 sum to {sum} ({:+e} from 1)",
                sum - 1.0
            ),
            Violation::Reach {
                node,
                parent,
                player,
                hand,
                combo,
                expected,
                found,
            } => {
                write!(
                    f,
                    "{player}'s reach of node {node} with hand {hand} ({combo}) is {found}, but "
                )?;
                match parent {
                    None => write!(f, "it is the root, where every reach is {expected}"),
                    Some((parent, action)) => write!(
                        f,
                        "its reach of node {parent} and action {action} there give {expected} \
                         ({:+e} off)",
                        found - expected
                    ),
                }
            }
            Violation::ValueSum(check) => write!(
                f,
                "the values {:?} sum to {}, but the pot of {} times the probability {} of \
                 ending, less the expected rake {}, is {}: {:+e} chips off, {:e} of the \
                 largest pot of {}",
                check.values,
                check.values[0] + check.values[1],
                check.pot,
                check.probability,
                check.rake,
                check.expected(),
                check.difference(),
                check.relative(),
                check.largest_pot
            ),
            Violation::NotFinite { quantity, value } => write!(f, "{quantity} is {value}"),
        }
    }
}

/// The largest deviation each check found, within its tolerance or not, for measuring
/// tolerances: from [`Report::worst`].
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Worst {
    /// The largest `|Σ_a σ(a | h) − 1|` over the hands whose probabilities are finite.
    pub strategy: f64,
    /// The largest `|found − expected|` over the reaches compared.
    pub reach: f64,
    /// [`ValueSum::relative`], if the value sum was checked.
    pub value_sum: f64,
}

/// What [`check_solution`] found.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Report {
    /// Every violation found, in the order of the checks: the shapes of the strategies, then
    /// their probabilities node by node, then the reaches down the tree, then the values,
    /// best responses and exploitability, then the value sum.
    pub violations: Vec<Violation>,
    /// The numbers of the value-sum check, or `None` if it was not made (see the
    /// [module documentation](self)).
    pub value_sum: Option<ValueSum>,
    /// The largest deviation each check found.
    pub worst: Worst,
}

impl Report {
    /// Whether the solution breaks no invariant.
    pub fn is_valid(&self) -> bool {
        self.violations.is_empty()
    }

    /// The violations of one invariant.
    pub fn of(&self, invariant: Invariant) -> impl Iterator<Item = &Violation> + '_ {
        self.violations
            .iter()
            .filter(move |violation| violation.invariant() == invariant)
    }
}

/// The most violations a report's text lists.
const LISTED: usize = 50;

impl fmt::Display for Report {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_valid() {
            return f.write_str("no invariant is broken");
        }
        let count = self.violations.len();
        let plural = if count == 1 { "" } else { "s" };
        write!(f, "{count} violation{plural} of the invariants:")?;
        for violation in self.violations.iter().take(LISTED) {
            write!(f, "\n- {violation}")?;
        }
        if self.violations.len() > LISTED {
            write!(f, "\n- and {} more", self.violations.len() - LISTED)?;
        }
        Ok(())
    }
}

/// Checks `solution` against every invariant of the [module documentation](self), and reports
/// each one it breaks.
pub fn check_solution(solution: &Solution) -> Report {
    check(solution)
}

/// The debug assertion that [`solve`](crate::solve()) makes on every solution it returns: in a
/// build with debug assertions, panics with the report if `solution` breaks an invariant.
pub(crate) fn debug_assert_valid(solution: &Solution) {
    if cfg!(debug_assertions) {
        assert_valid(&check_solution(solution));
    }
}

fn assert_valid(report: &Report) {
    assert!(
        report.is_valid(),
        "the solve returned a solution that breaks its invariants: {report}"
    );
}

/// What the checks read from a solution: [`Solution`]'s accessors, so that the tests can give
/// the checks a solution with a fault put in.
trait Reported {
    fn game(&self) -> &RiverGame;
    fn strategy(&self) -> &PublicProfile;
    fn reach(&self, node: u32) -> [Vec<f64>; 2];
    fn exploitability(&self) -> &Exploitability;
}

impl Reported for Solution {
    fn game(&self) -> &RiverGame {
        Solution::game(self)
    }

    fn strategy(&self) -> &PublicProfile {
        Solution::strategy(self)
    }

    fn reach(&self, node: u32) -> [Vec<f64>; 2] {
        Solution::reach(self, node)
    }

    fn exploitability(&self) -> &Exploitability {
        Solution::exploitability(self)
    }
}

fn check(solution: &impl Reported) -> Report {
    let mut report = Report::default();
    let shaped = check_shapes(solution, &mut report);
    let distributions = shaped && check_strategies(solution, &mut report);
    if shaped {
        check_reaches(solution, &mut report);
    }
    let finite = check_numbers(solution.exploitability(), &mut report);
    if distributions && finite {
        check_value_sum(solution, &mut report);
    }
    report
}

/// The player nodes of the tree, with the acting player and the number of actions.
fn player_nodes(tree: &PostflopTree) -> impl Iterator<Item = (u32, Player, usize)> + '_ {
    tree.nodes()
        .iter()
        .enumerate()
        .filter_map(|(node, n)| match n {
            Node::Player { actor, actions, .. } => Some((node as u32, *actor, actions.len())),
            _ => None,
        })
}

/// (1), the shape: whether every player node has a strategy of the right size.
fn check_shapes(solution: &impl Reported, report: &mut Report) -> bool {
    let game = solution.game();
    let mut shaped = true;
    for (node, actor, num_actions) in player_nodes(game.tree()) {
        let expected = num_actions * game.hands(actor).len();
        let found = solution.strategy().get(node as usize).map(<[f32]>::len);
        if found != Some(expected) {
            report.violations.push(Violation::StrategyShape {
                node,
                expected,
                found,
            });
            shaped = false;
        }
    }
    shaped
}

/// (1) and (4) for the strategies: whether every hand's probabilities are finite and a
/// distribution.
fn check_strategies(solution: &impl Reported, report: &mut Report) -> bool {
    let game = solution.game();
    let mut distributions = true;
    for (node, player, num_actions) in player_nodes(game.tree()) {
        let combos = game.hands(player).combos();
        let num_hands = combos.len();
        let strategy = solution
            .strategy()
            .get(node as usize)
            .expect("the shapes are checked first");
        for (hand, &combo) in combos.iter().enumerate() {
            let probabilities: Vec<f32> = (0..num_actions)
                .map(|action| strategy[action * num_hands + hand])
                .collect();
            let mut finite = true;
            for (action, &p) in probabilities.iter().enumerate() {
                if !p.is_finite() {
                    finite = false;
                    report.violations.push(Violation::NotFinite {
                        quantity: Quantity::Strategy {
                            node,
                            player,
                            hand,
                            combo,
                            action,
                        },
                        value: f64::from(p),
                    });
                }
            }
            if !finite {
                distributions = false;
                continue;
            }
            let sum: f64 = probabilities.iter().map(|&p| f64::from(p)).sum();
            let deviation = (sum - 1.0).abs();
            report.worst.strategy = report.worst.strategy.max(deviation);
            let in_range = probabilities.iter().all(|p| (0.0..=1.0).contains(p));
            if !in_range || deviation > STRATEGY_TOLERANCE {
                distributions = false;
                report.violations.push(Violation::Strategy {
                    node,
                    player,
                    hand,
                    combo,
                    probabilities,
                    sum,
                });
            }
        }
    }
    distributions
}

/// (2) and (4) for the reaches, down the tree from the root.
fn check_reaches(solution: &impl Reported, report: &mut Report) {
    let game = solution.game();
    let tree = game.tree();
    let ones = [Player::Oop, Player::Ip].map(|player| vec![1.0; game.hands(player).len()]);
    let root = solution.reach(PostflopTree::ROOT);
    compare_reach(game, PostflopTree::ROOT, None, &ones, &root, report);
    let mut stack = vec![(PostflopTree::ROOT, root)];
    while let Some((node, reach)) = stack.pop() {
        let Node::Player {
            actor, children, ..
        } = tree.node(node)
        else {
            continue;
        };
        let me = actor.index();
        let num_hands = game.hands(*actor).len();
        let strategy = solution
            .strategy()
            .get(node as usize)
            .expect("the shapes are checked first");
        for (action, (&child, probabilities)) in children
            .iter()
            .zip(strategy.chunks_exact(num_hands))
            .enumerate()
        {
            let mut expected = reach.clone();
            for (r, &p) in expected[me].iter_mut().zip(probabilities) {
                *r *= f64::from(p);
            }
            let found = solution.reach(child);
            compare_reach(game, child, Some((node, action)), &expected, &found, report);
            stack.push((child, found));
        }
    }
}

/// Compares the reaches the solution reports at `node` with what they should be.
fn compare_reach(
    game: &RiverGame,
    node: u32,
    parent: Option<(u32, usize)>,
    expected: &[Vec<f64>; 2],
    found: &[Vec<f64>; 2],
    report: &mut Report,
) {
    for index in 0..2 {
        let player = player_of(index);
        let combos = game.hands(player).combos();
        for (hand, ((&expected, &found), &combo)) in expected[index]
            .iter()
            .zip(&found[index])
            .zip(combos)
            .enumerate()
        {
            if !expected.is_finite() {
                // Already reported, at a probability or a reach above.
                continue;
            }
            if !found.is_finite() {
                report.violations.push(Violation::NotFinite {
                    quantity: Quantity::Reach {
                        node,
                        player,
                        hand,
                        combo,
                    },
                    value: found,
                });
                continue;
            }
            let deviation = (found - expected).abs();
            report.worst.reach = report.worst.reach.max(deviation);
            if deviation > REACH_TOLERANCE {
                report.violations.push(Violation::Reach {
                    node,
                    parent,
                    player,
                    hand,
                    combo,
                    expected,
                    found,
                });
            }
        }
    }
}

/// (4) for the values, best responses and exploitability: whether both values are finite.
fn check_numbers(exploitability: &Exploitability, report: &mut Report) -> bool {
    let [oop, ip] = exploitability.values;
    let [oop_response, ip_response] = exploitability.best_responses;
    let numbers = [
        (Quantity::Value(Player::Oop), oop),
        (Quantity::Value(Player::Ip), ip),
        (Quantity::BestResponse(Player::Oop), oop_response),
        (Quantity::BestResponse(Player::Ip), ip_response),
        (Quantity::Exploitability, exploitability.chips),
        (Quantity::PercentOfPot, exploitability.percent_of_pot),
    ];
    for (quantity, value) in numbers {
        if !value.is_finite() {
            report
                .violations
                .push(Violation::NotFinite { quantity, value });
        }
    }
    exploitability.values.iter().all(|v| v.is_finite())
}

/// (3).
fn check_value_sum(solution: &impl Reported, report: &mut Report) {
    let game = solution.game();
    let expected = expected_rake(game, solution.strategy());
    let largest_pot = expected
        .terminals
        .iter()
        .map(|terminal| terminal.raked_pot)
        .fold(0.0, f64::max);
    let check = ValueSum {
        values: solution.exploitability().values,
        pot: game.pot(),
        probability: expected.probability,
        rake: expected.rake,
        largest_pot,
    };
    let relative = check.relative();
    if relative.is_finite() {
        report.worst.value_sum = relative;
    }
    if relative.is_nan() || relative > VALUE_TOLERANCE {
        report.violations.push(Violation::ValueSum(check));
    }
    report.value_sum = Some(check);
}

#[cfg(test)]
mod tests {
    use pk_cfr::{PublicGame, PublicNode, vector_expected_values};

    use super::*;
    use crate::solve::{SolveOptions, solve};
    use crate::spot::Spot;

    const SPOT: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../reference/spots/river/dry_ip_polarized.toml"
    );

    /// A raked spot with two bet sizes and a raise, solved to the default target. The solve
    /// checks it too, as every solve in a test build does.
    fn solved() -> Solution {
        solve(&Spot::load(SPOT).unwrap(), &SolveOptions::default(), |_| {}).unwrap()
    }

    /// A fault put into a solution's reaches.
    #[derive(Debug, Clone, Copy)]
    enum ReachFault {
        /// Multiplies `player`'s reach with `hand` by `factor` at `node` and at every node
        /// below it, as one wrong step that the rest of the walk follows would.
        Below {
            node: u32,
            player: usize,
            hand: usize,
            factor: f64,
        },
        /// Sets `player`'s reach with `hand` to `value` at `node` alone.
        At {
            node: u32,
            player: usize,
            hand: usize,
            value: f64,
        },
    }

    /// A solution with faults put in, which the checks read as they read a [`Solution`].
    struct Faulty<'a> {
        solution: &'a Solution,
        strategy: PublicProfile,
        exploitability: Exploitability,
        reach: Option<ReachFault>,
    }

    impl<'a> Faulty<'a> {
        /// The solution as it is.
        fn new(solution: &'a Solution) -> Faulty<'a> {
            Faulty {
                solution,
                strategy: solution.strategy().clone(),
                exploitability: *solution.exploitability(),
                reach: None,
            }
        }

        /// Sets the probabilities of the hand `hand` at `node`.
        fn with_row(mut self, node: u32, hand: usize, row: &[f32]) -> Faulty<'a> {
            let strategy = self.strategy.clone();
            self.strategy = PublicProfile::from_fn(self.solution.game(), |n, h| {
                if (n as u32, h) == (node, hand) {
                    row.to_vec()
                } else {
                    strategy.hand(n, h).unwrap()
                }
            });
            self
        }

        /// Sets one probability.
        fn with_probability(self, node: u32, hand: usize, action: usize, p: f32) -> Faulty<'a> {
            let mut row = self.strategy.hand(node as usize, hand).unwrap();
            row[action] = p;
            self.with_row(node, hand, &row)
        }

        fn with_values(mut self, values: [f64; 2]) -> Faulty<'a> {
            self.exploitability.values = values;
            self
        }

        fn with_reach(mut self, fault: ReachFault) -> Faulty<'a> {
            self.reach = Some(fault);
            self
        }

        fn report(&self) -> Report {
            check(self)
        }
    }

    impl Reported for Faulty<'_> {
        fn game(&self) -> &RiverGame {
            self.solution.game()
        }

        fn strategy(&self) -> &PublicProfile {
            &self.strategy
        }

        fn reach(&self, node: u32) -> [Vec<f64>; 2] {
            let game = self.solution.game();
            let mut reach = game.reach(&self.strategy, node);
            match self.reach {
                Some(ReachFault::Below {
                    node: at,
                    player,
                    hand,
                    factor,
                }) if node == at || game.path(node).iter().any(|&(step, _)| step == at) => {
                    reach[player][hand] *= factor;
                }
                Some(ReachFault::At {
                    node: at,
                    player,
                    hand,
                    value,
                }) if node == at => reach[player][hand] = value,
                _ => {}
            }
            reach
        }

        fn exploitability(&self) -> &Exploitability {
            &self.exploitability
        }
    }

    /// The node after OOP checks at the root, where IP acts, and the OOP hand that checks most
    /// often at the root.
    fn after_check(solution: &Solution) -> (u32, usize) {
        let node = solution.tree().node(PostflopTree::ROOT).children()[0];
        let checks = |h: usize| solution.hand_strategy(PostflopTree::ROOT, h).unwrap()[0];
        let hand = (0..solution.hands(Player::Oop).len())
            .max_by(|&a, &b| checks(a).total_cmp(&checks(b)))
            .unwrap();
        assert!(checks(hand) > 0.5);
        (node, hand)
    }

    fn combo(solution: &Solution, player: Player, hand: usize) -> Combo {
        solution.hands(player).combos()[hand]
    }

    fn row_sum(row: &[f32]) -> f64 {
        row.iter().map(|&p| f64::from(p)).sum()
    }

    #[test]
    fn a_solved_spot_keeps_every_invariant() {
        let solution = solved();
        let report = check_solution(&solution);
        assert!(report.is_valid(), "{report}");
        assert_eq!(report.to_string(), "no invariant is broken");
        // Every check was made, and found something to measure within its tolerance.
        let value_sum = report.value_sum.expect("the value sum is checked");
        assert_eq!(value_sum.values, solution.values());
        assert!(value_sum.rake > 0.0);
        assert!(report.worst.strategy > 0.0 && report.worst.strategy <= STRATEGY_TOLERANCE);
        assert!(report.worst.reach <= REACH_TOLERANCE);
        assert!(report.worst.value_sum <= VALUE_TOLERANCE);
        // Through the tests' wrapper with no fault put in, the solution reads the same.
        assert_eq!(Faulty::new(&solution).report(), report);
    }

    #[test]
    fn a_strategy_that_is_not_a_distribution_is_reported() {
        let solution = solved();
        let root = PostflopTree::ROOT;
        let (node, hand) = after_check(&solution);
        // A hand's probabilities at the root sum to 1 − 1e-5.
        let mut row = solution.hand_strategy(root, hand).unwrap();
        row[0] -= 1e-5;
        let report = Faulty::new(&solution).with_row(root, hand, &row).report();
        assert_eq!(
            report.violations,
            [Violation::Strategy {
                node: root,
                player: Player::Oop,
                hand,
                combo: combo(&solution, Player::Oop, hand),
                probabilities: row.clone(),
                sum: row_sum(&row),
            }]
        );
        assert!((row_sum(&row) - 1.0 + 1e-5).abs() < 1e-6);
        // The value sum is not checked on a strategy that is not a distribution.
        assert_eq!(report.value_sum, None);
        // Half the tolerance is not reported as a strategy fault. The values, though, are no
        // longer those of the strategy, by about 1.4e-5 chips, and the value sum notices.
        let mut row = solution.hand_strategy(root, hand).unwrap();
        row[0] -= (STRATEGY_TOLERANCE / 2.0) as f32;
        let report = Faulty::new(&solution).with_row(root, hand, &row).report();
        let invariants: Vec<Invariant> = report.violations.iter().map(|v| v.invariant()).collect();
        assert_eq!(invariants, [Invariant::ValueSum], "{report}");

        // A probability below 0, in a row that still sums to 1, at IP's node after a check.
        let ip = 3;
        let mut row = solution.hand_strategy(node, ip).unwrap();
        row[0] += row[1] + 0.25;
        row[1] = -0.25;
        let report = Faulty::new(&solution).with_row(node, ip, &row).report();
        assert_eq!(
            report.violations,
            [Violation::Strategy {
                node,
                player: Player::Ip,
                hand: ip,
                combo: combo(&solution, Player::Ip, ip),
                probabilities: row.clone(),
                sum: row_sum(&row),
            }]
        );
        assert!((row_sum(&row) - 1.0).abs() <= STRATEGY_TOLERANCE);
    }

    /// A [`RiverGame`] whose player node `node` has `num_actions` actions, or is a terminal if
    /// that is `None`, for building profiles of the wrong shape.
    struct Reshaped<'a> {
        game: &'a RiverGame,
        node: usize,
        num_actions: Option<usize>,
    }

    impl PublicGame for Reshaped<'_> {
        fn num_hands(&self, player: usize) -> usize {
            self.game.num_hands(player)
        }

        fn deal(&self, player: usize) -> &[f64] {
            self.game.deal(player)
        }

        fn num_nodes(&self) -> usize {
            self.game.num_nodes()
        }

        fn root(&self) -> usize {
            self.game.root()
        }

        fn node(&self, node: usize) -> PublicNode {
            match (self.game.node(node), node == self.node, self.num_actions) {
                (PublicNode::Player { player, .. }, true, Some(num_actions)) => {
                    PublicNode::Player {
                        player,
                        num_actions,
                    }
                }
                (_, true, None) => PublicNode::Terminal,
                (public, _, _) => public,
            }
        }

        fn child(&self, node: usize, index: usize) -> usize {
            self.game.child(node, index)
        }

        fn outcome_probability(&self, node: usize, outcome: usize) -> f64 {
            self.game.outcome_probability(node, outcome)
        }

        fn rules_out(&self, node: usize, outcome: usize, player: usize, hand: usize) -> bool {
            self.game.rules_out(node, outcome, player, hand)
        }

        fn terminal_values(&self, node: usize, player: usize, reach: &[f64], values: &mut [f64]) {
            self.game.terminal_values(node, player, reach, values);
        }
    }

    #[test]
    fn a_strategy_of_the_wrong_size_is_reported_and_nothing_else_is_read() {
        let solution = solved();
        let game = solution.game();
        let (node, _) = after_check(&solution);
        let num_hands = game.num_hands(1);
        let PublicNode::Player { num_actions, .. } = game.node(node as usize) else {
            unreachable!();
        };
        let expected = num_actions * num_hands;
        // No strategy at the node, then one with an action too many.
        for (shape, found) in [
            (None, None),
            (Some(num_actions + 1), Some(expected + num_hands)),
        ] {
            let reshaped = Reshaped {
                game,
                node: node as usize,
                num_actions: shape,
            };
            let mut faulty = Faulty::new(&solution);
            faulty.strategy = PublicProfile::from_fn(&reshaped, |n, h| {
                let PublicNode::Player { num_actions, .. } = reshaped.node(n) else {
                    unreachable!();
                };
                let mut row = solution.strategy().hand(n, h).unwrap();
                row.resize(num_actions, 0.0);
                row
            });
            // The reaches and the value sum would panic on this profile, so they are not
            // checked.
            let report = faulty.report();
            assert_eq!(
                report.violations,
                [Violation::StrategyShape {
                    node,
                    expected,
                    found
                }],
                "{shape:?}"
            );
            assert_eq!(report.value_sum, None);
        }
    }

    #[test]
    fn a_number_that_is_not_finite_is_reported_once_where_it_arises() {
        let solution = solved();
        let root = PostflopTree::ROOT;
        let (node, hand) = after_check(&solution);
        let oop = combo(&solution, Player::Oop, hand);

        // A NaN or infinite probability. The reaches below it and the value sum do not report
        // it again.
        for p in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let report = Faulty::new(&solution)
                .with_probability(root, hand, 1, p)
                .report();
            assert_eq!(report.violations.len(), 1, "{p}: {report}");
            let Violation::NotFinite { quantity, value } = report.violations[0] else {
                panic!("{p}: {report}");
            };
            assert_eq!(
                quantity,
                Quantity::Strategy {
                    node: root,
                    player: Player::Oop,
                    hand,
                    combo: oop,
                    action: 1,
                }
            );
            assert_eq!(value.to_bits(), f64::from(p).to_bits());
            assert_eq!(report.value_sum, None);
        }

        // A NaN reach at one node. The reaches below it, which should follow from it, do not
        // report it again.
        let report = Faulty::new(&solution)
            .with_reach(ReachFault::At {
                node,
                player: 0,
                hand,
                value: f64::NAN,
            })
            .report();
        assert_eq!(report.violations.len(), 1, "{report}");
        let Violation::NotFinite { quantity, value } = report.violations[0] else {
            panic!("{report}");
        };
        assert_eq!(
            quantity,
            Quantity::Reach {
                node,
                player: Player::Oop,
                hand,
                combo: oop,
            }
        );
        assert!(value.is_nan());
        assert!(report.value_sum.is_some());

        // Each of the numbers the solution reports. A value that is not finite leaves the value
        // sum unchecked; the others do not.
        type Corrupt = fn(&mut Exploitability);
        let numbers: [(Quantity, Corrupt); 6] = [
            (Quantity::Value(Player::Oop), |e| e.values[0] = f64::NAN),
            (Quantity::Value(Player::Ip), |e| e.values[1] = f64::INFINITY),
            (Quantity::BestResponse(Player::Oop), |e| {
                e.best_responses[0] = f64::NAN
            }),
            (Quantity::BestResponse(Player::Ip), |e| {
                e.best_responses[1] = f64::NEG_INFINITY
            }),
            (Quantity::Exploitability, |e| e.chips = f64::NAN),
            (Quantity::PercentOfPot, |e| e.percent_of_pot = f64::INFINITY),
        ];
        for (expected, corrupt) in numbers {
            let mut faulty = Faulty::new(&solution);
            corrupt(&mut faulty.exploitability);
            let report = faulty.report();
            assert_eq!(report.violations.len(), 1, "{expected}: {report}");
            let Violation::NotFinite { quantity, value } = report.violations[0] else {
                panic!("{report}");
            };
            assert_eq!(quantity, expected);
            assert!(!value.is_finite());
            let is_value = matches!(expected, Quantity::Value(_));
            assert_eq!(report.value_sum.is_none(), is_value, "{expected}");
        }
    }

    #[test]
    fn values_that_do_not_sum_to_the_pot_less_the_rake_are_reported() {
        let solution = solved();
        let [v0, v1] = solution.values();
        let reference = check_solution(&solution).value_sum.unwrap();
        let tolerance = VALUE_TOLERANCE * reference.largest_pot;
        // A hundredth of a chip too much for OOP.
        let report = Faulty::new(&solution).with_values([v0 + 0.01, v1]).report();
        assert_eq!(report.violations.len(), 1, "{report}");
        let Violation::ValueSum(check) = report.violations[0] else {
            panic!("{report}");
        };
        assert_eq!(check.values, [v0 + 0.01, v1]);
        assert_eq!(
            (check.probability, check.rake),
            (reference.probability, reference.rake)
        );
        assert!((check.difference() - 0.01).abs() < 1e-9, "{check:?}");
        assert_eq!(report.value_sum, Some(check));
        // Ten times the tolerance is reported, and a tenth of it is not.
        for (offset, reported) in [(10.0 * tolerance, true), (0.1 * tolerance, false)] {
            let report = Faulty::new(&solution)
                .with_values([v0, v1 - offset])
                .report();
            assert_eq!(!report.is_valid(), reported, "{offset}: {report}");
            assert_eq!(
                report.of(Invariant::ValueSum).count(),
                usize::from(reported)
            );
        }

        // The values of a solver that forgot the rake: the same strategy's in the rake-free
        // game. They are off by the expected rake.
        let mut free = Spot::load(SPOT).unwrap();
        free.rake_percent = 0.0;
        free.rake_cap = 0;
        let free = RiverGame::new(&free).unwrap();
        let without = vector_expected_values(&free, solution.strategy());
        let report = Faulty::new(&solution).with_values(without).report();
        assert_eq!(report.violations.len(), 1, "{report}");
        let Violation::ValueSum(check) = report.violations[0] else {
            panic!("{report}");
        };
        assert!(
            (check.difference() - reference.rake).abs() < 1e-9,
            "{check:?}"
        );
        assert!(reference.rake > 100.0);
    }

    #[test]
    fn a_reach_that_does_not_follow_from_its_parent_is_reported() {
        let solution = solved();
        let root = PostflopTree::ROOT;
        let (node, hand) = after_check(&solution);
        let checks = f64::from(solution.hand_strategy(root, hand).unwrap()[0]);
        let oop = combo(&solution, Player::Oop, hand);

        // OOP's reach after its check is half what its check gives, and so is every reach of
        // that hand below: one wrong step, reported once.
        let report = Faulty::new(&solution)
            .with_reach(ReachFault::Below {
                node,
                player: 0,
                hand,
                factor: 0.5,
            })
            .report();
        assert_eq!(
            report.violations,
            [Violation::Reach {
                node,
                parent: Some((root, 0)),
                player: Player::Oop,
                hand,
                combo: oop,
                expected: checks,
                found: 0.5 * checks,
            }]
        );

        // The reach of IP, which did not act at the root, and should keep its reach of it.
        let report = Faulty::new(&solution)
            .with_reach(ReachFault::Below {
                node,
                player: 1,
                hand: 2,
                factor: 0.5,
            })
            .report();
        assert_eq!(
            report.violations,
            [Violation::Reach {
                node,
                parent: Some((root, 0)),
                player: Player::Ip,
                hand: 2,
                combo: combo(&solution, Player::Ip, 2),
                expected: 1.0,
                found: 0.5,
            }]
        );

        // At the root, where every reach is 1.
        let report = Faulty::new(&solution)
            .with_reach(ReachFault::Below {
                node: root,
                player: 0,
                hand,
                factor: 0.5,
            })
            .report();
        assert_eq!(
            report.violations,
            [Violation::Reach {
                node: root,
                parent: None,
                player: Player::Oop,
                hand,
                combo: oop,
                expected: 1.0,
                found: 0.5,
            }]
        );

        // Wrong at one node only, while the reaches below it follow the strategy: the step
        // into the node and each step out of it are wrong, and each is reported.
        let report = Faulty::new(&solution)
            .with_reach(ReachFault::At {
                node,
                player: 0,
                hand,
                value: 0.5 * checks,
            })
            .report();
        let children = solution.tree().node(node).children();
        let mut steps: Vec<(u32, Option<(u32, usize)>)> = report
            .violations
            .iter()
            .map(|violation| match *violation {
                Violation::Reach {
                    node,
                    parent,
                    player: Player::Oop,
                    hand: h,
                    ..
                } if h == hand => (node, parent),
                ref other => panic!("{other}"),
            })
            .collect();
        steps.sort();
        let mut expected = vec![(node, Some((root, 0)))];
        expected.extend(
            children
                .iter()
                .enumerate()
                .map(|(action, &child)| (child, Some((node, action)))),
        );
        expected.sort();
        assert_eq!(steps, expected);
        // The value sum does not read the reaches the solution reports.
        assert!(report.value_sum.is_some());
    }

    #[test]
    fn every_violation_is_reported_not_just_the_first() {
        let solution = solved();
        let root = PostflopTree::ROOT;
        let (node, hand) = after_check(&solution);
        let mut row = solution.hand_strategy(node, 0).unwrap();
        row[0] *= 0.5;
        let mut faulty = Faulty::new(&solution)
            .with_probability(root, hand, 2, 0.75)
            .with_row(node, 0, &row)
            .with_probability(node, 1, 0, f32::NAN)
            .with_reach(ReachFault::Below {
                node,
                player: 1,
                hand: 2,
                factor: 2.0,
            });
        faulty.exploitability.best_responses[1] = f64::NAN;
        let report = faulty.report();
        let found: Vec<(Invariant, Option<u32>)> = report
            .violations
            .iter()
            .map(|violation| {
                let node = match *violation {
                    Violation::Strategy { node, .. }
                    | Violation::Reach { node, .. }
                    | Violation::NotFinite {
                        quantity: Quantity::Strategy { node, .. },
                        ..
                    } => Some(node),
                    _ => None,
                };
                (violation.invariant(), node)
            })
            .collect();
        assert_eq!(
            found,
            [
                (Invariant::Strategy, Some(root)),
                (Invariant::Strategy, Some(node)),
                (Invariant::Finite, Some(node)),
                (Invariant::Reach, Some(node)),
                (Invariant::Finite, None),
            ],
            "{report}"
        );
        assert_eq!(report.of(Invariant::Strategy).count(), 2);

        // With the strategies sound, a value fault and a reach fault are both reported.
        let [v0, v1] = solution.values();
        let report = Faulty::new(&solution)
            .with_values([v0, v1 + 1.0])
            .with_reach(ReachFault::Below {
                node,
                player: 0,
                hand,
                factor: 0.5,
            })
            .report();
        let invariants: Vec<Invariant> = report.violations.iter().map(|v| v.invariant()).collect();
        assert_eq!(
            invariants,
            [Invariant::Reach, Invariant::ValueSum],
            "{report}"
        );
    }

    #[test]
    fn the_debug_assertion_passes_a_sound_solution() {
        debug_assert_valid(&solved());
    }

    #[test]
    #[should_panic(
        expected = "the solve returned a solution that breaks its invariants: 1 violation of the \
                    invariants:\n- value sum: "
    )]
    fn the_debug_assertion_fails_loudly_on_a_broken_solution() {
        let solution = solved();
        let [v0, v1] = solution.values();
        assert_valid(&Faulty::new(&solution).with_values([v0, v1 + 1.0]).report());
    }

    #[test]
    fn the_report_says_which_invariant_where_and_by_how_much() {
        let solution = solved();
        let (node, hand) = after_check(&solution);
        let report = Faulty::new(&solution)
            .with_probability(PostflopTree::ROOT, hand, 0, 0.0)
            .with_reach(ReachFault::Below {
                node,
                player: 1,
                hand: 2,
                factor: 0.5,
            })
            .report();
        let text = report.to_string();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 3, "{text}");
        assert_eq!(lines[0], "2 violations of the invariants:");
        let oop = combo(&solution, Player::Oop, hand);
        assert!(
            lines[1].starts_with(&format!(
                "- strategy: at node 0, OOP hand {hand} ({oop}) plays [0.0, "
            )),
            "{text}"
        );
        assert!(lines[1].contains("from 1)"), "{text}");
        let ip = combo(&solution, Player::Ip, 2);
        assert_eq!(
            lines[2],
            format!(
                "- reach: IP's reach of node {node} with hand 2 ({ip}) is 0.5, but its reach of \
                 node 0 and action 0 there give 1 (-5e-1 off)"
            )
        );

        let text = Faulty::new(&solution)
            .with_values([0.0, 0.0])
            .report()
            .to_string();
        assert!(
            text.contains("- value sum: the values [0.0, 0.0] sum to 0, but the pot of 2000"),
            "{text}"
        );

        let text = Faulty::new(&solution)
            .with_probability(node, 3, 1, f32::NAN)
            .report()
            .to_string();
        let ip = combo(&solution, Player::Ip, 3);
        assert!(
            text.ends_with(&format!(
                "- finite: the probability of action 1 at node {node} with IP hand 3 ({ip}) is \
                 NaN"
            )),
            "{text}"
        );
    }
}
