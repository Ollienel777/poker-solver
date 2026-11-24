# Reference fixtures

Reference data that tests and comparisons read. Nothing here is produced by this solver.

| Folder | Holds | Added in |
| --- | --- | --- |
| `ranges/<config>/<formation>.toml` | Heads-up ranges at the flop, in the [range-file format](../docs/ARCHITECTURE.md#range-file) | M3, M5, M7, M8 |
| `spots/river/*.toml` | River test spots, each with its tree sizing and rake | M3 |
| `spots/flop/*.toml` | Flop test spots on a reduced tree | M4 |

Rules:

- Record reference solutions by hand. Do not scrape or automate extraction from a commercial solver.
- Open-source solvers are run as external binaries only. Their code is never copied into this repo.
- Note the source and date in each file's `source` field.

## Range files

A range file's folder is its `config` and its file name is its `formation`, as in `ranges/a/BTN_vs_BB_SRP.toml`. A test in `pk-game` loads every file here, checks it against its config, and checks that writing it back is stable, so a mistyped file fails the test suite.

The first four files are illustrative. Their ranges were made up to exercise the format and are not from any reference, and each says so in its `source`. Their pots and stacks are exact for their lines under their configs, so tests can use them as fixtures:

| File | Line | Pot | Effective stack |
| --- | --- | --- | --- |
| `a/BTN_vs_BB_SRP` | BTN opens to 2.5bb, SB folds, BB calls | 2.5 + 2.5 + 0.5 (SB) = 5.5bb | 100 − 2.5 = 97.5bb |
| `a/BB_vs_BTN_3BP` | BTN opens to 2.5bb, SB folds, BB 3-bets to 10bb, BTN calls | 10 + 10 + 0.5 (SB) = 20.5bb | 100 − 10 = 90bb |
| `c/BTN_vs_BB_SRP` | BTN opens to 5bb over the straddle, SB folds, BB calls, UTG folds | 5 + 5 + 0.5 (SB) + 2 (straddle) + 4 (antes) = 16.5bb | 100 − 0.5 (ante) − 5 = 94.5bb |
| `c/BTN_vs_UTG_SRP` | BTN opens to 5bb, both blinds fold, UTG calls from the straddle | 5 + 5 + 0.5 (SB) + 1 (BB) + 4 (antes) = 15.5bb | 100 − 0.5 (ante) − 5 = 94.5bb |

Hand-recorded references go alongside them. Keep an illustrative file's `source` as it is, so that it is never mistaken for a reference.
