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
| `turn_srp` | Qs8s3d 2d | 11bb, 94.75bb | turn and river: bets of 0.5 and 1 pot, a raise to 3×, all-in; one raise a street | 313, 291 | 384 | 16,317 | 5,776 | 49 | 116 MB; 110.9 MB merged, as the solve runs since #39 |
| `flop_srp` | Qs8s3d | 5.5bb, 97.5bb | every street: one bet of 0.5 pot, no raise | 318, 295 | 117 | 191,844 | 85,264 | 1,226 | 1.15 GB; 969 MB merged, as the solve runs since #39 |

`flop_srp` is also tree (a) of [what a real flop solve costs](#what-a-real-flop-solve-costs).

## The machine, and how far to trust a number

An AMD Ryzen 7 7730U laptop (8 cores, 16 threads, 16 MB of L3), 16 GB of RAM, Windows 11 (10.0.26200), Rust 1.100.0, built with the release profile (fat LTO, one codegen unit). Every number here is on one thread unless it says otherwise. The solve runs on all the machine's threads by default since #40; the benchmarks call `pk_cfr` outside any rayon pool, so they run on one thread as before.

The machine runs other work, and its speed moves with it: on 2026-11-24 the same `flop_srp` iteration took from 1.9 s to 3.0 s over one afternoon, on unchanged code, and `turn_srp` from 0.16 s to 0.27 s. Criterion's intervals are within one run and do not show this. So:

- Compare a change by building both versions and running them back to back, alternating, three times or more, and compare the minima as well as the medians. A difference under about 10% needs that to be believed.
- Do not compare a new number with the baseline table below from another day.
- Each row of the log of changes says how it was measured.

A change meant to leave every number alone is checked with the example `fingerprint` (DECISIONS P49), which prints hashes of the bits of everything a solve computes: run it on the same spots and iteration counts before and after the change, on this machine, and the outputs must match line for line. Its header lists what it covers; #87 ran it on `deep_high_spr`, `four_flush` and `wet_oop_polarized` (200 iterations each), `turn_small` and `turn_medium` (100), `flop_small` (30), `flop_tiny` (100), `turn_srp` (12) and `flop_srp` (4). With `--threads <n>` it runs everything in a rayon pool of `n` threads, and its output does not name the count, so a change to the parallel walks is checked by comparing the outputs at several counts with each other and with the output before the change; #40 did so at 1, 2, 8 and 16 threads, and without the option, on those spots.

```bash
cargo run --release -p pk-postflop --example fingerprint -- crates/postflop/tests/data/turn_small.toml 100
cargo run --release -p pk-postflop --example fingerprint -- crates/postflop/tests/data/turn_small.toml 100 --threads 16
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
| 2026-11-25 | #40: on several threads the walks run the children of the first chance node on each line as rayon tasks, and add up their values in outcome order | `flop_srp`, one iteration, 16 threads (8 threads) | 1.978 s (median 2.094), one thread, `1c5e114` | 0.237 s (0.271); 0.340 s (0.370) | 8.34× (7.74×); 5.82× (5.66×) | alternating builds, below | every result bit for bit the same: `fingerprint` on 9 spots at 1, 2, 8 and 16 threads and without `--threads` |
| 2026-11-25 | #40 | `flop_srp`, one best response, 16 threads (8 threads) | 0.956 s (1.051) | 0.116 s (0.129); 0.163 s (0.178) | 8.22× (8.12×); 5.86× (5.91×) | alternating builds, below | the same |
| 2026-11-25 | #40 | `flop_srp`, one exploitability measurement, 16 threads (8 threads) | 3.627 s (3.720) | 0.462 s (0.533); 0.616 s (0.666) | 7.85× (6.98×); 5.89× (5.58×) | alternating builds, below | the same |
| 2026-11-25 | #40 | `turn_srp`, one iteration, 16 threads (8 threads) | 0.1445 s (0.1550) | 0.0294 s (0.0351); 0.0327 s (0.0390) | 4.92× (4.42×); 4.42× (3.97×) | alternating builds, below | the same |
| 2026-11-25 | #40 | `turn_srp`, one best response, 16 threads (8 threads) | 0.0718 s (0.0786) | 0.0116 s (0.0177); 0.0167 s (0.0200) | 6.16× (4.44×); 4.30× (3.93×) | alternating builds, below | the same |
| 2026-11-25 | #40 | `turn_srp`, one exploitability measurement, 16 threads (8 threads) | 0.2817 s (0.3356) | 0.0523 s (0.0679); 0.0601 s (0.0733) | 5.38× (4.94×); 4.69× (4.58×) | alternating builds, below | the same |
| 2026-11-25 | #40 | one iteration on one thread, `flop_srp` and `turn_srp` | 1.978 s (2.094), 0.1445 s (0.1550) | 2.008 s (2.090), 0.1460 s (0.1584) | +1.5% (−0.2%), +1.0% (+2.2%) | alternating builds, below | the same |
| 2026-11-25 | #40 | `flop_srp` solved to 0.3% of the pot: 80 iterations, ending at 0.2534% | 206.2 s, one thread, `1c5e114` | 196.6 s on one thread; 35.5 s on 16 | 5.8× on 16 threads | `solve_spot`, one run each, back to back; on 16 threads, iterations 20 to 30 took 11.3 s against about 3.5 s for every other ten, a stall of the machine | the same solve, iteration for iteration |
| 2026-11-25 | #40 | peak resident memory of those solves | 1.08 GB | 1.08 GB on one thread, 1.10 GB on 16 | +2% on 16 threads | `solve_spot` | the same |
| 2026-11-25 | #40 | a solve's peak heap on 16 threads against one, release build: `flop_srp`, `turn_srp` | 1,042.52 MB, 91.86 MB | 1,050.67 MB, 98.03 MB | +8.14 MB (+0.78%), +6.17 MB (+6.7%): the workspaces of the 49 and 48 outcomes, the pool and the outcomes' values | `tests/memory_estimate.rs`, its tests on one thread and on several | the same |
| 2026-11-25 | #40 | allocations in an iteration on several threads, `turn_small` and `flop_small` on 8 | | 0 after the first, in one call into the pool | | counting allocator, `tests/memory_estimate.rs` | the same |
| 2026-11-25 | #39: the solver iterates on the merged game, which deals once each class of turn or river cards that a symmetry of the suits makes alike; every measurement and the solution stay on the full tree | `flop_srp`, one iteration, 1 thread (16 threads) | 1.672 s (median 1.780); 0.1879 s (0.1987), `f4e3d6f` | 1.019 s (1.071); 0.1129 s (0.1226) | 1.64× (1.66×); 1.66× (1.62×) | alternating builds, below | within rounding of the full solve: the checks of DECISIONS P60; the reference comparison passes on the 7 gate spots |
| 2026-11-25 | #39 | `monotone_pot`, one iteration, 1 thread (16) | 2.507 s (2.593); 0.2918 s (0.3092) | 0.715 s (0.769); 0.0963 s (0.1025) | 3.50× (3.37×); 3.03× (3.02×) | alternating builds, below | the same |
| 2026-11-25 | #39 | `two_tone_raked`, one iteration, 1 thread (16) | 2.504 s (2.651); 0.2536 s (0.2744) | 1.258 s (1.533); 0.1515 s (0.1686) | 1.99× (1.73×); 1.67× (1.63×) | alternating builds, below | the same |
| 2026-11-25 | #39 | `paired_raise`, one iteration, 1 thread (16) | 1.308 s (1.437); 0.1414 s (0.1512) | 0.797 s (0.857); 0.0894 s (0.1002) | 1.64× (1.68×); 1.58× (1.51×) | alternating builds, below | the same |
| 2026-11-25 | #39 | `turn_srp`, one iteration, 1 thread (16) | 0.1301 s (0.1339); 0.0216 s (0.0243) | 0.0938 s (0.0958); 0.0174 s (0.0190) | 1.39× (1.40×); 1.24× (1.28×) | alternating builds, below | the same |
| 2026-11-25 | #39 | `dry_rainbow`, which nothing merges, one iteration, 1 thread (16) | 2.844 s (3.031); 0.2519 s (0.2844) | 2.873 s (3.011); 0.2472 s (0.2609) | 0.99× (1.01×); 1.02× (1.09×) | alternating builds, below | every result bit for bit the same |
| 2026-11-25 | #39 | one exploitability measurement, the average strategy built on the full tree included, 1 thread (16): `flop_srp`, `monotone_pot` | 3.362 s, 4.857 s; 0.597 s, 0.971 s | 3.382 s, 5.034 s; 0.613 s, 0.920 s | 0.99×, 0.96×; 0.97×, 1.06× | alternating builds, below; the other spots from 0.97× to 1.19× | the same |
| 2026-11-25 | #39 | building the average strategy on the full tree, a part of the measurement, 1 thread (16): `flop_srp`, `monotone_pot` | 0.242 s, 0.322 s; 0.215 s, 0.292 s | 0.278 s, 0.427 s; 0.230 s, 0.333 s | 0.87×, 0.75×; 0.94×, 0.88× | a second run of alternating builds, with the expansion as committed | the same |
| 2026-11-25 | #39, with P63 | `flop_srp` solved to 0.3% of the pot on 16 threads: 80 iterations, ending at 0.2534% (merged 0.2530%) | 21.1 s, `c1e173a` | 16.2 s | 1.30×: the 8 measurements are on the full tree and no faster | `solve_spot`, two runs each in alternating order, the machine idle: 21.10 and 21.15 s; 16.45 and 16.01 s. Single runs the day before, back to back, gave 25.7 s and 15.7 s | the same stopping iteration |
| 2026-11-25 | #39, with P63 | `monotone_pot` solved to 0.3% of the pot on 16 threads: 90 iterations, ending at 0.2548% (merged 0.2552%) | 32.3 s, `c1e173a` | 17.6 s | 1.84×: the 9 measurements are now about half the solve | `solve_spot`, two runs each in alternating order, the machine idle: 32.46 and 32.10 s; 17.62 and 17.51 s. Single runs the day before gave 34.2 s and 16.6 s | the same stopping iteration |
| 2026-11-25 | #39, with P63 | peak resident memory of those solves, `flop_srp` and `monotone_pot` | 1.10 GB, 1.55 GB | 921 to 924 MB, 1.07 GB | −16%, −31% | `solve_spot` | |
| 2026-11-25 | #39 | memory estimate on 16 threads, total (heap): `flop_srp`, `monotone_pot` | 1.16 GB (1.05 GB), 1.62 GB (1.49 GB) | 969 MB (881 MB), 1.12 GB (1.02 GB) | −17%, −31% | `solve_spot --estimate`, with the alike hands of the next row | |
| 2026-11-25 | #39, DECISIONS P63: at each terminal and chance node the merged walk gives the hands a node's stabilizer maps onto one another their first hand's value, so that they stay alike bit for bit | one merged iteration, `flop_srp` and `monotone_pot`, 1 thread (16) | 1.126 s, 0.771 s; 0.1094 s, 0.0796 s, `49b31d8` | 1.169 s, 0.805 s; 0.1092 s, 0.0823 s | +3.8%, +4.4%; 0%, +3.4% by the minima; +3.2%, +2.6%; +7.7%, +4.3% by the medians | alternating builds, 4 rounds, 4 timings a round, the machine idle; on 16 threads the rounds' minima overlap and single timings spread by 15% | unmerged results bit for bit the same; merged `flop_srp` and `monotone_pot` change in their last bits |

How #39 was measured. The program of #87 and #40, outside the repository, built against `f4e3d6f` and against the change, times on each spot, inside a rayon pool of the thread count timed, after 6 warm-up iterations in the pool: an iteration, then a measurement as the solve makes it, the average strategy on the full public tree (the solver's own on `f4e3d6f`, the merged average expanded through the counterparts on the change) and its exploitability, again and again. The two builds ran back to back in 4 rounds of alternating order (old, new, new, old), on one thread and on 16 in each, with 7 timings of each step a round on `turn_srp` and 3 on the flops. The machine was otherwise idle. `dry_rainbow`, whose deal no relabelling keeps, runs the full game on both builds and is the check that the rest of the code costs nothing; it came within 2% by the minima. The rows of the average strategy alone are a second run of the same kind, on `flop_srp` and `monotone_pot`, with the expansion as committed, which writes a node on a path of representatives straight from the solver's row; the first run, without that, had it 16% and 24% slower on one thread, and in that second run, on a slower afternoon, the iterations came out 1.72× and 3.41× on one thread and 1.63× and 3.14× on 16, as in the first. Each row gives the minimum of the timings and, in brackets, their median.

What a measurement costs against an iteration has changed: on `flop_srp` it was 2.0 iterations on one thread and 3.2 on 16, and is now 3.3 and 5.4; on `monotone_pot` it was 1.9 and 3.3 and is now 7.0 and 9.6. So at one measurement every 10 iterations, a merged `monotone_pot` solve on 16 threads spends about half its time measuring, as the whole solves above show. The measurement walks the full tree, which DECISIONS P58 chose so that every measurement checks the merging independently; walking the merged tree instead, or measuring less often, is where the next gain on a symmetric flop is.

How #87 was measured. The release builds of `0190ca3` and of the change each ran a small program, outside the repository, that times what `benches/solve.rs` times without criterion: 5 warm-up iterations, then, on the warm solver, an iteration, OOP's best response to the average strategy after the warm-up, and an exploitability measurement of it, again and again. The two builds ran back to back in 4 rounds of alternating order (old, new, new, old, and so on), with 7 timings of each step a round on `turn_srp` and 4 on `flop_srp`; the table gives the minimum of them all and, in brackets, the median. Another single-threaded solve ran on the machine throughout, with a burst on several threads every 15 to 20 minutes. In every step, each round's minimum of the new build is below each round's minimum of the old. The allocations are counted with a counting allocator in another such program, the same for both builds, as calls to allocate or reallocate. The thread-scaling rows are those of [How the current code would scale on threads](#how-the-current-code-would-scale-on-threads), repeated with both builds.

How #40 was measured. The program of #87, outside the repository, built against `1c5e114` and against the change, with every step run inside a rayon pool of the thread count timed (the build of `1c5e114` runs on one thread whatever the pool), and an untimed iteration in the pool after the warm-up, so that the workspaces of the split are sized before anything is timed. The two builds ran back to back in 4 rounds of alternating order (old, new, new, old, and so on), the old build on one thread and the new one on 1, 2, 4, 8 and 16 threads in each round, with 7 timings of each step a round on `turn_srp` and 4 on `flop_srp`. The machine was otherwise idle, as far as `tasklist` shows. Each cell is the minimum of the timings and, in brackets, their median, and the speed-up is against the old build on one thread in the same run, minimum against minimum and median against median:

| Step | Threads | `turn_srp` | Speed-up | `flop_srp` | Speed-up |
| --- | --- | --- | --- | --- | --- |
| iteration | `1c5e114`, 1 | 0.1445 s (0.1550) | | 1.978 s (2.094) | |
| | 1 | 0.1460 s (0.1584) | 0.99× (0.98×) | 2.008 s (2.090) | 0.99× (1.00×) |
| | 2 | 0.0963 s (0.1071) | 1.50× (1.45×) | 1.025 s (1.083) | 1.93× (1.93×) |
| | 4 | 0.0528 s (0.0557) | 2.74× (2.78×) | 0.552 s (0.586) | 3.58× (3.57×) |
| | 8 | 0.0327 s (0.0390) | 4.42× (3.97×) | 0.340 s (0.370) | 5.82× (5.66×) |
| | 16 | 0.0294 s (0.0351) | 4.92× (4.42×) | 0.237 s (0.271) | 8.34× (7.74×) |
| best response (OOP) | `1c5e114`, 1 | 0.0718 s (0.0786) | | 0.956 s (1.051) | |
| | 1 | 0.0706 s (0.0760) | 1.02× (1.03×) | 0.994 s (1.061) | 0.96× (0.99×) |
| | 2 | 0.0493 s (0.0531) | 1.46× (1.48×) | 0.500 s (0.558) | 1.91× (1.89×) |
| | 4 | 0.0257 s (0.0284) | 2.79× (2.76×) | 0.263 s (0.289) | 3.64× (3.64×) |
| | 8 | 0.0167 s (0.0200) | 4.30× (3.93×) | 0.163 s (0.178) | 5.86× (5.91×) |
| | 16 | 0.0116 s (0.0177) | 6.16× (4.44×) | 0.116 s (0.129) | 8.22× (8.12×) |
| exploitability measurement | `1c5e114`, 1 | 0.2817 s (0.3356) | | 3.627 s (3.720) | |
| | 1 | 0.3083 s (0.3373) | 0.91× (0.99×) | 3.672 s (3.805) | 0.99× (0.98×) |
| | 2 | 0.1812 s (0.1943) | 1.55× (1.73×) | 1.906 s (1.997) | 1.90× (1.86×) |
| | 4 | 0.0969 s (0.1050) | 2.91× (3.20×) | 1.042 s (1.074) | 3.48× (3.46×) |
| | 8 | 0.0601 s (0.0733) | 4.69× (4.58×) | 0.616 s (0.666) | 5.89× (5.58×) |
| | 16 | 0.0523 s (0.0679) | 5.38× (4.94×) | 0.462 s (0.533) | 7.85× (6.98×) |

That is the second of two such runs. In the first, an hour earlier, the new build on one thread came 13% slower than the old on a `flop_srp` iteration (2.127 s against 1.856 s) and 5% on `turn_srp`, in every round; on 16 threads it was 8.15× (7.65×) on a `flop_srp` iteration, 7.95× (8.21×) on a best response and 7.52× (7.18×) on a measurement, against that run's old build. The libraries' code was the same in both runs; only the timing program around them changed, to let it also time outside any pool, and with it the placement of the whole program's code, which fat LTO compiles as one unit. In the second run, and in a third that timed one-thread iterations alone, outside any pool and in a pool of one thread, for three builds alternating, the new build on one thread is within 2% of the old by the minima, as the whole solves above are within 5% the other way. So the 13% was the code's placement, not the walk, whose path on one thread differs from `1c5e114`'s by a subtraction at each row it reads and a test at each chance node.

Against the machine: 8 cores and 16 threads. On 8 threads a `flop_srp` iteration runs 5.8× as fast as on one, 73% of the cores, and on 16, two threads to a core, 8.3×. Against the bound of [independent solvers](#how-the-current-code-would-scale-on-threads), which the same machine gave as 1.75×, 3.19×, 6.27× and 9.6× at 2, 4, 8 and 16 threads on 2026-11-24, and as 1.75×, 3.43×, 6.11× and 6.65× on 2026-11-25 in a noisier hour, the flop is at the bound at 2 and 4 threads, at 93% of it at 8 and at 87% of the higher figure at 16. The turn spot is at 85% of it at 2 and 4 threads, 70% at 8 and 51% at 16: it loses the rest to threads waiting inside its many small chance nodes, as [where the time goes on threads](#where-the-time-goes-on-threads-40) measures.

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

Repeated twice for #40, on its code, whose walk outside a pool is that of #87, with the machine otherwise idle as far as `tasklist` shows, each solver running 20 iterations from the warm-up. One run each, so the spread is the two runs':

| Threads | 1 | 2 | 4 | 8 | 16 |
| --- | --- | --- | --- | --- | --- |
| seconds per iteration, per thread, 2026-11-24 | 0.164 | 0.188 | 0.206 | 0.208 | 0.259 |
| throughput against one thread | 1.0× | 1.75× | 3.19× | 6.27× | 9.6× |
| seconds per iteration, per thread, 2026-11-25 | 0.166 | 0.190 | 0.192 | 0.215 | 0.303 |
| throughput against one thread | 1.0× | 1.75× | 3.43× | 6.11× | 6.65× |

### Where the time goes on threads (#40)

Measured on 2026-11-25 with the code of #40, by a copy of it with counters added to the split walk, kept out of the repository: per iteration, the wall time, the wall time spent inside split chance nodes (the regions, from the fork of the first child to the return of the last), and the time spent walking the children, summed over the threads. Averages of 10 iterations of `turn_srp` and 3 of `flop_srp`, after 5 of warm-up and one in the pool, in one run.

| Spot, threads | Iteration | Outside the regions | Children a walk, and each one's time | Busy share of the threads inside the regions |
| --- | --- | --- | --- | --- |
| `turn_srp`, 2 | 79.8 ms | 0.7 ms | 720 (15 chance nodes of 48), 108 µs | 97.8% |
| `turn_srp`, 4 | 45.1 ms | 0.8 ms | 720, 114 µs | 92.9% |
| `turn_srp`, 8 | 34.4 ms | 0.9 ms | 720, 148 µs | 79.6% |
| `turn_srp`, 16 | 31.6 ms | 0.8 ms | 720, 170 µs | 49.8% |
| `flop_srp`, 2 | 1.135 s | 0.3 ms | 147 (3 chance nodes of 49), 7.63 ms | 98.8% |
| `flop_srp`, 8 | 0.379 s | 0.4 ms | 147, 9.58 ms | 92.9% |
| `flop_srp`, 16 | 0.258 s | 0.3 ms | 147, 11.29 ms | 80.6% |

What it shows:

- **Nothing outside the split chance nodes costs anything**: under 1 ms an iteration, the betting above the first card.
- **The turn spot loses half its threads at 16 to waiting inside the chance nodes.** Each of its 30 regions an iteration holds 48 children of 110–170 µs, 3 a thread at 16 threads, so the time the pool's threads take to wake and take their first child, and the region's last children, are a large part of each region. The flop's regions hold 49 children of 8–11 ms, and lose a fifth at 16 threads to the last round, 49 children being a little over 3 rounds of 16.
- **A child takes longer on more threads**: on the flop 26% longer at 8 threads than at 2 and 48% at 16, where two threads share each core, and on the turn 38% and 58%. That is what the threads share, the cores and the memory traffic, as the independent solvers above lose 10% at 8 and 38% at 16 against 2 (repeated below).
- **The shape check before each walk of a measurement** costs 21 ms a `flop_srp` measurement and 1.5 ms a `turn_srp` one: 0.5% of a measurement on one thread, and 4.8% and 2.6% on 16, under 1% of a solve, so it stays.

Splitting the river chance nodes inside the flop's turn subtrees as well, so that idle threads take rivers from the last turn subtrees, was tried in that copy, with a workspace per thread for the rivers, and timed against the first chance node alone in 3 alternating rounds of 4 iterations: 0.313 s (median 0.323) an iteration on 8 threads against 0.320 s (0.329), and 0.238 s (0.253) on 16 against 0.234 s (0.247). That is nothing beyond the noise, so the walk splits the first chance node on each line only (DECISIONS P51). What would help the turn spots is a split above their chance nodes, so that the 720 children of a walk share one region; that splits player nodes, which #40 leaves out.

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

Measured, with #39 (the log of changes above): on one thread an iteration takes what the counts say. `flop_srp` 1.64× as fast, against 1/0.61 = 1.65×; `monotone_pot`, on Qs 8s 3s, 3.50× against 1/0.287 = 3.48×; `paired_raise`, on 9s 9h 5d, 1.64× against 1.59× (its merged tree has 62.8% of the full one's nodes); `two_tone_raked`, on Jh Th 4c, 1.73× by the medians and 1.99× by the minima, against 1.65×, a noisier spot; and `turn_srp` 1.39× against 1/0.73 = 1.37×. The merged trees have 60.8% (`flop_srp`, 116,730 of 191,844 nodes), 28.8% (`monotone_pot`, 55,305) and 73.0% (`turn_srp`, 11,910 of 16,317) of the full trees' nodes. On 16 threads the two-tone and paired flops gain as much as on one, 1.58× to 1.67×, and two spots gain less: `monotone_pot` 3.03×, 87% of what the counts give, and `turn_srp` 1.24×, 90%. Threads share out the children of the first chance node on each line, and merging leaves fewer and unequal ones: on `monotone_pot` 23 turn classes for 16 threads, 13 of them with 35 river classes below and 10 with 22, where the full tree had 49 alike; on `turn_srp` 35 river classes instead of 48 children, each smaller, where the threads already waited half the time ([where the time goes on threads](#where-the-time-goes-on-threads-40)). Memory falls with the merged rows only, since the profiles of a measurement stay on the full tree: see [Memory](#memory). These timings are of the merged walk before DECISIONS P63, which keeps the hands a node's stabilizer maps onto one another alike bit for bit and adds 3.8% and 4.4% to an iteration of `flop_srp` and `monotone_pot` on one thread, and nothing measurable on 16 (the log of changes).

### What it says about the order of the planned optimizations

The plan is (1) merging suit-isomorphic chance branches, (2) threads across chance branches, (3) 16-bit storage. Each candidate, with what it addresses:

| Candidate | What it addresses | What it should buy | How hard |
| --- | --- | --- | --- |
| Threads across chance branches (#40, planned second; done, see the log of changes) | all of an iteration and of a measurement | at most 6.2× on 8 threads by the bound above, measured after #87 took the allocations out (5.7× while every node allocated); it bought 5.8× on 8 threads and 8.3× on 16 on a `flop_srp` iteration, 4.4× and 4.9× on `turn_srp` | medium: the rows of a subtree are contiguous, so a chance node's children can walk on disjoint slices and be summed in outcome order, bit for bit; it changes `pk_cfr` |
| Suit isomorphism (#39, planned first; done, see the log of changes) | the merged river subtrees, in time and in memory alike | 39% on `flop_srp`, 27% on `turn_srp`, 29% averaged over flops, nothing on an unpaired rainbow flop or a four-suit turn; it bought 1.64× on a `flop_srp` iteration and 3.50× on `monotone_pot` on one thread (1.66× and 3.03× on 16), and 16% and 31% of the solve's peak memory, the profiles of a measurement staying on the full tree | the hardest of these: weighted chance outcomes, values permuted back for the merged cards, and every read-out below a merged card mapped through the permutation; done instead by iterating on a merged game and expanding its strategy onto the full tree for every measurement and the solution, so that no read-out changed; it changes `pk_cfr` and the game |
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
2. **Threads next**: the largest gain by far, and independent of the board. Done in #40: a `flop_srp` solve to 0.3% of the pot in 35 s on 16 threads instead of 197 s on one, every result bit for bit the same.
3. **Isomorphism third**: about 29% less time and memory on an average flop, but nothing on the 31% of flops that are rainbow and unpaired, and the most work. Done in #39: an iteration takes what the counts say on one thread, `flop_srp`'s 1.64× as fast and `monotone_pot`'s 3.50×, a `flop_srp` solve to 0.3% on 16 threads takes 16.2 s instead of 21.1 s, and a measurement, which stays on the full tree, now costs 5 to 10 iterations on 16 threads.
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

Those solves run on one thread, and #40 left every figure of that table as it was, measured again on its code on 2026-11-25. On several threads, measured on the same day with the same spots and caps:

| Spot | Solve | Threads | Heap, test build | Heap, release build |
| --- | --- | --- | --- | --- |
| `turn_small` | 25 iterations | 2 | 1.228 MB, 1.210 MB: 1.015 | 1.145 MB, 1.074 MB: 1.066 |
| `turn_small` | 25 iterations | 4 | 1.240 MB, 1.215 MB: 1.020 | 1.157 MB, 1.114 MB: 1.038 |
| `turn_small` | 25 iterations | 16 | 1.314 MB, 1.292 MB: 1.018 | 1.231 MB, 1.190 MB: 1.034 |
| `flop_small` | 12 iterations | 2, 4, 16 | 52.2 MB, 51.9–52.0 MB: 1.004–1.005 | 45.6–45.7 MB, 45.4–45.5 MB: 1.004–1.006 |
| `turn_srp` | 12 iterations | 16 | 98.5 MB, 98.0 MB: 1.005 | 98.5 MB, 98.0 MB: 1.005 |
| `deep_high_spr` (river) | 25 iterations | 16, which it does not use | 718 kB, 710 kB: 1.010 | 703 kB, 695 kB: 1.010 |
| `flop_srp` | 12 iterations (slow test) | 16 | | 1.055 GB, 1.051 GB: 1.004 |

What the solve holds on several threads is the same for every count but the pool's few kilobytes a thread: a workspace for each outcome of the split chance nodes, for the CFR walk throughout and for a measurement walk while it runs, and every outcome's values at the split. On `flop_srp` that is 8.14 MB (0.78%) more than on one thread, measured, and on `turn_srp` 6.17 MB (6.7%): a workspace for its river betting, with two sizes, a raise and all-in, is about as large as one for the flop's turn and river, while the whole solve is a tenth of the flop's. The estimate is never below the measured peak; how far above it is depends on how the pool's threads shared out their jobs and on whether an earlier pool's threads were still freeing their memory, and matters only on the smallest spots, where the pool's allowance of 12 KiB and 6 KiB a thread is a share of the whole: 6.6% above on `turn_small`, a spot of 1 MB, on 2 threads. The test on several threads fails an estimate below the peak, or more than 5%, 16 KiB a thread and 64 KiB above it (DECISIONS P55): the 64 KiB for an earlier pool's threads freeing their memory during the measurement, which is the likeliest reading of `turn_small` on 2 threads in the release build: the estimate was 70.8 kB above the peak there, against 42.8 kB on 4 threads and 40.8 kB on 16, in a solve measured right after one on 16 threads.

Measured again on the code of #39 on 2026-11-25, the solve merging where it can: on `turn_srp` and `flop_srp`, and on the spots of `tests/data` written for the merging; every other spot above solves and is estimated as before. The merged solver stores the merged tree's rows, beside the full game and the merged game, and measures on the full tree, so the profiles of a measurement are what they were:

| Spot | Solve | Threads | Heap, test build | Heap, release build |
| --- | --- | --- | --- | --- |
| `turn_srp` | 12 iterations | 1 | 82.4 MB, 82.1 MB: 1.004 | 82.4 MB, 82.1 MB: 1.004 |
| `turn_srp` | 12 iterations | 16 | 87.5 MB, 87.0 MB: 1.005 | 87.5 MB, 87.1 MB: 1.005 |
| `turn_srp`, merging off | 12 iterations | 1 | 92.2 MB, 91.9 MB: 1.004 | 92.2 MB, 91.9 MB: 1.004 |
| `flop_srp` | 12 iterations (slow test) | 1 | | 874.0 MB, 870.6 MB: 1.004 |
| `flop_srp` | 12 iterations (slow test) | 16 | | 880.7 MB, 877.2 MB: 1.004 |
| `flop_monotone` | 12 iterations | 1 | 16.54 MB, 16.47 MB: 1.004 | 14.80 MB, 14.74 MB: 1.004 |
| `flop_monotone` | 12 iterations | 2, 16 | 16.71–16.79 MB, 16.63–16.72 MB: 1.004–1.005 | 14.97–15.06 MB, 14.90–14.99 MB: 1.004–1.005 |
| `flop_two_tone` | 12 iterations | 1 | 19.66 MB, 19.58 MB: 1.004 | 17.92 MB, 17.85 MB: 1.004 |
| `turn_four_flush` | 25 iterations | 1 | 1.273 MB, 1.263 MB: 1.008 | 1.164 MB, 1.155 MB: 1.007 |
| `turn_four_flush` | 25 iterations | 2, 16 | 1.44–1.53 MB, 1.39–1.51 MB: 1.010–1.039 | 1.44–1.52 MB, 1.36–1.49 MB: 1.023–1.053 |

On one thread the largest phase of the estimate is within a kilobyte of the measured peak on the merging's spots, and equal to it on `turn_srp`, merged or not; `turn_srp` with merging off holds the 91.86 MB it held before #39. `flop_srp`'s peak heap falls from 1,042.5 MB to 870.6 MB on one thread and from 1,050.7 MB to 877.2 MB on 16, 16% less: its regrets and strategy sums from 418 MB to 254 MB, its bookkeeping from 40 MB to 23 MB, against 8.4 MB for the merged game, its classes of alike hands (DECISIONS P63) included. The figures are with those classes, which add about 1 MB to `flop_srp`, nearly all of it a byte more in each merged node's record, which rounds it from 40 bytes to 48. Its two profiles, 430 MB, and the best response, 118 MB, are as before, and are now 63% of the peak.

| Spot | Solve, release build | Estimate, total | Peak resident memory |
| --- | --- | --- | --- |
| `toy_polarized` | to 0.3% | 16.8 MB, nearly all of it the allowance for the process | 4.7–5.1 MB |
| `turn_srp` | 25 iterations | 116.0 MB | 101.8 MB |
| `flop_srp` | 25 iterations | 1.15 GB | 1.08 GB |
| `flop_srp` | to 0.3%: 80 iterations, 268 s before #87 and 207 s after it, not side by side and with another solve running | 1.15 GB | 1.08 GB |
| `flop_srp` | to 0.3%, #40: 80 iterations, 206.2 s on `1c5e114`, 196.6 s on one thread and 35.5 s on 16, back to back | 1.15 GB on one thread, 1.16 GB on 16 | 1.08 GB on `1c5e114` and on one thread, 1.10 GB on 16 |
| `flop_srp` | to 0.3% on 16 threads, #39: 80 iterations, 21.1 s on `c1e173a` and 16.2 s merged, two runs each in alternating order | 1.16 GB on `c1e173a`, 969 MB merged | 1.10 GB on `c1e173a`, 921 to 924 MB merged |
| `monotone_pot` | to 0.3% on 16 threads, #39: 90 iterations, 32.3 s on `c1e173a` and 17.6 s merged, two runs each in alternating order | 1.62 GB on `c1e173a`, 1.12 GB merged | 1.55 GB on `c1e173a`, 1.07 GB merged |

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
