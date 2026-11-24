# Poker Solver: Project Brief for Claude Code

Nov 23, 2026 · @Nelson

> Exported from the Claude Docs brief. Open questions are now tracked as `type: decision` issues; see [docs/DECISIONS.md](docs/DECISIONS.md). The first five open questions were answered on 2026-11-23, and this file reflects those answers.

## Goal and scope

Build a personal-use No-Limit Hold'em cash game solver in two versions, covering four game configs. It is not a product: no accounts, no pre-solved library, no polished UI.

**Version 1, preflop only.** Outputs a full preflop strategy (frequencies per hand class at every preflop node) for a multiway table.

**Version 2, preflop + postflop.** Adds a heads-up postflop solver that takes the preflop ranges for a given formation plus a board and solves flop through river.

**Configs**

| Config | Stack | Straddle | Ante |
| --- | --- | --- | --- |
| A | 100bb | No | No |
| B | 200bb | No | No |
| C | 100bb | Yes | Yes |
| D | 200bb | Yes | Yes |

Straddle and ante must be parameters of one game config, not separate code paths. Each config is an independent solve.

## Decisions and assumptions

These are defaults chosen to get started. Change any of them in the first session; the ones marked "assumed" were never confirmed.

| Decision | Default | Status |
| --- | --- | --- |
| Language | Rust (rayon for parallelism, no GC, good reference solvers exist) | Decided 2026-11-23 |
| Table size | 8-handed | Decided 2026-11-23. The cost estimates below assumed 6-max |
| Blinds | SB 0.5bb, BB 1bb | Standard |
| Straddle | 2bb, posted UTG every hand, acts last preflop. No button straddle | Decided 2026-11-23 |
| Ante | 0.5bb from every player in configs C and D | Decided 2026-11-23 |
| Rake | 5% with no flop no drop, capped at 6bb in all four configs. Test spots can still set zero rake | Decided 2026-11-23; the cap in A and B confirmed 2026-11-24 |
| Preflop sizing | Open 2.5bb (3bb from SB), 3-bet 3x in position and 4x out of position, 4-bet 2.3x, all-in always allowed once a raise would commit over 40% of stack | Assumed, tunable |
| Postflop sizing | Two bet sizes per street (33% and 75% pot), one raise size, all-in on river | Assumed, tunable |
| Postflop scope | Heads-up only | Decided |
| Preflop method | Full-game MCCFR with abstracted postflop | Decided |
| Postflop method | Discounted CFR, no card abstraction | Decided |

**Out of scope:** multiway postflop solving, neural value networks, ICM and tournaments, a pre-solved library across all flops, node locking, hand history import, a web UI.

## Architecture

Two solvers share a core library and are chained through range files: the preflop solver writes ranges per formation, and the postflop solver reads them.

```
solver/
  crates/
    core/        cards, 7-card evaluator, equity, suit isomorphism, ranges (1326 combos, 169 classes)
    game/        GameConfig (stacks, blinds, straddle, ante, rake, sizings), action and tree builders
    cfr/         regret matching, CFR+, discounted CFR, best response, exploitability
    abstraction/ per-street hand bucketing for the preflop solver
    preflop/     multiplayer MCCFR over the full game with abstracted postflop
    postflop/    heads-up flop-to-river solver, no abstraction
    cli/         solve, export, inspect, compare commands
  configs/       a.toml, b.toml, c.toml, d.toml
  reference/     hand-recorded reference solutions used as test fixtures
```

**GameConfig.** One struct drives everything: number of players, stack depth, forced bets as a list of (seat, amount, live or dead), preflop action order derived from those bets, rake, and sizing rules. Straddle is one more forced bet that shifts who acts last. Ante is dead money. No solver code should branch on "is this a straddle game".

**Preflop solver.** The game is too large to solve exactly, so postflop is abstracted.

- Preflop hands use the 169 strategic classes, which is lossless.
- Flop, turn and river hands are clustered into buckets by equity distribution (k-means with earth mover's distance on equity histograms). Start with a few hundred buckets per street and tune.
- Postflop betting inside the preflop solve uses a deliberately small tree (one bet size, one raise) to keep memory bounded.
- Algorithm: external-sampling Monte Carlo CFR, with linear or discounted averaging. CFR has no convergence guarantee with three or more players, but it produces stable, sensible strategies in practice.
- Regrets stored compactly (32-bit or smaller), with checkpoints written to disk on a timer so spot instances can be interrupted.
- Output: for each preflop node, a 169-entry frequency table per action, plus the reach-weighted range for every formation that sees a flop.

**Postflop solver.** Heads-up, exact cards.

- Inputs: two 1326-combo weighted ranges, board, pot, effective stack, sizing rules, rake.
- Algorithm: discounted CFR over a full tree with chance nodes for turn and river.
- Performance work that matters, in order: vectorized range-vs-range terminal evaluation (sort by hand strength, prefix sums, card-removal correction), suit isomorphism on turn and river cards, multithreading across chance branches, 16-bit compressed strategy and regret storage.
- Output: strategy and EV per combo at every node, plus exploitability as a percent of pot.

**Chaining.** `cli export-ranges --config a --formation BTN_vs_BB_SRP` writes a range file; `cli solve-postflop --ranges <file> --board Ks7d2c` consumes it. Reference preflop ranges recorded by hand use the same file format, so the postflop solver can be built and used before the preflop solver exists.

## Milestones

Build the postflop solver first. It is the better-documented half, it can be validated exactly, and it exercises the CFR core the preflop solver reuses. Every milestone ends with a test that must pass before moving on.

1. **Foundations.** Card and range types, 7-card evaluator, equity calculator, GameConfig with all four configs loading from TOML.
   - Accept: evaluator reproduces the exact category counts over all 133,784,560 seven-card hands (for example 41,584 straight flushes and 3,473,184 full houses).
   - Accept: forced bets and preflop action order are correct for configs A through D, checked by unit tests.
2. **CFR on toy games.** Generic CFR, CFR+ and discounted CFR with a best-response exploitability function, run on Kuhn and Leduc poker.
   - Accept: Kuhn poker game value for the first player converges to -1/18 and exploitability goes to near zero.
3. **River solver.** Heads-up, fixed board, range versus range, vectorized showdown evaluation.
   - Accept: exploitability under 0.3% of pot on a set of test spots, with strategies matching a reference solver.
4. **Turn and flop solver.** Chance nodes, suit isomorphism, multithreading, compressed storage.
   - Accept: a 100bb single-raised-pot flop solve finishes locally on a reduced tree, and frequencies match a reference solver within a few percent on the test spots.
   - Accept: memory use is measured and reported per tree so AWS instance size can be chosen from data.
5. **Postflop CLI.** Solve from a range file and board, save and reload solutions, print a 13x13 strategy grid for any node.
   - At this point version 2's postflop half is usable with hand-recorded preflop ranges.
6. **Card abstraction.** Equity-histogram bucketing for flop, turn and river, cached to disk.
   - Accept: bucketing is deterministic and hands with obviously similar strength land together on spot checks.
7. **Preflop solver, heads-up first.** MCCFR over the abstracted full game with two players, where results can be sanity-checked against well-known heads-up ranges.
   - Accept: button opening and big blind defending frequencies are in a plausible range, and checkpoints resume correctly after a kill.
8. **Preflop solver, 8-handed.** Extend to N players, config A locally at low iteration counts, then at full scale on AWS.
   - Accept: opening ranges by position are within a few percentage points of reference ranges for config A.
   - This completes version 1 for config A.
9. **All four configs.** Run B, C and D. Export ranges for every heads-up formation.
10. **Chain and polish.** Preflop export feeds the postflop solver end to end. This completes version 2.

Rough effort for one person working steadily: milestones 1 to 5 in 2 to 3 months, 6 to 10 in another 2 to 4 months.

## Validation

A solver can run, converge and still be wrong, so every layer needs an independent check.

- **Exact checks.** Evaluator category counts, equity against known matchups (AA vs KK preflop is about 82%), Kuhn game value of -1/18.
- **Exploitability.** Compute a full best response for both players after every postflop solve and report it as a percent of pot. This is the primary correctness signal for heads-up solves and must be in place before any performance work.
- **Invariants.** Strategies sum to 1 at every node, reach probabilities are consistent, EV of the two players sums to the pot minus rake, results are identical across suit-isomorphic boards.
- **Cross-solver comparison.** Run the same tree in an open-source solver (postflop-solver in Rust, or TexasSolver) and diff frequencies and EVs. Use these as external reference binaries only. Both are believed to be AGPL licensed, so do not copy their code into this project without checking the licence.
- **Reference solutions.** A paid month of a commercial solver is useful as an answer key once milestones 4 and 8 produce output. Record solutions by hand into `reference/` for a few dozen preflop formations and a handful of flop spots. Do not scrape or automate extraction; that is very likely against the terms of service. Confirm the exact configs (200bb, straddle, ante, table size) are available before paying.
- **Multiway caveat.** There is no exploitability number that means much for an 8-handed table. For the preflop solver, rely on stability across random seeds, convergence of average strategy over iterations, and comparison with reference ranges.

## Compute and budget

The budget is a couple thousand dollars of AWS credits, which covers development, all four preflop configs and on-demand postflop solving, but not a pre-solved flop library. All figures were estimated for 6-max. The table is now 8-handed, so expect the preflop rows to cost 3 to 5 times more (see "What moves the numbers" below). Check current AWS pricing and the credit terms (expiry, excluded services, spot eligibility).

| Item | Per unit | Expected total |
| --- | --- | --- |
| Development and debugging | Local machine, small trees | About $0 |
| Preflop solve, one config | 12 to 48 hours on a 256 GB instance, about $25 to $200 | $100 to $800 for four |
| Preflop reruns | 3 to 5 full passes for bugs and tuning | $500 to $2,000 on demand, about a third on spot |
| Heads-up flop solve, 100bb | 5 to 30 minutes on 32 cores | Under $1 each |
| Heads-up flop solve, 200bb | Larger tree, more memory | $1 to $3 each |
| Reduced flop library (not planned) | 4,000 to 8,000 solves | $1,500 to $3,000 on spot |

**Rules for the project**

- Everything through milestone 7 runs locally. Rent large instances only for validated code.
- Checkpoint every 15 to 30 minutes and make resume a tested feature, so spot instances are safe to use.
- Every AWS run script sets an auto-shutdown on completion and on idle. A forgotten $4 per hour instance costs about $3,000 a month.
- Set a billing alarm before launching the first instance.
- Log memory and time per solve so instance size is chosen from measurements.
- Save compressed strategies only. Full flop solutions are gigabytes each; keep flop and turn strategies and re-solve rivers on demand if storage grows.

**What moves the numbers:** 8 or 9 players (3 to 5x on preflop), a tighter accuracy target (2 to 3x time), and each extra bet size per street (multiplies tree size).

## Open questions

Settle the first three before writing GameConfig, since they shape the preflop tree.

- [x] How many players: 6-max, or 8 to 9-handed as in most live straddle games? **8-handed.**
- [x] Ante format and size: one ante per player, or a single big blind ante? How large? **0.5bb per player.**
- [x] Straddle details: 2bb from UTG only, or also button straddles? **UTG only.**
- [x] Rake structure to model (percent, cap, no flop no drop), or rake-free? **5%, capped at 3 times the largest blind, no flop no drop.**
- [x] Rust or C++? The brief assumes Rust. **Rust.**
- [ ] Are limps allowed in the preflop tree? They matter more in straddle and ante games but enlarge the tree.
- [ ] Local machine specs (cores, RAM), which set how far milestones 3 to 7 can go before AWS is needed.
- [ ] Output format wanted for studying: terminal grids, JSON, or a simple local HTML viewer later?

## Kickoff prompt

Export this doc as Markdown, save it in an empty repo as `BRIEF.md`, then start Claude Code there and paste the prompt below.

```text
Read BRIEF.md in full. It describes a personal-use NLHE cash game solver I want to build with you.

Before writing any code:
1. Ask me the open questions from the brief, one batch, and wait for my answers.
2. Update BRIEF.md with my answers and create a CLAUDE.md that records the decisions, the crate layout, the build and test commands, and the rule that every milestone needs passing acceptance tests before the next one starts.
3. Propose a plan for Milestone 1 only (foundations) and wait for my approval.

Then implement Milestone 1:
- Cargo workspace with the crates from the brief (empty stubs are fine for later ones).
- Card, hand and range types; a fast 7-card evaluator; an equity calculator.
- GameConfig loaded from TOML, with configs A to D and unit tests for forced bets and preflop action order.
- An exhaustive evaluator test that checks category counts over all 7-card hands, marked as a slow test.

Working rules:
- Correctness before speed. Do not optimize anything until its test passes and there is a benchmark showing it matters.
- Write tests alongside the code and run them before telling me something is done.
- Do not copy code from AGPL solvers. They may be used only as external binaries to compare output.
- Nothing in this session should touch AWS. Everything runs locally for now.
- Stop at the end of Milestone 1 and summarize what was built, test results, and what Milestone 2 will need.
```

For later sessions, start with: "Read BRIEF.md and CLAUDE.md, then plan Milestone N and wait for approval."
