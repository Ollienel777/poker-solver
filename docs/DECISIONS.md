# Decisions

This file has three lists:

- **Decided:** what the brief settled, plus answers given since.
- **Proposed:** choices made while planning. Accept or overturn them by editing this file.
- **Open:** questions that still need an answer. Each has a `type: decision` issue.

When an open question is answered, move it to Decided with the date, update any affected docs and configs in the same PR, and close its issue.

## Decided

| ID | Decision | Source |
| --- | --- | --- |
| D1 | Two versions: version 1 is preflop only for a multiway table; version 2 adds a heads-up postflop solver fed by preflop ranges | Brief |
| D2 | Four configs: 100bb and 200bb, each with and without straddle + ante. Each config is an independent solve | Brief |
| D3 | Postflop solving is heads-up only | Brief |
| D4 | Preflop method: full-game external-sampling MCCFR with abstracted postflop | Brief |
| D5 | Postflop method: discounted CFR, no card abstraction | Brief |
| D6 | Straddle and ante are config parameters, never separate code paths | Brief |
| D7 | The postflop solver is built first, and every milestone ends with acceptance tests that pass before the next starts | Brief |
| D8 | Blinds are SB 0.5bb and BB 1bb | Brief |
| D9 | No AGPL code is copied; open-source solvers are used only as external binaries | Brief |
| D10 | Everything through M7 runs locally. AWS is used only for validated code, with auto-shutdown and a billing alarm | Brief |
| D11 | Out of scope: multiway postflop, neural value networks, ICM and tournaments, a pre-solved flop library, node locking, hand history import, a web UI | Brief |
| D12 | The language is Rust | Answered 2026-11-23, [#1](https://github.com/Ollienel777/poker-solver/issues/1) |
| D13 | All four configs are 8-handed, with positions UTG, UTG1, LJ, HJ, CO, BTN, SB, BB | Answered 2026-11-23, [#2](https://github.com/Ollienel777/poker-solver/issues/2) |
| D14 | Configs C and D have an ante of 0.5bb from every seat, which is 4bb of dead money | Answered 2026-11-23, [#3](https://github.com/Ollienel777/poker-solver/issues/3) |
| D15 | Configs C and D have a 2bb live straddle from UTG, posted every hand. Button straddles are not modelled | Answered 2026-11-23, [#4](https://github.com/Ollienel777/poker-solver/issues/4) |
| D16 | Rake is 5% of the pot with no flop no drop, capped at 6bb in all four configs | Answered 2026-11-23, [#5](https://github.com/Ollienel777/poker-solver/issues/5); the cap in A and B confirmed 2026-11-24 |
| D17 | A local solve may use at most 10 GB of memory. The dev machine is an AMD Ryzen 7 7730U with 16 logical processors, 16 GB of RAM and about 390 GB free on a 953 GB disk | Answered 2026-11-24, [#23](https://github.com/Ollienel777/poker-solver/issues/23) |
| D18 | The reference solver is postflop-solver (AGPL-3.0-or-later), pinned at commit 9d1509f. It is run through `postflop-ref`, a small wrapper program kept in a repository of its own under the same licence, which reads a spot file and writes a solution dump. No code of either is in this repository | Answered 2026-11-24, [#24](https://github.com/Ollienel777/poker-solver/issues/24) |
| D19 | The M2 gate's check of the vector form against scalar CFR is read as P20 states it | Answered 2026-11-24, [#22](https://github.com/Ollienel777/poker-solver/issues/22) |
| D20 | The M3 gate compares our solver with the reference with both run to 0.03% of the pot. Each strategy must be under 0.3% of the pot exploitable as our code measures it, each action's frequency over the range within 2 points at every node reached more than 1% of the time, and each player's value within 0.2% of the pot. At the one node the toy game's solution leaves free, IP's after a check, each solver is held to the analytical bound, a bet at most 2/3 of the time, in place of the frequency check | Answered 2026-11-24, after the first comparison had been run under the tolerance as proposed, [#34](https://github.com/Ollienel777/poker-solver/issues/34) |

Notes on the 2026-11-23 answers:

- **D13 raises the preflop cost.** The brief's estimates assumed 6-max, and it puts 8 players at 3 to 5 times the preflop cost. See the risk list in [ROADMAP.md](ROADMAP.md#risks).
- **D14 and D15 together** put 7.5bb in the pot before the first action in C and D: 0.5 + 1 + 2 from the blinds and straddle, and 4 from the antes.
- **D16's cap is 6bb in every config.** The answer was "capped at 6bb (x3 straddle)". It was first recorded as 3 times the effective blind, which gave 3bb in A and B, and on 2026-11-24 it was confirmed as 6bb there too. The raked river test spots keep a cap of 3bb of their own, which binds in more of their pots.
- **D16 replaces the brief's "zero rake for validation".** Test spots set their own rake, so the first reference comparisons can still be rake-free.

## Proposed

| ID | Proposal | Why |
| --- | --- | --- |
| P1 | Integer chips, with 1bb = 100 | Exact pot arithmetic and ties, with rounding done in one place |
| P2 | Preflop sizes are multiples of the effective blind (the largest live forced bet) | The straddle configs scale on their own: a 2.5x open is 5bb under a 2bb straddle, and no code special-cases the straddle |
| P3 | The `cfr` crate knows nothing about poker and holds both scalar and vector-form helpers | One regret implementation for toy games, preflop and postflop |
| P4 | MCCFR and vector-form CFR are validated on Leduc in M2 | Removes risk from M3 and M7, where bugs are harder to see |
| P5 | Postflop parallel work reduces in a fixed order | Results are bit-identical for any thread count, so tests can be exact |
| P6 | The preflop solve accumulates the average strategy only at preflop nodes | Only the preflop strategy is exported, so this saves memory (Pluribus does the same) |
| P7 | The showdown assumes equal live stacks, and the config loader rejects a config whose forced bets leave them unequal (revised 2026-11-23) | Per-player antes (D14) keep live stacks equal, so side pots can't occur. A big blind ante would have needed them |
| P8 | Range files are TOML holding standard range strings | They can be typed by hand from a reference and diffed, and the same file serves export |
| P9 | Postflop nodes store only the combos that are in range | Memory |
| P10 | The abstraction's river equity reuses the showdown sweep, once per board | The code is shared, and the pass takes minutes instead of hours |
| P11 | EMD between 1-D histograms is computed as the L1 distance between CDFs | Exact in one dimension, so no optimal-transport solver is needed |
| P12 | The straddler keeps the position name UTG | Formation names stay the same across configs |
| P13 | Packages are prefixed `pk-` | A crate named `core` collides with Rust's built-in `core` |
| P14 | Each milestone has one gate issue holding its acceptance tests | Makes the brief's "tests pass before moving on" rule visible in the tracker |
| P15 | `[postflop]` has two table-level fields that apply to every street. `max_raises` caps the raises per street after the opening bet (0 means none). `allin_threshold`, more than 0 and at most 1, turns into all-in a bet or raise that would commit more than that fraction of the effective stack. Configs A to D set 3 and 0.67 | Issue [#25](https://github.com/Ollienel777/poker-solver/issues/25) names both rules, the schema had no field for either, and reference solvers expose both knobs |
| P16 | The range-file format lives in `pk-game`, in `pk_game::range_file`, not in `pk-core` as first planned | A range file needs both `Range` and `Position`. `Position` is in `pk-game`, which depends on `pk-core`, so only `pk-game` can see both |
| P17 | Postflop exploitability is the average gain from deviating, `((BR_0 − v_0) + (BR_1 − v_1)) / 2`, where `v_i` is player `i`'s expected value when both play the strategy and `BR_i` its value when it best-responds. Values are in chips per hand dealt, with the deal normalised so that the weights of all the pairs of hands that can be held together sum to 1, and the result is reported as a percent of the pot at the root | Payoffs count from the start of the tree, so the two values sum to the pot minus the expected rake. The game is constant-sum only without rake, since a rake grows with the pot up to its cap. `(BR_0 + BR_1) / 2` would be about half the pot at an equilibrium. In a zero-sum game the two definitions agree |
| P18 | One CFR iteration is specified exactly in the `pk_cfr::regret` and `pk_cfr::tabular` module docs. Iterations count from 1. Players update in turn within an iteration, player 0 first. The discount of iteration `t` is applied after that iteration's regret and strategy are added. Values are computed in `f64` and rounded to `f32` only when stored | CFR+ and DCFR are defined with alternating updates, and one written specification is what lets the vector form and the postflop solver reproduce the scalar solver |
| P19 | MCCFR accumulates the average strategy in a separate pass that samples the player's own actions, and applies discounts lazily from a last-update stamp per information set | Adding the strategy at sampled opponent nodes also weights by the other opponents' reach once there are three or more players. Lazy discounting is exact and never walks every information set |
| P20 | The vector form is held to the scalar solver one iteration at a time from the same stored state, under every schedule. Side-by-side runs are required to match only under vanilla CFR and DCFR on Leduc | CFR+, linear CFR and DCFR can all grow a one-ulp `f32` difference past 1e-6 within tens of iterations, so two correct solvers that sum in different orders can part ways. On Leduc that happens under CFR+ and linear CFR after about 10 and 130 iterations. Measured in [#21](https://github.com/Ollienel777/poker-solver/issues/21) |
| P21 | Leduc treats its six cards as distinct, which gives 936 information sets | The vector form keeps one entry per private card, so the scalar game is a one-to-one target and exercises card removal the way hold'em will |
| P22 | A node's strategy over a range has two read-outs. `range_view_frequencies` weighs each hand by the player's own range weight and reach, and is what comparisons and range views use. `action_frequencies` also weighs by how much of the opponent's range each hand leaves possible, and is the probability of the action at the node | The two differ by up to 8.7 percentage points on the river test spots. Other solvers' range views use the first, and a comparison must use one weighting on both sides. Found by the review of [#85](https://github.com/Ollienel777/poker-solver/pull/85) |
| P23 | A flop or turn spot is solved on a public tree that expands the betting tree: each chance node has one child per card that can fall, the root of a copy of the betting below it, and every node knows its betting node and its board | `VectorCfr` stores one row per node and walks a tree, so it solves the expansion with no change to `pk_cfr`, and the size the expansion costs is what the memory estimator ([#38](https://github.com/Ollienel777/poker-solver/issues/38)) will measure. Keeping one betting tree with per-card data beside it would need a solver walk that knows about cards. Merging suit-isomorphic branches is [#39](https://github.com/Ollienel777/poker-solver/issues/39), after a benchmark |
| P24 | A chance node's outcomes are every card off its board, each with probability `1/(48 − b)` on a board of `b` cards, and each rules out the hands that hold its card | Once both players hold their hands, the next card is equally likely to be any of the `48 − b` cards neither holds, so for every pair of hands that can be held together the outcomes that rule out neither sum to 1, as `PublicGame` requires, with no special case. Leaving out a card that rules out a whole range would save a subtree in rare spots, and would need care to keep the distribution |
| P25 | Each player's hand list is the spot's, on its own board, for the whole game. A hand that holds a dealt card stays in every per-hand vector below it, with reach zero, and plays uniformly there | `VectorCfr` has one hand count per player for the whole game, so per-street hand vectors, which issue [#36](https://github.com/Ollienel777/poker-solver/issues/36) mentions, would change `pk_cfr`'s API. The cost is the ruled-out entries: a card is in 48 of a flop's 1,176 combos, so about 4% of each vector on the turn and 8% on the river. To revisit once the memory estimator measures it |
| P26 | Each terminal is evaluated on the board it is reached on, with a `BoardHands` built once for every board the public tree reaches. A river board's hands are ranked once and shared by the two orders its last two cards can fall in. The opponent's reach is gathered into the board's hand order and the values written back by an index into the spot's lists | One implementation of the fold and showdown sweeps serves every street, the river's unchanged, and the evaluator stays out of the loop. Folds could be evaluated on the spot's board, since the solver passes zero reach for ruled-out hands, but the gather gives the right values whatever reach it is given. A second, root-indexed sweep that skips blocked hands would duplicate the sweep |
| P27 | The game is one type, `PostflopGame`, and `RiverGame` is an alias of it. `PostflopGame::new`, which the river code calls as `RiverGame::new`, still takes river spots only and fails with `SolveError::NotRiver`; `PostflopGame::from_spot` takes any street. The dump, its import and the comparison stay river-only, and refuse a flop or turn solution | The river tests and the river-only code work unedited, a river solve is bit-identical to M3's, and there is one implementation of the payoffs and the evaluation. A second game type beside `RiverGame` would have two that could drift, and renaming it would edit every river test. A river-only constructor keeps code that needs a river game from being handed a flop game |
| P28 | The public tree has a node limit, `SolveOptions::max_nodes`, a million by default like the betting tree's. Its size is counted from the betting tree before anything is allocated, and a tree over the limit is refused with `SolveError::PublicTreeTooLarge` | A flop line that reaches the river is copied 49 × 48 times, and an unbounded build can take the machine down. The count is exact and cheap. The limit bounds nodes, not storage, which grows with the hands as well; the memory estimator ([#38](https://github.com/Ollienel777/poker-solver/issues/38)) bounds storage |
| P29 | A public node is named by its path of action tokens and cards, such as `x b50 c 7s x`, through `PostflopGame::find` and `PostflopGame::node_path`. A solution's node indices are the public tree's. Below a card, a hand that holds it has reach 0 and is not in the range, and `reach_probability` includes the chance of each card dealt on the way | The comparison, the example and `inspect` need to name nodes below a card, and the CLI's node paths already write cards so. With the cards' chance in it, `reach_probability` is the probability of the line and sums over a chance node's children to the chance node's; conditional on the cards, it would not |

## Open

The M1 questions are answered, so `GameConfig` ([#12](https://github.com/Ollienel777/poker-solver/issues/12)) is unblocked. These remain:

| Question | Default until answered | Blocks | Issue |
| --- | --- | --- | --- |
| Output format for study | terminal grids | `cli` export | [#44](https://github.com/Ollienel777/poker-solver/issues/44) |
| Limps in the preflop tree | none | Preflop tree | [#57](https://github.com/Ollienel777/poker-solver/issues/57) |
| 3+-way pots in the preflop solve | option C (minimal tree) | Preflop game layout | [#58](https://github.com/Ollienel777/poker-solver/issues/58) |
