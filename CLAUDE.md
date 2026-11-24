# CLAUDE.md

This is a personal-use NLHE cash-game solver in Rust, with multiway preflop MCCFR and heads-up postflop DCFR. Where things are:

- [BRIEF.md](BRIEF.md) for the goals
- [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for the design
- [docs/DECISIONS.md](docs/DECISIONS.md) for settled and open choices
- [docs/ROADMAP.md](docs/ROADMAP.md) for the order of work

The backlog lives in GitHub Issues: `gh issue list --milestone "M1 · Foundations"`.

## Working rules

- **Work one milestone at a time.** A milestone's build issues are blocked by the previous milestone's `type: gate` issue. Plan the milestone from its issues and wait for approval before writing code.
- **Gates close only when every acceptance test in them passes.** For slow tests, paste the output into the gate issue.
- **One branch and PR per milestone, one commit per issue.** The commit message names its issue, and the PR body closes them all with `Closes #N`.
- **Correctness before speed.** Optimize nothing until its test passes and a benchmark shows it matters. Perf PRs post before/after numbers.
- **Tests come with the code**, and you run them before saying something is done. Slow tests are marked `#[ignore = "slow"]`.
- **Never copy code from AGPL solvers** (postflop-solver, TexasSolver). Use them only as external binaries for comparison. Implement algorithms from papers and cite the paper in a doc comment.
- **No AWS work** outside `area: aws` issues, and nothing launches before the billing alarm exists (#69).
- **No solver code branches on straddle or ante.** They are forced bets in `GameConfig`.
- **Amounts are integer chips** (1bb = 100).
- **Determinism.** Every RNG is seeded, and postflop results must be identical for any thread count.
- **Only `pk-cli` prints.** Library crates report progress through callbacks.
- **Record decision changes.** If a design choice changes, update docs/DECISIONS.md in the same PR. If an open decision blocks the work, ask; don't assume.

## Commands

```bash
cargo build --release
cargo test                                  # fast suite
cargo test --release -- --ignored           # slow suite
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
```

Tests are built optimized (`[profile.test] opt-level = 3`) with debug assertions on, because they enumerate boards and run solves.

The toolchain is stable Rust, set in `rust-toolchain.toml`. This machine uses the GNU host (`x86_64-pc-windows-gnu`), which has no C compiler, so a crate that builds C code needs one installed first.

## Layout

`crates/{core,game,cfr,abstraction,postflop,preflop,cli}`, with packages named `pk-*` and the binary named `solver`. Configs are in `configs/`, fixtures in `reference/`, and run logs in `runs/` (gitignored).

## Starting a session

Use this prompt: "Read CLAUDE.md, then plan the next open milestone from its GitHub issues and wait for approval."
