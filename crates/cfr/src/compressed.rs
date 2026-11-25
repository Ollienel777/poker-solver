//! Regrets and strategy sums stored in 16 bits with one scale per slice: the storage that
//! [`VectorCfr`](crate::vector::VectorCfr) keeps under [`CompressedStorage`], in place of the
//! `f32` slices of [`crate::regret`], to halve the memory they take.
//!
//! # The encoding
//!
//! A slice of regrets, laid out `[actions × hands]` as in [`crate::regret`], is kept as one
//! `i16` code `c` per entry and one `f32` scale `k` for the whole slice, and stands for the
//! regrets `c · k`, each product rounded to `f32`. Codes run from −32,767 to 32,767
//! ([`REGRET_CODE_MAX`]), so a code and its negation are both codes. A slice of strategy sums is
//! kept as one `u16` code per entry, from 0 to 65,535 ([`SUM_CODE_MAX`]), and one `f32` scale,
//! and stands for the sums `c · k`. A slice of zeros has every code 0 and scale 0. A
//! [`VectorCfr`](crate::vector::VectorCfr) keeps one slice of each per player node, so the scale
//! is per node.
//!
//! # Rounding
//!
//! A value `y` in units of a scale is written as the code `⌊y + u⌋`, for a rounding offset `u`
//! from 0 up to but not including 1 that is the same for every entry of the slice. Each function
//! here takes `u` as an argument, and the sections below say where the floor is taken. A regret
//! code is clamped to ±32,767 and a sum code to 65,535 after it. With `u = 1/2` ([`NEAREST`])
//! the code is the value rounded to nearest, ties upwards, within half a step of it.
//!
//! [`CompressedStorage`] does not round to nearest. The update of node `n` on iteration `t`
//! rounds its strategy sums with the offset `u_t(n)`, the fractional part of `β(n) + t · φ`,
//! where `φ = (√5 − 1) / 2` and the start `β(n)` is hashed from the node index, taken down to a
//! multiple of 2^-24, and its regrets with `u_t(n) + 1/2` modulo 1 ([`offsets`]). Over the
//! iterations, `t · φ` modulo 1 is spread as evenly as a sequence can be (a Weyl sequence of low
//! discrepancy: of any `N` consecutive offsets, the share in an interval of length `ℓ` is within
//! O(log N / N) of `ℓ`). So a value whose fractional part `f`, in units of the scale, changes
//! slowly from one iteration to the next is rounded up on a share of about `f` of the iterations
//! and down on the rest, and its rounding errors cancel over the iterations instead of adding
//! up. The start differs from node to node, so that the nodes do not round up and down in step.
//!
//! Rounding to nearest loses what changes by less than half a step at each update, for good.
//! A strategy sum under DCFR gains, on iteration `t`, about `3/t` of what it holds, so after a
//! few thousand iterations the increment of every entry but the largest is below half a step:
//! rounded to nearest, the average strategy freezes and its exploitability grows again. A
//! regret's change falls below half a step whenever the node's largest regret keeps growing, as
//! a dominated action's does under CFR and linear CFR, which do not halve negative regrets as
//! DCFR(1.5, 0, 2) does. An offset drawn afresh at each update from a hash of the node and the
//! iteration is right on average over the iterations, but its errors add up like a random walk,
//! and the exploitability grows as the square root of the iterations. `BENCHMARKS.md` gives the
//! measurements, and DECISIONS P66 the choice.
//!
//! The offset is one number for the whole slice, a function of the node and the iteration only,
//! and every other step is applied entry by entry or is the largest value over the slice, which
//! does not depend on the order of the hands. So entries that hold the same value before an
//! update and receive the same increments hold the same codes after it: hands that a game names
//! alike (see [`crate::vector#alike-hands`]) stay alike, bit for bit, as with `f32` storage. And
//! an update reads nothing but its slice and its arguments, so the parallel walk's results do
//! not depend on the thread count.
//!
//! # Regrets
//!
//! [`update_regrets_16`] is [`update_regrets`](crate::regret::update_regrets) on the decoded
//! regrets, then a new encoding. With `p` and `n` the iteration's factors rounded to `f32`:
//!
//! 1. For each entry, `x = c · k + r`, then `x · p` if `x > 0` and `x · n` otherwise, each step
//!    rounded to `f32`, as in the `f32` update.
//! 2. `m = max |x|` over the slice. The new scale is `k = m / 32767`, and each code is
//!    `⌊x · (32767 / m) + u⌋`, clamped to ±32,767, the two quotients and the product rounded to
//!    `f32` and the floor taken exactly of the `f32` sum. So the entry of largest magnitude gets
//!    the code ±32,767 when rounding to nearest, and no code can overflow, however much the
//!    regrets grew.
//! 3. If that scale would be below 2^-115, [`MIN_SCALE`], which happens only when every regret
//!    in the slice is below about 2^-100 in magnitude, the slice is stored as zeros instead.
//!
//! The discount of DCFR depends on each regret's sign, so every regret is requantized at every
//! update. A regret of CFR+ that the update floors at zero is `x · 0`, which is code 0.
//!
//! [`current_strategy_16`] is regret matching on the codes themselves: each hand plays each
//! action in proportion to its positive code, or uniformly when none is positive. Regret
//! matching does not change when a hand's regrets are all scaled by one positive number, so this
//! is regret matching on the regrets `c · k`, without the scale; and the codes and their sums
//! are whole numbers below 2^24, exact in `f32`, so each probability is a correctly rounded
//! quotient.
//!
//! # Strategy sums
//!
//! The discount of a strategy sum is one factor `s` for the whole slice, so it is put on the
//! scale and no code moves for it: from `S = c · k`, the update `S' = (S + reach · σ) · s` of
//! [`accumulate_strategy`](crate::regret::accumulate_strategy) is `c' · k'` with `k' = k · s` and
//! `c' = c + reach · σ / k`. [`accumulate_strategy_16`] does this, each step rounded to `f32`:
//!
//! 1. For each entry, the increment `g = (reach(h) · σ(a, h)) · (1 / k)`, in units of the old
//!    scale; in a slice of zeros, `g = reach(h) · σ(a, h)`, in units of 1.
//! 2. If the slice was not zeros and every `c + ⌊g + u⌋` is at most 65,535, that is each new
//!    code, added as a whole number, and the new scale is `k · s`. An entry that gains nothing
//!    keeps its code exactly.
//! 3. Otherwise the slice is requantized: with `m` the largest `c + g`, each code is
//!    `⌊(c + g) · (2^15 / m) + u⌋` and the new scale is `k · s · (m / 2^15)`, with `k = 1` for a
//!    slice of zeros. The largest code is then 2^15, half of what a code can hold, so that the
//!    sums can double before the next requantization; with codes that never fall, the largest
//!    code is always from 2^15 to 65,535.
//! 4. If the new scale is below 2^-115, [`MIN_SCALE`], the slice is stored as zeros instead, so
//!    that `1 / k` is always finite. A slice of zeros that gains nothing stays zeros.
//!
//! [`average_strategy_16`] normalises the codes per hand, as
//! [`average_strategy`](crate::regret::average_strategy) normalises the sums, again without the
//! scale.
//!
//! # What is lost
//!
//! Each update rounds each regret, and each strategy sum's increment, to a step of the slice's
//! scale, which is set by the largest value of the slice: 1/32,767 of the largest regret's
//! magnitude, and from 1/65,535 to 1/32,768 of the largest strategy sum. A hand whose values are
//! tens of thousands of times smaller than the largest of its node's is stored coarsely or as
//! zero, and a change smaller than a step is kept only on average over the iterations, through
//! the offsets. `BENCHMARKS.md` measures what that costs: on the test spots, a compressed solve
//! keeps up with the `f32` solve until its exploitability is a few thousandths of a percent of
//! the pot, and stops improving there, from 0.0002% to 0.003% on the spots measured, where the
//! `f32` solve goes on.

use crate::regret::{Discount, normalize_positive, num_actions};
use crate::vector::sealed;

/// The largest magnitude of a regret code.
pub const REGRET_CODE_MAX: i16 = 32_767;

/// The largest strategy-sum code.
pub const SUM_CODE_MAX: u16 = 65_535;

/// The largest strategy-sum code right after a requantization: 2^15.
const SUM_REQUANTIZED: f32 = 32_768.0;

/// The smallest scale a slice that is not zeros may have: 2^-115. A smaller one would make
/// `1 / k`, or `32767 / m` for its largest regret `m`, overflow an `f32`.
pub const MIN_SCALE: f32 = f32::from_bits(12 << 23);

/// The rounding offset that rounds to nearest, ties upwards.
pub const NEAREST: f32 = 0.5;

/// `(√5 − 1) / 2`, the step of the sequence of [`offsets`], as the nearest `f64`.
const GOLDEN: f64 = 0.618_033_988_749_894_8;

/// 2^24, the offsets' resolution.
const OFFSET_STEPS: f64 = 16_777_216.0;

/// Stores each player node's regrets as `i16` codes and its strategy sums as `u16` codes, each
/// slice with one `f32` scale, as the [module documentation](self) specifies, rounded with the
/// offsets of [`offsets`]. Under it a [`VectorCfr`](crate::vector::VectorCfr) holds half the
/// memory for its regrets and strategy sums that it does under
/// [`F32Storage`](crate::vector::F32Storage), and its iterations differ from those by the
/// rounding.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CompressedStorage;

/// The stored state of one player node under [`CompressedStorage`]: its regret codes and their
/// scale, and its strategy-sum codes and theirs.
#[derive(Debug, Clone)]
pub struct CompressedRow {
    pub(crate) regrets: Vec<i16>,
    pub(crate) regret_scale: f32,
    pub(crate) strategy_sum: Vec<u16>,
    pub(crate) sum_scale: f32,
}

impl crate::vector::Storage for CompressedStorage {}

impl sealed::Storage for CompressedStorage {
    type Row = CompressedRow;

    fn zeros(len: usize) -> CompressedRow {
        CompressedRow {
            regrets: vec![0; len],
            regret_scale: 0.0,
            strategy_sum: vec![0; len],
            sum_scale: 0.0,
        }
    }

    fn len(row: &CompressedRow) -> usize {
        row.regrets.len()
    }

    fn current_strategy(row: &CompressedRow, num_hands: usize, strategy: &mut [f32]) {
        current_strategy_16(&row.regrets, num_hands, strategy);
    }

    fn average_strategy(row: &CompressedRow, num_hands: usize, average: &mut [f32]) {
        average_strategy_16(&row.strategy_sum, num_hands, average);
    }

    fn update(
        row: &mut CompressedRow,
        instantaneous: &mut [f32],
        strategy: &[f32],
        reach: &[f32],
        discount: Discount,
        node: usize,
        iteration: u64,
    ) {
        let [regret_offset, sum_offset] = offsets(node, iteration);
        update_regrets_16(
            &mut row.regrets,
            &mut row.regret_scale,
            instantaneous,
            discount,
            regret_offset,
        );
        accumulate_strategy_16(
            &mut row.strategy_sum,
            &mut row.sum_scale,
            strategy,
            reach,
            discount,
            sum_offset,
        );
    }

    fn decoded(row: &CompressedRow) -> (Vec<f32>, Vec<f32>) {
        let mut regrets = vec![0.0; row.regrets.len()];
        decode_regrets(&row.regrets, row.regret_scale, &mut regrets);
        let mut sums = vec![0.0; row.strategy_sum.len()];
        decode_strategy_sum(&row.strategy_sum, row.sum_scale, &mut sums);
        (regrets, sums)
    }
}

/// The rounding offsets of the regrets and of the strategy sums of the player node `node` at
/// its update on iteration `iteration`, as the [module documentation](self#rounding) defines
/// them. The sums' is `⌊2^24 · x⌋ / 2^24`, where `x` is the fractional part of
/// `β + iteration · φ` computed in `f64`, `φ` is `(√5 − 1) / 2` as the nearest `f64`, and `β` is
/// the top 24 bits of the first output of SplitMix64 seeded with `node`, over 2^24; the regrets'
/// is the sums' plus 1/2, modulo 1. Each is a multiple of 2^-24 from 0 to `1 − 2^-24`, exact in
/// `f32`.
pub fn offsets(node: usize, iteration: u64) -> [f32; 2] {
    let mut z = (node as u64).wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    let start = (z >> 40) as f64 / OFFSET_STEPS;
    let x = (start + iteration as f64 * GOLDEN).fract();
    // Not negative, so the cast's truncation is the floor.
    let steps = (x * OFFSET_STEPS) as u32;
    let half = 1 << 23;
    let regrets = (steps + half) % (1 << 24);
    [regrets, steps].map(|steps| steps as f32 / OFFSET_STEPS as f32)
}

/// `⌊z⌋` of an `f32` of magnitude below 2^24, exactly.
fn floor(z: f32) -> i32 {
    let truncated = z as i32;
    truncated - i32::from(truncated as f32 > z)
}

/// Adds one iteration's instantaneous regrets `instantaneous` to the regrets stored as `codes`
/// and `scale`, applies the iteration's discount and writes them back, as the
/// [module documentation](self#regrets) specifies, with the rounding offset `offset`. The
/// regrets after the update, before they are encoded, are left in `instantaneous`.
///
/// Panics if the slices differ in length.
pub fn update_regrets_16(
    codes: &mut [i16],
    scale: &mut f32,
    instantaneous: &mut [f32],
    discount: Discount,
    offset: f32,
) {
    assert_eq!(
        codes.len(),
        instantaneous.len(),
        "regret and instantaneous regret lengths differ"
    );
    debug_assert!(
        (0.0..1.0).contains(&offset),
        "offset {offset} is not in [0, 1)"
    );
    let positive = discount.positive as f32;
    let negative = discount.negative as f32;
    let k = *scale;
    let mut largest = 0.0f32;
    for (x, &code) in instantaneous.iter_mut().zip(codes.iter()) {
        let sum = f32::from(code) * k + *x;
        debug_assert!(sum.is_finite(), "regret {sum} is not finite");
        *x = sum * if sum > 0.0 { positive } else { negative };
        largest = largest.max(x.abs());
    }
    let max = f32::from(REGRET_CODE_MAX);
    let new_scale = largest / max;
    if new_scale.is_nan() || new_scale < MIN_SCALE {
        codes.fill(0);
        *scale = 0.0;
        return;
    }
    let to_codes = max / largest;
    let limit = i32::from(REGRET_CODE_MAX);
    for (code, &x) in codes.iter_mut().zip(instantaneous.iter()) {
        *code = floor(x * to_codes + offset).clamp(-limit, limit) as i16;
    }
    *scale = new_scale;
}

/// Adds one iteration's strategy `strategy`, weighted by the acting player's reach `reach`, to
/// the strategy sums stored as `codes` and `scale`, and applies the iteration's discount, as the
/// [module documentation](self#strategy-sums) specifies, with the rounding offset `offset`.
///
/// Panics unless `strategy` is one or more whole rows of `reach.len()` values and `codes` has
/// the same length.
pub fn accumulate_strategy_16(
    codes: &mut [u16],
    scale: &mut f32,
    strategy: &[f32],
    reach: &[f32],
    discount: Discount,
    offset: f32,
) {
    let num_hands = reach.len();
    num_actions(strategy.len(), num_hands);
    assert_eq!(
        codes.len(),
        strategy.len(),
        "strategy sum and strategy lengths differ"
    );
    debug_assert!(
        (0.0..1.0).contains(&offset),
        "offset {offset} is not in [0, 1)"
    );
    let factor = discount.strategy as f32;
    let k = *scale;
    let zeros = k == 0.0;
    // The increments in units of the old scale, or of 1 for a slice of zeros.
    let unit = if zeros { 1.0 } else { 1.0 / k };
    let gain = |weight: f32, probability: f32| {
        debug_assert!(weight >= 0.0, "reach {weight} is not a probability");
        (weight * probability) * unit
    };
    // The largest code with its increment rounded, and the largest value in units of the old
    // scale. Neither is negative, so a cast's truncation is the floor.
    let (mut largest_code, mut largest) = (0u32, 0.0f32);
    for (codes, probabilities) in codes
        .chunks_exact(num_hands)
        .zip(strategy.chunks_exact(num_hands))
    {
        for ((&code, &probability), &weight) in codes.iter().zip(probabilities).zip(reach) {
            let increment = gain(weight, probability);
            let rounded = u32::from(code).saturating_add((increment + offset) as u32);
            largest_code = largest_code.max(rounded);
            largest = largest.max(f32::from(code) + increment);
        }
    }
    if largest == 0.0 {
        // A slice of zeros that gains nothing.
        return;
    }
    let fits = !zeros && largest_code <= u32::from(SUM_CODE_MAX);
    let new_scale = if fits {
        k * factor
    } else {
        let old = if zeros { 1.0 } else { k };
        old * factor * (largest / SUM_REQUANTIZED)
    };
    if new_scale.is_nan() || new_scale < MIN_SCALE {
        codes.fill(0);
        *scale = 0.0;
        return;
    }
    let to_codes = SUM_REQUANTIZED / largest;
    for (codes, probabilities) in codes
        .chunks_exact_mut(num_hands)
        .zip(strategy.chunks_exact(num_hands))
    {
        for ((code, &probability), &weight) in codes.iter_mut().zip(probabilities).zip(reach) {
            let increment = gain(weight, probability);
            *code = if fits {
                // At most 65,535, as the first pass found.
                *code + (increment + offset) as u16
            } else {
                ((f32::from(*code) + increment) * to_codes + offset) as u16
            };
        }
    }
    *scale = new_scale;
}

/// Regret matching on regret codes: [`current_strategy`](crate::regret::current_strategy) of the
/// codes, each widened to `f32`, which is the strategy of the regrets they stand for (see the
/// [module documentation](self#regrets)).
///
/// Panics unless `codes` is one or more whole rows of `num_hands` values and `strategy` has the
/// same length.
pub fn current_strategy_16(codes: &[i16], num_hands: usize, strategy: &mut [f32]) {
    normalize_positive(codes, num_hands, strategy);
}

/// The average strategy of strategy-sum codes: [`average_strategy`](crate::regret::average_strategy)
/// of the codes, each widened to `f32`.
///
/// Panics unless `codes` is one or more whole rows of `num_hands` values and `average` has the
/// same length.
pub fn average_strategy_16(codes: &[u16], num_hands: usize, average: &mut [f32]) {
    normalize_positive(codes, num_hands, average);
}

/// Writes the regrets that `codes` and `scale` stand for, `c · k` rounded to `f32`.
///
/// Panics if the slices differ in length.
pub fn decode_regrets(codes: &[i16], scale: f32, regrets: &mut [f32]) {
    assert_eq!(codes.len(), regrets.len(), "code and regret lengths differ");
    for (regret, &code) in regrets.iter_mut().zip(codes) {
        *regret = f32::from(code) * scale;
    }
}

/// Writes the strategy sums that `codes` and `scale` stand for, `c · k` rounded to `f32`.
///
/// Panics if the slices differ in length.
pub fn decode_strategy_sum(codes: &[u16], scale: f32, sums: &mut [f32]) {
    assert_eq!(codes.len(), sums.len(), "code and sum lengths differ");
    for (sum, &code) in sums.iter_mut().zip(codes) {
        *sum = f32::from(code) * scale;
    }
}

#[cfg(test)]
mod tests {
    use rand_chacha::ChaCha8Rng;
    use rand_chacha::rand_core::{Rng, SeedableRng};

    use super::*;
    use crate::regret::{
        Dcfr, Schedule, accumulate_strategy, average_strategy, current_strategy, update_regrets,
    };

    /// No discount: the update stores what it is given.
    const KEEP: Discount = Discount {
        positive: 1.0,
        negative: 1.0,
        strategy: 1.0,
    };

    fn unit(rng: &mut ChaCha8Rng) -> f32 {
        (rng.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }

    fn below(rng: &mut ChaCha8Rng, bound: usize) -> usize {
        ((u128::from(rng.next_u64()) * bound as u128) >> 64) as usize
    }

    /// A random value of either sign whose magnitude spans 12 orders.
    fn spread(rng: &mut ChaCha8Rng) -> f32 {
        let magnitude = 10f32.powf(12.0 * unit(rng) - 6.0);
        if unit(rng) < 0.5 {
            -magnitude
        } else {
            magnitude
        }
    }

    /// The regrets `values` encoded from a slice of zeros, with no discount.
    fn encode_regrets(values: &[f32], offset: f32) -> (Vec<i16>, f32) {
        let mut codes = vec![0; values.len()];
        let mut scale = 0.0;
        update_regrets_16(&mut codes, &mut scale, &mut values.to_vec(), KEEP, offset);
        (codes, scale)
    }

    fn decode(codes: &[i16], scale: f32) -> Vec<f32> {
        let mut regrets = vec![0.0; codes.len()];
        decode_regrets(codes, scale, &mut regrets);
        regrets
    }

    fn decode_sums(codes: &[u16], scale: f32) -> Vec<f32> {
        let mut sums = vec![0.0; codes.len()];
        decode_strategy_sum(codes, scale, &mut sums);
        sums
    }

    #[test]
    fn regrets_round_trip_within_half_a_step_and_the_largest_lands_on_the_largest_code() {
        // Rows of either sign and magnitudes over 12 orders: each regret decodes within half a
        // step of its scale, the scale is the largest magnitude over 32,767, and the entry of the
        // largest magnitude is ±32,767. With any other offset each is within a step. Both bounds
        // allow for the f32 roundings of `x · (32767 / m)` and of the sum before the floor, at
        // most 2^-8 of a step. Measured, the largest gaps are 0.49991 and 1.0026 steps.
        let mut rng = ChaCha8Rng::seed_from_u64(41);
        let mut largest_gaps = [0.0f64; 2];
        for _ in 0..500 {
            let len = 1 + below(&mut rng, 40);
            let values: Vec<f32> = (0..len).map(|_| spread(&mut rng)).collect();
            let largest = values.iter().fold(0.0f32, |m, v| m.max(v.abs()));
            let (codes, scale) = encode_regrets(&values, NEAREST);
            assert_eq!(scale, largest / 32_767.0);
            for (&value, decoded) in values.iter().zip(decode(&codes, scale)) {
                let gap = f64::from((decoded - value).abs()) / f64::from(scale);
                assert!(gap <= 0.504, "{value} decodes to {decoded}");
                largest_gaps[0] = largest_gaps[0].max(gap);
            }
            let at = values.iter().position(|v| v.abs() == largest).unwrap();
            assert_eq!(codes[at], if values[at] > 0.0 { 32_767 } else { -32_767 });
            let offset = unit(&mut rng);
            let (codes, scale) = encode_regrets(&values, offset);
            for (&value, decoded) in values.iter().zip(decode(&codes, scale)) {
                let gap = f64::from((decoded - value).abs()) / f64::from(scale);
                assert!(
                    gap <= 1.004,
                    "{value} decodes to {decoded} with offset {offset}"
                );
                largest_gaps[1] = largest_gaps[1].max(gap);
            }
        }
        println!("largest gaps in steps: {largest_gaps:?}");
    }

    #[test]
    fn a_row_of_zeros_stays_zeros_and_plays_uniformly() {
        let mut codes = vec![0i16; 6];
        let mut scale = 0.0;
        update_regrets_16(&mut codes, &mut scale, &mut [0.0; 6], KEEP, NEAREST);
        assert_eq!((codes.as_slice(), scale), (&[0i16; 6][..], 0.0));
        let mut strategy = [0.0; 6];
        current_strategy_16(&codes, 2, &mut strategy);
        assert_eq!(strategy, [1.0 / 3.0; 6]);
        // Strategy sums with no reach stay zeros, under any offset.
        let mut sums = vec![0u16; 6];
        let mut sum_scale = 0.0;
        for offset in [0.0, NEAREST, 0.999] {
            let reach = [0.0, 0.0];
            accumulate_strategy_16(&mut sums, &mut sum_scale, &strategy, &reach, KEEP, offset);
            assert_eq!((sums.as_slice(), sum_scale), (&[0u16; 6][..], 0.0));
        }
        let mut average = [0.0; 6];
        average_strategy_16(&sums, 2, &mut average);
        assert_eq!(average, [1.0 / 3.0; 6]);
        // A row that has learned, and is then given regrets that cancel it, is zeros again.
        let mut values = [3.0, -1.0, 0.5, 0.0];
        update_regrets_16(&mut codes[..4], &mut scale, &mut values, KEEP, NEAREST);
        let back = decode(&codes[..4], scale);
        let mut cancel: Vec<f32> = back.iter().map(|v| -v).collect();
        update_regrets_16(&mut codes[..4], &mut scale, &mut cancel, KEEP, NEAREST);
        assert_eq!((&codes[..4], scale), (&[0i16; 4][..], 0.0));
    }

    #[test]
    fn negative_regrets_keep_their_sign_and_their_hand_plays_uniformly() {
        // Two hands, three actions: hand 0's regrets all negative, hand 1's mixed.
        let values = [-4.0, 6.0, -2.0, -8.0, -1.0, 2.0];
        let (codes, scale) = encode_regrets(&values, NEAREST);
        assert_eq!(codes[3], -32_767);
        for (&value, &code) in values.iter().zip(&codes) {
            assert_eq!(value.signum(), f32::from(code).signum(), "{value}: {code}");
        }
        let mut strategy = [0.0; 6];
        current_strategy_16(&codes, 2, &mut strategy);
        let mut expected = [0.0; 6];
        current_strategy(&decode(&codes, scale), 2, &mut expected);
        assert_eq!([strategy[0], strategy[2], strategy[4]], [1.0 / 3.0; 3]);
        for (a, b) in strategy.iter().zip(expected) {
            assert!((a - b).abs() < 1e-6);
        }
        assert!((strategy[1] - 0.75).abs() < 1e-4 && (strategy[5] - 0.25).abs() < 1e-4);
    }

    #[test]
    fn cfr_plus_floors_every_negative_regret_at_code_zero() {
        // Under CFR+ the negative factor is zero, so a regret that is not positive after adding
        // is code 0 exactly, and no code is ever negative, over many updates of random regrets.
        let mut rng = ChaCha8Rng::seed_from_u64(7);
        let (mut codes, mut scale) = (vec![0i16; 24], 0.0);
        for t in 1..=200 {
            let discount = Schedule::CfrPlus.discount(t);
            let instantaneous: Vec<f32> = (0..24).map(|_| 2.0 * unit(&mut rng) - 1.0).collect();
            let before = decode(&codes, scale);
            let mut x = instantaneous.clone();
            update_regrets_16(&mut codes, &mut scale, &mut x, discount, NEAREST);
            for ((&code, &before), &r) in codes.iter().zip(&before).zip(&instantaneous) {
                assert!(code >= 0, "{code}");
                if before + r <= 0.0 {
                    assert_eq!(code, 0);
                }
            }
        }
        assert!(codes.iter().any(|&code| code > 0));
    }

    #[test]
    fn a_growing_row_never_overflows_and_follows_the_f32_update() {
        // Regrets fed instantaneous regrets that grow a thousandfold an iteration, then by half
        // as much again, and strategy sums fed reaches that grow too, under DCFR: no code overflows, the
        // largest regret code is 32,767 and the largest sum code from 2^15 to 65,535, and every
        // decoded value is within a step of the f32 update's, made from the decoded state.
        let mut rng = ChaCha8Rng::seed_from_u64(3);
        let schedule = Schedule::Discounted(Dcfr::default());
        let (num_actions, num_hands) = (3, 5);
        let len = num_actions * num_hands;
        let (mut codes, mut scale) = (vec![0i16; len], 0.0);
        let (mut sums, mut sum_scale) = (vec![0u16; len], 0.0);
        let mut growth = 1.0f32;
        for t in 1..=60 {
            let discount = schedule.discount(t);
            growth *= if t <= 6 { 1000.0 } else { 1.5 };
            let instantaneous: Vec<f32> = (0..len)
                .map(|_| growth * (2.0 * unit(&mut rng) - 1.0))
                .collect();
            let mut expected = decode(&codes, scale);
            update_regrets(&mut expected, &instantaneous, discount);
            let mut x = instantaneous.clone();
            update_regrets_16(&mut codes, &mut scale, &mut x, discount, NEAREST);
            let largest = expected.iter().fold(0.0f32, |m, v| m.max(v.abs()));
            assert_eq!(codes.iter().map(|c| c.unsigned_abs()).max(), Some(32_767));
            for (a, b) in decode(&codes, scale).iter().zip(&expected) {
                assert!((a - b).abs() <= largest / 32_767.0, "{t}: {a} against {b}");
            }

            let mut strategy = vec![0.0; len];
            current_strategy_16(&codes, num_hands, &mut strategy);
            let reach: Vec<f32> = (0..num_hands)
                .map(|_| unit(&mut rng) * growth.sqrt())
                .collect();
            let mut expected = decode_sums(&sums, sum_scale);
            accumulate_strategy(&mut expected, &strategy, &reach, discount);
            let offset = offsets(9, t)[1];
            accumulate_strategy_16(
                &mut sums,
                &mut sum_scale,
                &strategy,
                &reach,
                discount,
                offset,
            );
            let largest = expected.iter().fold(0.0f32, |m, &v| m.max(v));
            let most = *sums.iter().max().unwrap();
            assert!((32_768..=65_535).contains(&most), "{t}: {most}");
            for (a, b) in decode_sums(&sums, sum_scale).iter().zip(&expected) {
                assert!(
                    (a - b).abs() <= 1.0001 * largest / 32_768.0,
                    "{t}: {a} against {b}"
                );
            }
        }
    }

    #[test]
    fn a_sum_that_gains_nothing_keeps_its_code_while_the_scale_takes_the_discount() {
        // Two hands: hand 0 plays action 0, hand 1 action 1, and then hand 1 is no longer
        // reached. Its code stays as it was, the zeros stay zeros, and the scale takes the
        // discount alone until hand 0's sum has doubled.
        let schedule = Schedule::Discounted(Dcfr::default());
        let strategy = [1.0, 0.0, 0.0, 1.0];
        let (mut sums, mut scale) = (vec![0u16; 4], 0.0);
        let reach = [1.0, 0.5];
        let discount = schedule.discount(1);
        accumulate_strategy_16(&mut sums, &mut scale, &strategy, &reach, discount, NEAREST);
        assert_eq!(sums, [32_768, 0, 0, 16_384]);
        for t in 2..=6 {
            let (before, old_scale) = (sums.clone(), scale);
            let discount = schedule.discount(t);
            let reach = [0.01, 0.0];
            accumulate_strategy_16(&mut sums, &mut scale, &strategy, &reach, discount, 0.25);
            assert_eq!(sums[3], before[3], "{t}");
            assert_eq!(&sums[1..3], &[0, 0]);
            assert!(sums[0] > before[0]);
            assert_eq!(scale, old_scale * discount.strategy as f32, "{t}");
        }
    }

    #[test]
    fn a_slice_too_small_for_its_scale_is_stored_as_zeros() {
        let (codes, scale) = encode_regrets(&[1e-32, -3e-33], NEAREST);
        assert_eq!((codes.as_slice(), scale), (&[0i16, 0][..], 0.0));
        let (codes, scale) = encode_regrets(&[1e-28, -3e-29], NEAREST);
        assert!(scale >= MIN_SCALE && codes[0] == 32_767);
        let (mut sums, mut sum_scale) = (vec![0u16; 2], 0.0);
        let (strategy, reach) = ([0.5, 0.5], [1e-36]);
        accumulate_strategy_16(&mut sums, &mut sum_scale, &strategy, &reach, KEEP, NEAREST);
        assert_eq!((sums.as_slice(), sum_scale), (&[0u16, 0][..], 0.0));
    }

    #[test]
    fn entries_that_hold_and_gain_the_same_values_keep_the_same_codes() {
        // Hands 1 and 3 are copies of each other in every input, as alike hands are: after every
        // update they hold the same codes, bit for bit.
        let mut rng = ChaCha8Rng::seed_from_u64(11);
        let schedule = Schedule::Discounted(Dcfr::default());
        let (num_actions, num_hands) = (3, 4);
        let len = num_actions * num_hands;
        let (mut codes, mut scale) = (vec![0i16; len], 0.0);
        let (mut sums, mut sum_scale) = (vec![0u16; len], 0.0);
        for t in 1..=300 {
            let discount = schedule.discount(t);
            let mut instantaneous: Vec<f32> = (0..len).map(|_| spread(&mut rng)).collect();
            let mut reach: Vec<f32> = (0..num_hands).map(|_| unit(&mut rng)).collect();
            for action in 0..num_actions {
                instantaneous[action * num_hands + 3] = instantaneous[action * num_hands + 1];
            }
            reach[3] = reach[1];
            let mut strategy = vec![0.0; len];
            current_strategy_16(&codes, num_hands, &mut strategy);
            let [regret_offset, sum_offset] = offsets(5, t);
            let x = &mut instantaneous;
            update_regrets_16(&mut codes, &mut scale, x, discount, regret_offset);
            let (s, r) = (&strategy, &reach);
            accumulate_strategy_16(&mut sums, &mut sum_scale, s, r, discount, sum_offset);
            for action in 0..num_actions {
                let (a, b) = (action * num_hands + 1, action * num_hands + 3);
                assert_eq!((codes[a], sums[a]), (codes[b], sums[b]), "{t}");
            }
        }
    }

    #[test]
    fn the_offsets_are_spread_evenly_over_the_iterations() {
        // Each is a multiple of 2^-24 in [0, 1), and the regrets' is the sums' plus 1/2 modulo
        // 1. Over the first 10,000 iterations of a node, every sixteenth of [0, 1) holds 625 of
        // the sums' offsets within a few, as a sequence of low discrepancy does, where
        // independent draws would stray by about 24. And the nodes start at different places.
        for node in [0, 1, 7, 123_456] {
            let mut bins = [0u32; 16];
            for t in 1..=10_000 {
                let [regrets, sums] = offsets(node, t);
                for offset in [regrets, sums] {
                    assert!((0.0..1.0).contains(&offset));
                    assert_eq!((f64::from(offset) * OFFSET_STEPS).fract(), 0.0);
                }
                let steps = |offset: f32| (f64::from(offset) * OFFSET_STEPS) as u32;
                assert_eq!(steps(regrets), (steps(sums) + (1 << 23)) % (1 << 24));
                bins[(sums * 16.0) as usize] += 1;
            }
            assert!(
                bins.iter().all(|&n| n.abs_diff(625) <= 4),
                "node {node}: {bins:?}"
            );
        }
        let starts: Vec<f32> = (0..50).map(|node| offsets(node, 0)[1]).collect();
        for (i, a) in starts.iter().enumerate() {
            assert!(starts[i + 1..].iter().all(|b| b != a));
        }
    }

    #[test]
    fn a_compressed_solve_of_leduc_converges_within_a_small_distance_of_the_f32_solve() {
        // Leduc under DCFR and CFR+, 1,000 and 3,000 iterations: the compressed solve's
        // exploitability is within 4e-4 chips of the f32 solve's, and each player's value under
        // its average strategy within 1e-3 chips. Measured, the exploitabilities are 1.877e-4
        // and 3.444e-4 after 1,000 DCFR iterations and 1.946e-5 and 2.047e-4 after 3,000,
        // 2.574e-4 and 3.241e-4, 4.147e-5 and 2.252e-4 under CFR+: the compressed solve stops
        // improving near 2e-4 chips, where the f32 solve goes on to 2e-5 (BENCHMARKS.md).
        use crate::public_leduc::PublicLeduc;
        use crate::vector::{F32Storage, VectorCfr};
        use crate::vector_best_response::{vector_expected_values, vector_exploitability};

        let game = PublicLeduc::new();
        for schedule in [Schedule::Discounted(Dcfr::default()), Schedule::CfrPlus] {
            let mut full = VectorCfr::<_, F32Storage>::with_storage(game.clone(), schedule);
            let mut half = VectorCfr::<_, CompressedStorage>::with_storage(game.clone(), schedule);
            for iterations in [1_000, 3_000] {
                full.run(iterations - full.iterations(), |_| {});
                half.run(iterations - half.iterations(), |_| {});
                let (ours, theirs) = (half.average_strategy(), full.average_strategy());
                let exploitability = vector_exploitability(&game, &ours);
                let target = vector_exploitability(&game, &theirs);
                println!("{schedule:?}, {iterations}: {exploitability:.3e} against {target:.3e}");
                assert!(
                    (exploitability - target).abs() < 4e-4,
                    "{schedule:?}, {iterations}: {exploitability:e} against {target:e}"
                );
                let values = vector_expected_values(&game, &ours);
                let expected = vector_expected_values(&game, &theirs);
                for (a, b) in values.iter().zip(expected) {
                    assert!(
                        (a - b).abs() < 1e-3,
                        "{schedule:?}, {iterations}: {a} against {b}"
                    );
                }
            }
        }
    }

    #[test]
    fn the_strategies_of_codes_are_those_of_the_values_they_stand_for() {
        let mut rng = ChaCha8Rng::seed_from_u64(5);
        for _ in 0..300 {
            let num_actions = 1 + below(&mut rng, 5);
            let num_hands = 1 + below(&mut rng, 6);
            let len = num_actions * num_hands;
            let values: Vec<f32> = (0..len).map(|_| spread(&mut rng)).collect();
            let (codes, scale) = encode_regrets(&values, NEAREST);
            let (mut ours, mut theirs) = (vec![0.0; len], vec![0.0; len]);
            current_strategy_16(&codes, num_hands, &mut ours);
            current_strategy(&decode(&codes, scale), num_hands, &mut theirs);
            for (a, b) in ours.iter().zip(&theirs) {
                assert!((a - b).abs() < 1e-6, "{ours:?} against {theirs:?}");
            }
            let sums: Vec<u16> = (0..len).map(|_| (rng.next_u64() >> 48) as u16).collect();
            average_strategy_16(&sums, num_hands, &mut ours);
            average_strategy(&decode_sums(&sums, 0.37), num_hands, &mut theirs);
            for (a, b) in ours.iter().zip(&theirs) {
                assert!((a - b).abs() < 1e-6, "{ours:?} against {theirs:?}");
            }
        }
    }
}
