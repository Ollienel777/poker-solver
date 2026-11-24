# Configs

One TOML file per game config. Each file is an independent solve.

| File | Stack | Straddle | Ante |
| --- | --- | --- | --- |
| `a.toml` | 100bb | No | No |
| `b.toml` | 200bb | No | No |
| `c.toml` | 100bb | 2bb from UTG | 0.5bb from every seat |
| `d.toml` | 200bb | 2bb from UTG | 0.5bb from every seat |

All four are 8-handed with 5% rake and no flop no drop. The rake cap is 3bb in A and B and 6bb in C and D.

The schema is described in [docs/ARCHITECTURE.md](../docs/ARCHITECTURE.md#gameconfig-schema). Amounts are in big blinds and must be whole chips (1bb = 100 chips).
