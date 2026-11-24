//! Regret matching and the per-iteration update schedules of CFR, CFR+, linear CFR and
//! discounted CFR.
//!
//! Every function works on values laid out as `[actions × hands]`, action-major: the value for
//! action `a` and hand `h` is at index `a * num_hands + h`. A scalar solver, with one
//! information set per slice, passes `num_hands = 1`. A vector-form solver keeps one entry per
//! private hand. Values are stored as `f32` and updated in place, and nothing here allocates.
//!
//! # One iteration
//!
//! Iterations are numbered from `t = 1`, and regrets and strategy sums start at zero. For each
//! slice of regrets and its strategy sum, iteration `t` does this:
//!
//! 1. `σ_t` is the [`current_strategy`] of the stored regrets. On iteration 1 every regret is
//!    zero, so `σ_1` is uniform.
//! 2. The traversal plays `σ_t` and forms the instantaneous regret `r_t(a, h)`: the
//!    counterfactual value of taking action `a` with hand `h`, minus the counterfactual value of
//!    the node for hand `h`.
//! 3. [`update_regrets`] adds `r_t` to the stored regrets, then applies the discount
//!    `schedule.discount(t)`.
//! 4. [`accumulate_strategy`] adds `σ_t`, weighted by the acting player's own reach, to the
//!    strategy sum, then applies the same discount.
//!
//! Each call applies its iteration's discount, so both updates must be made exactly once per
//! iteration for every slice, even when `r_t` or the reach is zero. With alternating updates,
//! both players' updates within iteration `t` use the same `t`. The strategy to report is the
//! [`average_strategy`] of the strategy sum.
//!
//! ```
//! use pk_cfr::{
//!     Dcfr, Schedule, accumulate_strategy, average_strategy, current_strategy, update_regrets,
//! };
//!
//! // One information set with two actions, whose first action always gains regret 1.
//! let schedule = Schedule::Discounted(Dcfr::default());
//! let mut regrets = [0.0; 2];
//! let mut strategy_sum = [0.0; 2];
//! let mut strategy = [0.0; 2];
//! for t in 1..=3 {
//!     current_strategy(&regrets, 1, &mut strategy);
//!     let discount = schedule.discount(t);
//!     update_regrets(&mut regrets, &[1.0, -1.0], discount);
//!     accumulate_strategy(&mut strategy_sum, &strategy, &[1.0], discount);
//! }
//! // Iteration 1 played (1/2, 1/2) and iterations 2 and 3 played (1, 0). With γ = 2 they weigh
//! // 1, 4 and 9, so the average plays the first action with probability 13.5 / 14.
//! let mut average = [0.0; 2];
//! average_strategy(&strategy_sum, 1, &mut average);
//! assert!((average[0] - 27.0 / 28.0).abs() < 1e-6);
//! ```
//!
//! # References
//!
//! - Hart and Mas-Colell, *A Simple Adaptive Procedure Leading to Correlated Equilibrium*,
//!   Econometrica (2000): regret matching.
//! - Zinkevich, Johanson, Bowling and Piccione, *Regret Minimization in Games with Incomplete
//!   Information*, NIPS (2007): CFR, which plays regret matching at every information set and
//!   averages the strategies weighted by the player's own reach.
//! - Tammelin, *Solving Large Imperfect Information Games Using CFR+*, arXiv:1407.5042 (2014):
//!   regret matching+ and weighted averaging.
//! - Brown and Sandholm, *Solving Imperfect-Information Games via Discounted Regret
//!   Minimization*, AAAI (2019): linear CFR and discounted CFR.

/// How stored regrets and strategy sums are weighted across iterations.
///
/// On iteration `t`, counted from 1, and after that iteration's own contribution has been
/// added, every stored regret `x` is multiplied by `p_t` if `x > 0` and by `n_t` otherwise, and
/// every strategy-sum entry is multiplied by `s_t`:
///
/// | Schedule | `p_t` | `n_t` | `s_t` |
/// |---|---|---|---|
/// | [`Vanilla`](Schedule::Vanilla) | 1 | 1 | 1 |
/// | [`CfrPlus`](Schedule::CfrPlus) | 1 | 0 | t / (t + 1) |
/// | [`Linear`](Schedule::Linear) | t / (t + 1) | t / (t + 1) | t / (t + 1) |
/// | [`Discounted`](Schedule::Discounted) | t^α / (t^α + 1) | t^β / (t^β + 1) | (t / (t + 1))^γ |
///
/// The discount of iteration `t` falls on a sum that already holds iteration `t`, so after `T`
/// iterations the strategy played on iteration `t` carries the weight `s_t · s_{t+1} ⋯ s_T`.
/// For `s_k = k / (k + 1)` that product is `t / (T + 1)`, which is linear weighting, and for
/// `s_k = (k / (k + 1))^γ` it is `(t / (T + 1))^γ`. Brown and Sandholm discount only on
/// iterations `t ≤ t' < T`. The extra factor applied here on iteration `T` scales a hand's whole
/// strategy sum, and all of its positive regrets, by one amount, so it changes no strategy.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Schedule {
    /// CFR (Zinkevich et al. 2007). Regrets and strategies are plain sums: after `T`
    /// iterations, `R = Σ r_t` and `S = Σ π_t σ_t`.
    Vanilla,
    /// CFR+ (Tammelin 2014). Regret matching+ floors each regret at zero after adding,
    /// `R_t = max(0, R_{t-1} + r_t)`, and the average weighs iteration `t` by `t` (linear
    /// averaging with no delay): after `T` iterations, `S = Σ (t / (T + 1)) π_t σ_t`.
    ///
    /// Brown and Sandholm's experiments weigh CFR+ iterations by `t²`; this follows Tammelin.
    CfrPlus,
    /// Linear CFR (Brown and Sandholm 2019). Iteration `t` weighs `t` in both regrets and
    /// strategies: after `T` iterations, `R = Σ (t / (T + 1)) r_t` and
    /// `S = Σ (t / (T + 1)) π_t σ_t`. It equals `Discounted` with α = β = γ = 1.
    Linear,
    /// Discounted CFR, written DCFR(α, β, γ) (Brown and Sandholm 2019). Regrets that are
    /// positive after adding are multiplied by `t^α / (t^α + 1)`, the others by
    /// `t^β / (t^β + 1)`, and after `T` iterations `S = Σ (t / (T + 1))^γ π_t σ_t`.
    Discounted(Dcfr),
}

/// The parameters of discounted CFR. They must be finite.
///
/// The default, α = 3/2, β = 0 and γ = 2, is the setting Brown and Sandholm (2019) recommend.
/// With β = 0, negative regrets are halved on every iteration.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Dcfr {
    /// Exponent of the multiplier `t^α / (t^α + 1)` for positive regrets.
    pub alpha: f64,
    /// Exponent of the multiplier `t^β / (t^β + 1)` for regrets that are zero or negative.
    pub beta: f64,
    /// Exponent of the multiplier `(t / (t + 1))^γ` for the strategy sum.
    pub gamma: f64,
}

impl Default for Dcfr {
    fn default() -> Dcfr {
        Dcfr {
            alpha: 1.5,
            beta: 0.0,
            gamma: 2.0,
        }
    }
}

/// The multipliers one iteration applies, from [`Schedule::discount`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Discount {
    /// Multiplies a regret that is positive once the iteration's regret has been added.
    pub positive: f64,
    /// Multiplies a regret that is zero or negative once the iteration's regret has been added.
    pub negative: f64,
    /// Multiplies the strategy sum once the iteration's strategy has been added.
    pub strategy: f64,
}

impl Schedule {
    /// The multipliers `p_t`, `n_t` and `s_t` of iteration `t`, counted from 1, computed in
    /// `f64`.
    ///
    /// Panics if `t` is zero, or if a [`Dcfr`] parameter is not finite.
    pub fn discount(self, t: u64) -> Discount {
        assert!(t >= 1, "iterations are counted from 1");
        let t = t as f64;
        let linear = t / (t + 1.0);
        match self {
            Schedule::Vanilla => Discount {
                positive: 1.0,
                negative: 1.0,
                strategy: 1.0,
            },
            Schedule::CfrPlus => Discount {
                positive: 1.0,
                negative: 0.0,
                strategy: linear,
            },
            Schedule::Linear => Discount {
                positive: linear,
                negative: linear,
                strategy: linear,
            },
            Schedule::Discounted(Dcfr { alpha, beta, gamma }) => {
                assert!(
                    alpha.is_finite() && beta.is_finite() && gamma.is_finite(),
                    "DCFR parameters must be finite"
                );
                Discount {
                    positive: power_ratio(t, alpha),
                    negative: power_ratio(t, beta),
                    strategy: linear.powf(gamma),
                }
            }
        }
    }
}

/// `t^e / (t^e + 1)`, or 1 when `t^e` overflows an `f64`.
fn power_ratio(t: f64, exponent: f64) -> f64 {
    let power = t.powf(exponent);
    if power.is_infinite() {
        1.0
    } else {
        power / (power + 1.0)
    }
}

/// Regret matching: writes the strategy that plays each action in proportion to its positive
/// regret.
///
/// For each hand `h`, with `R⁺(a, h) = max(R(a, h), 0)`:
///
/// ```text
/// σ(a, h) = R⁺(a, h) / Σ_b R⁺(b, h)    if that sum is positive
///         = 1 / actions                otherwise
/// ```
///
/// The sum runs over the actions in order, in `f32`. Regret matching+ uses this same strategy;
/// it differs only in how the regrets are updated (see [`Schedule::CfrPlus`]).
///
/// Panics unless `regrets` is one or more whole rows of `num_hands` values and `strategy` has
/// the same length.
pub fn current_strategy(regrets: &[f32], num_hands: usize, strategy: &mut [f32]) {
    normalize_positive(regrets, num_hands, strategy);
}

/// Writes the average strategy: the strategy sum normalised per hand.
///
/// ```text
/// σ̄(a, h) = S(a, h) / Σ_b S(b, h)    if that sum is positive
///          = 1 / actions              otherwise, for a hand that was never reached
/// ```
///
/// Panics unless `strategy_sum` is one or more whole rows of `num_hands` values and `average`
/// has the same length.
pub fn average_strategy(strategy_sum: &[f32], num_hands: usize, average: &mut [f32]) {
    normalize_positive(strategy_sum, num_hands, average);
}

/// For each hand, writes each value's positive part divided by the hand's total of positive
/// parts, or `1 / actions` for every action when that total is zero.
fn normalize_positive(values: &[f32], num_hands: usize, out: &mut [f32]) {
    let num_actions = num_actions(values.len(), num_hands);
    assert_eq!(out.len(), values.len(), "output and input lengths differ");
    let uniform = 1.0 / num_actions as f32;
    let (first_values, later_values) = values.split_at(num_hands);
    // The first output row holds each hand's total until the last step.
    let (totals, later_out) = out.split_at_mut(num_hands);
    for (total, &value) in totals.iter_mut().zip(first_values) {
        *total = value.max(0.0);
    }
    for (row, values) in later_out
        .chunks_exact_mut(num_hands)
        .zip(later_values.chunks_exact(num_hands))
    {
        for ((out, &value), total) in row.iter_mut().zip(values).zip(totals.iter_mut()) {
            *out = value.max(0.0);
            *total += *out;
        }
    }
    for row in later_out.chunks_exact_mut(num_hands) {
        for (out, &total) in row.iter_mut().zip(totals.iter()) {
            *out = if total > 0.0 { *out / total } else { uniform };
        }
    }
    for (total, &value) in totals.iter_mut().zip(first_values) {
        *total = if *total > 0.0 {
            value.max(0.0) / *total
        } else {
            uniform
        };
    }
}

/// Adds one iteration's instantaneous regrets to the stored regrets, then applies that
/// iteration's discount, element by element:
///
/// ```text
/// x = R + r
/// R = x · positive    if x > 0
///   = x · negative    otherwise
/// ```
///
/// The sign test is on `x`, after the addition. `discount` must be `schedule.discount(t)` for
/// the iteration `t` that produced `r`. The factors are rounded to `f32` once, and the
/// arithmetic is `f32` in the order written. With [`Schedule::CfrPlus`] the negative factor is
/// zero, which is the regret-matching+ update `R = max(0, R + r)`.
///
/// The update is element by element, so it needs no layout. Panics if the slices differ in
/// length.
pub fn update_regrets(regrets: &mut [f32], instantaneous: &[f32], discount: Discount) {
    assert_eq!(
        regrets.len(),
        instantaneous.len(),
        "regret and instantaneous regret lengths differ"
    );
    let positive = discount.positive as f32;
    let negative = discount.negative as f32;
    for (regret, &r) in regrets.iter_mut().zip(instantaneous) {
        let x = *regret + r;
        debug_assert!(x.is_finite(), "regret {x} is not finite");
        *regret = x * if x > 0.0 { positive } else { negative };
    }
}

/// Adds one iteration's strategy, weighted by the acting player's reach, to the strategy sum,
/// then applies that iteration's discount:
///
/// ```text
/// S(a, h) = (S(a, h) + reach(h) · σ(a, h)) · discount.strategy
/// ```
///
/// `strategy` is `σ_t`, the strategy played on iteration `t`: the one [`current_strategy`]
/// produced before that iteration's [`update_regrets`]. `reach[h]` is the acting player's own
/// probability of reaching this node with hand `h` while playing `σ_t`, which is `π_i(I)` in
/// Zinkevich et al., so `num_hands` is `reach.len()`. `discount` must be `schedule.discount(t)`.
/// The factor is rounded to `f32` once, and the arithmetic is `f32` in the order written.
///
/// Panics unless `strategy` is one or more whole rows of `reach.len()` values and
/// `strategy_sum` has the same length.
pub fn accumulate_strategy(
    strategy_sum: &mut [f32],
    strategy: &[f32],
    reach: &[f32],
    discount: Discount,
) {
    let num_hands = reach.len();
    num_actions(strategy.len(), num_hands);
    assert_eq!(
        strategy_sum.len(),
        strategy.len(),
        "strategy sum and strategy lengths differ"
    );
    let factor = discount.strategy as f32;
    for (sums, probabilities) in strategy_sum
        .chunks_exact_mut(num_hands)
        .zip(strategy.chunks_exact(num_hands))
    {
        for ((sum, &probability), &weight) in sums.iter_mut().zip(probabilities).zip(reach) {
            debug_assert!(weight >= 0.0, "reach {weight} is not a probability");
            *sum = (*sum + weight * probability) * factor;
        }
    }
}

/// The number of actions in `len` values laid out as `[actions × num_hands]`.
fn num_actions(len: usize, num_hands: usize) -> usize {
    assert!(
        num_hands > 0 && len >= num_hands && len.is_multiple_of(num_hands),
        "{len} values are not one or more whole rows of {num_hands} hands"
    );
    len / num_hands
}

#[cfg(test)]
mod tests {
    use rand_chacha::ChaCha8Rng;
    use rand_chacha::rand_core::{Rng, SeedableRng};

    use super::*;

    const THIRD: f64 = 1.0 / 3.0;

    fn dcfr(alpha: f64, beta: f64, gamma: f64) -> Schedule {
        Schedule::Discounted(Dcfr { alpha, beta, gamma })
    }

    fn schedules() -> [Schedule; 5] {
        [
            Schedule::Vanilla,
            Schedule::CfrPlus,
            Schedule::Linear,
            Schedule::Discounted(Dcfr::default()),
            dcfr(0.5, -1.0, 3.0),
        ]
    }

    fn below(rng: &mut ChaCha8Rng, bound: usize) -> usize {
        ((u128::from(rng.next_u64()) * bound as u128) >> 64) as usize
    }

    fn uniform(rng: &mut ChaCha8Rng, low: f32, high: f32) -> f32 {
        let unit = (rng.next_u64() >> 40) as f32 / (1u64 << 24) as f32;
        low + (high - low) * unit
    }

    /// Asserts that every value is within `tolerance` times the largest expected magnitude (or
    /// times 1, if that is smaller).
    fn assert_close(actual: &[f32], expected: &[f64], tolerance: f64, what: &str) {
        assert_eq!(actual.len(), expected.len(), "{what}: lengths differ");
        let scale = expected.iter().fold(1.0f64, |max, &e| max.max(e.abs()));
        for (&a, &e) in actual.iter().zip(expected) {
            assert!(
                (f64::from(a) - e).abs() <= tolerance * scale,
                "{what}: got {actual:?}, expected {expected:?}"
            );
        }
    }

    fn strategy_of(regrets: &[f32], num_hands: usize) -> Vec<f32> {
        let mut strategy = vec![0.0; regrets.len()];
        current_strategy(regrets, num_hands, &mut strategy);
        strategy
    }

    fn average_of(strategy_sum: &[f32], num_hands: usize) -> Vec<f32> {
        let mut average = vec![0.0; strategy_sum.len()];
        average_strategy(strategy_sum, num_hands, &mut average);
        average
    }

    /// The state after one iteration of [`run`].
    struct Step {
        strategy: Vec<f32>,
        regrets: Vec<f32>,
        strategy_sum: Vec<f32>,
    }

    /// Runs a schedule on one slice, one iteration per row of `instantaneous`, with a fixed
    /// reach per hand.
    fn run(schedule: Schedule, instantaneous: &[Vec<f32>], reach: &[f32]) -> Vec<Step> {
        let len = instantaneous[0].len();
        let mut regrets = vec![0.0; len];
        let mut strategy_sum = vec![0.0; len];
        let mut steps = Vec::new();
        for (t, r) in (1..).zip(instantaneous) {
            let strategy = strategy_of(&regrets, reach.len());
            let discount = schedule.discount(t);
            update_regrets(&mut regrets, r, discount);
            accumulate_strategy(&mut strategy_sum, &strategy, reach, discount);
            steps.push(Step {
                strategy,
                regrets: regrets.clone(),
                strategy_sum: strategy_sum.clone(),
            });
        }
        steps
    }

    /// Instantaneous regrets for one hand with three actions, over three iterations. The worked
    /// examples below run every schedule on them with reach 1.
    const EXAMPLE: [[f32; 3]; 3] = [[6.0, -3.0, -3.0], [-6.0, 3.0, 9.0], [3.0, 0.0, -6.0]];

    /// Hand-computed values for the worked example: per iteration, the strategy played and the
    /// regrets and strategy sum after the update, then the strategy iteration 4 would play and
    /// the average strategy.
    struct Worked {
        strategies: [[f64; 3]; 3],
        regrets: [[f64; 3]; 3],
        strategy_sums: [[f64; 3]; 3],
        next_strategy: [f64; 3],
        average: [f64; 3],
    }

    fn check_worked_example(schedule: Schedule, worked: Worked) {
        let steps = run(schedule, &EXAMPLE.map(Vec::from), &[1.0]);
        for (t, step) in (1..).zip(&steps) {
            let i = t - 1;
            let what = |name: &str| format!("{schedule:?}, iteration {t}, {name}");
            assert_close(
                &step.strategy,
                &worked.strategies[i],
                1e-6,
                &what("strategy"),
            );
            assert_close(&step.regrets, &worked.regrets[i], 1e-6, &what("regrets"));
            assert_close(
                &step.strategy_sum,
                &worked.strategy_sums[i],
                1e-6,
                &what("strategy sum"),
            );
        }
        let last = steps.last().unwrap();
        assert_close(
            &strategy_of(&last.regrets, 1),
            &worked.next_strategy,
            1e-6,
            &format!("{schedule:?}, next strategy"),
        );
        assert_close(
            &average_of(&last.strategy_sum, 1),
            &worked.average,
            1e-6,
            &format!("{schedule:?}, average strategy"),
        );
    }

    #[test]
    fn regret_matching_normalises_positive_regrets_per_hand() {
        // Three actions and two hands. Hand 0 has regrets (2, -1, 6), so it plays 2/8, 0 and
        // 6/8. Hand 1 has regrets (-3, 0, -1), none positive, so it plays uniformly.
        let regrets = [2.0, -3.0, -1.0, 0.0, 6.0, -1.0];
        let expected = [0.25, THIRD, 0.0, THIRD, 0.75, THIRD];
        assert_close(&strategy_of(&regrets, 2), &expected, 1e-7, "strategy");
    }

    #[test]
    fn a_hand_plays_uniformly_only_when_no_regret_is_positive() {
        assert_eq!(strategy_of(&[0.0; 4], 1), [0.25; 4]);
        assert_eq!(strategy_of(&[0.0, 7.0, 0.0], 1), [0.0, 1.0, 0.0]);
        // A single action is played with certainty whatever its regret.
        assert_eq!(strategy_of(&[-5.0, 3.0], 2), [1.0, 1.0]);
    }

    #[test]
    fn average_strategy_normalises_the_sum_and_is_uniform_for_unreached_hands() {
        // Two actions and two hands. Hand 0 has sums (1, 3); hand 1 was never reached.
        let strategy_sum = [1.0, 0.0, 3.0, 0.0];
        assert_eq!(average_of(&strategy_sum, 2), [0.25, 0.5, 0.75, 0.5]);
    }

    #[test]
    fn schedules_give_the_documented_factors() {
        let cases = [
            (Schedule::Vanilla, 1, [1.0, 1.0, 1.0]),
            (Schedule::Vanilla, 4, [1.0, 1.0, 1.0]),
            (Schedule::CfrPlus, 1, [1.0, 0.0, 0.5]),
            (Schedule::CfrPlus, 4, [1.0, 0.0, 0.8]),
            (Schedule::Linear, 1, [0.5, 0.5, 0.5]),
            (Schedule::Linear, 4, [0.8, 0.8, 0.8]),
            // The default DCFR: 1^1.5 = 1, 4^1.5 = 8 and 9^1.5 = 27, while t^0 = 1.
            (dcfr(1.5, 0.0, 2.0), 1, [0.5, 0.5, 0.25]),
            (dcfr(1.5, 0.0, 2.0), 4, [8.0 / 9.0, 0.5, 16.0 / 25.0]),
            (dcfr(1.5, 0.0, 2.0), 9, [27.0 / 28.0, 0.5, 81.0 / 100.0]),
            (dcfr(2.0, 0.0, 2.0), 2, [0.8, 0.5, 4.0 / 9.0]),
            (dcfr(2.0, 0.0, 2.0), 3, [0.9, 0.5, 9.0 / 16.0]),
            // 4^0.5 = 2 and 4^-1 = 1/4, and γ = 0 weighs every iteration equally.
            (dcfr(0.5, -1.0, 0.0), 4, [2.0 / 3.0, 0.2, 1.0]),
        ];
        for (schedule, t, [positive, negative, strategy]) in cases {
            let discount = schedule.discount(t);
            for (got, expected) in [
                (discount.positive, positive),
                (discount.negative, negative),
                (discount.strategy, strategy),
            ] {
                assert!(
                    (got - expected).abs() < 1e-12,
                    "{schedule:?} at t = {t}: got {discount:?}"
                );
            }
        }
    }

    #[test]
    fn the_default_dcfr_is_brown_and_sandholms() {
        let defaults = Dcfr {
            alpha: 1.5,
            beta: 0.0,
            gamma: 2.0,
        };
        assert_eq!(Dcfr::default(), defaults);
    }

    #[test]
    fn regrets_are_discounted_by_their_sign_after_adding() {
        // The default DCFR at t = 4: positive regrets are multiplied by 8/9, the rest by 1/2.
        // x = R + r = (9, 18, -2, 0). The first entry was negative and the third positive
        // before adding; the sign that counts is the one after.
        let mut regrets = [-1.0, 20.0, 3.0, 2.0];
        let instantaneous = [10.0, -2.0, -5.0, -2.0];
        update_regrets(
            &mut regrets,
            &instantaneous,
            Schedule::Discounted(Dcfr::default()).discount(4),
        );
        assert_close(&regrets, &[8.0, 16.0, -1.0, 0.0], 1e-6, "regrets");
    }

    #[test]
    fn strategy_sums_add_reach_weighted_strategies_then_discount() {
        // Two actions and two hands at t = 4 of the default DCFR, so s_4 = (4/5)^2 = 16/25.
        // Hand 0 plays (1/2, 1/2) with reach 2 and hand 1 plays (1, 0) with reach 1/2, adding
        // (1, 1/2, 1, 0) to the sums (1, 2, 3, 0) for (2, 5/2, 4, 0) before the discount.
        let mut strategy_sum = [1.0, 2.0, 3.0, 0.0];
        accumulate_strategy(
            &mut strategy_sum,
            &[0.5, 1.0, 0.5, 0.0],
            &[2.0, 0.5],
            Schedule::Discounted(Dcfr::default()).discount(4),
        );
        assert_close(&strategy_sum, &[1.28, 1.6, 2.56, 0.0], 1e-6, "strategy sum");
    }

    #[test]
    fn vanilla_cfr_matches_the_worked_example() {
        // Plain sums: R_t = R_{t-1} + r_t and S_t = S_{t-1} + σ_t.
        check_worked_example(
            Schedule::Vanilla,
            Worked {
                strategies: [[THIRD; 3], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]],
                regrets: [[6.0, -3.0, -3.0], [0.0, 0.0, 6.0], [3.0, 0.0, 0.0]],
                strategy_sums: [
                    [THIRD; 3],
                    [4.0 / 3.0, THIRD, THIRD],
                    [4.0 / 3.0, THIRD, 4.0 / 3.0],
                ],
                next_strategy: [1.0, 0.0, 0.0],
                average: [4.0 / 9.0, 1.0 / 9.0, 4.0 / 9.0],
            },
        );
    }

    #[test]
    fn cfr_plus_matches_the_worked_example() {
        // R_t = max(0, R_{t-1} + r_t) and S_t = (S_{t-1} + σ_t) · t/(t+1).
        // t = 1: R = max(0, (6, -3, -3)) = (6, 0, 0); S = (1/3, 1/3, 1/3) · 1/2.
        // t = 2: σ = (1, 0, 0); R = max(0, (0, 3, 9)); S = (7/6, 1/6, 1/6) · 2/3.
        // t = 3: σ = (0, 3/12, 9/12); R = (3, 3, 3); S = (7/9, 13/36, 31/36) · 3/4.
        // The average weighs iterations 1, 2, 3 by 1, 2, 3: (7/3, 13/12, 31/12) / 6.
        check_worked_example(
            Schedule::CfrPlus,
            Worked {
                strategies: [[THIRD; 3], [1.0, 0.0, 0.0], [0.0, 0.25, 0.75]],
                regrets: [[6.0, 0.0, 0.0], [0.0, 3.0, 9.0], [3.0, 3.0, 3.0]],
                strategy_sums: [
                    [1.0 / 6.0; 3],
                    [7.0 / 9.0, 1.0 / 9.0, 1.0 / 9.0],
                    [7.0 / 12.0, 13.0 / 48.0, 31.0 / 48.0],
                ],
                next_strategy: [THIRD; 3],
                average: [7.0 / 18.0, 13.0 / 72.0, 31.0 / 72.0],
            },
        );
    }

    #[test]
    fn linear_cfr_matches_the_worked_example() {
        // R_t = (R_{t-1} + r_t) · t/(t+1) and S_t = (S_{t-1} + σ_t) · t/(t+1).
        // t = 1: R = (6, -3, -3) · 1/2.
        // t = 2: σ = (1, 0, 0); R = (-3, 3/2, 15/2) · 2/3 = (-2, 1, 5).
        // t = 3: σ = (0, 1/6, 5/6); R = (1, 1, -1) · 3/4; S = (7/9, 5/18, 17/18) · 3/4.
        // As a check, Σ t·r_t = (3, 3, -3), which is R_3 · (T + 1) with T = 3.
        check_worked_example(
            Schedule::Linear,
            Worked {
                strategies: [[THIRD; 3], [1.0, 0.0, 0.0], [0.0, 1.0 / 6.0, 5.0 / 6.0]],
                regrets: [[3.0, -1.5, -1.5], [-2.0, 1.0, 5.0], [0.75, 0.75, -0.75]],
                strategy_sums: [
                    [1.0 / 6.0; 3],
                    [7.0 / 9.0, 1.0 / 9.0, 1.0 / 9.0],
                    [7.0 / 12.0, 5.0 / 24.0, 17.0 / 24.0],
                ],
                next_strategy: [0.5, 0.5, 0.0],
                average: [7.0 / 18.0, 5.0 / 36.0, 17.0 / 36.0],
            },
        );
    }

    #[test]
    fn discounted_cfr_matches_the_worked_example() {
        // DCFR(2, 0, 2), whose factors are rational: p_t = t²/(t²+1), n_t = 1/2 and
        // s_t = (t/(t+1))², so (p, s) is (1/2, 1/4), (4/5, 4/9) and (9/10, 9/16) for t = 1, 2, 3.
        // t = 1: R = (6, -3, -3) · 1/2; S = (1/3, 1/3, 1/3) · 1/4.
        // t = 2: σ = (1, 0, 0); x = (-3, 3/2, 15/2), so R = (-3/2, 6/5, 6);
        //        S = (13/12, 1/12, 1/12) · 4/9.
        // t = 3: σ = (0, 1/6, 5/6); x = (3/2, 6/5, 0), so R = (27/20, 27/25, 0);
        //        S = (13/27, 11/54, 47/54) · 9/16.
        // As a check, S_3 = Σ (t/4)² σ_t.
        check_worked_example(
            dcfr(2.0, 0.0, 2.0),
            Worked {
                strategies: [[THIRD; 3], [1.0, 0.0, 0.0], [0.0, 1.0 / 6.0, 5.0 / 6.0]],
                regrets: [[3.0, -1.5, -1.5], [-1.5, 1.2, 6.0], [1.35, 1.08, 0.0]],
                strategy_sums: [
                    [1.0 / 12.0; 3],
                    [13.0 / 27.0, 1.0 / 27.0, 1.0 / 27.0],
                    [13.0 / 48.0, 11.0 / 96.0, 47.0 / 96.0],
                ],
                next_strategy: [5.0 / 9.0, 4.0 / 9.0, 0.0],
                average: [13.0 / 42.0, 11.0 / 84.0, 47.0 / 84.0],
            },
        );
    }

    #[test]
    fn hands_in_one_slice_update_independently() {
        // Hand 0 replays the worked example with reach 1, and hand 1 replays it negated with
        // reach 1/2. Each column must equal the same hand run alone, bit for bit.
        let negated = EXAMPLE.map(|row| row.map(|r| -r));
        let together: Vec<Vec<f32>> = EXAMPLE
            .iter()
            .zip(&negated)
            .map(|(first, second)| {
                first
                    .iter()
                    .zip(second)
                    .flat_map(|(&a, &b)| [a, b])
                    .collect()
            })
            .collect();
        let column = |values: &[f32], hand: usize| -> Vec<f32> {
            values.iter().skip(hand).step_by(2).copied().collect()
        };
        for schedule in schedules() {
            let both = run(schedule, &together, &[1.0, 0.5]);
            let alone = [
                run(schedule, &EXAMPLE.map(Vec::from), &[1.0]),
                run(schedule, &negated.map(Vec::from), &[0.5]),
            ];
            for (hand, steps) in alone.iter().enumerate() {
                for (step, single) in both.iter().zip(steps) {
                    assert_eq!(
                        column(&step.strategy, hand),
                        single.strategy,
                        "{schedule:?}"
                    );
                    assert_eq!(column(&step.regrets, hand), single.regrets, "{schedule:?}");
                    assert_eq!(
                        column(&step.strategy_sum, hand),
                        single.strategy_sum,
                        "{schedule:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn each_schedule_matches_its_definition_over_random_runs() {
        // Random layouts, regrets and reaches, checked against each schedule's definition
        // computed in f64: Zinkevich's plain sums, Tammelin's max(0, R + r) with weights t,
        // weights t for linear CFR, and the DCFR recurrence with weights t^γ.
        let mut rng = ChaCha8Rng::seed_from_u64(15);
        let iterations = 40;
        for schedule in schedules() {
            for _ in 0..25 {
                let num_actions = 1 + below(&mut rng, 4);
                let num_hands = 1 + below(&mut rng, 6);
                let len = num_actions * num_hands;
                let mut regrets = vec![0.0; len];
                let mut strategy_sum = vec![0.0; len];
                let mut regret_oracle = vec![0.0f64; len];
                let mut sum_oracle = vec![0.0f64; len];
                for t in 1..=iterations {
                    let strategy = strategy_of(&regrets, num_hands);
                    let instantaneous: Vec<f32> =
                        (0..len).map(|_| uniform(&mut rng, -1.0, 1.0)).collect();
                    let reach: Vec<f32> = (0..num_hands)
                        .map(|_| match below(&mut rng, 4) {
                            0 => 0.0,
                            _ => uniform(&mut rng, 0.0, 1.0),
                        })
                        .collect();
                    let discount = schedule.discount(t);
                    update_regrets(&mut regrets, &instantaneous, discount);
                    accumulate_strategy(&mut strategy_sum, &strategy, &reach, discount);

                    let t = t as f64;
                    for (i, (oracle, &r)) in
                        regret_oracle.iter_mut().zip(&instantaneous).enumerate()
                    {
                        let r = f64::from(r);
                        let contribution = f64::from(reach[i % num_hands]) * f64::from(strategy[i]);
                        let (regret, weight) = match schedule {
                            Schedule::Vanilla => (*oracle + r, 1.0),
                            Schedule::CfrPlus => ((*oracle + r).max(0.0), t),
                            Schedule::Linear => (*oracle + t * r, t),
                            Schedule::Discounted(Dcfr { alpha, beta, gamma }) => {
                                let x = *oracle + r;
                                let power = t.powf(if x > 0.0 { alpha } else { beta });
                                (x * power / (power + 1.0), t.powf(gamma))
                            }
                        };
                        *oracle = regret;
                        sum_oracle[i] += weight * contribution;
                    }
                }
                let end = iterations as f64 + 1.0;
                let (regret_scale, sum_scale) = match schedule {
                    Schedule::Vanilla => (1.0, 1.0),
                    Schedule::CfrPlus => (1.0, end),
                    Schedule::Linear => (end, end),
                    Schedule::Discounted(Dcfr { gamma, .. }) => (1.0, end.powf(gamma)),
                };
                let what = format!("{schedule:?}, {num_actions} actions × {num_hands} hands");
                let expected: Vec<f64> = regret_oracle.iter().map(|r| r / regret_scale).collect();
                assert_close(&regrets, &expected, 1e-5, &format!("{what}, regrets"));
                let expected: Vec<f64> = sum_oracle.iter().map(|s| s / sum_scale).collect();
                assert_close(
                    &strategy_sum,
                    &expected,
                    1e-5,
                    &format!("{what}, strategy sum"),
                );
            }
        }
    }

    #[test]
    fn dcfr_with_unit_parameters_plays_like_linear_cfr() {
        let mut rng = ChaCha8Rng::seed_from_u64(1);
        let (num_actions, num_hands) = (3, 4);
        let len = num_actions * num_hands;
        let reach: Vec<f32> = (0..num_hands)
            .map(|_| uniform(&mut rng, 0.0, 1.0))
            .collect();
        let instantaneous: Vec<Vec<f32>> = (0..100)
            .map(|_| (0..len).map(|_| uniform(&mut rng, -1.0, 1.0)).collect())
            .collect();
        let linear = run(Schedule::Linear, &instantaneous, &reach);
        let unit = run(dcfr(1.0, 1.0, 1.0), &instantaneous, &reach);
        for (t, (linear, unit)) in (1..).zip(linear.iter().zip(&unit)) {
            let expected: Vec<f64> = linear.strategy.iter().map(|&p| f64::from(p)).collect();
            assert_close(&unit.strategy, &expected, 1e-6, &format!("strategy {t}"));
            let expected: Vec<f64> = average_of(&linear.strategy_sum, num_hands)
                .iter()
                .map(|&p| f64::from(p))
                .collect();
            assert_close(
                &average_of(&unit.strategy_sum, num_hands),
                &expected,
                1e-6,
                &format!("average {t}"),
            );
        }
    }

    #[test]
    fn scaling_a_hands_regrets_leaves_its_strategy_unchanged() {
        let mut rng = ChaCha8Rng::seed_from_u64(2);
        for _ in 0..500 {
            let num_actions = 1 + below(&mut rng, 5);
            let num_hands = 1 + below(&mut rng, 6);
            let regrets: Vec<f32> = (0..num_actions * num_hands)
                .map(|_| uniform(&mut rng, -10.0, 10.0))
                .collect();
            let hand = below(&mut rng, num_hands);
            let strategy = strategy_of(&regrets, num_hands);
            let scaled_by = |scale: f32| -> Vec<f32> {
                let mut scaled = regrets.clone();
                for value in scaled.iter_mut().skip(hand).step_by(num_hands) {
                    *value *= scale;
                }
                strategy_of(&scaled, num_hands)
            };
            // A power of two scales exactly, so the strategy is bit for bit the same.
            let power_of_two = 2.0f32.powi(below(&mut rng, 41) as i32 - 20);
            assert_eq!(scaled_by(power_of_two), strategy, "scale {power_of_two}");
            let scale = uniform(&mut rng, -10.0, 10.0).exp();
            let expected: Vec<f64> = strategy.iter().map(|&p| f64::from(p)).collect();
            assert_close(
                &scaled_by(scale),
                &expected,
                1e-6,
                &format!("scale {scale}"),
            );
        }
    }

    #[test]
    fn matched_strategies_are_distributions() {
        let mut rng = ChaCha8Rng::seed_from_u64(3);
        for _ in 0..500 {
            let num_actions = 1 + below(&mut rng, 5);
            let num_hands = 1 + below(&mut rng, 6);
            let regrets: Vec<f32> = (0..num_actions * num_hands)
                .map(|_| uniform(&mut rng, -10.0, 10.0))
                .collect();
            let strategy = strategy_of(&regrets, num_hands);
            for hand in 0..num_hands {
                let column = strategy.iter().skip(hand).step_by(num_hands);
                assert!(column.clone().all(|&p| (0.0..=1.0).contains(&p)));
                let total: f64 = column.map(|&p| f64::from(p)).sum();
                assert!((total - 1.0).abs() < 1e-6, "{regrets:?} gives {strategy:?}");
            }
        }
    }

    /// The payoffs of rock, paper, scissors and a fourth action that loses 1 to each of them,
    /// when rock beating scissors pays 2. The game is symmetric with value zero, and its only
    /// equilibrium plays (1/4, 1/2, 1/4, 0). The fourth action keeps the equilibrium from being
    /// fully mixed, so self-play with the sign of the regrets flipped ends somewhere else.
    const PAYOFF: [[f64; 4]; 4] = [
        [0.0, -1.0, 2.0, 1.0],
        [1.0, 0.0, -1.0, 1.0],
        [-2.0, 1.0, 0.0, 1.0],
        [-1.0, -1.0, -1.0, 0.0],
    ];

    fn values_against(opponent: &[f32]) -> [f64; 4] {
        PAYOFF.map(|row| {
            row.iter()
                .zip(opponent)
                .map(|(&u, &p)| u * f64::from(p))
                .sum()
        })
    }

    #[test]
    fn alternating_self_play_finds_the_equilibrium_under_every_schedule() {
        let schedules = [
            Schedule::Vanilla,
            Schedule::CfrPlus,
            Schedule::Linear,
            Schedule::Discounted(Dcfr::default()),
        ];
        for schedule in schedules {
            let mut regrets = [[0.0f32; 4]; 2];
            let mut strategy_sums = [[0.0f32; 4]; 2];
            for t in 1..=1000 {
                let discount = schedule.discount(t);
                for player in 0..2 {
                    let opponent = strategy_of(&regrets[1 - player], 1);
                    let strategy = strategy_of(&regrets[player], 1);
                    let values = values_against(&opponent);
                    let value: f64 = values
                        .iter()
                        .zip(&strategy)
                        .map(|(&v, &p)| v * f64::from(p))
                        .sum();
                    let instantaneous = values.map(|v| (v - value) as f32);
                    update_regrets(&mut regrets[player], &instantaneous, discount);
                    accumulate_strategy(&mut strategy_sums[player], &strategy, &[1.0], discount);
                }
            }
            let exploitability: f64 = strategy_sums
                .iter()
                .map(|sum| {
                    values_against(&average_of(sum, 1))
                        .into_iter()
                        .fold(f64::MIN, f64::max)
                })
                .sum();
            assert!(
                exploitability < 0.01,
                "{schedule:?}: exploitability {exploitability}"
            );
        }
    }

    #[test]
    #[should_panic(expected = "counted from 1")]
    fn iteration_zero_is_rejected() {
        Schedule::Linear.discount(0);
    }

    #[test]
    #[should_panic(expected = "whole rows")]
    fn a_slice_of_partial_rows_is_rejected() {
        current_strategy(&[0.0; 5], 2, &mut [0.0; 5]);
    }
}
