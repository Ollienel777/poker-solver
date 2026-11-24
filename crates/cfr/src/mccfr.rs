//! External-sampling Monte Carlo CFR over any [`Game`], seeded and reproducible, with every
//! [`Schedule`] applied exactly although most information sets are not visited on an iteration.
//!
//! [`Mccfr`] is the preflop solver's algorithm, run here on toy games where its answers can be
//! checked. Each iteration samples chance and every player but one, explores every action of
//! that one player (the traverser), and updates the traverser's regrets with the sampled
//! counterfactual regrets of Lanctot et al. (2009). The average strategy is accumulated by a
//! separate pass that samples the player's own actions, as in Pluribus (Brown and Sandholm
//! 2019), because that weighs an information set by its player's own reach alone for any number
//! of players. The notation is that of [`crate::tabular`].
//!
//! # One iteration
//!
//! Iterations are numbered from `t = 1`, every regret `R(I, a)` and strategy sum `S(I, a)` starts
//! at zero, and `d = schedule.discount(t)`. This section describes the iteration as if every
//! information set were discounted on every iteration; [Discounting](#discounting) says how the
//! solver gets the same numbers while touching only the information sets the samples reach.
//!
//! Every strategy below is `σ(I)`, the [`current_strategy`] of `I`'s regrets as they stand when
//! it is read. Iteration `t` handles the players one at a time, player 0 first. For each player
//! `i` in `0, 1, …, n − 1`:
//!
//! 1. **Strategy pass.** A depth-first walk from the root:
//!    - at a terminal, it stops;
//!    - at a chance node, it draws one outcome from the chance probabilities and follows it;
//!    - at a history in an information set `I` of player `i`, it adds 1 to the visit count
//!      `m(I)`, draws one action from `σ(I)` and follows it;
//!    - at a node of any other player, it follows every action, in index order.
//!
//!    Then every information set `I` of player `i`, in key order, gets one
//!    [`accumulate_strategy`] with `σ(I)`, the reach `m(I)` and `d`, so that
//!    `S(I) ← (S(I) + m(I) · σ(I)) · s_t`, with `m(I) = 0` where the walk did not go. Player `i`'s
//!    regrets have not been updated on iteration `t` yet, so `σ(I)` is the strategy player `i`
//!    plays on iteration `t`, as in tabular CFR.
//! 2. **Regret pass.** A depth-first walk from the root that returns the sampled value `ṽ(h)`:
//!    - at a terminal, `ṽ(h) = u_i(h)`;
//!    - at a chance node, it draws one outcome `o` from the chance probabilities, and
//!      `ṽ(h) = ṽ(h·o)`;
//!    - at a history in an information set of another player, it draws one action `a` from
//!      that information set's `σ`, and `ṽ(h) = ṽ(h·a)`;
//!    - at a history `h` in an information set `I` of player `i`, it walks every action `a`, in
//!      index order, including those `σ(I)` gives probability zero; then
//!      `ṽ(h) = Σ_a σ(I, a) · ṽ(h·a)`, summed in index order, and the sampled regret is
//!      `r̃(I, a) = ṽ(h·a) − ṽ(h)`.
//!
//!    Then every information set `I` of player `i`, in key order, gets one [`update_regrets`]
//!    with `r̃(I, ·)` rounded to `f32` and `d`, with `r̃(I, ·) = 0` where the walk did not go.
//!    Players before `i` have already been updated on iteration `t`, so, as in tabular CFR, the
//!    walk for player `i` plays against their strategies for iteration `t + 1`.
//!
//! Under perfect recall the regret pass reaches each information set of player `i` at most once:
//! two histories it reaches part at a node of player `i`, which the player remembers. The solver
//! panics if it reaches one twice. The strategy pass can reach an information set several times,
//! when it holds histories that differ only in other players' actions.
//!
//! Values, reaches and `r̃` are `f64`, and `σ` is widened exactly from `f32`. The strategy to
//! report is the [`average_strategy`] of each strategy sum.
//!
//! ## Random numbers
//!
//! All randomness comes from one [`ChaCha8Rng`] made by [`SeedableRng::seed_from_u64`] from the
//! seed given to [`Mccfr::new`]. It is never reseeded, so iteration `t + 1` continues the stream
//! where iteration `t` left it. Each sampled node, at a chance node or a sampled player's node,
//! takes one `next_u64` value `x`, in the order the walks above reach those nodes: the strategy
//! pass and then the regret pass for player 0, then both for player 1, and so on, each walk
//! depth first and in index order. Nothing else draws. The draw picks an outcome from the
//! probabilities `p_0, p_1, …` (chance's, or `σ` widened to `f64`) like this:
//!
//! ```text
//! u = (x >> 11) · 2^-53                      a uniform number in [0, 1)
//! o = the first index with u < p_0 + … + p_o  the sum taken in f64, in index order
//! ```
//!
//! or the last index with positive probability if rounding leaves the sum short of `u`. An
//! outcome of probability zero is never drawn.
//!
//! # Why the estimates are right
//!
//! **The sampled regret is unbiased.** In the regret pass for player `i`, every action of
//! player `i` is walked and every other step is drawn from the probabilities that `π_{-i}`
//! multiplies, so a history `h` is reached with probability `π_{-i}(h)`. The draws below `h` are
//! fresh, so given that `h` is reached, `ṽ(h)` has expectation `v_i(h)`, by induction from the
//! terminals. At most one history of an information set `I` is reached, so
//!
//! ```text
//! E[r̃(I, a)] = Σ_{h ∈ I} π_{-i}(h) · (v_i(h·a) − v_i(h)) = r(I, a)
//! ```
//!
//! which is exactly the counterfactual regret that step 3 of tabular CFR adds for the same
//! profile. No importance weight appears: the probability of reaching `h` is the factor
//! `π_{-i}(h)` that the counterfactual regret carries. The unbiasedness tests check this for
//! every information set against the exact counterfactual regrets of the same profile, on Kuhn,
//! Leduc and random games with two and three players.
//!
//! **The visit count weighs an information set by its player's own reach.** In the strategy
//! pass for player `i`, a history `h` is reached with probability `π_i(h) · π_c(h)`, the
//! product of player `i`'s own action probabilities and chance's on the way to `h`; the other
//! players' steps are all followed. So
//!
//! ```text
//! E[m(I)] = Σ_{h ∈ I} π_i(h) · π_c(h) = π_i(I) · c(I),    c(I) = Σ_{h ∈ I} π_c(h)
//! ```
//!
//! The factor `c(I)` depends only on chance's probabilities: not on any strategy, not on `t`,
//! and not on the number of players. In expectation the pass therefore adds what tabular CFR
//! adds, `π_i(I) · σ(I)`, times one positive constant per information set, and such a constant
//! changes no average strategy under any schedule (see [`crate::tabular`]). An information set
//! with `c(I) = 0` is never reached by chance, and its average stays uniform, as it does in
//! tabular CFR, whose regrets there stay zero.
//!
//! The textbook form of external sampling instead adds `σ(I)` at the other players' nodes that
//! a regret pass samples. Such a node of player `j` is reached in player `i`'s regret pass with
//! probability `π_{-i}(h)`, which is `π_j(h) · π_c(h)` when there are two players, the same
//! weighting as above. With three or more it also multiplies the reach of every player other
//! than `i` and `j`, which changes from iteration to iteration as their strategies do, so
//! iterations would be weighted by what the third player happened to be doing. A test computes
//! both weightings exactly on random two- and three-player games to show the difference.
//!
//! # Discounting
//!
//! A [`Schedule`] multiplies every stored regret and strategy sum on every iteration, but an
//! information set that no sample reaches gets `r̃ = 0` and `m = 0`. Between two updates, its
//! regrets are only ever multiplied, by `p_k` if positive and `n_k` otherwise, and a factor that
//! is positive cannot change a sign, while a factor of zero maps a non-positive regret to zero.
//! So the regret `R(I, a)` stored when `I` was last updated, on iteration `ρ`, stands after
//! iteration `T ≥ ρ` at
//!
//! ```text
//! R(I, a) · F_+(ρ + 1, T)    if R(I, a) > 0        F_x(a, b) = f_x(a) · f_x(a+1) ⋯ f_x(b)
//! R(I, a) · F_−(ρ + 1, T)    otherwise             (1 when a > b)
//! ```
//!
//! where `f_+(k)`, `f_−(k)` and `f_s(k)` are `schedule.discount(k)`'s positive, negative and
//! strategy factors, and likewise a strategy sum stands at `S(I, a) · F_s(ς + 1, T)`, with `ς`
//! the iteration of its last update. The solver stores `ρ` and `ς` with each information set
//! and applies the missed factors when it reads or updates it:
//!
//! - to read `σ(I)` on iteration `t`, it takes the [`current_strategy`] of the regrets brought
//!   to iteration `t − 1`, each computed in `f64` as `R(I, a) · F` and rounded to `f32`, or of
//!   the stored regrets if `I` was updated on iteration `t` already (`ρ = t`). In the
//!   description above, an information set of a player who has had its turn on iteration `t`
//!   has been discounted for `t` even if no sample reached it, which multiplies its positive
//!   regrets by one more factor `f_+(t)`. Regret matching ignores the other regrets, and a
//!   positive factor shared by all the positive ones changes no strategy, so the two readings
//!   agree up to rounding;
//! - to update `I`'s regrets on iteration `t`, it brings them to `t − 1` that way, applies
//!   [`update_regrets`] with `r̃` and `d`, and sets `ρ = t`;
//! - to update `I`'s strategy sum, it brings it to `t − 1`, applies [`accumulate_strategy`] with
//!   `m(I)` and `d`, and sets `ς = t`;
//! - [`Mccfr::regrets`], [`Mccfr::strategy_sum`] and the two profiles bring every value to the
//!   last completed iteration.
//!
//! The products are computed in `f64`:
//!
//! | Schedule | `F_+(a, b)` | `F_−(a, b)` | `F_s(a, b)` |
//! |---|---|---|---|
//! | [`Vanilla`](Schedule::Vanilla) | 1 | 1 | 1 |
//! | [`CfrPlus`](Schedule::CfrPlus) | 1 | 0 | a / (b + 1) |
//! | [`Linear`](Schedule::Linear) | a / (b + 1) | a / (b + 1) | a / (b + 1) |
//! | [`Discounted`](Schedule::Discounted) | `exp(Λ_+(b) − Λ_+(a − 1))` | `exp(Λ_−(b) − Λ_−(a − 1))` | (a / (b + 1))^γ |
//!
//! for `a ≤ b`, where `Λ_x(k) = ln max(f_x(1), m) + … + ln max(f_x(k), m)` with `m` the smallest
//! normal `f64`, summed in that order and kept in a table that grows by one entry per iteration
//! (a factor below `m` zeroes any `f32` either way). The others are the products of the factors
//! `schedule.discount` gives, in closed form: `k / (k + 1)` telescopes. Every schedule is
//! supported, and each one weighs iterations exactly as it does in tabular CFR, with the sampled
//! `r̃` and `m(I) · σ(I)` in place of the exact regret and `π_i(I) · σ(I)`: after `T`
//! iterations, for example, linear averaging gives `S(I) = Σ_t (t / (T + 1)) · m_t(I) · σ_t(I)`
//! and DCFR's gives `Σ_t (t / (T + 1))^γ · m_t(I) · σ_t(I)`. The equality with discounting
//! every information set on every iteration is exact in real arithmetic and holds up to `f32`
//! rounding in practice; for [`Vanilla`](Schedule::Vanilla) and [`CfrPlus`](Schedule::CfrPlus)
//! the regrets agree bit for bit. A test runs an eager implementation of the iteration above,
//! started from the solver's own state before every iteration, and compares the two after it.
//!
//! CFR+ is supported, though its floor at zero acts on noisy sampled regrets; the slow
//! convergence table measures all four schedules.
//!
//! The cost is one iteration number per information set for its regrets and one for its
//! strategy sum, a few multiplications per value read, and for DCFR's regret factors, whose
//! products have no closed form, one table entry of two `f64` per iteration. Nothing ever
//! walks every information set.
//!
//! # Storage and determinism
//!
//! [`Mccfr::new`] walks the whole tree once, as [`crate::tabular::Cfr`] does, to find every
//! information set and check the game's shape. Information sets are stored in a `Vec` in key
//! order and found through a `BTreeMap`; nothing is hashed. Every walk visits children in index
//! order, every sum is taken in a fixed order, and the solver is single-threaded, so a run is a
//! function of the game, the schedule, the seed and the number of iterations: the same seed
//! gives the same regrets and strategy sums bit for bit, and running `N` iterations and then
//! `N` more, from a clone or not, gives the same result as running `2N` at once. The
//! transcendental functions (`exp`, `ln`, `powf`) come from the platform's math library, so
//! results are reproducible on one platform but may differ in the last bits between platforms;
//! [`Vanilla`](Schedule::Vanilla), [`CfrPlus`](Schedule::CfrPlus) and
//! [`Linear`](Schedule::Linear) use none of them.
//!
//! # References
//!
//! - Lanctot, Waugh, Zinkevich and Bowling, *Monte Carlo Sampling for Regret Minimization in
//!   Extensive Games*, NIPS (2009): external sampling, and the unbiasedness of the sampled
//!   counterfactual regret.
//! - Brown and Sandholm, *Superhuman AI for multiplayer poker*, Science (2019), supplementary
//!   materials: the average strategy accumulated by a separate pass that samples the player's
//!   own actions and explores the others'.
//! - Brown and Sandholm, *Solving Imperfect-Information Games via Discounted Regret
//!   Minimization*, AAAI (2019): linear and discounted CFR.
//! - Zinkevich, Johanson, Bowling and Piccione, *Regret Minimization in Games with Incomplete
//!   Information*, NIPS (2007): counterfactual regret and the average strategy weighted by
//!   `π_i(I)`.

use std::collections::BTreeMap;

use rand_chacha::ChaCha8Rng;
use rand_chacha::rand_core::{Rng, SeedableRng};

use crate::regret::{
    Discount, Schedule, accumulate_strategy, average_strategy, current_strategy, update_regrets,
};
use crate::tabular::{Game, Node, Profile};

/// External-sampling Monte Carlo CFR on a [`Game`], with the regret and averaging schedule of
/// a [`Schedule`] and a seeded random stream. One iteration is specified in the
/// [module documentation](self).
///
/// The solver is `Clone`, and a clone carries the random stream's position, so it serves as a
/// checkpoint: a clone run on gives the same result as the original.
#[derive(Debug, Clone)]
pub struct Mccfr<G: Game> {
    game: G,
    schedule: Schedule,
    seed: u64,
    rng: ChaCha8Rng,
    iterations: u64,
    /// The information sets in key order.
    keys: Vec<G::InfoSet>,
    /// The position of each information set in `keys` and `rows`.
    index: BTreeMap<G::InfoSet, usize>,
    rows: Vec<Row>,
    factors: Factors,
    /// The visit counts `m(I)` of the strategy pass under way, zero between passes.
    visits: Vec<u32>,
    /// The information sets the strategy pass under way has reached, in the order it first
    /// reached them, with their strategies.
    reached: Vec<(usize, Vec<f32>)>,
    /// The sampled regrets of the regret pass under way, in the order it produced them.
    sampled: Vec<(usize, Vec<f64>)>,
}

/// The stored state of one information set.
#[derive(Debug, Clone)]
struct Row {
    regrets: Vec<f32>,
    strategy_sum: Vec<f32>,
    /// `ρ`: the last iteration whose discount `regrets` includes.
    regrets_through: u64,
    /// `ς`: the last iteration whose discount `strategy_sum` includes.
    sum_through: u64,
}

impl<G: Game> Mccfr<G> {
    /// A solver for `game` with every regret and strategy sum at zero, whose random stream
    /// is seeded with `seed`.
    ///
    /// Panics if the game is malformed, for the reasons [`crate::tabular::Cfr::new`] gives.
    pub fn new(game: G, schedule: Schedule, seed: u64) -> Mccfr<G> {
        assert!(game.num_players() >= 1, "a game needs at least one player");
        // Fails early on DCFR parameters that are not finite.
        schedule.discount(1);
        let mut shapes = BTreeMap::new();
        discover(&game, &game.root(), &mut shapes);
        let keys: Vec<G::InfoSet> = shapes.keys().cloned().collect();
        let index = keys.iter().cloned().zip(0..).collect();
        let rows: Vec<Row> = shapes
            .into_values()
            .map(|(_, num_actions)| Row {
                regrets: vec![0.0; num_actions],
                strategy_sum: vec![0.0; num_actions],
                regrets_through: 0,
                sum_through: 0,
            })
            .collect();
        let visits = vec![0; rows.len()];
        Mccfr {
            game,
            schedule,
            seed,
            rng: ChaCha8Rng::seed_from_u64(seed),
            iterations: 0,
            keys,
            index,
            rows,
            factors: Factors::new(schedule),
            visits,
            reached: Vec::new(),
            sampled: Vec::new(),
        }
    }

    /// The game being solved.
    pub fn game(&self) -> &G {
        &self.game
    }

    /// The schedule the solver runs.
    pub fn schedule(&self) -> Schedule {
        self.schedule
    }

    /// The seed of the random stream.
    pub fn seed(&self) -> u64 {
        self.seed
    }

    /// The number of iterations completed.
    pub fn iterations(&self) -> u64 {
        self.iterations
    }

    /// Runs one iteration, as the [module documentation](self) specifies.
    ///
    /// Panics if the game turns out not to have perfect recall: a regret pass that reaches one
    /// information set twice.
    pub fn iterate(&mut self) {
        let t = self.iterations + 1;
        self.factors.extend_to(t);
        let discount = self.schedule.discount(t);
        for player in 0..self.game.num_players() {
            self.strategy_pass(player, t, discount);
            self.regret_pass(player, t, discount);
        }
        self.iterations = t;
    }

    /// Runs `iterations` iterations, calling `on_iteration` after each one with the solver as
    /// it stands.
    pub fn run(&mut self, iterations: u64, mut on_iteration: impl FnMut(&Mccfr<G>)) {
        for _ in 0..iterations {
            self.iterate();
            on_iteration(self);
        }
    }

    /// The average strategy of every information set, which is the strategy the solver
    /// converges with. An information set its player's strategy pass never reached plays
    /// uniformly.
    pub fn average_strategy(&self) -> Profile<G::InfoSet> {
        self.profile(|row, out| {
            let sum = row.sum_at(self.iterations, &self.factors);
            average_strategy(&sum, 1, out);
        })
    }

    /// The strategy the next iteration will play at every information set: regret matching on
    /// the regrets as they stand after the last completed iteration.
    pub fn current_strategy(&self) -> Profile<G::InfoSet> {
        self.profile(|row, out| {
            let regrets = row.regrets_at(self.iterations, &self.factors);
            current_strategy(&regrets, 1, out);
        })
    }

    /// The regrets of an information set, one per action, as they stand after the last
    /// completed iteration: the values that discounting every information set on every
    /// iteration would have stored.
    pub fn regrets(&self, info_set: &G::InfoSet) -> Option<Vec<f32>> {
        let &position = self.index.get(info_set)?;
        Some(self.rows[position].regrets_at(self.iterations, &self.factors))
    }

    /// The strategy sum of an information set, one per action, as it stands after the last
    /// completed iteration, in the same sense as [`Mccfr::regrets`].
    pub fn strategy_sum(&self, info_set: &G::InfoSet) -> Option<Vec<f32>> {
        let &position = self.index.get(info_set)?;
        Some(self.rows[position].sum_at(self.iterations, &self.factors))
    }

    fn profile(&self, strategy: impl Fn(&Row, &mut [f32])) -> Profile<G::InfoSet> {
        self.keys
            .iter()
            .zip(&self.rows)
            .map(|(info_set, row)| {
                let mut out = vec![0.0; row.regrets.len()];
                strategy(row, &mut out);
                (info_set.clone(), out)
            })
            .collect()
    }

    /// Step 1 of the iteration for `player`.
    fn strategy_pass(&mut self, player: usize, t: u64, discount: Discount) {
        let view = View::new(&self.game, &self.index, &self.rows, &self.factors, t);
        view.strategy_walk(
            player,
            &self.game.root(),
            &mut self.rng,
            &mut self.visits,
            &mut self.reached,
        );
        // Only the information sets the walk reached have m(I) > 0. The others are discounted
        // lazily, which `sum_through` records.
        for (position, strategy) in self.reached.drain(..) {
            let row = &mut self.rows[position];
            let count = std::mem::take(&mut self.visits[position]);
            row.strategy_sum = row.sum_at(t - 1, &self.factors);
            accumulate_strategy(&mut row.strategy_sum, &strategy, &[count as f32], discount);
            row.sum_through = t;
        }
    }

    /// Step 2 of the iteration for `player`.
    fn regret_pass(&mut self, player: usize, t: u64, discount: Discount) {
        let view = View::new(&self.game, &self.index, &self.rows, &self.factors, t);
        view.regret_walk(player, &self.game.root(), &mut self.rng, &mut self.sampled);
        for (position, sampled) in self.sampled.drain(..) {
            let row = &mut self.rows[position];
            assert!(
                row.regrets_through < t,
                "{:?} was reached twice in one regret pass, so the game does not have perfect \
                 recall",
                self.keys[position]
            );
            let instantaneous: Vec<f32> = sampled.iter().map(|&r| r as f32).collect();
            row.regrets = row.regrets_at(t - 1, &self.factors);
            update_regrets(&mut row.regrets, &instantaneous, discount);
            row.regrets_through = t;
        }
    }
}

impl Row {
    /// The regrets as they stand after iteration `through`, which is at least `ρ`.
    fn regrets_at(&self, through: u64, factors: &Factors) -> Vec<f32> {
        debug_assert!(through >= self.regrets_through);
        if through == self.regrets_through {
            return self.regrets.clone();
        }
        let [positive, negative] = factors.regret_products(self.regrets_through + 1, through);
        self.regrets
            .iter()
            .map(|&r| {
                let factor = if r > 0.0 { positive } else { negative };
                (f64::from(r) * factor) as f32
            })
            .collect()
    }

    /// The strategy sum as it stands after iteration `through`, which is at least `ς`.
    fn sum_at(&self, through: u64, factors: &Factors) -> Vec<f32> {
        debug_assert!(through >= self.sum_through);
        if through == self.sum_through {
            return self.strategy_sum.clone();
        }
        let factor = factors.strategy_product(self.sum_through + 1, through);
        self.strategy_sum
            .iter()
            .map(|&s| (f64::from(s) * factor) as f32)
            .collect()
    }
}

/// The products `F_+`, `F_−` and `F_s` of the module documentation.
#[derive(Debug, Clone)]
struct Factors {
    schedule: Schedule,
    /// For [`Schedule::Discounted`] only: entry `k` holds `[Λ_+(k), Λ_−(k)]`, from `k = 0`.
    logs: Vec<[f64; 2]>,
}

impl Factors {
    fn new(schedule: Schedule) -> Factors {
        let logs = match schedule {
            Schedule::Discounted(_) => vec![[0.0, 0.0]],
            _ => Vec::new(),
        };
        Factors { schedule, logs }
    }

    /// Makes the table hold every iteration up to `t`.
    fn extend_to(&mut self, t: u64) {
        if !matches!(self.schedule, Schedule::Discounted(_)) {
            return;
        }
        while (self.logs.len() as u64) <= t {
            let k = self.logs.len() as u64;
            let discount = self.schedule.discount(k);
            let [positive, negative] = self.logs[self.logs.len() - 1];
            let ln = |factor: f64| factor.max(f64::MIN_POSITIVE).ln();
            self.logs.push([
                positive + ln(discount.positive),
                negative + ln(discount.negative),
            ]);
        }
    }

    /// `[F_+(a, b), F_−(a, b)]`, for `1 ≤ a`.
    fn regret_products(&self, a: u64, b: u64) -> [f64; 2] {
        debug_assert!(a >= 1);
        if a > b {
            return [1.0, 1.0];
        }
        match self.schedule {
            Schedule::Vanilla => [1.0, 1.0],
            Schedule::CfrPlus => [1.0, 0.0],
            Schedule::Linear => {
                let ratio = a as f64 / (b + 1) as f64;
                [ratio, ratio]
            }
            Schedule::Discounted(_) => {
                let (high, low) = (self.logs[b as usize], self.logs[(a - 1) as usize]);
                [(high[0] - low[0]).exp(), (high[1] - low[1]).exp()]
            }
        }
    }

    /// `F_s(a, b)`, for `1 ≤ a`.
    fn strategy_product(&self, a: u64, b: u64) -> f64 {
        debug_assert!(a >= 1);
        if a > b {
            return 1.0;
        }
        let ratio = a as f64 / (b + 1) as f64;
        match self.schedule {
            Schedule::Vanilla => 1.0,
            Schedule::CfrPlus | Schedule::Linear => ratio,
            Schedule::Discounted(dcfr) => ratio.powf(dcfr.gamma),
        }
    }
}

/// What the walks of one iteration read: the game and the stored state, which stay fixed for
/// the whole walk.
struct View<'a, G: Game> {
    game: &'a G,
    index: &'a BTreeMap<G::InfoSet, usize>,
    rows: &'a [Row],
    factors: &'a Factors,
    /// The iteration under way.
    t: u64,
}

impl<'a, G: Game> View<'a, G> {
    fn new(
        game: &'a G,
        index: &'a BTreeMap<G::InfoSet, usize>,
        rows: &'a [Row],
        factors: &'a Factors,
        t: u64,
    ) -> View<'a, G> {
        View {
            game,
            index,
            rows,
            factors,
            t,
        }
    }

    /// The position of `info_set`, checking its number of actions.
    fn position(&self, info_set: &G::InfoSet, num_actions: usize) -> usize {
        let position = *self
            .index
            .get(info_set)
            .unwrap_or_else(|| panic!("{info_set:?} was not in the tree when the solver was made"));
        assert_eq!(
            self.rows[position].regrets.len(),
            num_actions,
            "{info_set:?} changed its number of actions"
        );
        position
    }

    /// `σ(I)` on iteration `t`: regret matching on the regrets brought to iteration `t − 1`, or
    /// as stored if they have been updated on iteration `t` already.
    fn strategy(&self, position: usize) -> Vec<f32> {
        let row = &self.rows[position];
        let regrets = row.regrets_at(row.regrets_through.max(self.t - 1), self.factors);
        let mut strategy = vec![0.0; regrets.len()];
        current_strategy(&regrets, 1, &mut strategy);
        strategy
    }

    /// The strategy pass's walk below `state`, counting visits to `player`'s information sets
    /// in `visits` and listing each one in `reached` the first time.
    fn strategy_walk(
        &self,
        player: usize,
        state: &G::State,
        rng: &mut ChaCha8Rng,
        visits: &mut [u32],
        reached: &mut Vec<(usize, Vec<f32>)>,
    ) {
        match self.game.node(state) {
            Node::Terminal { .. } => {}
            Node::Chance { probabilities } => {
                let outcome = draw(rng, probabilities.iter().copied());
                let child = self.game.next(state, outcome);
                self.strategy_walk(player, &child, rng, visits, reached);
            }
            Node::Player {
                player: acting,
                info_set,
                num_actions,
            } if acting == player => {
                let position = self.position(&info_set, num_actions);
                let strategy = self.strategy(position);
                let action = draw(rng, strategy.iter().map(|&p| f64::from(p)));
                if visits[position] == 0 {
                    reached.push((position, strategy));
                }
                visits[position] += 1;
                let child = self.game.next(state, action);
                self.strategy_walk(player, &child, rng, visits, reached);
            }
            Node::Player { num_actions, .. } => {
                for action in 0..num_actions {
                    let child = self.game.next(state, action);
                    self.strategy_walk(player, &child, rng, visits, reached);
                }
            }
        }
    }

    /// The regret pass's walk below `state` for the traverser `player`, returning `ṽ(h)` and
    /// appending `(position, r̃(I, ·))` to `sampled` for each of the traverser's information sets
    /// it reaches.
    fn regret_walk(
        &self,
        player: usize,
        state: &G::State,
        rng: &mut ChaCha8Rng,
        sampled: &mut Vec<(usize, Vec<f64>)>,
    ) -> f64 {
        match self.game.node(state) {
            Node::Terminal { utilities } => utilities[player],
            Node::Chance { probabilities } => {
                let outcome = draw(rng, probabilities.iter().copied());
                let child = self.game.next(state, outcome);
                self.regret_walk(player, &child, rng, sampled)
            }
            Node::Player {
                player: acting,
                info_set,
                num_actions,
            } => {
                let position = self.position(&info_set, num_actions);
                let strategy = self.strategy(position);
                if acting != player {
                    let action = draw(rng, strategy.iter().map(|&p| f64::from(p)));
                    let child = self.game.next(state, action);
                    return self.regret_walk(player, &child, rng, sampled);
                }
                let values: Vec<f64> = (0..num_actions)
                    .map(|action| {
                        let child = self.game.next(state, action);
                        self.regret_walk(player, &child, rng, sampled)
                    })
                    .collect();
                let mut value = 0.0;
                for (&probability, &action_value) in strategy.iter().zip(&values) {
                    value += f64::from(probability) * action_value;
                }
                sampled.push((position, values.iter().map(|&v| v - value).collect()));
                value
            }
        }
    }
}

/// Draws one outcome from the probabilities `weights`, as the module documentation specifies.
fn draw(rng: &mut ChaCha8Rng, weights: impl Iterator<Item = f64>) -> usize {
    let u = (rng.next_u64() >> 11) as f64 * (1.0 / (1u64 << 53) as f64);
    let mut cumulative = 0.0;
    let mut last_positive = None;
    for (outcome, weight) in weights.enumerate() {
        if weight > 0.0 {
            cumulative += weight;
            last_positive = Some(outcome);
            if u < cumulative {
                return outcome;
            }
        }
    }
    last_positive.expect("some outcome has positive probability")
}

/// Records the acting player and number of actions of every information set below `state`,
/// with the checks of [`crate::tabular::Cfr::new`].
fn discover<G: Game>(
    game: &G,
    state: &G::State,
    shapes: &mut BTreeMap<G::InfoSet, (usize, usize)>,
) {
    let num_children = match game.node(state) {
        Node::Terminal { utilities } => {
            assert_eq!(
                utilities.len(),
                game.num_players(),
                "the terminal {state:?} needs one utility per player"
            );
            assert!(
                utilities.iter().all(|u| u.is_finite()),
                "the terminal {state:?} has a utility that is not finite"
            );
            return;
        }
        Node::Chance { probabilities } => {
            assert!(
                !probabilities.is_empty(),
                "the chance node {state:?} has no outcomes"
            );
            assert!(
                probabilities.iter().all(|&p| p.is_finite() && p >= 0.0),
                "the chance node {state:?} has an outcome probability that is negative or not finite"
            );
            let total: f64 = probabilities.iter().sum();
            assert!(
                (total - 1.0).abs() <= 1e-9,
                "the chance probabilities at {state:?} sum to {total}, not 1"
            );
            probabilities.len()
        }
        Node::Player {
            player,
            info_set,
            num_actions,
        } => {
            assert!(
                player < game.num_players(),
                "player {player} acts at {state:?}, but the game has {} players",
                game.num_players()
            );
            assert!(num_actions >= 1, "{info_set:?} has no actions");
            let shape = (player, num_actions);
            let known = *shapes.entry(info_set.clone()).or_insert(shape);
            assert_eq!(
                known, shape,
                "{info_set:?} has states with different (player, number of actions)"
            );
            num_actions
        }
    };
    for index in 0..num_children {
        discover(game, &game.next(state, index), shapes);
    }
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use rand_chacha::ChaCha8Rng;
    use rand_chacha::rand_core::{Rng, SeedableRng};

    use super::*;
    use crate::best_response::{best_response, exploitability};
    use crate::kuhn::{Kuhn, KuhnCard, KuhnInfoSet};
    use crate::leduc::Leduc;
    use crate::regret::Dcfr;
    use crate::tabular::{Cfr, expected_values};

    const DCFR: Schedule = Schedule::Discounted(Dcfr {
        alpha: 1.5,
        beta: 0.0,
        gamma: 2.0,
    });

    fn schedules() -> [Schedule; 5] {
        [
            Schedule::Vanilla,
            Schedule::CfrPlus,
            Schedule::Linear,
            DCFR,
            Schedule::Discounted(Dcfr {
                alpha: 0.5,
                beta: -1.0,
                gamma: 3.0,
            }),
        ]
    }

    fn below(rng: &mut ChaCha8Rng, bound: usize) -> usize {
        ((u128::from(rng.next_u64()) * bound as u128) >> 64) as usize
    }

    fn uniform(rng: &mut ChaCha8Rng, low: f64, high: f64) -> f64 {
        let unit = (rng.next_u64() >> 11) as f64 / (1u64 << 53) as f64;
        low + (high - low) * unit
    }

    fn widen(strategy: &[f32]) -> Vec<f64> {
        strategy.iter().map(|&p| f64::from(p)).collect()
    }

    // ---------------------------------------------------------------------------------------
    // Random games.

    /// A node of a [`TreeGame`].
    #[derive(Debug, Clone)]
    enum Spec {
        Terminal(Vec<f64>),
        Chance(Vec<f64>, Vec<usize>),
        Player(usize, String, Vec<usize>),
    }

    /// A game given as an explicit tree, whose states are node indices. Nodes are added
    /// children first, and the last node added is the root.
    #[derive(Debug, Clone)]
    struct TreeGame {
        num_players: usize,
        nodes: Vec<Spec>,
    }

    impl TreeGame {
        fn add(&mut self, spec: Spec) -> usize {
            self.nodes.push(spec);
            self.nodes.len() - 1
        }
    }

    impl Game for TreeGame {
        type State = usize;
        type InfoSet = String;

        fn num_players(&self) -> usize {
            self.num_players
        }

        fn root(&self) -> usize {
            self.nodes.len() - 1
        }

        fn node(&self, &state: &usize) -> Node<String> {
            match &self.nodes[state] {
                Spec::Terminal(utilities) => Node::Terminal {
                    utilities: utilities.clone(),
                },
                Spec::Chance(probabilities, _) => Node::Chance {
                    probabilities: probabilities.clone(),
                },
                Spec::Player(player, key, children) => Node::Player {
                    player: *player,
                    info_set: key.clone(),
                    num_actions: children.len(),
                },
            }
        }

        fn next(&self, &state: &usize, index: usize) -> usize {
            match &self.nodes[state] {
                Spec::Terminal(_) => panic!("terminal states have no children"),
                Spec::Chance(_, children) | Spec::Player(_, _, children) => children[index],
            }
        }
    }

    /// A probability distribution over `n` outcomes, with about one outcome in four at zero.
    fn random_distribution(rng: &mut ChaCha8Rng, n: usize) -> Vec<f64> {
        let mut weights: Vec<f64> = (0..n)
            .map(|_| {
                if below(rng, 4) == 0 {
                    0.0
                } else {
                    uniform(rng, 0.05, 1.0)
                }
            })
            .collect();
        if weights.iter().all(|&w| w == 0.0) {
            weights[n - 1] = 1.0;
        }
        let total: f64 = weights.iter().sum();
        weights.iter().map(|w| w / total).collect()
    }

    /// A node of the public tree that every deal of a [`random_game`] shares.
    enum Public {
        Terminal,
        Chance(Vec<f64>, Vec<usize>),
        Player(usize, Vec<usize>),
        /// `first` acts, then `second` acts without seeing `first`'s action, and the game ends.
        Hidden {
            first: usize,
            second: usize,
            first_actions: usize,
            second_actions: usize,
        },
    }

    fn random_public_tree(
        rng: &mut ChaCha8Rng,
        num_players: usize,
        depth: usize,
        tree: &mut Vec<Public>,
    ) -> usize {
        let public = if depth >= 4 || (depth >= 2 && below(rng, 3) == 0) {
            Public::Terminal
        } else if depth >= 1 && below(rng, 5) == 0 {
            let n = 2 + below(rng, 2);
            let probabilities = random_distribution(rng, n);
            let children = (0..n)
                .map(|_| random_public_tree(rng, num_players, depth + 1, tree))
                .collect();
            Public::Chance(probabilities, children)
        } else if num_players >= 2 && depth >= 1 && below(rng, 4) == 0 {
            let first = below(rng, num_players);
            Public::Hidden {
                first,
                second: (first + 1 + below(rng, num_players - 1)) % num_players,
                first_actions: 2 + below(rng, 2),
                second_actions: 1 + below(rng, 3),
            }
        } else {
            let n = if below(rng, 6) == 0 {
                1
            } else {
                2 + below(rng, 2)
            };
            let children = (0..n)
                .map(|_| random_public_tree(rng, num_players, depth + 1, tree))
                .collect();
            Public::Player(below(rng, num_players), children)
        };
        tree.push(public);
        tree.len() - 1
    }

    fn terminal(rng: &mut ChaCha8Rng, game: &mut TreeGame) -> usize {
        let utilities = (0..game.num_players)
            .map(|_| uniform(rng, -1.0, 1.0))
            .collect();
        game.add(Spec::Terminal(utilities))
    }

    fn instantiate(
        rng: &mut ChaCha8Rng,
        game: &mut TreeGame,
        tree: &[Public],
        public: usize,
        types: &[usize],
    ) -> usize {
        match &tree[public] {
            Public::Terminal => terminal(rng, game),
            Public::Chance(probabilities, children) => {
                let children = children
                    .iter()
                    .map(|&child| instantiate(rng, game, tree, child, types))
                    .collect();
                game.add(Spec::Chance(probabilities.clone(), children))
            }
            Public::Player(player, children) => {
                let children = children
                    .iter()
                    .map(|&child| instantiate(rng, game, tree, child, types))
                    .collect();
                let key = format!("player {player}, type {}, node {public}", types[*player]);
                game.add(Spec::Player(*player, key, children))
            }
            &Public::Hidden {
                first,
                second,
                first_actions,
                second_actions,
            } => {
                let key = format!("player {second}, type {}, after {public}", types[second]);
                let children = (0..first_actions)
                    .map(|_| {
                        let leaves = (0..second_actions).map(|_| terminal(rng, game)).collect();
                        game.add(Spec::Player(second, key.clone(), leaves))
                    })
                    .collect();
                let key = format!("player {first}, type {}, node {public}", types[first]);
                game.add(Spec::Player(first, key, children))
            }
        }
    }

    /// A random general-sum game shaped like Leduc, as in the tests of [`crate::tabular`]:
    /// chance deals each player one of two private types, some deals with probability zero,
    /// then the players act in a random public tree with chance nodes. A player's information
    /// set is its type and the public node, except that some nodes are followed by a second
    /// player who acts without seeing the first's action, so that one information set holds
    /// histories that differ only in another player's action. The game has perfect recall.
    fn random_game(rng: &mut ChaCha8Rng, num_players: usize) -> TreeGame {
        let mut tree = Vec::new();
        let root = random_public_tree(rng, num_players, 0, &mut tree);
        let deal = random_distribution(rng, 1 << num_players);
        let mut game = TreeGame {
            num_players,
            nodes: Vec::new(),
        };
        let children = (0..deal.len())
            .map(|index| {
                let types: Vec<usize> = (0..num_players).map(|i| (index >> i) & 1).collect();
                instantiate(rng, &mut game, &tree, root, &types)
            })
            .collect();
        game.add(Spec::Chance(deal, children));
        game
    }

    /// Random games with two and three players, alternately, each with at least one
    /// information set whose histories differ only in another player's action.
    fn random_games(seed: u64, count: usize) -> Vec<TreeGame> {
        let mut rng = ChaCha8Rng::seed_from_u64(seed);
        let mut games = Vec::new();
        while games.len() < count {
            let game = random_game(&mut rng, 2 + games.len() % 2);
            let hidden = game
                .nodes
                .iter()
                .any(|spec| matches!(spec, Spec::Player(_, key, _) if key.contains("after")));
            if hidden {
                games.push(game);
            }
        }
        games
    }

    // ---------------------------------------------------------------------------------------
    // Exact oracles, computed in f64 from the definitions by walking the whole tree.

    /// For every information set `I` of `traverser`, the counterfactual regrets `r(I, ·)` under
    /// `profile`, and `Σ_{h ∈ I} π_{-i}(h)`, the probability that a regret pass reaches `I`.
    fn exact_regrets<G: Game>(
        game: &G,
        profile: &Profile<G::InfoSet>,
        traverser: usize,
    ) -> BTreeMap<G::InfoSet, (Vec<f64>, f64)> {
        let mut out = BTreeMap::new();
        regret_oracle(game, profile, traverser, &game.root(), 1.0, &mut out);
        out
    }

    fn regret_oracle<G: Game>(
        game: &G,
        profile: &Profile<G::InfoSet>,
        traverser: usize,
        state: &G::State,
        other_reach: f64,
        out: &mut BTreeMap<G::InfoSet, (Vec<f64>, f64)>,
    ) -> f64 {
        let walk = |index: usize, reach: f64, out: &mut BTreeMap<_, _>| {
            let child = game.next(state, index);
            regret_oracle(game, profile, traverser, &child, reach, out)
        };
        match game.node(state) {
            Node::Terminal { utilities } => utilities[traverser],
            Node::Chance { probabilities } => (0..probabilities.len())
                .map(|o| probabilities[o] * walk(o, other_reach * probabilities[o], out))
                .sum(),
            Node::Player {
                player, info_set, ..
            } => {
                let sigma = widen(&profile[&info_set]);
                if player != traverser {
                    return (0..sigma.len())
                        .map(|a| sigma[a] * walk(a, other_reach * sigma[a], out))
                        .sum();
                }
                let values: Vec<f64> = (0..sigma.len())
                    .map(|a| walk(a, other_reach, out))
                    .collect();
                let value: f64 = sigma.iter().zip(&values).map(|(p, v)| p * v).sum();
                let entry = out
                    .entry(info_set)
                    .or_insert_with(|| (vec![0.0; sigma.len()], 0.0));
                for (regret, v) in entry.0.iter_mut().zip(&values) {
                    *regret += other_reach * (v - value);
                }
                entry.1 += other_reach;
                value
            }
        }
    }

    /// What [`weights`] adds up for each information set of one player `j`.
    #[derive(Debug, Clone, Copy, PartialEq)]
    enum Weighting {
        /// `Σ_{h ∈ I} π_j(h) · π_c(h)`: the expected visit count of the strategy pass.
        StrategyPass,
        /// `π_j(I)`: the player's own reach.
        OwnReach,
        /// The textbook scheme's weight: the probability that a regret pass of another player
        /// samples a history of `I`, `Σ_{h ∈ I} π_{-i}(h)`, summed over the other players `i`.
        Textbook,
    }

    /// For every information set of `player`, the quantity `weighting` names, under `profile`.
    fn weights<G: Game>(
        game: &G,
        profile: &Profile<G::InfoSet>,
        player: usize,
        weighting: Weighting,
    ) -> BTreeMap<G::InfoSet, f64> {
        let walkers: Vec<usize> = match weighting {
            Weighting::Textbook => (0..game.num_players()).filter(|&i| i != player).collect(),
            _ => vec![player],
        };
        let mut out = BTreeMap::new();
        for traverser in walkers {
            let walker = Walker {
                game,
                profile,
                player,
                traverser,
                weighting,
            };
            walker.walk(&game.root(), 1.0, &mut out);
        }
        out
    }

    struct Walker<'a, G: Game> {
        game: &'a G,
        profile: &'a Profile<G::InfoSet>,
        player: usize,
        /// For the textbook weighting, the player whose regret pass is walked.
        traverser: usize,
        weighting: Weighting,
    }

    impl<G: Game> Walker<'_, G> {
        fn walk(&self, state: &G::State, weight: f64, out: &mut BTreeMap<G::InfoSet, f64>) {
            let next = |index: usize| self.game.next(state, index);
            match self.game.node(state) {
                Node::Terminal { .. } => {}
                Node::Chance { probabilities } => {
                    for (o, &p) in probabilities.iter().enumerate() {
                        let weight = match self.weighting {
                            Weighting::OwnReach => weight,
                            _ => weight * p,
                        };
                        self.walk(&next(o), weight, out);
                    }
                }
                Node::Player {
                    player, info_set, ..
                } => {
                    let sigma = widen(&self.profile[&info_set]);
                    if player == self.player {
                        let total = out.entry(info_set).or_insert(0.0);
                        match self.weighting {
                            // Every history of the information set has the same own reach.
                            Weighting::OwnReach => *total = weight,
                            _ => *total += weight,
                        }
                    }
                    // Which players' actions are followed with weight 1 rather than sampled:
                    // the others' in the strategy pass, the traverser's in a regret pass.
                    let followed = match self.weighting {
                        Weighting::Textbook => player == self.traverser,
                        _ => player != self.player,
                    };
                    for (a, &p) in sigma.iter().enumerate() {
                        self.walk(&next(a), if followed { weight } else { weight * p }, out);
                    }
                }
            }
        }
    }

    #[test]
    fn the_exact_regrets_are_those_of_tabular_cfr() {
        // Checks the oracle the unbiasedness tests rely on: one iteration of vanilla CFR from
        // zero gives player 0 the counterfactual regrets of the uniform profile.
        fn check<G: Game + Clone>(game: G) {
            let mut cfr = Cfr::new(game.clone(), Schedule::Vanilla);
            let uniform = cfr.current_strategy();
            cfr.iterate();
            let exact = exact_regrets(&game, &uniform, 0);
            assert!(!exact.is_empty());
            for (key, (regrets, _)) in &exact {
                let actual = cfr.regrets(key).unwrap();
                for (&a, &e) in actual.iter().zip(regrets) {
                    let what = format!("{key:?}: {actual:?} against {regrets:?}");
                    assert!((f64::from(a) - e).abs() <= 1e-6, "{what}");
                }
            }
        }
        check(Kuhn);
        check(Leduc);
        for game in random_games(1, 6) {
            check(game);
        }
    }

    // ---------------------------------------------------------------------------------------
    // Unbiasedness.

    /// Per-sample sums and sums of squares, one entry per (information set, action). An
    /// information set that a sample does not reach counts as a zero in that sample.
    struct Moments {
        samples: usize,
        sums: Vec<Vec<f64>>,
        squares: Vec<Vec<f64>>,
        /// The sample that last added to each information set.
        last: Vec<usize>,
    }

    impl Moments {
        fn new(sizes: impl Iterator<Item = usize>) -> Moments {
            let sums: Vec<Vec<f64>> = sizes.map(|n| vec![0.0; n]).collect();
            Moments {
                samples: 0,
                squares: sums.clone(),
                last: vec![0; sums.len()],
                sums,
            }
        }

        fn begin_sample(&mut self) {
            self.samples += 1;
        }

        fn add(&mut self, position: usize, values: &[f64]) {
            assert!(
                self.last[position] < self.samples,
                "one sample reached an information set twice"
            );
            self.last[position] = self.samples;
            for (k, &value) in values.iter().enumerate() {
                self.sums[position][k] += value;
                self.squares[position][k] += value * value;
            }
        }

        /// The sample mean of one entry and its standard error.
        fn mean_and_error(&self, position: usize, k: usize) -> (f64, f64) {
            let n = self.samples as f64;
            let mean = self.sums[position][k] / n;
            let variance = ((self.squares[position][k] - n * mean * mean) / (n - 1.0)).max(0.0);
            (mean, (variance / n).sqrt())
        }
    }

    /// How far sample means fall from their expected values, in standard errors.
    #[derive(Debug, Default)]
    struct ZTest {
        /// Entries compared by their z-score.
        tested: usize,
        /// Entries whose samples never vary, which must equal their expected value exactly.
        constant: usize,
        /// Entries reached by too few samples for a z-score, which are not compared.
        skipped: usize,
        /// Entries never reached that are not zero, or constant entries that are off.
        violations: usize,
        max_z: f64,
        sum_of_squares: f64,
        sum: f64,
    }

    impl ZTest {
        /// Compares one entry's mean with `expected`. `reached` is the expected number of
        /// samples that reach its information set: zero means the entry must be exactly zero,
        /// and under 100 it is skipped, because its mean is then far from normal.
        fn compare(&mut self, (mean, error): (f64, f64), expected: f64, reached: f64) {
            if reached == 0.0 {
                if mean != 0.0 || expected != 0.0 {
                    self.violations += 1;
                }
                self.constant += 1;
            } else if reached < 100.0 {
                self.skipped += 1;
            } else if error == 0.0 {
                if (mean - expected).abs() > 1e-12 {
                    self.violations += 1;
                }
                self.constant += 1;
            } else {
                let z = (mean - expected) / error;
                self.tested += 1;
                self.max_z = self.max_z.max(z.abs());
                self.sum_of_squares += z * z;
                self.sum += z;
            }
        }

        /// The mean of z², which is about 1 when the expected values are right.
        fn mean_square(&self) -> f64 {
            self.sum_of_squares / self.tested as f64
        }

        /// Whether the means are consistent with the expected values: none of the exact
        /// checks fails, no z-score reaches 5, and the mean z² is in [0.6, 1.5].
        ///
        /// The entries are not independent. Those of one information set are tied by
        /// `Σ_a σ(I, a) r(I, a) = 0`, so with two actions their z-scores are equal and
        /// opposite, and information sets on one path share samples. That widens the spread of
        /// the mean z² well beyond that of independent entries; measured, it ranged from 0.86
        /// to 1.29 over the games, sample counts and thresholds tried, with the mean z within
        /// 0.05 of zero every time. A bias of b standard errors in every
        /// entry would add b² to it. The maximum is unaffected by the correlation.
        fn passes(&self) -> bool {
            let mean_square = self.mean_square();
            self.violations == 0 && self.max_z < 5.0 && (0.6..=1.5).contains(&mean_square)
        }

        fn merge(&mut self, other: &ZTest) {
            self.tested += other.tested;
            self.constant += other.constant;
            self.skipped += other.skipped;
            self.violations += other.violations;
            self.max_z = self.max_z.max(other.max_z);
            self.sum_of_squares += other.sum_of_squares;
            self.sum += other.sum;
        }
    }

    impl std::fmt::Display for ZTest {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(
                f,
                "{} entries tested, max |z| {:.2}, mean z {:+.3}, mean z² {:.3}; {} exact, {} \
                 skipped, {} violations",
                self.tested,
                self.max_z,
                self.sum / self.tested as f64,
                self.mean_square(),
                self.constant,
                self.skipped,
                self.violations
            )
        }
    }

    /// A solver for `game` after 20 iterations of DCFR whose regrets are then replaced by
    /// random ones, about one in ten negative, each information set last updated on a random
    /// earlier iteration. Its strategies are mixed, some actions have probability zero, and
    /// reading them needs the lazy discount.
    fn scrambled<G: Game>(game: G, rng: &mut ChaCha8Rng) -> Mccfr<G> {
        let mut solver = Mccfr::new(game, DCFR, 0);
        solver.run(20, |_| {});
        for row in &mut solver.rows {
            for regret in &mut row.regrets {
                *regret = if below(rng, 10) == 0 {
                    -uniform(rng, 0.0, 1.0)
                } else {
                    uniform(rng, 0.05, 1.0)
                } as f32;
            }
            row.regrets_through = below(rng, 21) as u64;
        }
        solver
    }

    /// `profile` moved 3% of the way to uniform play at every information set.
    fn nearby<K: Ord + Clone>(profile: &Profile<K>) -> Profile<K> {
        profile
            .iter()
            .map(|(key, strategy)| {
                let uniform = 1.0 / strategy.len() as f32;
                let moved = strategy.iter().map(|&p| 0.97 * p + 0.03 * uniform);
                (key.clone(), moved.collect())
            })
            .collect()
    }

    fn view<G: Game>(solver: &Mccfr<G>) -> View<'_, G> {
        View::new(
            &solver.game,
            &solver.index,
            &solver.rows,
            &solver.factors,
            solver.iterations + 1,
        )
    }

    /// Runs `samples` regret passes for each traverser from the solver's current profile,
    /// without updating anything, and compares the mean sampled regret of every
    /// (information set, action) with its exact counterfactual regret under each profile in
    /// `references`, giving one [`ZTest`] per reference.
    fn regret_ztests<G: Game>(
        solver: &Mccfr<G>,
        references: &[Profile<G::InfoSet>],
        samples: usize,
        seed: u64,
    ) -> Vec<ZTest> {
        let view = view(solver);
        let profile = solver.current_strategy();
        let mut tests: Vec<ZTest> = references.iter().map(|_| ZTest::default()).collect();
        let mut rng = ChaCha8Rng::seed_from_u64(seed);
        let mut sampled = Vec::new();
        for traverser in 0..solver.game.num_players() {
            let mut moments = Moments::new(solver.rows.iter().map(|row| row.regrets.len()));
            for _ in 0..samples {
                moments.begin_sample();
                view.regret_walk(traverser, &solver.game.root(), &mut rng, &mut sampled);
                for (position, regrets) in sampled.drain(..) {
                    moments.add(position, &regrets);
                }
            }
            // How often the samples reach each information set follows the profile they play.
            let reach = exact_regrets(&solver.game, &profile, traverser);
            for (reference, test) in references.iter().zip(&mut tests) {
                for (key, (regrets, _)) in exact_regrets(&solver.game, reference, traverser) {
                    let position = solver.index[&key];
                    let reached = reach[&key].1 * samples as f64;
                    for (action, &expected) in regrets.iter().enumerate() {
                        test.compare(moments.mean_and_error(position, action), expected, reached);
                    }
                }
            }
        }
        tests
    }

    /// Runs `samples` strategy passes for each player from the solver's current profile and
    /// compares the mean visit count of every information set with `Σ_{h ∈ I} π_j(h) π_c(h)`.
    fn visit_ztest<G: Game>(solver: &Mccfr<G>, samples: usize, seed: u64) -> ZTest {
        let view = view(solver);
        let profile = solver.current_strategy();
        let mut test = ZTest::default();
        let mut rng = ChaCha8Rng::seed_from_u64(seed);
        let mut visits = vec![0; solver.rows.len()];
        let mut reached = Vec::new();
        for player in 0..solver.game.num_players() {
            let mut moments = Moments::new(solver.rows.iter().map(|_| 1));
            for _ in 0..samples {
                moments.begin_sample();
                view.strategy_walk(
                    player,
                    &solver.game.root(),
                    &mut rng,
                    &mut visits,
                    &mut reached,
                );
                for (position, strategy) in reached.drain(..) {
                    assert_eq!(strategy, view.strategy(position));
                    let count = std::mem::take(&mut visits[position]);
                    moments.add(position, &[f64::from(count)]);
                }
                assert!(visits.iter().all(|&v| v == 0));
            }
            let expected = weights(&solver.game, &profile, player, Weighting::StrategyPass);
            for (key, expected) in expected {
                let position = solver.index[&key];
                let reached = expected * samples as f64;
                test.compare(moments.mean_and_error(position, 0), expected, reached);
            }
        }
        test
    }

    /// Checks that the sampled regrets pass against the exact regrets of the profile they
    /// were sampled from, and that the same samples fail against the exact regrets of a
    /// profile 3% away, which shows that the check can fail.
    fn check_unbiased(what: &str, own: &ZTest, control: &ZTest) {
        println!("{what}: {own}");
        println!("{what}, against a profile 3% away: {control}");
        assert!(own.passes(), "{what}: {own}");
        assert!(!control.passes(), "{what}: the control passed: {control}");
    }

    #[test]
    fn sampled_regrets_are_unbiased_with_two_players() {
        // The mean sampled regret of every (information set, action) over many regret passes
        // from one fixed profile, against the exact counterfactual regret of that profile.
        // Measured, as printed with --nocapture, with the control's failures:
        //
        // | game  | passes  | tested | max |z| | mean z² | control's max |z| | its violations |
        // |-------|---------|--------|---------|---------|-------------------|----------------|
        // | Kuhn  | 100,000 |     21 |    1.63 |   0.894 |              10.8 |              3 |
        // | Leduc |  40,000 |  1,530 |    3.64 |   1.087 |              6.47 |             87 |
        //
        // Leduc skipped 567 entries that fewer than 100 passes were expected to reach. The
        // control's violations are entries that its profile reaches and this one never does.
        let mut rng = ChaCha8Rng::seed_from_u64(20);
        let solver = scrambled(Kuhn, &mut rng);
        let references = [
            solver.current_strategy(),
            nearby(&solver.current_strategy()),
        ];
        let tests = regret_ztests(&solver, &references, 100_000, 1);
        check_unbiased("Kuhn", &tests[0], &tests[1]);
        let solver = scrambled(Leduc, &mut rng);
        let references = [
            solver.current_strategy(),
            nearby(&solver.current_strategy()),
        ];
        let tests = regret_ztests(&solver, &references, 40_000, 2);
        check_unbiased("Leduc", &tests[0], &tests[1]);
    }

    /// The pooled regret z-tests of the random games with `num_players` players.
    fn random_game_ztests(num_players: usize) -> [ZTest; 2] {
        let mut rng = ChaCha8Rng::seed_from_u64(21);
        let mut pooled = [ZTest::default(), ZTest::default()];
        for (index, game) in random_games(20, 16).into_iter().enumerate() {
            if game.num_players != num_players {
                continue;
            }
            let solver = scrambled(game, &mut rng);
            let references = [
                solver.current_strategy(),
                nearby(&solver.current_strategy()),
            ];
            let tests = regret_ztests(&solver, &references, 12_000, index as u64);
            for (pool, test) in pooled.iter_mut().zip(&tests) {
                pool.merge(test);
            }
        }
        pooled
    }

    #[test]
    fn sampled_regrets_are_unbiased_on_random_two_player_games() {
        // Eight random two-player games, with information sets whose histories differ only in
        // the other player's action, which Kuhn and Leduc lack; 12,000 regret passes per
        // traverser. Measured: 354 entries tested, max |z| 3.63, mean z² 1.29, and the
        // control's max |z| was 25.0.
        let [own, control] = random_game_ztests(2);
        check_unbiased("random two-player games", &own, &control);
    }

    #[test]
    fn sampled_regrets_are_unbiased_with_three_players() {
        // Eight random three-player games, each with chance nodes inside the tree,
        // zero-probability outcomes and an information set whose histories differ only in
        // another player's action; 12,000 regret passes per traverser. Measured: 281 entries
        // tested, max |z| 2.73, mean z² 1.04, and the control's max |z| was 5.69, with 40
        // violations.
        let [own, control] = random_game_ztests(3);
        check_unbiased("random three-player games", &own, &control);
    }

    #[test]
    fn strategy_pass_visits_follow_own_reach_times_chance() {
        // The mean visit count m(I) over many strategy passes, against the exact
        // Σ_{h ∈ I} π_j(h) π_c(h), on Leduc and on random games with two and three players.
        // Measured: Leduc tested 375 information sets with max |z| 2.88 and mean z² 1.01, and
        // the random games 191 with max |z| 2.23 and mean z² 1.06.
        let mut rng = ChaCha8Rng::seed_from_u64(22);
        let solver = scrambled(Leduc, &mut rng);
        let leduc = visit_ztest(&solver, 12_000, 3);
        println!("Leduc: {leduc}");
        let mut pooled = ZTest::default();
        for (index, game) in random_games(22, 12).into_iter().enumerate() {
            let solver = scrambled(game, &mut rng);
            pooled.merge(&visit_ztest(&solver, 6_000, 100 + index as u64));
        }
        println!("random games: {pooled}");
        assert!(leduc.passes(), "{leduc}");
        assert!(pooled.passes(), "{pooled}");
    }

    /// The acting player of every information set of `game`.
    fn players<G: Game>(game: &G) -> BTreeMap<G::InfoSet, usize> {
        let mut shapes = BTreeMap::new();
        discover(game, &game.root(), &mut shapes);
        shapes
            .into_iter()
            .map(|(key, (player, _))| (key, player))
            .collect()
    }

    /// A profile with a random, fully mixed strategy at every information set.
    fn random_profile<G: Game>(game: &G, rng: &mut ChaCha8Rng) -> Profile<G::InfoSet> {
        let mut shapes = BTreeMap::new();
        discover(game, &game.root(), &mut shapes);
        shapes
            .into_iter()
            .map(|(key, (_, n))| {
                let weights: Vec<f64> = (0..n).map(|_| uniform(rng, 0.1, 1.0)).collect();
                let total: f64 = weights.iter().sum();
                (key, weights.iter().map(|w| (w / total) as f32).collect())
            })
            .collect()
    }

    #[test]
    fn only_the_strategy_pass_weighs_by_own_reach_alone_with_three_players() {
        // For each information set I of each player j, computed exactly: the expected weight a
        // scheme gives I on one iteration, divided by j's own reach π_j(I), under two profiles
        // that differ only in the other players' strategies. For the strategy pass the ratio is
        // c(I) under both, with any number of players. For the textbook scheme it is too with
        // two players, but with three it moves with the third player's strategy. Measured over
        // sixteen random games, the largest relative change was below 1e-12 for the strategy
        // pass with two and three players and for the textbook scheme with two, and 0.93 for
        // the textbook scheme with three.
        let mut rng = ChaCha8Rng::seed_from_u64(23);
        // The largest relative change of each scheme's ratio, by number of players.
        let mut pass_change = [0.0f64; 2];
        let mut textbook_change = [0.0f64; 2];
        for game in random_games(23, 16) {
            let players = players(&game);
            for j in 0..game.num_players {
                let first = random_profile(&game, &mut rng);
                let mut second = random_profile(&game, &mut rng);
                for (key, strategy) in first.iter() {
                    if players[key] == j {
                        second.insert(key.clone(), strategy.to_vec());
                    }
                }
                let own = weights(&game, &first, j, Weighting::OwnReach);
                assert_eq!(own, weights(&game, &second, j, Weighting::OwnReach));
                for (weighting, change) in [
                    (Weighting::StrategyPass, &mut pass_change),
                    (Weighting::Textbook, &mut textbook_change),
                ] {
                    let before = weights(&game, &first, j, weighting);
                    let after = weights(&game, &second, j, weighting);
                    for (key, &reach) in &own {
                        let (before, after) = (before[key] / reach, after[key] / reach);
                        if before > 0.0 {
                            let relative = (after - before).abs() / before;
                            let slot = &mut change[game.num_players - 2];
                            *slot = slot.max(relative);
                        }
                    }
                }
            }
        }
        println!("strategy pass: {pass_change:?}, textbook: {textbook_change:?}");
        assert!(pass_change.iter().all(|&c| c < 1e-12), "{pass_change:?}");
        assert!(textbook_change[0] < 1e-12, "{textbook_change:?}");
        assert!(textbook_change[1] > 0.5, "{textbook_change:?}");
    }

    // ---------------------------------------------------------------------------------------
    // The iteration, written again from the documentation.

    /// The iteration of the module documentation implemented again from its text, the way it
    /// is described there: every information set of a player is discounted on that player's
    /// turn of every iteration, in `f32` through the functions of [`crate::regret`], and draws
    /// follow the documented recipe.
    struct Eager<'a, G: Game> {
        game: &'a G,
        schedule: Schedule,
        rng: ChaCha8Rng,
        t: u64,
        keys: Vec<G::InfoSet>,
        index: BTreeMap<G::InfoSet, usize>,
        players: Vec<usize>,
        regrets: Vec<Vec<f32>>,
        sums: Vec<Vec<f32>>,
    }

    impl<'a, G: Game> Eager<'a, G> {
        fn new(game: &'a G, schedule: Schedule, seed: u64) -> Eager<'a, G> {
            let mut shapes = BTreeMap::new();
            discover(game, &game.root(), &mut shapes);
            let keys: Vec<G::InfoSet> = shapes.keys().cloned().collect();
            let regrets: Vec<Vec<f32>> = shapes.values().map(|&(_, n)| vec![0.0; n]).collect();
            Eager {
                game,
                schedule,
                rng: ChaCha8Rng::seed_from_u64(seed),
                t: 0,
                index: keys.iter().cloned().zip(0..).collect(),
                keys,
                players: shapes.values().map(|&(player, _)| player).collect(),
                sums: regrets.clone(),
                regrets,
            }
        }

        /// One draw from `probabilities`: `u = (x >> 11) · 2^-53`, then the first outcome whose
        /// running sum exceeds `u`, or the last with positive probability.
        fn draw(&mut self, probabilities: &[f64]) -> usize {
            let u = (self.rng.next_u64() >> 11) as f64 * 2f64.powi(-53);
            let mut total = 0.0;
            for (outcome, &p) in probabilities.iter().enumerate() {
                total += p;
                if u < total {
                    return outcome;
                }
            }
            probabilities.iter().rposition(|&p| p > 0.0).unwrap()
        }

        fn iterate(&mut self) {
            self.t += 1;
            let discount = self.schedule.discount(self.t);
            let root = self.game.root();
            for player in 0..self.game.num_players() {
                let sigma: Vec<Vec<f32>> = self
                    .regrets
                    .iter()
                    .map(|regrets| {
                        let mut strategy = vec![0.0; regrets.len()];
                        current_strategy(regrets, 1, &mut strategy);
                        strategy
                    })
                    .collect();
                let mut visits = vec![0u32; self.keys.len()];
                self.strategy_walk(player, &root, &sigma, &mut visits);
                let mut sampled = vec![None; self.keys.len()];
                for position in 0..self.keys.len() {
                    if self.players[position] == player {
                        let reach = [visits[position] as f32];
                        accumulate_strategy(
                            &mut self.sums[position],
                            &sigma[position],
                            &reach,
                            discount,
                        );
                    }
                }
                self.regret_walk(player, &root, &sigma, &mut sampled);
                for (position, sampled) in sampled.iter().enumerate() {
                    if self.players[position] == player {
                        let regrets = &mut self.regrets[position];
                        let instantaneous: Vec<f32> = match sampled {
                            Some(r) => r.iter().map(|&r: &f64| r as f32).collect(),
                            None => vec![0.0; regrets.len()],
                        };
                        update_regrets(regrets, &instantaneous, discount);
                    }
                }
            }
        }

        fn strategy_walk(
            &mut self,
            player: usize,
            state: &G::State,
            sigma: &[Vec<f32>],
            visits: &mut [u32],
        ) {
            let game = self.game;
            match game.node(state) {
                Node::Terminal { .. } => {}
                Node::Chance { probabilities } => {
                    let outcome = self.draw(&probabilities);
                    self.strategy_walk(player, &game.next(state, outcome), sigma, visits);
                }
                Node::Player {
                    player: acting,
                    info_set,
                    num_actions,
                } => {
                    let position = self.index[&info_set];
                    if acting == player {
                        visits[position] += 1;
                        let action = self.draw(&widen(&sigma[position]));
                        self.strategy_walk(player, &game.next(state, action), sigma, visits);
                    } else {
                        for action in 0..num_actions {
                            self.strategy_walk(player, &game.next(state, action), sigma, visits);
                        }
                    }
                }
            }
        }

        fn regret_walk(
            &mut self,
            player: usize,
            state: &G::State,
            sigma: &[Vec<f32>],
            sampled: &mut [Option<Vec<f64>>],
        ) -> f64 {
            let game = self.game;
            match game.node(state) {
                Node::Terminal { utilities } => utilities[player],
                Node::Chance { probabilities } => {
                    let outcome = self.draw(&probabilities);
                    self.regret_walk(player, &game.next(state, outcome), sigma, sampled)
                }
                Node::Player {
                    player: acting,
                    info_set,
                    num_actions,
                } => {
                    let position = self.index[&info_set];
                    let strategy = widen(&sigma[position]);
                    if acting != player {
                        let action = self.draw(&strategy);
                        return self.regret_walk(player, &game.next(state, action), sigma, sampled);
                    }
                    let values: Vec<f64> = (0..num_actions)
                        .map(|action| {
                            self.regret_walk(player, &game.next(state, action), sigma, sampled)
                        })
                        .collect();
                    let mut value = 0.0;
                    for (p, v) in strategy.iter().zip(&values) {
                        value += p * v;
                    }
                    assert!(sampled[position].is_none(), "reached {info_set:?} twice");
                    sampled[position] = Some(values.iter().map(|v| v - value).collect());
                    value
                }
            }
        }
    }

    /// The largest difference between two rows of values, divided by the largest magnitude in
    /// `expected` (or by 1, if that is smaller).
    fn relative_error(actual: &[f32], expected: &[f32]) -> f64 {
        let scale = expected
            .iter()
            .fold(1.0f64, |max, &e| max.max(f64::from(e).abs()));
        let error = actual.iter().zip(expected).fold(0.0f64, |max, (&a, &e)| {
            max.max((f64::from(a) - f64::from(e)).abs())
        });
        error / scale
    }

    /// Runs the solver, and before each of its iterations gives the eager reference the
    /// solver's regrets and strategy sums as they stand and its random stream, then runs one
    /// iteration of each and compares every regret and strategy sum. Starting each iteration
    /// from the same state keeps rounding differences from building up, so what is compared
    /// is one iteration of the lazy solver against one iteration as documented. Returns the
    /// largest relative errors of the regrets and of the sums.
    fn check_against_eager<G: Game + Clone>(
        game: &G,
        schedule: Schedule,
        iterations: u64,
        seed: u64,
    ) -> [f64; 2] {
        let mut solver = Mccfr::new(game.clone(), schedule, seed);
        let mut eager = Eager::new(game, schedule, seed);
        // Under these two every catch-up factor of a regret is 1, or 0 for one that is not
        // positive, so the lazy regrets are the eager ones exactly. Under vanilla CFR the
        // strategy sums are too.
        let exact_regrets = matches!(schedule, Schedule::Vanilla | Schedule::CfrPlus);
        let exact_sums = schedule == Schedule::Vanilla;
        let mut worst = [0.0f64; 2];
        for t in 1..=iterations {
            for (position, key) in eager.keys.iter().enumerate() {
                eager.regrets[position] = solver.regrets(key).unwrap();
                eager.sums[position] = solver.strategy_sum(key).unwrap();
            }
            eager.rng = solver.rng.clone();
            eager.t = solver.iterations();
            solver.iterate();
            eager.iterate();
            assert!(
                solver.rng == eager.rng,
                "{schedule:?}, iteration {t}: the draws differ"
            );
            for (position, key) in eager.keys.iter().enumerate() {
                let regrets = solver.regrets(key).unwrap();
                let sums = solver.strategy_sum(key).unwrap();
                let what = format!("{schedule:?}, iteration {t}, {key:?}");
                if exact_regrets {
                    assert_eq!(regrets, eager.regrets[position], "{what}");
                }
                if exact_sums {
                    assert_eq!(sums, eager.sums[position], "{what}");
                }
                worst[0] = worst[0].max(relative_error(&regrets, &eager.regrets[position]));
                worst[1] = worst[1].max(relative_error(&sums, &eager.sums[position]));
            }
        }
        worst
    }

    #[test]
    fn iterations_match_an_eager_reference_written_from_the_documentation() {
        // Same state, same draws, same numbers, on every iteration of runs on Kuhn, Leduc and
        // random games: the lazy discount, the passes and the order of the draws are as
        // documented. Measured, the largest error relative to a row's largest value (or 1):
        //
        // | schedule         | regrets | sums    |
        // |------------------|---------|---------|
        // | CFR              | 0       | 0       |
        // | CFR+             | 0       | 1.2e-7  |
        // | linear CFR       | 1.2e-7  | 1.2e-7  |
        // | DCFR(1.5, 0, 2)  | 1.2e-7  | 1.2e-7  |
        // | DCFR(0.5, -1, 3) | 1.4e-7  | 1.3e-7  |
        //
        // An iteration that ran free from its own state instead would drift: rounding
        // differences of about 1e-7 in σ move r̃ by as much times the values, which are
        // chips, and under DCFR(0.5, -1, 3), whose positive regrets lose a tenth on every
        // iteration, some information sets' regrets are only 1e-4, so the drift reached 1e-3
        // in regrets and 2e-2 in sums on Leduc after 200 iterations, with the random streams
        // still in step.
        let tolerances = [0.0, 1e-6, 1e-6, 1e-6, 1e-6];
        for (schedule, tolerance) in schedules().into_iter().zip(tolerances) {
            let mut worst = check_against_eager(&Kuhn, schedule, 200, 1);
            for (index, game) in random_games(24, 6).iter().enumerate() {
                let random = check_against_eager(game, schedule, 100, 10 + index as u64);
                worst = [worst[0].max(random[0]), worst[1].max(random[1])];
            }
            let leduc = check_against_eager(&Leduc, schedule, 60, 2);
            println!(
                "{schedule:?}: regrets {:.1e} on Leduc and {:.1e} elsewhere, sums {:.1e} and {:.1e}",
                leduc[0], worst[0], leduc[1], worst[1]
            );
            for error in leduc.into_iter().chain(worst) {
                assert!(
                    error <= tolerance,
                    "{schedule:?}: {error:e} against {tolerance:e}"
                );
            }
        }
    }

    // ---------------------------------------------------------------------------------------
    // Determinism.

    /// Every stored bit of a solver: the regrets, strategy sums and iteration stamps of every
    /// information set, and the random stream.
    type Fingerprint = (Vec<(Vec<u32>, Vec<u32>, u64, u64)>, ChaCha8Rng);

    fn fingerprint<G: Game>(solver: &Mccfr<G>) -> Fingerprint {
        let bits = |values: &[f32]| values.iter().map(|v| v.to_bits()).collect();
        let rows = solver
            .rows
            .iter()
            .map(|row| {
                (
                    bits(&row.regrets),
                    bits(&row.strategy_sum),
                    row.regrets_through,
                    row.sum_through,
                )
            })
            .collect();
        (rows, solver.rng.clone())
    }

    #[test]
    fn a_seed_fixes_every_bit_of_the_result() {
        fn check<G: Game + Clone>(game: G, schedule: Schedule, iterations: u64) {
            let run = |seed: u64, n: u64| {
                let mut solver = Mccfr::new(game.clone(), schedule, seed);
                solver.run(n, |_| {});
                solver
            };
            let reference = fingerprint(&run(7, 2 * iterations));
            assert!(fingerprint(&run(7, 2 * iterations)) == reference);
            // Another seed gives other regrets, not just another stream.
            assert!(fingerprint(&run(8, 2 * iterations)).0 != reference.0);
            // N iterations, then N more on a clone or on the original, are 2N iterations.
            let mut original = run(7, iterations);
            let mut resumed = original.clone();
            resumed.run(iterations, |_| {});
            original.run(iterations, |_| {});
            assert!(fingerprint(&resumed) == reference);
            assert!(fingerprint(&original) == reference);
            assert_eq!(original.average_strategy(), resumed.average_strategy());
        }
        check(Kuhn, DCFR, 1000);
        check(Leduc, DCFR, 150);
        check(Leduc, Schedule::Linear, 150);
        let game = random_games(25, 2).pop().unwrap();
        assert_eq!(game.num_players, 3);
        check(game, DCFR, 500);
    }

    #[test]
    fn catch_up_factors_are_the_products_of_the_schedules_factors() {
        // F_+, F_− and F_s over random gaps of up to 20,000 iterations, against the factors
        // of schedule.discount multiplied one by one in f64.
        let mut rng = ChaCha8Rng::seed_from_u64(26);
        let last = 20_000;
        for schedule in schedules() {
            let mut factors = Factors::new(schedule);
            factors.extend_to(last);
            for _ in 0..100 {
                let a = 1 + below(&mut rng, last as usize) as u64;
                let b = a - 1 + below(&mut rng, (last - a + 2) as usize) as u64;
                let mut direct = [1.0f64; 3];
                for k in a..=b {
                    let discount = schedule.discount(k);
                    direct[0] *= discount.positive;
                    direct[1] *= discount.negative;
                    direct[2] *= discount.strategy;
                }
                let [positive, negative] = factors.regret_products(a, b);
                let computed = [positive, negative, factors.strategy_product(a, b)];
                for (got, want) in computed.into_iter().zip(direct) {
                    // A product below 1e-300 zeroes any f32 regret whatever its exact value.
                    let close =
                        (got - want).abs() <= 1e-8 * want || (got < 1e-300 && want < 1e-300);
                    assert!(
                        close,
                        "{schedule:?} over {a}..={b}: {got:e} against {want:e}"
                    );
                }
            }
        }
    }

    // ---------------------------------------------------------------------------------------
    // Convergence.

    /// Runs `solver` on to each iteration count of `ladder` and returns the exploitability of
    /// its average strategy at each.
    fn climb<G: Game>(solver: &mut Mccfr<G>, ladder: &[u64]) -> Vec<f64> {
        ladder
            .iter()
            .map(|&target| {
                solver.run(target - solver.iterations(), |_| {});
                exploitability(solver.game(), &solver.average_strategy())
            })
            .collect()
    }

    fn show(points: &[f64]) -> String {
        let cells: Vec<String> = points.iter().map(|p| format!("{p:.3e}")).collect();
        cells.join(", ")
    }

    fn falls_at_every_rung(points: &[f64]) -> bool {
        points.windows(2).all(|pair| pair[1] < pair[0])
    }

    const THIRD: f64 = 1.0 / 3.0;
    const KUHN_VALUE: f64 = -1.0 / 18.0;
    /// Leduc's game value for player 0, as in [`crate::leduc`].
    const LEDUC_VALUE: f64 = -0.085606;

    /// The probability of betting, or calling, at a Kuhn information set.
    fn kuhn_bet(profile: &Profile<KuhnInfoSet>, card: KuhnCard, history: &'static str) -> f64 {
        f64::from(profile[&KuhnInfoSet { card, history }][Kuhn::BET])
    }

    /// Player 1's information sets in Kuhn poker and its unique equilibrium probability of
    /// betting or calling at each (see [`crate::kuhn`]).
    const KUHN_PLAYER_1: [(KuhnCard, &str, f64); 6] = [
        (KuhnCard::Jack, "p", THIRD),
        (KuhnCard::Jack, "b", 0.0),
        (KuhnCard::Queen, "p", 0.0),
        (KuhnCard::Queen, "b", THIRD),
        (KuhnCard::King, "p", 1.0),
        (KuhnCard::King, "b", 1.0),
    ];

    /// The largest violation of the conditions on Kuhn's family of equilibrium strategies for
    /// player 0, with α read off its jack, and the α read.
    fn kuhn_player_0_distance(profile: &Profile<KuhnInfoSet>) -> (f64, f64) {
        use KuhnCard::{Jack, King, Queen};
        let bet = |card, history| kuhn_bet(profile, card, history);
        let alpha = bet(Jack, "");
        let violations = [
            (alpha - THIRD).max(0.0),
            bet(Queen, ""),
            (bet(King, "") - 3.0 * alpha).abs(),
            bet(Jack, "pb"),
            (bet(Queen, "pb") - (alpha + THIRD)).abs(),
            1.0 - bet(King, "pb"),
        ];
        (violations.into_iter().fold(0.0, f64::max), alpha)
    }

    /// Full CFR's average strategy for Kuhn after 3,000 iterations of DCFR, which the
    /// [`crate::kuhn`] tests find within 3e-4 of the equilibrium family.
    fn kuhn_full_cfr() -> Profile<KuhnInfoSet> {
        let mut cfr = Cfr::new(Kuhn, DCFR);
        cfr.run(3_000, |_| {});
        cfr.average_strategy()
    }

    /// How far a Kuhn average strategy is from what is pinned down: the largest difference of
    /// player 1's probabilities from its unique equilibrium and from `full`'s, player 0's
    /// distance from its equilibrium family with the α read, and the error in player 0's
    /// value.
    fn kuhn_distances(average: &Profile<KuhnInfoSet>, full: &Profile<KuhnInfoSet>) -> [f64; 5] {
        let largest = |reference: &dyn Fn(KuhnCard, &'static str, f64) -> f64| {
            KUHN_PLAYER_1
                .iter()
                .map(|&(card, history, bet)| {
                    (kuhn_bet(average, card, history) - reference(card, history, bet)).abs()
                })
                .fold(0.0, f64::max)
        };
        let from_equilibrium = largest(&|_, _, bet| bet);
        let from_full = largest(&|card, history, _| kuhn_bet(full, card, history));
        let (player_0, alpha) = kuhn_player_0_distance(average);
        let value = expected_values(&Kuhn, average)[0];
        [
            from_equilibrium,
            from_full,
            player_0,
            alpha,
            (value - KUHN_VALUE).abs(),
        ]
    }

    #[test]
    fn kuhn_exploitability_falls_and_the_average_approaches_the_equilibrium() {
        // Linear and discounted averaging, seeds 1, 2 and 3. Kuhn has only 12 information
        // sets, so one seed's exploitability is noisy: measured over eight seeds, one in three
        // rose somewhere between rungs three times apart, even ten times apart. So "falls
        // steadily" here means that the mean over the seeds falls at every rung, and that
        // every run stays below 2.5/√T at every rung T, a bound that tightens; over the eight
        // seeds the largest value of exploitability · √T was 1.8.
        //
        // Where Kuhn's equilibrium is pinned down, the average approaches it and full CFR's:
        // player 1's strategy is unique, and player 0's lies in a family with one parameter.
        // Measured after 30,000 iterations, largest over the six runs, with the tolerances:
        //
        // | quantity                                           | measured | tolerance |
        // |----------------------------------------------------|----------|-----------|
        // | exploitability (chips)                             | 7.5e-3   | 1.44e-2   |
        // | player 1, largest difference from the equilibrium  | 3.6e-2   | 6e-2      |
        // | player 1, largest difference from full CFR's       | 3.6e-2   | 6e-2      |
        // | player 0, distance from the family                 | 4.7e-2   | 8e-2      |
        // | player 0's value, error against -1/18              | 8.1e-4   | 1.5e-3    |
        //
        // The exploitability's tolerance is the bound 2.5/√T. Full CFR's player 1 is within
        // 3e-4 of the equilibrium, so the two differences are nearly the same. The slow
        // `kuhn_convergence_table` runs on to 1,000,000 iterations.
        const LADDER: [u64; 5] = [300, 1_000, 3_000, 10_000, 30_000];
        let full = kuhn_full_cfr();
        for schedule in [Schedule::Linear, DCFR] {
            let mut mean = [0.0; LADDER.len()];
            for seed in 1..=3 {
                let mut solver = Mccfr::new(Kuhn, schedule, seed);
                let points = climb(&mut solver, &LADDER);
                let [player_1, from_full, player_0, alpha, value] =
                    kuhn_distances(&solver.average_strategy(), &full);
                println!(
                    "Kuhn {schedule:?}, seed {seed}: exploitability {}; player 1 is \
                     {player_1:.2e} from the equilibrium and {from_full:.2e} from full CFR, \
                     player 0 {player_0:.2e} from the family at α = {alpha:.3}, and the value \
                     is off by {value:.1e}",
                    show(&points),
                );
                for (&iterations, &point) in LADDER.iter().zip(&points) {
                    let bound = 2.5 / (iterations as f64).sqrt();
                    assert!(
                        point < bound,
                        "{schedule:?}, seed {seed}: {point:e} at {iterations}"
                    );
                }
                for (total, point) in mean.iter_mut().zip(&points) {
                    *total += point / 3.0;
                }
                let what = format!("{schedule:?}, seed {seed}");
                assert!(player_1 < KUHN_PLAYER_1_TOLERANCE, "{what}: {player_1:e}");
                assert!(from_full < KUHN_PLAYER_1_TOLERANCE, "{what}: {from_full:e}");
                assert!(player_0 < KUHN_PLAYER_0_TOLERANCE, "{what}: {player_0:e}");
                assert!(value < KUHN_VALUE_TOLERANCE, "{what}: {value:e}");
            }
            println!("Kuhn {schedule:?}, mean: {}", show(&mean));
            assert!(falls_at_every_rung(&mean), "{schedule:?}: {mean:?}");
        }
    }

    const KUHN_PLAYER_1_TOLERANCE: f64 = 6e-2;
    const KUHN_PLAYER_0_TOLERANCE: f64 = 8e-2;
    const KUHN_VALUE_TOLERANCE: f64 = 1.5e-3;

    #[test]
    #[ignore = "slow"]
    fn kuhn_convergence_table() {
        // Linear and discounted averaging with seeds 1, 2 and 3 to 1,000,000 iterations, each
        // run on its own thread. Prints, per run and rung, the exploitability and player 1's
        // largest difference from full CFR's strategy, with --nocapture. As measured, the
        // range over the three seeds:
        //
        // | iterations | linear: exploitability | player 1 from full CFR | DCFR: exploitability | player 1 from full CFR |
        // |-----------:|------------------------|------------------------|----------------------|------------------------|
        // |      1,000 | 1.3e-2 to 3.8e-2       | 4.5e-2 to 3.0e-1       | 2.5e-2 to 3.0e-2     | 9.4e-2 to 1.4e-1       |
        // |  1,000,000 | 6.8e-4 to 8.0e-4       | 1.2e-3 to 1.8e-3       | 5.9e-4 to 1.8e-3     | 3.6e-3 to 6.4e-3       |
        //
        // Player 0's value was within 1e-4 of -1/18 after 1,000,000 iterations in every run.
        const LADDER: [u64; 4] = [1_000, 10_000, 100_000, 1_000_000];
        let full = kuhn_full_cfr();
        let schedules = [("linear", Schedule::Linear), ("DCFR(1.5, 0, 2)", DCFR)];
        let runs: Vec<Vec<Vec<[f64; 6]>>> = std::thread::scope(|scope| {
            let handles: Vec<Vec<_>> = schedules
                .iter()
                .map(|&(_, schedule)| {
                    (1..=3)
                        .map(|seed| {
                            let full = &full;
                            scope.spawn(move || {
                                let mut solver = Mccfr::new(Kuhn, schedule, seed);
                                LADDER
                                    .iter()
                                    .map(|&target| {
                                        solver.run(target - solver.iterations(), |_| {});
                                        let average = solver.average_strategy();
                                        let e = exploitability(&Kuhn, &average);
                                        let [p1, f, p0, a, v] = kuhn_distances(&average, full);
                                        [e, p1, f, p0, a, v]
                                    })
                                    .collect()
                            })
                        })
                        .collect()
                })
                .collect();
            handles
                .into_iter()
                .map(|seeds| seeds.into_iter().map(|h| h.join().unwrap()).collect())
                .collect()
        });
        println!(
            "| schedule | seed | iterations | exploitability | player 1 from full CFR | player 0 from the family | value error |"
        );
        println!("|---|---:|---:|---:|---:|---:|---:|");
        for ((name, _), seeds) in schedules.iter().zip(&runs) {
            let mut mean = [0.0; LADDER.len()];
            for (seed, rungs) in (1..).zip(seeds) {
                for (&iterations, row) in LADDER.iter().zip(rungs) {
                    println!(
                        "| {name} | {seed} | {iterations} | {:.3e} | {:.2e} | {:.2e} | {:.1e} |",
                        row[0], row[2], row[3], row[5]
                    );
                    let bound = 2.5 / (iterations as f64).sqrt();
                    assert!(
                        row[0] < bound,
                        "{name}, seed {seed}: {:e} at {iterations}",
                        row[0]
                    );
                }
                for (total, row) in mean.iter_mut().zip(rungs) {
                    *total += row[0] / 3.0;
                }
            }
            println!("{name}, mean exploitability: {}", show(&mean));
            assert!(falls_at_every_rung(&mean), "{name}: {mean:?}");
        }
    }

    /// The iteration counts of the fast Leduc checks.
    const LEDUC_LADDER: [u64; 5] = [1_000, 2_000, 5_000, 10_000, 20_000];

    /// Checks that with seed 1, `schedule`'s exploitability on Leduc falls at every rung of
    /// [`LEDUC_LADDER`] and ends below `threshold`. As a consistency check, which any profile
    /// passes, the best responses bracket the game value and player 0's value is within twice
    /// the exploitability of it, so the value converges as the exploitability does.
    fn check_leduc_threshold(schedule: Schedule, threshold: f64) {
        let mut solver = Mccfr::new(Leduc, schedule, 1);
        let points = climb(&mut solver, &LEDUC_LADDER);
        let average = solver.average_strategy();
        let value = expected_values(&Leduc, &average)[0];
        let low = -best_response(&Leduc, &average, 1).value;
        let high = best_response(&Leduc, &average, 0).value;
        println!(
            "Leduc {schedule:?}: exploitability {}; value {value:.4}, game value in \
             [{low:.4}, {high:.4}]",
            show(&points)
        );
        assert!(falls_at_every_rung(&points), "{schedule:?}: {points:?}");
        let last = points[LEDUC_LADDER.len() - 1];
        assert!(
            last < threshold,
            "{schedule:?}: {last:e} after 20,000 iterations is not below {threshold:e}"
        );
        assert!(low <= LEDUC_VALUE && LEDUC_VALUE <= high);
        assert!((value - LEDUC_VALUE).abs() <= 2.0 * last);
    }

    // Regression thresholds for the fast suite, with seed 1 after 20,000 iterations. The
    // exploitability measured in the first good run, in chips:
    //
    // | Schedule        | 1,000  | 2,000  | 5,000  | 10,000 | 20,000    | threshold |
    // |-----------------|--------|--------|--------|--------|-----------|-----------|
    // | linear          | 1.081  | 0.7143 | 0.3246 | 0.2143 | 1.7840e-1 | 2.2e-1    |
    // | DCFR(1.5, 0, 2) | 1.111  | 0.8459 | 0.5059 | 0.3157 | 2.2041e-1 | 2.75e-1   |
    //
    // Each threshold is about 1.25 times its measurement. Over seeds 1 to 8, every run fell at
    // every rung, and after 20,000 iterations linear runs ranged over [0.141, 0.178] and DCFR
    // runs over [0.205, 0.238]. Full CFR reaches 8.9e-3 and 4.7e-4 after 500 iterations (see
    // `crate::leduc`); Monte Carlo CFR is not faster on a game this small, and its
    // exploitability falls as about 1/√T. The slow `leduc_convergence_table` has thresholds at
    // 1,000,000 iterations.

    #[test]
    fn linear_mccfr_is_below_its_leduc_threshold_after_20000_iterations() {
        check_leduc_threshold(Schedule::Linear, 2.2e-1);
    }

    #[test]
    fn dcfr_mccfr_is_below_its_leduc_threshold_after_20000_iterations() {
        check_leduc_threshold(DCFR, 2.75e-1);
    }

    /// The iteration counts of the slow convergence table.
    const TABLE_LADDER: [u64; 7] = [1_000, 3_000, 10_000, 30_000, 100_000, 300_000, 1_000_000];

    /// One run of the slow table: the exploitability at each count of [`TABLE_LADDER`], player
    /// 0's value at the end, and the microseconds per iteration.
    struct Run {
        points: Vec<f64>,
        value: f64,
        microseconds: f64,
    }

    fn table_run(schedule: Schedule, seed: u64) -> Run {
        let mut solver = Mccfr::new(Leduc, schedule, seed);
        let mut points = Vec::new();
        let mut seconds = 0.0;
        for target in TABLE_LADDER {
            let start = Instant::now();
            solver.run(target - solver.iterations(), |_| {});
            seconds += start.elapsed().as_secs_f64();
            points.push(exploitability(&Leduc, &solver.average_strategy()));
        }
        let value = expected_values(&Leduc, &solver.average_strategy())[0];
        let microseconds = seconds * 1e6 / TABLE_LADDER[TABLE_LADDER.len() - 1] as f64;
        Run {
            points,
            value,
            microseconds,
        }
    }

    #[test]
    #[ignore = "slow"]
    fn leduc_convergence_table() {
        // Every schedule with seeds 1, 2 and 3, each run on its own thread, to 1,000,000
        // iterations. Prints the table of exploitability, in chips, with --nocapture. Every
        // run falls at every rung. As measured in the first good run, with seed 1:
        //
        // | iterations | CFR       | CFR+      | linear    | DCFR(1.5, 0, 2) |
        // |-----------:|----------:|----------:|----------:|----------------:|
        // |      1,000 | 1.261     | 1.156     | 1.081     | 1.111           |
        // |     10,000 | 2.869e-1  | 3.453e-1  | 2.143e-1  | 3.157e-1        |
        // |    100,000 | 7.090e-2  | 1.023e-1  | 7.719e-2  | 9.204e-2        |
        // |  1,000,000 | 2.288e-2  | 3.430e-2  | 2.249e-2  | 3.228e-2        |
        // |  threshold | 2.9e-2    | 4.3e-2    | 2.8e-2    | 4.1e-2          |
        //
        // Each threshold is about 1.25 times its measurement. The means over the three seeds
        // after 1,000,000 iterations were 2.31e-2, 3.44e-2, 2.32e-2 and 3.36e-2. Sampling
        // noise dominates here, so the schedules that weigh late iterations more, or floor
        // noisy regrets, end higher than uniform weighting. Player 0's value ended in
        // [-0.0883, -0.0864] against the game value of -0.0856. With twelve runs at once, an
        // iteration took 50 to 55 µs; alone, about 27 µs.
        let schedules = [
            ("CFR", Schedule::Vanilla, 2.9e-2),
            ("CFR+", Schedule::CfrPlus, 4.3e-2),
            ("linear", Schedule::Linear, 2.8e-2),
            ("DCFR(1.5, 0, 2)", DCFR, 4.1e-2),
        ];
        let runs: Vec<Vec<Run>> = std::thread::scope(|scope| {
            let handles: Vec<Vec<_>> = schedules
                .iter()
                .map(|&(_, schedule, _)| {
                    (1..=3)
                        .map(|seed| scope.spawn(move || table_run(schedule, seed)))
                        .collect()
                })
                .collect();
            handles
                .into_iter()
                .map(|seeds| seeds.into_iter().map(|h| h.join().unwrap()).collect())
                .collect()
        });
        let names: Vec<&str> = schedules.iter().map(|(name, ..)| *name).collect();
        println!("Seed 1:");
        println!("| iterations | {} |", names.join(" | "));
        println!("|---:|{}", "---:|".repeat(names.len()));
        for (row, iterations) in TABLE_LADDER.iter().enumerate() {
            let cells: Vec<String> = runs
                .iter()
                .map(|r| format!("{:.3e}", r[0].points[row]))
                .collect();
            println!("| {iterations} | {} |", cells.join(" | "));
        }
        println!("Mean over seeds 1, 2 and 3:");
        println!("| iterations | {} |", names.join(" | "));
        println!("|---:|{}", "---:|".repeat(names.len()));
        for (row, iterations) in TABLE_LADDER.iter().enumerate() {
            let cells: Vec<String> = runs
                .iter()
                .map(|r| {
                    format!(
                        "{:.3e}",
                        r.iter().map(|run| run.points[row]).sum::<f64>() / 3.0
                    )
                })
                .collect();
            println!("| {iterations} | {} |", cells.join(" | "));
        }
        for ((name, _, threshold), seeds) in schedules.iter().zip(&runs) {
            for (seed, run) in (1..).zip(seeds) {
                println!(
                    "{name}, seed {seed}: {:.1} µs per iteration, player 0's value {:.5}",
                    run.microseconds, run.value
                );
                assert!(
                    falls_at_every_rung(&run.points),
                    "{name}, seed {seed}: {:?}",
                    run.points
                );
            }
            let last = seeds[0].points[TABLE_LADDER.len() - 1];
            assert!(
                last < *threshold,
                "{name}: {last:e} after 1,000,000 iterations is not below {threshold:e}"
            );
        }
    }
}
