# poker-solver

A personal-use No-Limit Hold'em cash-game solver in Rust.

- **Version 1** solves preflop for a multiway (8-handed) table.
- **Version 2** adds a heads-up postflop solver, fed by the preflop ranges for a formation plus a board.

There are four configs: 100bb and 200bb, each with and without a straddle and ante.

**Status: M5, the postflop command line, is built, and its gate's two tests pass.** The `solver` command line solves a spot from a range file and a board, or from a spot file, saves the solution to a file, shows any node of it as 13x13 grids of hand classes, and compares two solutions, or a solution and a reference solver ([Trying it](#trying-it)). Underneath it, `solve` takes a spot (a board of three, four or five cards, two ranges, a pot, a stack, a betting tree and a rake) and finds equilibrium strategies with vector-form discounted CFR, over every card that can still fall, with no card abstraction. It runs on all the machine's threads and gives the same answer on any number of them, deals runouts that differ only by a relabelling of the suits once, and refuses a solve whose memory estimate is over a limit before it allocates anything. The four 100bb single-raised-pot flops in `reference/spots/flop/` solve to under 0.3% of the pot in 15 to 32 seconds on 8 cores, holding 0.4 to 1.1 GB, and the nine river spots in `reference/spots/river/` in about 0.3 seconds or less; a toy game matches its analytical solution. Underneath it, `pk-core` has cards, ranges, the evaluator, equity and suit isomorphism, `pk-game` has `GameConfig`, the betting-tree builder and the range-file format, and `pk-cfr` has the CFR core validated on Kuhn and Leduc. With both solvers at 0.03% of the pot, it matches postflop-solver on the nine river spots, three turn spots and four flop spots: frequencies within 2 points on a spot's first street and within 10 below one dealt card, and values within 0.2% of the pot, with the toy game's one free node held to its analytical bound. The reference's answer for a flop spot holds the flop and the turn, so the river of a flop spot is not compared. Run through the command line, the same comparison passes on `reference/ranges/a/BTN_vs_BB_SRP.toml` with the flop Ks7d2c and one bet a street; that range file is illustrative, and the same run passes on `reference/ranges/a/BTN_vs_BB_SRP_recorded.toml`, ranges recorded by hand from a reference ([#49](https://github.com/Ollienel777/poker-solver/issues/49)), which is M5's gate. Next is a local browser viewer for studying solutions, a milestone of its own (DECISIONS D24), and then M6, card abstraction. The backlog is in [Issues](https://github.com/Ollienel777/poker-solver/issues), grouped into [milestones](https://github.com/Ollienel777/poker-solver/milestones) M1 to M10.

## Trying it

The `solver` command line solves a spot and writes the solution to a file. Build it, and make a folder for the solutions, from the repository's root:

```bash
cargo build --release
mkdir -p solutions
```

A spot file solves as it is. The test spots in [`reference/spots/`](reference/spots) are spot files, and [reference/README.md](reference/README.md) says what each one tests. A river spot takes well under a second:

```bash
target/release/solver solve-postflop --spot reference/spots/river/dry_ip_polarized.toml --out solutions/dry_ip_polarized.pksol
```

It prints the spot, the memory estimates, a line per exploitability measurement with the time and the memory so far, and the result: how the solve stopped, the exploitability and each player's value. Then it writes the solution file. `toy_polarized` is the toy game with a known answer: OOP bets its sets (99 and 66) and JT an eighth of the time, IP calls two thirds of the bets, and the values are 40/11 and 70/11 bb.

A range file and a board make a spot under the config the range file names, read from `configs/` under the current directory or from `--config`. Config A's own tree is too large to solve on a flop, so `--tree` gives a smaller one, one half-pot bet a street and no raises:

```bash
target/release/solver estimate --ranges reference/ranges/a/BTN_vs_BB_SRP.toml --board Ks7d2c --tree configs/trees/one_bet.toml
target/release/solver solve-postflop --ranges reference/ranges/a/BTN_vs_BB_SRP.toml --board Ks7d2c --tree configs/trees/one_bet.toml --out solutions/BTN_vs_BB_SRP_Ks7d2c.pksol
```

The first prints the memory the solve will need, and the memory that saving its solution will need, item by item, without solving: about 1.1 GB and 1.0 GB. The second solves the spot in about half a minute on 16 threads, and ends:

```text
Stopped      at the target, after 70 iterations, in 27.68s
Exploitable  0.2923% of the pot, 0.0161bb per hand dealt
Values       OOP (BB) 1.56bb, IP (BTN) 3.28bb, per hand dealt
Peak memory  1.06 GB resident, against an estimate of 1.11 GB
Saving       `solutions/BTN_vs_BB_SRP_Ks7d2c.pksol`, holding the flop, the turn and the river
Saved        226.2 MB in 17.10s; as stored, in 16 bits a probability, exploitable for 0.2922% of the pot
```

The river is nearly all of a flop solution's file: `--last-street turn` leaves it out, and the same solution is then 1.8 MB. A solve says where its solution goes, with `--out`, or `--no-out` to keep nothing, and does not overwrite a file unless `--force` is given.

Without `--tree`, both say that config A's tree is over the node limit, and what to do. `--target <percent>` and `--max-iters <n>` change when a solve stops, `--threads <n>` how many threads it uses, and `--max-memory <GB>` the limit over which a solve is refused, 10 by default, which holds the save as it holds the solve. Each solve appends a line to `runs/log.jsonl` with its memory estimate, its peak memory, its time and its exploitability; `--no-log` turns that off. `solver solve-postflop --help` lists every option, and [ARCHITECTURE.md](docs/ARCHITECTURE.md#cli) describes the commands and the [tree file](docs/ARCHITECTURE.md#tree-file).

`solver inspect` shows a saved solution: the file, its spot and its solve, then a node, the root unless `--node` names another by its path, with each action's frequency, a 13x13 grid of hand classes per action, and one of the EVs. After OOP checks and IP bets half the pot:

```bash
target/release/solver inspect solutions/BTN_vs_BB_SRP_Ks7d2c.pksol --node "x b50"
```

```text
Node         `x b50`, on the flop, Ks7d2c
Pot          8.25bb; OOP has put in 0bb and has 97.5bb behind, IP 2.75bb and 94.75bb
Reached      85.42% of the hands dealt
Ranges       OOP 394.88 combos, IP 339.13: each hand's range weight times its own reach
Acts         OOP

  action                taken  of range
  f     fold           48.95%    48.73%
  c     call 2.75bb    51.05%    51.27%
...
c (call 2.75bb): the percent of each class's combos in OOP's range that take it; 51.27% of the range
      A   K   Q   J   T   9   8   7   6   5   4   3   2
  A         100  98  88  74  73 100  13  71  70  65 100
  K 100     100 100 100 100 100 100 100 100 100 100 100
  Q     100 100  76  74  70   6 100   0   0   0
  J  93 100   2 100  78  72   7 100   0
  T  34 100   0   1 100  78  67 100   0
  9   0               1 100  72 100  24
  8   0                   0 100 100  66   3
  7 100                         100 100 100
  6   0                              97  20   0
  5   0                                  91   9
  4   0                                      40   0
  3   0                                           1
  2 100                                             100
```

`--hand KQo` adds one class combo by combo, `--spot` prints the spot as a spot file, and `--dump` writes the solution as JSON. The root of this 226 MB file comes up in about a quarter of a second, since it reads none of the river. Saved with `--last-street turn`, the file is 1.8 MB, and a node on its river is shown by solving that river again from the ranges that reach it, in a few milliseconds.

`solver compare A B` compares two solutions of one spot, each a solution file or a JSON dump, ours or another solver's, under the gates' tolerances, and `solver compare A --reference` compares one with what the reference solver writes for its spot, once `PK_REFERENCE_SOLVER` names it ([reference/README.md](reference/README.md)). It prints which solution is which, the tolerances, what it could not check, and the comparison, and exits with 0 when every check passes and 1 when one does not. The spot above, solved again to 0.03% of the pot, as the gates compare, against the reference:

```bash
target/release/solver solve-postflop --ranges reference/ranges/a/BTN_vs_BB_SRP.toml --board Ks7d2c --tree configs/trees/one_bet.toml --target 0.03 --out solutions/BTN_vs_BB_SRP_Ks7d2c.tight.pksol
target/release/solver compare solutions/BTN_vs_BB_SRP_Ks7d2c.tight.pksol --reference
```

```text
Checked      not everything:
- B holds the flop and the turn only, so its strategy cannot be measured here: its exploitability is the 0.0297% of the pot its solver reports, which the verdict holds to the limit without checking it
...
Values at the root, in chips per hand dealt (limit 0.2% of the pot apart):
  OOP: A 156.76, B 156.71 (as reported, not measured): 0.0084% of the pot apart
  IP: A 327.44, B 327.40 (as reported, not measured): 0.0074% of the pot apart
...
  flop: 3 of 4 nodes checked (limit 2 points apart)
    largest difference 0.265 points, check at [check]
  turn, below 1 deal: 392 of 588 nodes checked (limit 10 points apart)
    largest difference 2.766 points, bet 275 at [check, check, deal Ah]
...
Verdict      A and B agree within the tolerances: every check passes
```

`solver serve` serves a folder of solution files to a browser, on this machine only:

```bash
target/release/solver serve solutions
```

It prints the address, `http://127.0.0.1:8080/` unless `--port` says another, and a line per request. The page there shows a solution a node at a time: the board, the action line as steps to click, the actions with their frequencies, a 13x13 grid with each hand's cell coloured by what the hand does, or by its EV, the combos of a hand clicked, and the cards to choose from where the turn or the river is dealt. It reads the server's JSON, `/api/files` for the list and `/api/node?file=BTN_vs_BB_SRP_Ks7d2c.pksol&path=x+b50` for a node, which is what `inspect --json` prints ([ARCHITECTURE.md](docs/ARCHITECTURE.md#serve)). Solutions stay loaded between requests, up to `--max-memory`, 10 GB by default. New solve, on the page, solves a spot from a range file of `reference/ranges` (`--ranges`), a board and a tree of `configs/trees` (`--trees`) into the same folder: it shows what the solve would need before it starts, refuses what `solve-postflop` would refuse, shows the solve's progress while it runs, and opens the new file when it is done. Run `serve` from the repository's root, where the range files' configs are.

## Docs

| File | What it holds |
| --- | --- |
| [BRIEF.md](BRIEF.md) | The original brief: goals, configs, milestones, validation, budget |
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | Crates, data types, algorithms, file formats, testing strategy |
| [docs/ROADMAP.md](docs/ROADMAP.md) | Milestones with every issue in order, plus risks |
| [docs/DECISIONS.md](docs/DECISIONS.md) | What's decided, what planning proposed, what's still open |
| [CLAUDE.md](CLAUDE.md) | Working rules for Claude Code sessions in this repo |

## How it fits together

```
configs/*.toml ──► preflop solver (MCCFR, abstracted postflop) ──► range files ──┐
                                                                                 ├─► postflop solver (DCFR) ──► solution ──► inspect
reference/ranges (hand-recorded) ────────────────────────────────────────────────┘
```

Range files recorded by hand use the same format as exported ones, so the postflop half is usable (after M5) before the preflop solver exists.

## Layout (planned)

```
crates/
  core/         pk-core         cards, ranges, evaluator, equity, suit isomorphism, hand indexer
  game/         pk-game         GameConfig, action order, betting-tree builders
  cfr/          pk-cfr          regret matching, CFR+/DCFR, MCCFR, best response
  abstraction/  pk-abstraction  equity histograms, k-means, bucket tables
  postflop/     pk-postflop     heads-up flop-to-river solver
  preflop/      pk-preflop      N-player MCCFR, checkpoints, range export
  cli/          pk-cli          the `solver` binary
configs/        a.toml … d.toml
reference/      hand-recorded ranges, test spots, reference solutions
```
