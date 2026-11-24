# Reference fixtures

Reference data that tests and comparisons read. Nothing here is produced by this solver.

| Folder | Holds | Added in |
| --- | --- | --- |
| `ranges/<config>/<formation>.toml` | Preflop ranges in the range-file format | M3, M5, M7, M8 |
| `spots/river/*.toml` | River test spots, each with its tree sizing and rake | M3 |
| `spots/flop/*.toml` | Flop test spots on a reduced tree | M4 |

Rules:

- Record reference solutions by hand. Do not scrape or automate extraction from a commercial solver.
- Open-source solvers are run as external binaries only. Their code is never copied into this repo.
- Note the source and date in each file's `source` field.
