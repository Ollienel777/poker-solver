# Decisions

This file has three lists:

- **Decided:** what the brief settled.
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

## Proposed

| ID | Proposal | Why |
| --- | --- | --- |
| P1 | Integer chips, with 1bb = 100 | Exact pot arithmetic and ties, with rounding done in one place |
| P2 | Preflop sizes are multiples of the effective blind (the largest live forced bet) | The straddle configs scale on their own: a 2.5x open is 5bb under a 2bb straddle, and no code special-cases the straddle |
| P3 | The `cfr` crate knows nothing about poker and holds both scalar and vector-form helpers | One regret implementation for toy games, preflop and postflop |
| P4 | MCCFR and vector-form CFR are validated on Leduc in M2 | Removes risk from M3 and M7, where bugs are harder to see |
| P5 | Postflop parallel work reduces in a fixed order | Results are bit-identical for any thread count, so tests can be exact |
| P6 | The preflop solve accumulates the average strategy only at preflop nodes | Only the preflop strategy is exported, so this saves memory (Pluribus does the same) |
| P7 | The N-player showdown implements layered side pots | A BB ante leaves the BB with a shorter live stack, so equal stacks can't be assumed |
| P8 | Range files are TOML holding standard range strings | They can be typed by hand from a reference and diffed, and the same file serves export |
| P9 | Postflop nodes store only the combos that are in range | Memory |
| P10 | The abstraction's river equity reuses the showdown sweep, once per board | The code is shared, and the pass takes minutes instead of hours |
| P11 | EMD between 1-D histograms is computed as the L1 distance between CDFs | Exact in one dimension, so no optimal-transport solver is needed |
| P12 | The straddler keeps the position name UTG | Formation names stay the same across configs |
| P13 | Packages are prefixed `pk-` | A crate named `core` collides with Rust's built-in `core` |
| P14 | Each milestone has one gate issue holding its acceptance tests | Makes the brief's "tests pass before moving on" rule visible in the tracker |

## Open

The brief says to settle table size, ante and straddle before writing `GameConfig` ([#12](https://github.com/Ollienel777/poker-solver/issues/12)). Rake belongs there too, because `GameConfig` carries the rake fields.

| Question | Default until answered | Blocks | Issue |
| --- | --- | --- | --- |
| Rust or C++ | Rust | Workspace setup | [#1](https://github.com/Ollienel777/poker-solver/issues/1) |
| Table size | 6-max | `GameConfig` | [#2](https://github.com/Ollienel777/poker-solver/issues/2) |
| Ante format and size | none | `GameConfig` | [#3](https://github.com/Ollienel777/poker-solver/issues/3) |
| Straddle rules | 2bb, live, UTG, always posted | `GameConfig` | [#4](https://github.com/Ollienel777/poker-solver/issues/4) |
| Rake | rake-free | `GameConfig`, rake at terminals | [#5](https://github.com/Ollienel777/poker-solver/issues/5) |
| Dev machine specs | none | Local test sizes, memory limit | [#23](https://github.com/Ollienel777/poker-solver/issues/23) |
| Reference solver | postflop-solver or TexasSolver, as a binary | Cross-solver checks | [#24](https://github.com/Ollienel777/poker-solver/issues/24) |
| Output format for study | terminal grids | `cli` export | [#44](https://github.com/Ollienel777/poker-solver/issues/44) |
| Limps in the preflop tree | none | Preflop tree | [#57](https://github.com/Ollienel777/poker-solver/issues/57) |
| 3+-way pots in the preflop solve | option C (minimal tree) | Preflop game layout | [#58](https://github.com/Ollienel777/poker-solver/issues/58) |
