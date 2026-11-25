# poker-solver

A personal-use No-Limit Hold'em cash-game solver in Rust.

- **Version 1** solves preflop for a multiway (8-handed) table.
- **Version 2** adds a heads-up postflop solver, fed by the preflop ranges for a formation plus a board.

There are four configs: 100bb and 200bb, each with and without a straddle and ante.

**Status: M4, the turn and flop solver, is built and matches a reference solver on every test spot.** `solve` takes a spot (a board of three, four or five cards, two ranges, a pot, a stack, a betting tree and a rake) and finds equilibrium strategies with vector-form discounted CFR, over every card that can still fall, with no card abstraction. It runs on all the machine's threads and gives the same answer on any number of them, deals runouts that differ only by a relabelling of the suits once, and refuses a solve whose memory estimate is over a limit before it allocates anything. The four 100bb single-raised-pot flops in `reference/spots/flop/` solve to under 0.3% of the pot in 15 to 32 seconds on 8 cores, holding 0.4 to 1.1 GB, and the nine river spots in `reference/spots/river/` in about 0.3 seconds or less; a toy game matches its analytical solution. Underneath it, `pk-core` has cards, ranges, the evaluator, equity and suit isomorphism, `pk-game` has `GameConfig`, the betting-tree builder and the range-file format, and `pk-cfr` has the CFR core validated on Kuhn and Leduc. There is no command line yet, but an example program solves a spot and prints the answer ([Trying it](#trying-it)). With both solvers at 0.03% of the pot, it matches postflop-solver on the nine river spots, three turn spots and four flop spots: frequencies within 2 points on a spot's first street and within 10 below one dealt card, and values within 0.2% of the pot, with the toy game's one free node held to its analytical bound. The reference's answer for a flop spot holds the flop and the turn, so the river of a flop spot is not compared. Next is M5, the command line. The backlog is in [Issues](https://github.com/Ollienel777/poker-solver/issues), grouped into [milestones](https://github.com/Ollienel777/poker-solver/milestones) M1 to M10.

## Trying it

Until the command line arrives in M5, a Cargo example solves a spot file and prints what it found. On a river spot:

```bash
cargo run --release -p pk-postflop --example solve_spot -- reference/spots/river/dry_ip_polarized.toml
cargo run --release -p pk-postflop --example solve_spot -- reference/spots/river/dry_ip_polarized.toml --node "x"
```

It prints the spot, a line per exploitability measurement, the result and each player's value, then the strategy at the root, or at the node that `--node` names by its actions' tokens: each action's frequency over the acting player's range, a 13x13 hand-class grid per action, and the nodes that follow, to pass to `--node` next. `--target <percent>` and `--iterations <cap>` change when the solve stops. In the second command, IP's polarized range mostly bets pot after OOP checks:

```text
  x     check                      32.27%
  b50   bet to 1000 chips (10bb)    0.41%
  b100  bet to 2000 chips (20bb)   67.32%
```

Any of the nine spots in [`reference/spots/river/`](reference/spots/river) works, and [reference/README.md](reference/README.md#river-spots) says what each one tests. `toy_polarized` is the toy game with a known answer: OOP bets its sets (99 and 66) and JT an eighth of the time, and IP calls two thirds of the bets.

A turn or flop spot works the same way, and takes longer. The spots are in [`reference/spots/turn/`](reference/spots/turn) and [`reference/spots/flop/`](reference/spots/flop), and [reference/README.md](reference/README.md#turn-and-flop-spots) describes them:

```bash
cargo run --release -p pk-postflop --example solve_spot -- reference/spots/flop/monotone_pot.toml --estimate
cargo run --release -p pk-postflop --example solve_spot -- reference/spots/flop/monotone_pot.toml
```

The first command prints the memory the solve will need, item by item, without solving: about 0.6 GB here. The second solves the spot, in about 16 seconds on 8 cores. `--threads <n>` sets how many threads it uses and `--max-memory <GB>` the limit over which a solve is refused, 10 by default. Each solve appends a line to `runs/log.jsonl` with its memory estimate, its peak memory, its time and its exploitability; `--no-log` turns that off.

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
