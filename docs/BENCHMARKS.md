# Benchmarks

What the postflop solver's hot paths cost, where an iteration spends its time, and what each optimization changes. The working rule is that nothing is optimized until a benchmark shows it matters, so each performance PR adds its before-and-after rows to [the log of changes](#log-of-changes) below, measured as [the machine section](#the-machine-and-how-far-to-trust-a-number) describes.

## Running them

```bash
cargo bench -p pk-postflop                           # everything, about 10 minutes
cargo bench -p pk-postflop --bench solve -- turn_srp # one spot
cargo bench -p pk-postflop --bench terminal          # the sweeps alone
```

Criterion keeps its reports in `target/criterion`, and a later run prints its change against the last one on the same machine.

## The spots

Two spots are fixed in `crates/postflop/benches/data`, so that every number below is of the same work (DECISIONS P40). Their ranges are illustrative, a BTN-against-BB single-raised pot written for these files, not taken from any source. Each benchmark checks the hand counts and the public tree's size before it times anything, so a change to the tree builder or to a file fails loudly instead of changing the benchmark.

| Spot | Board | Pot, stack | Tree | Hands (OOP, IP) | Betting nodes | Public nodes | Player nodes | Boards | Memory estimate |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `turn_srp` | Qs8s3d 2d | 11bb, 94.75bb | turn and river: bets of 0.5 and 1 pot, a raise to 3×, all-in; one raise a street | 313, 291 | 384 | 16,317 | 5,776 | 49 | 116 MB |
| `flop_srp` | Qs8s3d | 5.5bb, 97.5bb | every street: one bet of 0.5 pot, no raise | 318, 295 | 117 | 191,844 | 85,264 | 1,226 | 1.15 GB |

`flop_srp` is also tree (a) of [what a real flop solve costs](#what-a-real-flop-solve-costs).

## The machine, and how far to trust a number

An AMD Ryzen 7 7730U laptop (8 cores, 16 threads, 16 MB of L3), 16 GB of RAM, Windows 11 (10.0.26200), Rust 1.100.0, built with the release profile (fat LTO, one codegen unit). The solver is single-threaded, and so is every number here unless it says otherwise.

The machine runs other work, and its speed moves with it: on 2026-11-24 the same `flop_srp` iteration took from 1.9 s to 3.0 s over one afternoon, on unchanged code, and `turn_srp` from 0.16 s to 0.27 s. Criterion's intervals are within one run and do not show this. So:

- Compare a change by building both versions and running them back to back, alternating, three times or more, and compare the minima as well as the medians. A difference under about 10% needs that to be believed.
- Do not compare a new number with the baseline table below from another day.
- Each row of the log of changes says how it was measured.

A change meant to leave every number alone is checked with the example `fingerprint` (DECISIONS P49), which prints hashes of the bits of everything a solve computes: run it on the same spots and iteration counts before and after the change, on this machine, and the outputs must match line for line. Its header lists what it covers; #87 ran it on `deep_high_spr`, `four_flush` and `wet_oop_polarized` (200 iterations each), `turn_small` and `turn_medium` (100), `flop_small` (30), `flop_tiny` (100), `turn_srp` (12) and `flop_srp` (4).

```bash
cargo run --release -p pk-postflop --example fingerprint -- crates/postflop/tests/data/turn_small.toml 100
```

## Baseline

2026-11-24, commit `b43ce4e` (the public tree of #36), before any M4 optimization. Criterion's estimate, with its 95% interval for the long ones.

| Benchmark | `turn_srp` | `flop_srp` |
| --- | --- | --- |
| build the betting tree | 82 µs | 24 µs |
| build the game: the public tree, and the hands on every board | 9.8 ms | 200 ms |
| build the solver: a row of regrets and sums at every player node | 15.0 ms | 341 ms |
| one DCFR iteration, both players | 224 ms (216–232) | 2.69 s (2.65–2.72) |
| the average strategy | 12.9 ms | 147 ms |
| one best response (OOP) | 101 ms | 1.29 s (1.27–1.31) |
| one exploitability measurement: both values and both best responses | 371 ms (366–377) | 4.68 s (4.58–4.79) |

Terminal evaluation, on one river board:

| Benchmark | Hands (OOP, IP) | Time |
| --- | --- | --- |
| fold, full ranges | 1,081, 1,081 | 10.9 µs |
| showdown, full ranges | 1,081, 1,081 | 24.1 µs |
| ranking a river board's hands (`BoardHands::new`) | 1,081, 1,081 | 444 µs |
| fold, a river of `turn_srp` (7h) | 301, 284 | 2.91 µs |
| showdown, the same river | 301, 284 | 6.41 µs |
| through the game: a fold on the spot's own board, evaluated directly | 313, 291 | 2.94 µs |
| through the game: a fold on that river, with the gather and the scatter | 313, 291 | 4.10 µs |
| through the game: a showdown on that river, with the gather and the scatter | 313, 291 | 7.33 µs |

On the same afternoon, between runs, an iteration of `turn_srp` outside criterion took 0.165–0.27 s and one of `flop_srp` 1.9–3.0 s; the criterion run above was in the slower part of that range.

## Log of changes

One row per benchmark a change moves, newest last. "Before" and "after" are measured in the same session as the machine section asks, and the exploitability column records the check that the change did not make the test spots worse.

| Date | Change | Benchmark | Before | After | Change | How measured | Exploitability |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 2026-11-24 | Baseline, `b43ce4e` | (the tables above) | | | | criterion, one run | |
| 2026-11-24 | #87: the walks keep their working memory, the game lists the hands each card rules out, and keeps room for the gather and the scatter | `turn_srp`, one iteration | 0.162 s (median 0.168) | 0.128 s (0.131) | −21% (−22%) | alternating builds, below | every result bit for bit the same: `fingerprint` on 9 spots |
| 2026-11-24 | #87 | `turn_srp`, one best response | 0.0741 s (0.0794) | 0.0628 s (0.0653) | −15% (−18%) | alternating builds, below | the same |
| 2026-11-24 | #87 | `turn_srp`, one exploitability measurement | 0.276 s (0.288) | 0.237 s (0.249) | −14% (−14%) | alternating builds, below | the same |
| 2026-11-24 | #87 | `flop_srp`, one iteration | 2.30 s (2.55) | 1.74 s (1.88) | −25% (−26%) | alternating builds, below | the same |
| 2026-11-24 | #87 | `flop_srp`, one best response | 1.14 s (1.25) | 0.86 s (0.93) | −24% (−26%) | alternating builds, below | the same |
| 2026-11-24 | #87 | `flop_srp`, one exploitability measurement | 4.04 s (4.36) | 3.25 s (3.42) | −20% (−22%) | alternating builds, below | the same |
| 2026-11-24 | #87 | `turn_srp`, allocations in an iteration | 147,260 | 0, and 63 in the first | | counting allocator | the same |
| 2026-11-24 | #87 | `flop_srp`, allocations in an iteration | 1,788,758 | 0, and 75 in the first | | counting allocator | the same |
| 2026-11-24 | #87 | allocations in an exploitability measurement, `turn_srp` and `flop_srp` | 203,646 and 2,469,090 | 5,918 and 85,448, nearly all the two best responses' own strategies | −97% | counting allocator | the same |
| 2026-11-24 | #87 | a solve's peak heap, release build: `flop_srp`, `turn_srp`, `deep_high_spr` | 1,042.32 MB, 91.62 MB, 548 kB | 1,042.52 MB, 91.86 MB, 695 kB | +0.02%, +0.26%, +27% | `tests/memory_estimate.rs` on both builds | the same |
| 2026-11-24 | #87 | `turn_srp`, 8 independent solvers: throughput against one, and iterations a second in all | 5.8× (5.7–6.0), 26.2 a second | 6.2× (6.0–6.4), 37.8 a second | +44% iterations a second | alternating builds, below; one core taken by another job | the same |

How #87 was measured. The release builds of `0190ca3` and of the change each ran a small program, outside the repository, that times what `benches/solve.rs` times without criterion: 5 warm-up iterations, then, on the warm solver, an iteration, OOP's best response to the average strategy after the warm-up, and an exploitability measurement of it, again and again. The two builds ran back to back in 4 rounds of alternating order (old, new, new, old, and so on), with 7 timings of each step a round on `turn_srp` and 4 on `flop_srp`; the table gives the minimum of them all and, in brackets, the median. Another single-threaded solve ran on the machine throughout, with a burst on several threads every 15 to 20 minutes. In every step, each round's minimum of the new build is below each round's minimum of the old. The allocations are counted with a counting allocator in another such program, the same for both builds, as calls to allocate or reallocate. The thread-scaling rows are those of [How the current code would scale on threads](#how-the-current-code-would-scale-on-threads), repeated with both builds.

## Where an iteration spends its time

Measured on 2026-11-24 on the code of `b43ce4e`, in a release build, with a sampling profiler written for the purpose and kept out of the repository: a thread that suspends the solver's thread about once a millisecond, unwinds its stack with `RtlVirtualUnwind`, and afterwards resolves every frame, inline frames included, through `dbghelp` and the PDB. A sample counts towards the innermost frame of our own code that names a category (the sweep, the walk's node kinds by source line, the regret functions), and towards allocation when it is inside the allocator below that frame (`HeapAlloc`, `HeapFree` and their zeroing, all in `ntdll`). Shares are percent of the samples; with 5,000 to 7,400 samples a share is good to about ±1.3 points.

| Share of the time, % | `turn_srp`, iteration | `flop_srp`, iteration | `flop_srp`'s tree with 16 hands a side, iteration | `flop_srp`, exploitability |
| --- | --- | --- | --- | --- |
| terminal evaluation: the showdown sweep | 34.4 | 32.7 | 13.8 | 36.6 |
| terminal evaluation: the fold sweep | 11.9 | 8.8 | 3.2 | 9.7 |
| around it: the gather into a later board's order and the scatter back (`terminal_values`) | 8.2 | 7.4 | 5.8 | 9.5 |
| around it: the walk's terminal, zeroing the values of impossible hands | 2.7 | 3.0 | 1.4 | 3.2 |
| regret matching (the current strategy at every player node) | 7.9 | 9.8 | 7.9 | |
| instantaneous regrets, and the reach as `f32` | 6.2 | 6.5 | 2.3 | |
| regret and strategy-sum updates | 2.5 | 2.6 | 3.7 | |
| the rest of a node's arithmetic: reach products, action values, value sums | 6.3 | 6.5 | 9.4 | 11.3 |
| chance nodes: `rules_out` per hand and outcome, and the reach vectors after each card | 2.8 | 7.6 | 3.1 | 11.9 |
| allocation and freeing | 16.6 | 14.3 | 46.5 | 14.8 |
| everything else: trait calls, dispatch, the tree's shape check before each walk | 0.6 | 0.9 | 3.1 | 3.0 |
| samples | 4,848 (40 iterations) | 7,398 (5) | 4,993 (20) | 6,260 (2) |

What the columns show:

- **Terminal evaluation leads at realistic hand counts**: the sweeps are 46% of a turn iteration and 41% of a flop iteration, and 57% and 52% with the gather, the scatter and the walk's terminal around them, before the allocations those make. The showdown sweep alone is a third of the time, and within it `ReachSum::add`, which adds each opponent hand's reach into the per-card sums, is 13–16% of all samples: two dependent read-modify-writes per hand, done twice, once for the total and once in strength order.
- **Allocation is about a sixth of the time at 300 hands, and half at 16.** Every node of every walk allocates its vectors: the strategy, the reach of each action, the values, the instantaneous regrets, and at a chance node three vectors per card; `terminal_values` allocates two more on every board but the spot's. A counting allocator counts 147,000 allocations and frees per `turn_srp` iteration (374 MB asked for) and 1.79 million per `flop_srp` iteration (4.2 GB), and 2.47 million for one `flop_srp` exploitability measurement. An allocation and a free cost 70–90 ns here, and a zeroed block of 2.5–10 KB 180–350 ns. The cost per allocation does not grow with the hands, which is why its share falls from 46% to 14% between 16 and 300 hands. It does not dwarf everything else at realistic hand counts; the sweeps do more.
- **Chance nodes are cheap on the turn and not on the flop**: 7.6% of a flop iteration and 12% of its exploitability pass, nearly all of it `rules_out`, called once per hand and card for both players at every chance node.
- **The exploitability measurement** costs as much as 1.8 iterations (`flop_srp`: 0.15 s for the average strategy and 4.68 s for the measurement, against 2.69 s an iteration; `turn_srp` 1.7), so at the default of one measurement every 10 iterations, measuring is 15% of a solve's time.

### What cheap changes would buy, measured and left out

Each was tried in a throwaway copy of the code, timed against the unchanged code in alternating runs, and not kept.

- **A faster allocator.** With a global allocator that keeps freed blocks in per-size lists, uncontended, a `turn_srp` iteration took 0.150–0.157 s against 0.167–0.172 s (minima of 20 iterations, in five alternations), 7–11% less pair by pair, and 6–17% by the medians; a `flop_srp` iteration took 10–23% less, in noisier runs. That is about what removing the per-node allocations would buy single-threaded, since the zeroing stays; the strategies came out bit for bit the same.
- **Sizing the instantaneous regrets.** `own_node` builds them with `flat_map(..).collect()`, which cannot know the length and grows the vector by doubling at every node of the traverser. Writing them into a `vec![0.0; len]` instead, a one-line change in `pk_cfr::vector`, took a `turn_srp` iteration from 0.164–0.168 s to 0.155–0.158 s (minima of 20 iterations, in four alternations), 4–8% less pair by pair, bit for bit the same.

### How the current code would scale on threads

Independent solvers of `turn_srp`, one per thread, each running 20 iterations, give the most that threads over chance branches could reach with the code as it is:

| Threads | 1 | 2 | 4 | 8 | 16 |
| --- | --- | --- | --- | --- | --- |
| seconds per iteration, per thread | 0.184 | 0.209 | 0.228 | 0.259 | 0.386 |
| throughput against one thread | 1.0× | 1.8× | 3.2× | 5.7× | 7.6× |

The same test with the showdown sweep alone, which allocates nothing, slows each thread by about 13% at 8 threads, against 40% for the solver, so most of the solver's loss is not the cores' clock but what they share: the allocator, which every node of every walk calls, and the memory traffic of the buffers it hands out.

Repeated for #87, which takes the allocator out of the walks, with the builds before and after it alternating in 4 rounds, each solver running 20 iterations from the start. Another single-threaded solve ran on the machine throughout, so one core was taken, and the times are higher than above. Medians of the rounds, with the range of the throughput:

| Threads | 1 | 2 | 4 | 8 | 16 |
| --- | --- | --- | --- | --- | --- |
| seconds per iteration, per thread, before #87 | 0.225 | 0.269 | 0.296 | 0.305 | 0.448 |
| throughput against one thread, before | 1.0× | 1.7× (1.6–1.7) | 3.0× (2.9–3.1) | 5.8× (5.7–6.0) | 8.1× (7.1–8.8) |
| seconds per iteration, per thread, after #87 | 0.163 | 0.196 | 0.198 | 0.212 | 0.298 |
| throughput against one thread, after | 1.0× | 1.7× (1.6–1.7) | 3.3× (3.1–3.5) | 6.2× (6.0–6.4) | 8.7× (8.3–8.9) |

At 8 threads a solver now slows by 30% against 36% before, and the 8 do 37.8 iterations a second against 26.2, 44% more. So the allocator was a brake on threads, but not the main one. What the solvers still share is most likely the rest of the memory traffic: each streams its 92 MB of regrets and strategy sums every iteration, through 16 MB of L3 that all eight share.

### Merging suit-isomorphic branches

Counted with `pk_core::isomorphism` for ranges written as hand classes, which every relabelling of the suits leaves alone: the subtrees left after merging the cards that a symmetry of the board makes equivalent. An iteration's time is close to proportional to the river subtrees, which hold all but a few hundred of the terminals.

| Board | Turn subtrees left | River subtrees left |
| --- | --- | --- |
| `flop_srp`, Qs8s3d (two-tone) | 36 of 49 | 1,429 of 2,352 (61%) |
| Ks7d2c (rainbow, unpaired) | 49 of 49 | 2,352 of 2,352 (100%) |
| 7s7h2c (rainbow, paired) | 37 of 49 | 1,476 of 2,352 (63%) |
| Ks7s2s (monotone) | 23 of 49 | 675 of 2,352 (29%) |
| `turn_srp`, Qs8s3d 2d | | 35 of 48 (73%) |
| a turn of four suits, such as Qs8s3d 2h | | 48 of 48 (100%) |
| every flop, each once | | 71% on average: 29% on the 5% of flops that are monotone, 61% on the 55% that are two-tone, 92% on the 40% of three suits |

### What it says about the order of the planned optimizations

The plan is (1) merging suit-isomorphic chance branches, (2) threads across chance branches, (3) 16-bit storage. Each candidate, with what it addresses:

| Candidate | What it addresses | What it should buy | How hard |
| --- | --- | --- | --- |
| Threads across chance branches (#40, planned second) | all of an iteration and of a measurement | at most 6.2× on 8 threads by the bound above, measured after #87 took the allocations out (5.7× while every node allocated) | medium: the rows of a subtree are contiguous, so a chance node's children can walk on disjoint slices and be summed in outcome order, bit for bit; it changes `pk_cfr` |
| Suit isomorphism (#39, planned first) | the merged river subtrees, in time and in memory alike | 39% on `flop_srp`, 27% on `turn_srp`, 29% averaged over flops, nothing on an unpaired rainbow flop or a four-suit turn | the hardest of these: weighted chance outcomes, values permuted back for the merged cards, and every read-out below a merged card (paths, reaches, range views) mapped through the permutation; it changes `pk_cfr` and the game |
| Per-node allocation (done in #87, see the log of changes) | 14–17% of an iteration at 300 hands, 46% at 16, and the sharing that slows threads | about 10% single-threaded, measured above, and more under threads | easy to medium: a stack of buffers by depth in the walks of `pk_cfr::vector` and `pk_cfr::vector_best_response`, and scratch for `terminal_values`; bit for bit the same |
| Faster sweeps (not planned) | the showdown and fold sweeps, 41–46% | perhaps 15–20% if the showdown sweep were made twice as fast: one pass over the opponent's hands instead of two, and the per-card sums kept without the dependent stores | medium: subtle code, but every sweep is already held to an O(n²) oracle |
| The gather and the scatter (not planned; their allocations removed in #87) | 8%, and 3–4% more in their allocations | most of the 11–12% | medium: evaluate on the spot's hand lists with the dead hands' reach at zero, or at least reuse the two buffers |
| `rules_out` (done in #87) | with the vectors made after each card, 3% of a turn iteration, 7.6% of a flop iteration and 12% of a flop measurement | most of that | easy: a list per card of the hands it rules out, which needs a method on `PublicGame` |
| The instantaneous-regret buffer (done in #87) | the doubling `collect` in `own_node` | 4–8%, measured above | trivial, one line in `pk_cfr::vector` |
| Fewer copies at a measurement (not planned) | memory: the last profile (215 MB on `flop_srp`) and the best response's pure strategy (115 MB), 31% of the 1.05 GB peak | 330 MB on `flop_srp`, more than 16-bit storage saves | easy: drop the last profile before measuring, and compute best-response values without building the strategy; no change to the numbers |
| 16-bit storage (#41, planned third) | memory: the regrets and sums, 418 MB of the 1.05 GB peak (40%) | 209 MB (20%); no speed, and some cost in regret matching and the updates | medium, and it changes the numbers, so every exploitability check must be redone |
| Measuring less often (not planned) | 15% of a solve's time at a measurement every 10 iterations | most of that | trivial, an option's default, but it decides when solves stop |

So the planned order is not what the measurements favour:

1. **Per-node allocation first**, with the two smaller fixes beside it (the instantaneous-regret buffer and the `rules_out` lists): together about a fifth to a quarter of an iteration, bit for bit the same results, the cheapest work on the list, and the thing most likely to stop threads from scaling. Done in #87: a fifth to a quarter less time an iteration, bit for bit the same, as the log of changes records.
2. **Threads next**: the largest gain by far, and independent of the board.
3. **Isomorphism third**: about 29% less time and memory on an average flop, but nothing on the 31% of flops that are rainbow and unpaired, and the most work.
4. **For memory, the copies before the precision**: holding one profile and no best-response strategy saves more than 16-bit storage does, at no cost to the numbers. 16-bit storage after that, if a tree that matters still does not fit.

## Memory

The estimate of [`pk_postflop::memory`](../crates/postflop/src/memory.rs) against what a solve takes. The heap columns come from `tests/memory_estimate.rs`, which solves each spot under a counting allocator with the solve capped as shown, measuring every 10 iterations, once in a test build (with debug assertions, so with the invariant check the solve then makes, which the estimate counts) and once in a release build (`cargo test --release`). Each cell is the estimate, what was measured, and their ratio. The resident columns further down come from the example `solve_spot` in a release build, reading the process's peak working set from Windows.

| Spot | Solve | Heap, test build | Heap, release build |
| --- | --- | --- | --- |
| `deep_high_spr` (river) | 25 iterations | 718 kB, 710 kB: 1.010 | 703 kB, 695 kB: 1.010 |
| `turn_small` | 25 iterations | 1.030 MB, 1.021 MB: 1.008 | 921 kB, 914 kB: 1.008 |
| `turn_small` | 10 iterations, one measurement | 896 kB, 888 kB: 1.009 | 788 kB, 780 kB: 1.009 |
| `turn_small` | to 0.3% of the pot | 896 kB, 888 kB: 1.009 | 837 kB, 829 kB: 1.009 |
| `turn_srp` | 12 iterations | 92.2 MB, 91.9 MB: 1.004 | 92.2 MB, 91.9 MB: 1.004 |
| `flop_small` | 12 iterations | 52.0 MB, 51.8 MB: 1.004 | 45.4 MB, 45.2 MB: 1.004 |
| `flop_srp` | 12 iterations (slow test) | 1.047 GB, 1.043 GB: 1.004 | 1.047 GB, 1.043 GB: 1.004 |

Measured on the code of #87, which keeps the CFR walk's working memory from the first iteration to the end of the solve (DECISIONS P48). Without its margin of 1/256 and 4 KiB, the largest phase of the estimate is within 0.1% of the measured peak on every spot, and it is never below. Before #87 it was 2.5% above on `deep_high_spr` in a release build, where it bounded the walks' buffers down the costliest line; it now counts them as they are kept, a frame for each depth of the tree. The test fails an estimate below the measured peak, or more than 5% above it.

| Spot | Solve, release build | Estimate, total | Peak resident memory |
| --- | --- | --- | --- |
| `toy_polarized` | to 0.3% | 16.8 MB, nearly all of it the allowance for the process | 4.7–5.1 MB |
| `turn_srp` | 25 iterations | 116.0 MB | 101.8 MB |
| `flop_srp` | 25 iterations | 1.15 GB | 1.08 GB |
| `flop_srp` | to 0.3%: 80 iterations, 268 s before #87 and 207 s after it, not side by side and with another solve running | 1.15 GB | 1.08 GB |

The total adds to the heap an allowance for the allocator (32 bytes an allocation and 1/16 of the bytes) and 16 MiB for the process; it came 6% and 14% above the peak resident memory, which is the right side to err on. The figures are those of #87, which keeps the CFR walk's working memory through the solve: on these spots it moved the estimate by 0.25 MB at most and the peak resident memory by 0.2 MB, and `toy_polarized`'s peak stayed at 4.8 MB.

Where the flop spot's peak goes, while it measures exploitability: regrets and strategy sums 418 MB, two average-strategy profiles 430 MB, a best response 115 MB, the solver's bookkeeping 40 MB, the hands on its 1,226 boards 26 MB, the public tree 13 MB. Only the first is what the solve has to hold; the profiles and the best response are the "fewer copies" row above.

## What a real flop solve costs

A 100bb single-raised pot on the flop, pot 5.5bb and stacks 97.5bb, with `flop_srp`'s ranges (318 and 295 hands, about 300 a side) and three trees. The memory is the estimate, with the default options; the iteration was timed on the same afternoon as the baseline, one tree after another, so the ratios hold better than the seconds.

| Tree | Public nodes | Player nodes | Terminals | Memory: total (heap) | One iteration | Against (a) |
| --- | --- | --- | --- | --- | --- | --- |
| (a) one half-pot bet a street, no raise (`flop_srp`) | 191,844 | 85,264 | 106,136 | 1.15 GB (1.05 GB) | 2.4–3.0 s (1.9–2.1 s earlier) | 1 |
| (b) as (a), with all-in offered on the river as a bet and as a raise | 445,860 | 169,936 | 275,480 | 2.75 GB (2.54 GB) | 6.8–7.5 s | 2.5× |
| (b′) as (a), with all-in on the river as a bet only | 318,852 | 127,600 | 190,808 | 1.94 GB (1.79 GB) | 4.7–5.1 s | 1.75× |
| (c) one half-pot bet and one raise to 3× a street | 885,690 | 354,276 | 530,184 | 5.33 GB (4.94 GB) | 13.7–14.2 s | 5.0× |

All four are under the node limit of a million, (c) at 89% of it, and under the 10 GB limit. An iteration's time follows the number of terminals closely, and a solve adds a measurement every 10 iterations at about 1.8 iterations each.

Tree (a) solved to its target of 0.3% of the pot in 80 iterations, ending at 0.25%, in 268 s with its measurements (3.35 s an iteration all told), at a peak resident memory of 1.08 GB. At as many iterations, (b) would take about 11 minutes and (c) about 22. A tree with more actions usually needs more iterations to reach the same exploitability, so those are lower bounds: today, single-threaded on this machine, a flop solve with a raise on every street is a matter of half an hour or more and about 5 GB.
