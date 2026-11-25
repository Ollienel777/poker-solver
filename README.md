# poker-solver

A personal-use No-Limit Hold'em cash-game solver in Rust.

- **Version 1** solves preflop for a multiway (8-handed) table.
- **Version 2** adds a heads-up postflop solver, fed by the preflop ranges for a formation plus a board.

There are four configs: 100bb and 200bb, each with and without a straddle and ante.

**Status: M4, the turn and flop solver, is built and matches a reference solver on every test spot.** `solve` takes a spot (a board of three, four or five cards, two ranges, a pot, a stack, a betting tree and a rake) and finds equilibrium strategies with vector-form discounted CFR, over every card that can still fall, with no card abstraction. It runs on all the machine's threads and gives the same answer on any number of them, deals runouts that differ only by a relabelling of the suits once, and refuses a solve whose memory estimate is over a limit before it allocates anything. The four 100bb single-raised-pot flops in `reference/spots/flop/` solve to under 0.3% of the pot in 15 to 32 seconds on 8 cores, holding 0.4 to 1.1 GB, and the nine river spots in `reference/spots/river/` in about 0.3 seconds or less; a toy game matches its analytical solution. Underneath it, `pk-core` has cards, ranges, the evaluator, equity and suit isomorphism, `pk-game` has `GameConfig`, the betting-tree builder and the range-file format, and `pk-cfr` has the CFR core validated on Kuhn and Leduc. The `solver` command line solves a spot from a range file and a board, or from a spot file ([Trying it](#trying-it)). With both solvers at 0.03% of the pot, it matches postflop-solver on the nine river spots, three turn spots and four flop spots: frequencies within 2 points on a spot's first street and within 10 below one dealt card, and values within 0.2% of the pot, with the toy game's one free node held to its analytical bound. The reference's answer for a flop spot holds the flop and the turn, so the river of a flop spot is not compared. Next is M5, the command line. The backlog is in [Issues](https://github.com/Ollienel777/poker-solver/issues), grouped into [milestones](https://github.com/Ollienel777/poker-solver/milestones) M1 to M10.

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

Until `solver inspect` arrives, the example `solve_spot` prints a spot file's strategy at a node as a 13x13 grid of hand classes per action: `cargo run --release -p pk-postflop --example solve_spot -- reference/spots/river/dry_ip_polarized.toml --node "x"`.

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
