# poker-solver

A personal-use No-Limit Hold'em cash-game solver in Rust.

- **Version 1** solves preflop for a multiway (6-max) table.
- **Version 2** adds a heads-up postflop solver, fed by the preflop ranges for a formation plus a board.

There are four configs: 100bb and 200bb, each with and without a straddle and ante.

**Status: planning.** There is no code yet. The backlog is in [Issues](https://github.com/Ollienel777/poker-solver/issues), grouped into [milestones](https://github.com/Ollienel777/poker-solver/milestones) M1 to M10, and starts with the M1 decisions (#1 to #5).

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
