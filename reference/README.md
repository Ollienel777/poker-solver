# Reference fixtures

Reference data that tests and comparisons read. Nothing here is produced by this solver.

| Folder | Holds | Added in |
| --- | --- | --- |
| `ranges/<config>/<formation>.toml` | Heads-up ranges at the flop, in the [range-file format](../docs/ARCHITECTURE.md#range-file) | M3, M5, M7, M8 |
| `spots/river/*.toml` | River test spots, each with its tree sizing and rake, in the [spot-file format](../docs/ARCHITECTURE.md#spot-file) | M3 |
| `spots/turn/*.toml` | Turn test spots, compared with the reference by the M4 gate's test | M4 |
| `spots/flop/*.toml` | Flop test spots on a reduced tree, compared with the reference by the M4 gate's test | M4 |

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

Pot and stack are at the start of the river, and the stack is what each player has left to bet. Bets are fractions of the pot, and raises go to a multiple of the bet faced. "One raise" is `max_raises = 1`: after one raise a player facing a bet may only fold or call. Nodes counts every node of the tree, and a player's hands are the combos of their range with a positive weight that miss the board. Five spots are rake-free, for the first reference comparisons, and four take 5% capped at 3bb. The configs cap at 6bb (DECISIONS D16); the spots keep the lower cap because it binds in more of their pots.

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

### Reading a reference's answer

A reference solver's solution of a spot is read as a [solution dump](../docs/ARCHITECTURE.md#solution-dump), a JSON file that a wrapper program around the reference writes. The reference is postflop-solver, and its wrapper is `postflop-ref`, which is kept in a repository of its own because it links AGPL code (DECISIONS D18); build it there with `cargo build --release`. The format says what every number means; what a wrapper also needs to know:

- It is run as a command template with `{spot}`, the path of one of these spot files, and `{out}`, where to write the dump. `PK_REFERENCE_SOLVER` holds the template as a JSON array of strings, such as `["C:/tools/ref-wrapper.exe", "{spot}", "{out}"]`, and `cargo test -p pk-postflop --release -- --ignored the_configured_reference_solver_matches_ours` then compares it with this solver on every river spot. A failing run should exit with a non-zero code and say why on standard error.
- Its amounts are the street totals in the table above, in chips, with an all-in written as a bet or raise of the whole stack. The tree is checked before anything else, node by node, so a tree that differs anywhere is reported with the actions each side offers there.
- It lists every decision node, with a probability row for every hand of the acting player at each, and EVs counted from the start of the river, converted if the reference counts them otherwise. It writes no frequencies over a range: this solver sums every range itself, the same way for both sides.
- Both solvers are run to 0.03% of the pot (DECISIONS D20). Measured between two sound solves of these spots (`honest_solvers_table` in `crates/postflop/tests/compare.rs`), frequencies agreed within the gate's 2 points on every spot with both at 0.03% of the pot, and differed by up to 6.2 points with both at 0.1%.

A turn or flop spot's answer is a version 2 dump ([Solution dump](../docs/ARCHITECTURE.md#version-2-flop-and-turn-spots)), and differs only in this:

- Paths hold the cards dealt, each as `{"kind": "deal", "card": "7s"}` right after the action that closes a street, and every amount is the acting player's total for its street, from zero on each street: the all-in on the turn of a flop spot is what the player had left when the turn began. A node has a row for each hand of the acting player that holds no card dealt on its path, and none for a hand that holds one. There are no nodes after a card that cannot fall, because every pair of hands that can be held holds it.
- A dump may hold only the first street, or on a flop spot the first two, after every card, and says so with `"complete": false`. `postflop-ref` writes a flop spot's flop and turn by default, and every street of a turn spot; a complete flop dump with real ranges runs to a gigabyte. The comparison compares what both solutions hold. A partial dump's exploitability cannot be measured here, so its solver's own figure is shown as reported and held to the limit, and the text says it could not be checked; its EVs are checked against its strategy only where the strategy below the node is in the dump.
- `null` is the EV of a hand the reference has none for, such as one that never plays to the node itself, and is never read as zero.
- Below a deal, a node is checked when play reaches it more than 1% of the time given the cards dealt on its path, and its frequencies are held to `Tolerances::frequency_below_deals` rather than the first street's 2 points (DECISIONS P35, P36).
- `cargo test -p pk-postflop --release --test compare_streets -- --ignored the_configured_reference_solver_matches_ours_on_turn_and_flop_spots` compares the reference with this solver on every spot in `spots/turn/` and `spots/flop/`, each solved by both to 0.03% of the pot, and does nothing while those folders are missing or empty. `the_configured_reference_solver_on_the_test_spots` does the same on four small spots of `crates/postflop/tests/data`, which are not gate fixtures. Three of them have narrow made-up ranges, of 8 to 100 combos a side, on which a single hand moves a frequency below a deal by many points between two sound solvers, so it asserts every check but the frequencies below a deal, and prints those.
- To measure the reference's whole strategy on a flop spot, so that its exploitability is measured here rather than reported, give the wrapper `"--streets", "3"` in the template. The dump is complete, and large: 34 MB for `flop_small`, with 8 hands a side, and 230 MB with about a hundred.

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
- After OOP checks, IP wins the 10bb pot whether it checks or bets, so its play there is only bounded: it bets at most 2/3 of the time, or OOP's nuts would rather check. Two correct solvers can differ there: our solve bets about half the time, and postflop-solver never does. The comparison holds each to the bound at that node, in place of matching their frequencies.
- In the payoffs of `pk_postflop::payoff`, which count from the start of the river, each nut combo wins 10 + 5 × 2/3 = 40/3bb and each air combo 0. Over the 22 × 12 deals, which are all possible, OOP's value is 40/11 ≈ 3.64bb and IP's is 70/11 ≈ 6.36bb, adding up to the 10bb pot.

The frequencies are over each range as a whole: any split of the 2 bluffs among the air combos, or of the calls among IP's combos, is an equilibrium.

## Turn and flop spots

The files in `spots/turn/` and `spots/flop/` are the test spots of milestone M4. The gate's comparison (`the_configured_reference_solver_matches_ours_on_turn_and_flop_spots`, [above](#reading-a-references-answer)) runs the reference on each file and solves it with this solver, both to 0.03% of the pot; the reference's solutions are not stored. A test in `pk-postflop` (`tests/street_spots.rs`) loads every file, builds its betting tree and its game, checks that it writes back unchanged, walks every tree to check the rules [below](#reproducing-the-turn-and-flop-trees), checks that the set has what the table says it stresses, and checks both tables below against the files.

Every range here is illustrative, as on the river: written for these fixtures on 2026-11-24, to tell a plausible story about what reaches each spot, and not taken from any solver, chart, site or book; each spot's `description` says so and tells the story. Three flop spots hold the whole ranges of `ranges/a/BTN_vs_BB_SRP`, which are illustrative too, and `checked_flop_two_bets` gives OOP that file's BB range. Every spot is on a BTN vs BB line at 100bb, with BB out of position: the single-raised pot of `ranges/a/BTN_vs_BB_SRP`, 5.5bb with 97.5bb behind on the flop, or for `three_bet_low_spr` the 3-bet pot of `ranges/a/BB_vs_BTN_3BP`, 20.5bb with 90bb behind; a turn spot's pot and stack are those left by the flop its description gives. The ranges are written in hand classes (`AKs`, `77`, `KTo+`) and never name a single combo, so every symmetry of a board's suits keeps them.

| Spot | Stresses | Board | Pot | Stack | Tree | Rake | Suits alike | Betting nodes | Public nodes | OOP hands | IP hands |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `dry_rainbow` | The gate's plain 100bb single-raised-pot flop solve: the full ranges on a dry rainbow flop, with no flush draw or straight draw, and one bet a street | `Kd7c2h` | 5.5bb | 97.5bb | one bet, 0.5, on each street; no raises | none | none | 117 | 191,844 | 452 | 422 |
| `two_tone_raked` | A wet two-tone flop, with a flush draw and many straight draws, the full ranges, and a capped rake that binds in the larger pots only | `JhTh4c` | 5.5bb | 97.5bb | one bet, 0.5, on each street; no raises | 5%, cap 1bb | d, s | 117 | 191,844 | 438 | 422 |
| `monotone_pot` | A monotone flop, the full ranges, and pot-sized bets, which put 71.5bb of the 97.5bb in on the longest line | `Qs8s3s` | 5.5bb | 97.5bb | one bet, 1, on each street; no raises | none | c, d, h | 117 | 191,844 | 450 | 431 |
| `paired_raise` | A paired flop, narrower ranges, and a raise on the flop: the largest public tree, two thirds larger than one bet a street | `9s9h5d` | 5.5bb | 97.5bb | flop: one bet, 0.5; raises to 3×; turn and river: one bet, 0.5; no raises; one raise | 5%, cap 1bb | h, s | 195 | 319,740 | 127 | 126 |
| `checked_flop_two_bets` | A wide turn after the flop was checked through, with two bet sizes on both streets | `Th7h4c2c` | 5.5bb | 97.5bb | bets 0.5 and 1 on each street; no raises | none | d, s | 90 | 3,615 | 438 | 422 |
| `called_flop_raise_allin` | A turn after a called flop bet, with a raise and all-in, so that an all-in called on the turn deals the river with no decision, and a river bet of three quarters of the pot | `Qd9c8h5s` | 11bb | 94.75bb | turn: one bet, 0.5, or all-in; raises to 3×, or all-in; river: one bet, 0.75; no raises; one raise | 5%, cap 2bb | none | 76 | 2,379 | 358 | 412 |
| `three_bet_low_spr` | A 3-bet pot with a stack-to-pot ratio near 2, where all-in is the natural bet on both streets, on a paired turn | `Kc9c4h4d` | 41bb | 79.75bb | one bet, 0.5, or all-in on each street; facing a bet, all-in only; one raise | 5%, cap 6bb | d, h | 88 | 3,237 | 99 | 150 |

Pot and stack are at the start of the spot, and sizes are as on the river: bets are fractions of the pot, and raises go to a multiple of the bet faced. Suits alike are the suits that a symmetry of the board exchanges, the ones a later merging of isomorphic turn and river cards can merge: on `dry_rainbow` no two suits are alike, on `two_tone_raked` the two suits not on the board are, on `paired_raise` the suits of the pair, and on `monotone_pot` the three suits not on the board. Betting nodes counts the betting tree, in which a chance node has one child. Public nodes counts the tree the solver plays, with a child at each chance node for every card off the board; `PostflopGame::from_spot` refuses one of more than 1,000,000 nodes. A player's hands are as on the river: the combos of its range with a positive weight that miss the board.

Three spots are rake-free and four take 5%. The configs' cap of 6bb (DECISIONS D16) binds in `three_bet_low_spr` once the pot passes 120bb, but in no pot of the single-raised flop spots, the largest of which, 88bb on `paired_raise`, pays 4.4bb, and in `called_flop_raise_allin` only once an all-in is called. So the single-raised spots cap lower, where the cap binds in their larger pots and not in their smaller ones: 1bb on the flop, which binds from a 22bb pot, and 2bb on `called_flop_raise_allin`, which binds above 40bb. No pot is raked exactly at its cap.

### Reproducing the turn and flop trees

What the river section says [about the trees](#reproducing-the-trees) holds on every street. What a reference also needs to know:

- Each street has its own sizing under `[tree]`, the same for both players. OOP acts first on every street, and its first bet on a street is sized as any other: there are no separate sizes for a bet into the player who called the street before.
- A bet is a fraction of the whole pot when it is made, the chips put in on earlier streets included. A raise goes to a multiple of the opponent's total for the street. `max_raises` counts again from zero on each street.
- All-in is everything the player had left when the street began. After an all-in is called before the river, the remaining cards are dealt with no decisions.
- The rake is taken once, when the hand ends, from the pot both players matched over every street.
- Every card that can fall is dealt, each with a subtree of its own. A reference that merges the cards a symmetry of the suits makes alike writes each card as itself in its dump, as `postflop-ref` does.
- The amounts are whole because the pots are. Every pot of a single-raised spot is 550 chips times a whole number: a half-pot bet called doubles the pot, a pot-sized bet called triples it, and a raise to 3× of a half-pot bet, called, quadruples it. The three-quarter-pot river bet of `called_flop_raise_allin` is made only into pots of 1,100, 2,200 and 4,400 chips, and `three_bet_low_spr`'s pots are 4,100 and 8,200 chips.
- No size reaches the stack on any street, so no size is turned into all-in and no two sizes land on one amount: every size in `[tree]` is an action of its own at every node where it can be taken, and fold and call stand alone only facing an all-in or after the last raise allowed. The test checks this node by node.

The amounts in each tree, as the street total each bet or raise goes to, street by street:

| Spot | Street | Bets | Raises to | All-in |
| --- | --- | --- | --- | --- |
| `dry_rainbow` | flop | 2.75bb | none | none |
| | turn | 2.75, 5.5bb | none | none |
| | river | 2.75, 5.5, 11bb | none | none |
| `two_tone_raked` | flop | 2.75bb | none | none |
| | turn | 2.75, 5.5bb | none | none |
| | river | 2.75, 5.5, 11bb | none | none |
| `monotone_pot` | flop | 5.5bb | none | none |
| | turn | 5.5, 16.5bb | none | none |
| | river | 5.5, 16.5, 49.5bb | none | none |
| `paired_raise` | flop | 2.75bb | 8.25bb | none |
| | turn | 2.75, 5.5, 11bb | none | none |
| | river | 2.75, 5.5, 11, 22bb | none | none |
| `checked_flop_two_bets` | turn | 2.75, 5.5bb | none | none |
| | river | 2.75, 5.5, 8.25, 11, 16.5bb | none | none |
| `called_flop_raise_allin` | turn | 5.5bb | 16.5bb | 94.75bb |
| | river | 8.25, 16.5, 33bb | none | none |
| `three_bet_low_spr` | turn | 20.5bb | none | 79.75bb |
| | river | 20.5, 41bb | none | 59.25, 79.75bb |
