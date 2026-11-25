# Configs

One TOML file per game config. Each file is an independent solve.

| File | Stack | Straddle | Ante |
| --- | --- | --- | --- |
| `a.toml` | 100bb | No | No |
| `b.toml` | 200bb | No | No |
| `c.toml` | 100bb | 2bb from UTG | 0.5bb from every seat |
| `d.toml` | 200bb | 2bb from UTG | 0.5bb from every seat |

All four are 8-handed with 5% rake, capped at 6bb, and no flop no drop (DECISIONS D16).

The schema is described in [docs/ARCHITECTURE.md](../docs/ARCHITECTURE.md#gameconfig-schema). Amounts are in big blinds and must be whole chips (1bb = 100 chips).

`trees/` holds tree files, which size the postflop betting tree in place of a config's `[postflop]` table (`solver solve-postflop --tree`; [Tree file](../docs/ARCHITECTURE.md#tree-file)). `trees/one_bet.toml` is one half-pot bet a street and no raises, small enough to solve a 100bb flop with real ranges on the development machine.
