# Reference fixtures

Reference data that tests and comparisons read. Nothing here is produced by this solver.

| Folder | Holds | Added in |
| --- | --- | --- |
| `ranges/<config>/<formation>.toml` | Heads-up ranges at the flop, in the [range-file format](../docs/ARCHITECTURE.md#range-file) | M3, M5, M7, M8 |
| `spots/river/*.toml` | River test spots, each with its tree sizing and rake, in the [spot-file format](../docs/ARCHITECTURE.md#spot-file) | M3 |
| `spots/flop/*.toml` | Flop test spots on a reduced tree | M4 |

Rules:

- Record reference solutions by hand. Do not scrape or automate extraction from a commercial solver.
- Open-source solvers are run as external binaries only. Their code is never copied into this repo.
- Note the source and date in each file: in a range file's `source` field, in a spot's `description`.

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

## River spots

The files in `spots/river/` are the test spots of milestone M3. The river solver must solve each of them to low exploitability and match the reference solver on each. A test in `pk-postflop` (`tests/river_spots.rs`) loads every file, builds its tree and its hands, checks that it writes back unchanged, and checks both tables below against the files, so the tables cannot drift from them.

Every range here is illustrative. The ranges were written for these fixtures, to tell a plausible story about what reaches each river, and were not taken from any solver, chart, site or book; each spot's `description` says so. Nothing depends on their being realistic.

| Spot | Stresses | Board | Pot | Stack | Tree | Rake | Nodes | OOP hands | IP hands |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `toy_polarized` | The polarized toy game, which has a known solution: the first correctness check ([below](#the-toy-game)) | `Ks9h6d3c2s` | 10bb | 20bb | one bet, 0.5; no raises | none | 9 | 22 | 12 |
| `dry_wide_single_bet` | A dry board, with no straight or flush possible, and wide ranges on the simplest real tree | `Kd9d7s4h2c` | 12bb | 90bb | one bet, 0.75; no raises | none | 9 | 287 | 250 |
| `dry_ip_polarized` | IP polarized (sets, two pair, missed draws) against a condensed OOP range of one-pair hands, on a dry board | `KcJh8s3d2c` | 20bb | 80bb | bets 0.5 and 1; raises to 3×; one raise | 5%, cap 3bb | 27 | 105 | 104 |
| `wet_wide_two_bets` | A wet board (the flush in, a straight for any 7 or QJ) and wide ranges, with two bet sizes and a raise | `Td9s8s6h2s` | 20bb | 80bb | bets 0.33 and 0.75; raises to 2.5×; one raise | none | 27 | 246 | 202 |
| `wet_oop_polarized` | The reverse: OOP polarized (straights, flushes, sets, missed draws) against a condensed IP range, with an overbet | `JhTc7h6d3h` | 30bb | 60bb | bets 0.5 and 1.5; no raises | 5%, cap 3bb | 15 | 91 | 87 |
| `paired_low_spr` | A paired board and a stack-to-pot ratio of 0.75, where all-in is the natural bet; narrow 3-bet-pot ranges | `QdQc8h5s3c` | 40bb | 30bb | one bet, 0.5, or all-in; facing a bet, all-in only; one raise | 5%, cap 3bb | 21 | 71 | 85 |
| `four_flush` | Four hearts on the board: any heart makes a flush, ranked by the hearts held, so card removal decides who can hold the better one | `Kh9h6h4h2c` | 16bb | 64bb | one bet, 0.5; raises to 2.5×; two raises | none | 21 | 187 | 161 |
| `deep_high_spr` | A stack-to-pot ratio near 18, with re-raises and all-in at every decision, and wide ranges | `AhJs7c4d3s` | 5.5bb | 97.5bb | bets 0.5 and 1, or all-in; raises to 3×, or all-in; three raises | 5%, cap 3bb | 87 | 315 | 291 |
| `double_paired_ties` | A double-paired board on which about a fifth of the pairs of hands split the pot, against about 1% on the other boards: the tie handling | `Kc9d9c5s5h` | 8bb | 24bb | one bet, 1, or all-in; facing a bet, all-in only; one raise | none | 21 | 298 | 241 |

Pot and stack are at the start of the river, and the stack is what each player has left to bet. Bets are fractions of the pot, and raises go to a multiple of the bet faced. "One raise" is `max_raises = 1`: after one raise a player facing a bet may only fold or call. Nodes counts every node of the tree, and a player's hands are the combos of their range with a positive weight that miss the board. Five spots are rake-free, for the first reference comparisons, and four take the configs' 5% capped at 3bb (DECISIONS D16).

### Reproducing the trees

Each spot's `[tree]` fixes its sizing, so a reference solver can be set up to build the same tree. The rules are in the `pk_game::tree` documentation; what matters for a reference is:

- OOP acts first on the river. With no bet to face, a player checks or bets each fraction of the pot in `bets`. Facing a bet, a player folds, calls, or raises *to* each multiple of the bet faced in `raises`. `allin = true` adds all-in as one more bet and one more raise. `max_raises` counts raises after the opening bet, all-in raises included.
- `allin_threshold = 1.0` in every spot, so no size is turned into all-in by a threshold. Turn off the reference's own all-in threshold, and any merging of nearby sizes.
- Every bet and raise is a whole number of chips before any rounding, so a solver that works in fractions gets the same amounts. A test walks every tree to check this. Give the amounts in chips (1bb = 100 chips) to a reference that takes integers.
- No sized bet or raise reaches the stack on a street without all-in, so the trees never depend on how a reference treats an oversized bet. Where all-in is offered, a raise larger than the stack is the all-in.
- A reference that sizes raises as a fraction of the pot, not a multiple of the bet, needs a separate size for each bet it faces in the trees with two bet sizes. The amounts it must reach are below.

The amounts in each tree, as the street total each bet or raise goes to:

| Spot | Bets | Raises to | All-in |
| --- | --- | --- | --- |
| `toy_polarized` | 5bb | none | none |
| `dry_wide_single_bet` | 9bb | none | none |
| `dry_ip_polarized` | 10, 20bb | 30, 60bb | none |
| `wet_wide_two_bets` | 6.6, 15bb | 16.5, 37.5bb | none |
| `wet_oop_polarized` | 15, 45bb | none | none |
| `paired_low_spr` | 20bb | none | 30bb |
| `four_flush` | 8bb | 20, 50bb | none |
| `deep_high_spr` | 2.75, 5.5bb | 8.25, 16.5, 24.75, 49.5, 74.25bb | 97.5bb |
| `double_paired_ties` | 8bb | none | 24bb |

### The toy game

`toy_polarized` is not a real poker spot. It is the polarized river game, whose solution is known, so it is the solver's first correctness check. OOP is the bettor, because OOP acts first. On `Ks9h6d3c2s`:

| Group | Range | Combos | Hand |
| --- | --- | --- | --- |
| OOP's nuts | `99,66` | 9s9d, 9s9c, 9d9c, 6s6h, 6s6c, 6h6c: 6 | a set |
| OOP's air | `JTs,JTo` | all 16 | king high |
| IP's bluff-catchers | `KQs,KQo` | the 12 that hold the Kh, Kd or Kc | a pair of kings |

Every nut combo beats every IP combo, which beats every air combo, and no card in one range is in the other, so every OOP hand meets every IP hand and card removal changes nothing. A test checks all of this with the evaluator. OOP may check or bet b = 0.5 of the pot (5bb into 10bb), and nobody may raise. The spot is rake-free.

At equilibrium:

- OOP bets all 6 nut combos, and bluffs with 2 of its 16 air combos in total, so that bluffs are b / (1 + 2b) = 1/4 of its bets. It checks the other 14 air combos. With more air than that, some air must check, and the bluffs' indifference fixes IP's calling frequency.
- IP calls a bet 1 / (1 + b) = 2/3 of the time, which leaves the bluffs indifferent: they win the 10bb pot a third of the time and lose the 5bb bet two thirds of the time. A bet's 25% of bluffs leaves IP indifferent in turn: calling 5bb wins 15bb a quarter of the time.
- After OOP checks, IP wins the 10bb pot whether it checks or bets, so its play there is not pinned down.
- In the payoffs of `pk_postflop::payoff`, which count from the start of the river, each nut combo wins 10 + 5 × 2/3 = 40/3bb and each air combo 0. Over the 22 × 12 deals, which are all possible, OOP's value is 40/11 ≈ 3.64bb and IP's is 70/11 ≈ 6.36bb, adding up to the 10bb pot.

The frequencies are over each range as a whole: any split of the 2 bluffs among the air combos, or of the calls among IP's combos, is an equilibrium.
