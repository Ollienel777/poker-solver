//! The merged game of `pk_postflop::merged` against the full game it merges, on small spots of
//! `tests/data` written for these tests: a monotone flop, a two-tone flop, a paired flop, an
//! unpaired rainbow flop, a turn of four clubs, and a monotone flop whose ranges keep only two of
//! its three alike suits alike.
//!
//! Every check here compares with the full game's solver, which knows nothing of the merging, or
//! with the full game's measurements: the classes are held to what the board and the ranges
//! allow, the stored values after a few iterations to the full solver's, node by node and hand by
//! hand, and the solves to the full game's exploitability and values.

use pk_cfr::{Dcfr, PublicGame, PublicNode, PublicProfile, Schedule, VectorCfr};
use pk_core::{Card, CardSet, SuitPermutation};
use pk_game::Player;
use pk_postflop::{MergedGame, PostflopGame, Solution, SolveOptions, Spot, exploitability, solve};

fn load(name: &str) -> Spot {
    let path = format!("{}/tests/data/{name}.toml", env!("CARGO_MANIFEST_DIR"));
    Spot::load(path).unwrap()
}

fn card(text: &str) -> Card {
    text.parse().unwrap()
}

fn cards(text: &str) -> CardSet {
    text.parse().unwrap()
}

/// The spots with a symmetry the merging uses.
const SYMMETRIC: [&str; 5] = [
    "flop_monotone",
    "flop_two_tone",
    "flop_paired",
    "turn_four_flush",
    "flop_broken",
];

fn dcfr() -> Schedule {
    Schedule::Discounted(Dcfr::default())
}

/// For each hand of `player`, the hand `σ(h)`: the hand list's position of the combo's image,
/// found here from the combos, apart from the merged game's own tables.
fn relabelled_hands(game: &PostflopGame, player: Player, sigma: SuitPermutation) -> Vec<usize> {
    let list = game.hands(player);
    list.combos()
        .iter()
        .map(|&combo| {
            list.position(sigma.apply_combo(combo))
                .unwrap_or_else(|| panic!("{sigma} does not keep the hand list"))
        })
        .collect()
}

/// The largest difference between a stored value of the full solver and the merged solver's
/// at the counterpart of its node, hand `h` read at `τ(h)`, as a share of the largest value of
/// the full solver's slice, over the regrets and the strategy sums; and the nodes compared.
fn largest_gap(full: &VectorCfr<PostflopGame>, merged: &VectorCfr<MergedGame<'_>>) -> (f64, usize) {
    let (full_game, merged_game) = (full.game(), merged.game());
    let mut largest = 0.0f64;
    let mut compared = 0;
    for node in 0..full_game.num_nodes() {
        let PublicNode::Player { player, .. } = full_game.node(node) else {
            continue;
        };
        let (counterpart, tau) = merged_game.counterpart(node as u32);
        let relabelled = relabelled_hands(full_game, [Player::Oop, Player::Ip][player], tau);
        let num_hands = relabelled.len();
        let pairs = [
            (full.regrets(node), merged.regrets(counterpart as usize)),
            (
                full.strategy_sum(node),
                merged.strategy_sum(counterpart as usize),
            ),
        ];
        for (ours, theirs) in pairs {
            let (ours, theirs) = (ours.unwrap(), theirs.unwrap());
            assert_eq!(ours.len(), theirs.len(), "node {node}");
            let scale = ours.iter().fold(0.0f32, |most, v| most.max(v.abs()));
            for (index, &value) in ours.iter().enumerate() {
                let (action, hand) = (index / num_hands, index % num_hands);
                let other = theirs[action * num_hands + relabelled[hand]];
                let gap = f64::from((value - other).abs());
                if scale > 0.0 {
                    largest = largest.max(gap / f64::from(scale));
                } else {
                    assert_eq!(gap, 0.0, "node {node}: a slice of zeros against {other}");
                }
            }
        }
        compared += 1;
    }
    (largest, compared)
}

#[test]
fn every_node_of_the_full_game_has_a_counterpart_on_its_board_relabelled() {
    // The counterpart of a full node is a merged node on the same betting node, whose board is
    // the full node's relabelled by τ, which keeps the deal; and every merged node is the
    // counterpart of the full node on its own path, with τ the identity.
    for name in SYMMETRIC {
        let spot = load(name);
        let full = PostflopGame::from_spot(&spot).unwrap();
        let merged = MergedGame::new(&full).unwrap();
        let group = merged.group();
        let mut own = vec![false; merged.num_nodes()];
        for node in 0..full.num_nodes() as u32 {
            let (counterpart, tau) = merged.counterpart(node);
            assert!(group.contains(tau), "{name}, node {node}");
            assert_eq!(
                merged.tree_node(counterpart),
                full.tree_node(node),
                "{name}"
            );
            assert_eq!(
                tau.apply_set(full.board(node)),
                merged.board(counterpart),
                "{name}, node {node}"
            );
            if tau == SuitPermutation::IDENTITY {
                own[counterpart as usize] |= full.board(node) == merged.board(counterpart);
            }
            for player in [Player::Oop, Player::Ip] {
                // τ keeps each player's hands and their weights.
                let list = full.hands(player);
                for (hand, &image) in relabelled_hands(&full, player, tau).iter().enumerate() {
                    assert_eq!(list.weights()[hand], list.weights()[image], "{name}");
                }
            }
        }
        assert!(
            own.iter().all(|&own| own),
            "{name}: a merged node is no full node's own"
        );
        assert!(merged.num_nodes() < full.num_nodes(), "{name}");
        println!(
            "{name}: {} of the full game's {} nodes, under {group}",
            merged.num_nodes(),
            full.num_nodes()
        );
    }
}

/// The classes at the merged chance node after `path`, the full game's node of that path, as
/// card sets in the order of their representatives, each written as a card set.
fn classes_after(merged: &MergedGame<'_>, path: &str) -> Vec<String> {
    let node = merged.full().find(path).unwrap();
    let (counterpart, tau) = merged.counterpart(node);
    assert_eq!(
        tau,
        SuitPermutation::IDENTITY,
        "`{path}` is a path of representatives"
    );
    let outcomes = merged.outcomes(counterpart);
    outcomes
        .iter()
        .enumerate()
        .map(|(outcome, &representative)| {
            let mut class = CardSet::single(representative);
            for (image, sigma) in merged.outcome_images(counterpart, outcome) {
                assert_eq!(sigma.apply_card(image), representative, "{path}");
                class = class | CardSet::single(image);
            }
            class.to_string()
        })
        .collect()
}

/// The class of `card` among `classes`.
fn class_of<'c>(classes: &'c [String], card: &str) -> &'c str {
    let card = self::card(card);
    classes
        .iter()
        .find(|class| cards(class).contains(card))
        .unwrap_or_else(|| panic!("{card} is in no class"))
}

#[test]
fn the_cards_merged_are_those_a_symmetry_of_the_board_and_the_ranges_makes_alike() {
    // Monotone Ks 9s 4s: the three other suits are alike, so each rank's club, diamond and heart
    // are one class, and each spade alone: 23 turn classes. After a heart turn only clubs and
    // diamonds stay alike; after a spade turn all three do.
    let spot = load("flop_monotone");
    let full = PostflopGame::from_spot(&spot).unwrap();
    let merged = MergedGame::new(&full).unwrap();
    assert_eq!(merged.group().order(), 6);
    let turns = classes_after(&merged, "x x");
    assert_eq!(turns.len(), 23);
    assert_eq!(class_of(&turns, "5h"), "5h5d5c");
    assert_eq!(class_of(&turns, "As"), "As");
    // The turn 5c is the representative; after it only diamonds and hearts may swap.
    let rivers = classes_after(&merged, "x x 5c x x");
    assert_eq!(rivers.len(), 10 + 12 + 13);
    assert_eq!(class_of(&rivers, "7d"), "7h7d");
    assert_eq!(class_of(&rivers, "7c"), "7c");
    let rivers = classes_after(&merged, "x x As x x");
    assert_eq!(class_of(&rivers, "7d"), "7h7d7c");

    // Two-tone Qh 8h 3c: diamonds and spades, the suits off the board, are alike.
    let spot = load("flop_two_tone");
    let full = PostflopGame::from_spot(&spot).unwrap();
    let merged = MergedGame::new(&full).unwrap();
    let turns = classes_after(&merged, "x x");
    assert_eq!(turns.len(), 36);
    assert_eq!(class_of(&turns, "Ks"), "KsKd");
    assert_eq!(class_of(&turns, "Kh"), "Kh");
    // After the turn Kd, the representative, nothing is alike any more.
    assert_eq!(classes_after(&merged, "x x Kd x x").len(), 48);
    assert_eq!(classes_after(&merged, "x x Kh x x").len(), 35);

    // Paired 9s 9h 5d: the suits of the pair are alike.
    let spot = load("flop_paired");
    let full = PostflopGame::from_spot(&spot).unwrap();
    let merged = MergedGame::new(&full).unwrap();
    let turns = classes_after(&merged, "x x");
    assert_eq!(turns.len(), 37);
    assert_eq!(class_of(&turns, "Ah"), "AsAh");
    assert_eq!(class_of(&turns, "9c"), "9c");
    assert_eq!(class_of(&turns, "Ad"), "Ad");

    // A turn of four clubs: the other three suits are alike, 22 river classes.
    let spot = load("turn_four_flush");
    let full = PostflopGame::from_spot(&spot).unwrap();
    let merged = MergedGame::new(&full).unwrap();
    let rivers = classes_after(&merged, "x x");
    assert_eq!(rivers.len(), 22);
    assert_eq!(class_of(&rivers, "Ah"), "AsAhAd");
    assert_eq!(class_of(&rivers, "Ac"), "Ac");
}

#[test]
fn a_symmetry_of_the_board_that_the_ranges_break_merges_nothing() {
    // flop_broken is the monotone flop with OOP holding AcQc and AdQd but not AhQh. The board
    // alone makes the heart, diamond and club of each rank alike, but only clubs and diamonds
    // keep the ranges, so the hearts are kept apart: 5h is a class of its own, and 5c and 5d
    // one class.
    let spot = load("flop_broken");
    let board = pk_core::DealtBoard::new(spot.board).unwrap();
    assert_eq!(class_of_board(board, "5h"), "5h5d5c");
    let full = PostflopGame::from_spot(&spot).unwrap();
    let merged = MergedGame::new(&full).unwrap();
    assert_eq!(merged.group().to_string(), "{(), (c d)}");
    let turns = classes_after(&merged, "x x");
    assert_eq!(class_of(&turns, "5h"), "5h");
    assert_eq!(class_of(&turns, "5d"), "5d5c");
    assert_eq!(class_of(&turns, "Ah"), "Ah");
    assert_eq!(turns.len(), 10 + 13 + 13);
    let rivers = classes_after(&merged, "x x 5h x x");
    assert_eq!(class_of(&rivers, "7d"), "7d7c");
    assert_eq!(class_of(&rivers, "7h"), "7h");
}

/// The class of `card` among the cards that can follow `board`, under the board's symmetries
/// alone.
fn class_of_board(board: pk_core::DealtBoard, card: &str) -> String {
    let next = board.next_cards();
    next.class_of(self::card(card))
        .unwrap()
        .members()
        .to_string()
}

#[test]
fn a_spot_with_no_symmetry_to_use_has_no_merged_game() {
    // An unpaired rainbow flop, a turn of four suits, ranges that name single combos, and a
    // river spot, which has no chance node.
    for spot in [
        load("flop_rainbow"),
        load("turn_wide"),
        load("turn_small"),
        load("flop_small"),
        Spot::load(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../reference/spots/river/four_flush.toml"
        ))
        .unwrap(),
    ] {
        let full = PostflopGame::from_spot(&spot).unwrap();
        assert!(MergedGame::new(&full).is_none(), "{}", spot.name);
    }
}

#[test]
fn after_one_two_and_three_iterations_the_merged_solver_stores_what_the_full_one_does() {
    // From zero, under every schedule: each stored regret and strategy sum of the full solver,
    // at every player node, equals the merged solver's at the node's counterpart with its hands
    // relabelled, within a few f32 roundings of the largest value in its slice. At a node on a
    // path of representatives that is the merged solver's own value; below an image it is the
    // representative's, relabelled.
    let schedules = [
        Schedule::Vanilla,
        Schedule::CfrPlus,
        Schedule::Linear,
        dcfr(),
    ];
    let mut worst = 0.0f64;
    for name in SYMMETRIC {
        let spot = load(name);
        let full_game = PostflopGame::from_spot(&spot).unwrap();
        for schedule in schedules {
            let mut full = VectorCfr::new(full_game.clone(), schedule);
            let mut merged = VectorCfr::new(MergedGame::new(&full_game).unwrap(), schedule);
            for t in 1..=3 {
                full.iterate();
                merged.iterate();
                let (gap, compared) = largest_gap(&full, &merged);
                println!("{name}, {schedule:?}, iteration {t}: {gap:.3e} over {compared} nodes");
                worst = worst.max(gap);
                assert!(
                    gap <= ONE_ITERATION,
                    "{name}, {schedule:?}, iteration {t}: {gap:e}"
                );
            }
        }
    }
    println!("largest gap after 1 to 3 iterations: {worst:e}");
}

/// How far a stored value of the merged solver may be from the full solver's after one to three
/// iterations from zero, as a share of the largest value in its slice: four `f32` roundings of
/// that value. Measured, at most 2.5e-16, at values near zero in a slice; the `f64` values from
/// which the two solvers round differ in their last bits, which rarely moves an `f32`.
const ONE_ITERATION: f64 = 2.0 * f32::EPSILON as f64;

/// A solve of `spot` capped at `iterations` and measured at the end only, merged or not.
fn capped(spot: &Spot, iterations: u64, merge: bool) -> Solution {
    let options = SolveOptions {
        target: None,
        max_iterations: iterations,
        check_every: iterations.max(1),
        merge_isomorphic: merge,
        ..SolveOptions::default()
    };
    solve(spot, &options, |_| {}).unwrap()
}

/// How far two solutions are apart, each as an absolute difference.
#[derive(Debug, Default, Clone, Copy)]
struct Apart {
    /// The exploitabilities, in percent of the pot.
    exploitability: f64,
    /// The larger of the two players' values, in percent of the pot.
    values: f64,
    /// The largest difference in an action's frequency, as a range view shows it, at a node
    /// that either reaches more than 1% of the time given the cards dealt on its path.
    frequency: f64,
    /// The largest difference in one hand's probability of an action at such a node, among the
    /// hands that hold 1% of the range there or more.
    hand: f64,
    /// The largest difference in one hand's probability of an action at any node.
    anywhere: f64,
}

/// How often play reaches `node` given the cards dealt on its path: its reach over the
/// probability that those cards fall.
fn reach_given_cards(solution: &Solution, node: u32) -> f64 {
    let game = solution.game();
    let cards: f64 = game
        .path(node)
        .into_iter()
        .filter(|&(at, _)| matches!(game.node(at as usize), PublicNode::Chance { .. }))
        .map(|(at, _)| game.outcome_probability(at as usize, 0))
        .product();
    solution.reach_probability(node) / cards
}

/// How far `b` is from `a`, where `map(n)` gives the node of `b` that node `n` of `a` is, and
/// the relabelling `σ` that takes a hand's combo at `n` to its combo there.
fn apart(a: &Solution, b: &Solution, map: impl Fn(u32) -> (u32, SuitPermutation)) -> Apart {
    let pot = f64::from(a.game().pot());
    let mut gap = Apart {
        exploitability: 100.0 * (a.exploitability().chips - b.exploitability().chips).abs() / pot,
        values: (0..2)
            .map(|p| 100.0 * (a.values()[p] - b.values()[p]).abs() / pot)
            .fold(0.0, f64::max),
        ..Apart::default()
    };
    let game = a.game();
    for node in 0..game.num_nodes() as u32 {
        let Some(actor) = game.actor(node) else {
            continue;
        };
        let (other, sigma) = map(node);
        let (ours, theirs) = (game.hands(actor), b.game().hands(actor));
        let hand_of = |hand: usize| {
            theirs
                .position(sigma.apply_combo(ours.combos()[hand]))
                .expect("a relabelling of the deal keeps the hand list")
        };
        let gap_at = |hand: usize| -> f64 {
            let p = a.hand_strategy(node, hand).unwrap();
            let q = b.hand_strategy(other, hand_of(hand)).unwrap();
            p.iter()
                .zip(&q)
                .map(|(p, q)| f64::from((p - q).abs()))
                .fold(0.0, f64::max)
        };
        for hand in 0..ours.len() {
            gap.anywhere = gap.anywhere.max(gap_at(hand));
        }
        let checked = reach_given_cards(a, node) > 0.01 || reach_given_cards(b, other) > 0.01;
        let (Some(frequencies), Some(other_frequencies)) = (
            a.range_view_frequencies(node),
            b.range_view_frequencies(other),
        ) else {
            continue;
        };
        if !checked {
            continue;
        }
        for (p, q) in frequencies.iter().zip(&other_frequencies) {
            gap.frequency = gap.frequency.max((p - q).abs());
        }
        let weights = a.range_view_weights(node).unwrap();
        for hand in (0..ours.len()).filter(|&hand| weights[hand] >= 0.01) {
            gap.hand = gap.hand.max(gap_at(hand));
        }
    }
    gap
}

/// How far the merged solve of `spot` is from the solve with merging off, both capped at
/// `iterations`.
fn merged_apart(spot: &Spot, iterations: u64) -> (Apart, Solution, Solution) {
    let off = capped(spot, iterations, false);
    let on = capped(spot, iterations, true);
    let gap = apart(&off, &on, |node| (node, SuitPermutation::IDENTITY));
    (gap, off, on)
}

#[test]
#[ignore = "slow"]
fn how_far_merged_and_full_solves_drift_apart() {
    // Measures, for each symmetric spot and a range of iteration counts, how far the merged
    // solve is from the solve with merging off: the exploitability and the values, in percent of
    // the pot, and the strategies, as range-view frequencies at the nodes reached more than 1% of
    // the time given their cards, as single hands' probabilities there, and as single hands'
    // probabilities anywhere. The tolerances of the fast tests come from this table.
    // The last three columns are the solve with merging off against itself, each node against
    // the node the merging maps it to with the hands relabelled, which needs no merged solve.
    println!(
        "spot, iterations: exploitability off and on (% pot); merged against full: \
         exploitability, values (% pot), frequency, hand, anywhere; full against itself: \
         frequency, hand, anywhere"
    );
    for name in SYMMETRIC {
        let spot = load(name);
        for iterations in [1, 3, 10, 30, 100, 300, 1000, 3000] {
            let (gap, off, on) = merged_apart(&spot, iterations);
            let merged = MergedGame::new(off.game()).unwrap();
            let own = own_full_nodes(&merged);
            let itself = apart(&off, &off, |node| {
                let (counterpart, tau) = merged.counterpart(node);
                (own[counterpart as usize], tau)
            });
            println!(
                "{name}, {iterations}: {:.5} {:.5}; {:.2e} {:.2e} {:.2e} {:.2e} {:.2e}; \
                 {:.2e} {:.2e} {:.2e}",
                off.exploitability().percent_of_pot,
                on.exploitability().percent_of_pot,
                gap.exploitability,
                gap.values,
                gap.frequency,
                gap.hand,
                gap.anywhere,
                itself.frequency,
                itself.hand,
                itself.anywhere
            );
        }
    }
}

/// For each node of the merged game, the full node on its own path of representatives, whose
/// counterpart it is with `τ` the identity.
fn own_full_nodes(merged: &MergedGame<'_>) -> Vec<u32> {
    let full = merged.full();
    let mut own = vec![u32::MAX; merged.num_nodes()];
    for node in 0..full.num_nodes() as u32 {
        let (counterpart, tau) = merged.counterpart(node);
        if tau == SuitPermutation::IDENTITY {
            assert_eq!(
                own[counterpart as usize],
                u32::MAX,
                "two full nodes are one's own"
            );
            own[counterpart as usize] = node;
        }
    }
    assert!(own.iter().all(|&node| node != u32::MAX));
    own
}

/// The largest difference between the full solver's stored values at a node and at the node
/// the merging maps it to, the full node of its counterpart's own path, hand `h` read at
/// `τ(h)`, as a share of the slice's largest: how far the full solver is from symmetric.
fn full_asymmetry(full: &VectorCfr<PostflopGame>, merged: &MergedGame<'_>, own: &[u32]) -> f64 {
    let game = full.game();
    let mut largest = 0.0f64;
    for node in 0..game.num_nodes() {
        let PublicNode::Player { player, .. } = game.node(node) else {
            continue;
        };
        let (counterpart, tau) = merged.counterpart(node as u32);
        let mirror = own[counterpart as usize] as usize;
        let relabelled = relabelled_hands(game, [Player::Oop, Player::Ip][player], tau);
        let num_hands = relabelled.len();
        for (ours, theirs) in [
            (full.regrets(node), full.regrets(mirror)),
            (full.strategy_sum(node), full.strategy_sum(mirror)),
        ] {
            let (ours, theirs) = (ours.unwrap(), theirs.unwrap());
            let scale = ours.iter().fold(0.0f32, |most, v| most.max(v.abs()));
            for (index, &value) in ours.iter().enumerate() {
                let (action, hand) = (index / num_hands, index % num_hands);
                let gap = f64::from((value - theirs[action * num_hands + relabelled[hand]]).abs());
                if scale > 0.0 {
                    largest = largest.max(gap / f64::from(scale));
                }
            }
        }
    }
    largest
}

#[test]
#[ignore = "slow"]
fn where_merged_and_full_solvers_part() {
    // Iteration by iteration under DCFR: how far the merged solver's stored values are from the
    // full solver's (as the fast test measures them), and how far the full solver's are from
    // themselves under the relabellings the merging uses, which needs no merged game at all.
    for name in SYMMETRIC {
        let spot = load(name);
        let game = PostflopGame::from_spot(&spot).unwrap();
        let merged_game = MergedGame::new(&game).unwrap();
        let own = own_full_nodes(&merged_game);
        let mut full = VectorCfr::new(game.clone(), dcfr());
        let mut merged = VectorCfr::new(MergedGame::new(&game).unwrap(), dcfr());
        for t in 1..=12 {
            full.iterate();
            merged.iterate();
            let (gap, _) = largest_gap(&full, &merged);
            let asymmetry = full_asymmetry(&full, &merged_game, &own);
            println!(
                "{name}, iteration {t}: merged against full {gap:.3e}, \
                 full against itself {asymmetry:.3e}"
            );
        }
    }
}

/// `spot` with its suits relabelled by `sigma`: the board and both ranges.
fn relabelled(spot: &Spot, sigma: SuitPermutation) -> Spot {
    let mut image = spot.clone();
    image.name = format!("{} relabelled by {sigma}", spot.name);
    image.board = sigma.apply_set(spot.board);
    image.oop_range = sigma.apply_range(&spot.oop_range);
    image.ip_range = sigma.apply_range(&spot.ip_range);
    image
}

/// How far the solve of `spot` relabelled by `sigma` is from the solve of `spot`, both capped
/// at `iterations`, merged or not: each node against the node of the relabelled path, each hand
/// against its relabelled combo.
fn relabelled_apart(spot: &Spot, sigma: SuitPermutation, iterations: u64, merge: bool) -> Apart {
    let image = relabelled(spot, sigma);
    let (a, b) = (
        capped(spot, iterations, merge),
        capped(&image, iterations, merge),
    );
    let (game, other) = (a.game(), b.game());
    apart(&a, &b, |node| {
        let path: Vec<String> = game
            .node_path(node)
            .split_whitespace()
            .map(|token| match token.parse::<Card>() {
                Ok(card) => sigma.apply_card(card).to_string(),
                Err(_) => token.to_string(),
            })
            .collect();
        (other.find(&path.join(" ")).unwrap(), sigma)
    })
}

/// The relabellings each spot is checked under, none of them a symmetry of its deal, so that
/// the relabelled spot is on another board, with other representatives.
fn relabellings(name: &str) -> SuitPermutation {
    match name {
        // Ks 9s 4s to Kc 9c 4c: the alike suits go from c, d, h to d, h, s.
        "flop_monotone" | "flop_broken" => SuitPermutation::swap(0, 3),
        // Qh 8h 3c to Qh 8h 3s: from d, s to c, d.
        "flop_two_tone" => SuitPermutation::swap(0, 3),
        // 9s 9h 5d to 9c 9h 5s, and so on.
        _ => SuitPermutation::new([3, 2, 1, 0]),
    }
}

/// Checks that `name` relabelled solves to `name`: after 3 iterations within
/// RELABELLED_EARLY, and, if `late`, after 300 iterations within MERGED_LATE in the values and
/// the exploitability.
fn check_relabelled(name: &str, late: bool) {
    let spot = load(name);
    let sigma = relabellings(name);
    let early = relabelled_apart(&spot, sigma, 3, true);
    println!("{name} relabelled by {sigma}, 3 iterations: {early:?}");
    assert!(early.anywhere <= RELABELLED_EARLY, "{name}: {early:?}");
    assert!(early.values <= RELABELLED_EARLY, "{name}: {early:?}");
    assert!(
        early.exploitability <= RELABELLED_EARLY,
        "{name}: {early:?}"
    );
    if late {
        let late = relabelled_apart(&spot, sigma, 300, true);
        println!("{name} relabelled by {sigma}, 300 iterations: {late:?}");
        assert!(late.values <= MERGED_LATE, "{name}: {late:?}");
        assert!(late.exploitability <= MERGED_LATE, "{name}: {late:?}");
    }
}

#[test]
fn a_spot_with_its_suits_relabelled_solves_to_the_spot_relabelled() {
    // The gate's "suit-isomorphic boards give identical results". After 3 iterations every
    // strategy, value and exploitability of the relabelled spot is the spot's, node for node and
    // hand for hand, within RELABELLED_EARLY, on every spot, merged, and on the rainbow flop,
    // which nothing merges. After 300, the values and the exploitability are within MERGED_LATE
    // of the pot, as for a merged solve against a full one, here on the two-tone flop and the
    // rainbow flop, and in `every_spot_relabelled_solves_to_the_spot_relabelled` on all.
    for name in SYMMETRIC.iter().chain(&["flop_rainbow"]) {
        let late = matches!(*name, "flop_two_tone" | "flop_rainbow");
        check_relabelled(name, late);
    }
}

#[test]
#[ignore = "slow"]
fn every_spot_relabelled_solves_to_the_spot_relabelled() {
    for name in SYMMETRIC.iter().chain(&["flop_rainbow"]) {
        check_relabelled(name, true);
    }
}

/// How far a solve after 3 iterations may be from the solve of its spot relabelled, or from
/// the solve with merging off: in a probability, and in percent of the pot. Measured, 0.
const RELABELLED_EARLY: f64 = 1e-9;

/// How far the exploitability and the values of a solve after 300 iterations may be from
/// those of the solve with merging off, or of the spot relabelled, in percent of the pot.
const MERGED_LATE: f64 = 0.01;

/// Checks the merged solve of `name` against the solve with merging off: after 3 iterations
/// within RELABELLED_EARLY, and after 300 within MERGED_LATE in the exploitability and the values,
/// with both within 0.05% of the pot of an equilibrium.
fn check_merged_against_full(name: &str) {
    let spot = load(name);
    let early = merged_apart(&spot, 3).0;
    assert!(early.anywhere <= RELABELLED_EARLY, "{name}: {early:?}");
    let (late, off, on) = merged_apart(&spot, 300);
    println!(
        "{name}: {:.5}% and {:.5}% of the pot exploitable; {late:?}",
        off.exploitability().percent_of_pot,
        on.exploitability().percent_of_pot
    );
    assert!(late.exploitability <= MERGED_LATE, "{name}: {late:?}");
    assert!(late.values <= MERGED_LATE, "{name}: {late:?}");
    for solution in [&off, &on] {
        assert!(solution.exploitability().percent_of_pot <= 0.05, "{name}");
    }
}

#[test]
fn merged_solves_reach_the_exploitability_and_values_of_full_ones() {
    // After 300 iterations, merged and full solves, both measured on the full tree, are within
    // MERGED_LATE of the pot in exploitability and in values, and both are within 0.05% of the
    // pot of an equilibrium (measured, at most 0.022%), here on the two-tone flop and the turn,
    // and in `on_every_symmetric_spot_merged_solves_reach_what_full_ones_do` on all five. Their
    // strategies are not compared: at a hand indifferent between its actions, a last-bit
    // difference moves the probabilities by up to 0.8 in these solves, as much as the full solve
    // moves between subtrees that a relabelling maps onto each other (see
    // `how_far_merged_and_full_solves_drift_apart`).
    for name in ["flop_two_tone", "turn_four_flush"] {
        check_merged_against_full(name);
    }
}

#[test]
#[ignore = "slow"]
fn on_every_symmetric_spot_merged_solves_reach_what_full_ones_do() {
    for name in SYMMETRIC {
        check_merged_against_full(name);
    }
}

/// The bits of every probability of a profile, node by node.
fn profile_bits(profile: &PublicProfile) -> Vec<(usize, Vec<u32>)> {
    let nodes = profile.nodes();
    nodes
        .map(|node| {
            let bits = profile
                .get(node)
                .unwrap()
                .iter()
                .map(|p| p.to_bits())
                .collect();
            (node, bits)
        })
        .collect()
}

#[test]
fn with_merging_off_or_nothing_to_merge_a_solve_is_the_full_solvers_bit_for_bit() {
    // A symmetric spot with merging off, and the rainbow flop, which nothing merges, with it on:
    // the solution's strategy is the full game's solver's average after as many iterations, and
    // its exploitability that strategy's on the full game, bit for bit. Merged, the symmetric
    // spot's is not: it agrees only within rounding.
    for (name, merge) in [
        ("flop_monotone", false),
        ("flop_rainbow", true),
        ("turn_four_flush", false),
    ] {
        let spot = load(name);
        let solution = capped(&spot, 40, merge);
        let game = PostflopGame::from_spot(&spot).unwrap();
        let mut solver = VectorCfr::new(game, dcfr());
        solver.run(40, |_| {});
        let average = solver.average_strategy();
        assert!(
            profile_bits(solution.strategy()) == profile_bits(&average),
            "{name}"
        );
        let measured = exploitability(solver.game(), &average);
        assert_eq!(
            solution.exploitability().chips.to_bits(),
            measured.chips.to_bits(),
            "{name}"
        );
    }
    let merged = capped(&load("flop_monotone"), 40, true);
    let off = capped(&load("flop_monotone"), 40, false);
    assert!(profile_bits(merged.strategy()) != profile_bits(off.strategy()));
}

#[test]
fn a_merged_average_strategy_is_the_merged_solvers_own_on_paths_of_representatives() {
    // The expansion copies each counterpart's average, relabelled: on a path of
    // representatives the merged solver's own average, bit for bit, and everywhere a
    // distribution for each hand.
    let spot = load("flop_monotone");
    let game = PostflopGame::from_spot(&spot).unwrap();
    let mut solver = VectorCfr::new(MergedGame::new(&game).unwrap(), dcfr());
    solver.run(20, |_| {});
    let own = solver.average_strategy();
    let expanded = MergedGame::average_strategy(&solver);
    let player_nodes = (0..game.num_nodes())
        .filter(|&node| matches!(game.node(node), PublicNode::Player { .. }))
        .count();
    assert_eq!(expanded.nodes().count(), player_nodes);
    let merged = solver.game();
    let mut on_paths = 0;
    for node in expanded.nodes() {
        let (counterpart, tau) = merged.counterpart(node as u32);
        let PublicNode::Player { player, .. } = game.node(node) else {
            unreachable!("a profile has strategies at player nodes only");
        };
        let relabelled = relabelled_hands(&game, [Player::Oop, Player::Ip][player], tau);
        let theirs = own.get(counterpart as usize).unwrap();
        let ours = expanded.get(node).unwrap();
        let num_hands = relabelled.len();
        for (index, &p) in ours.iter().enumerate() {
            let (action, hand) = (index / num_hands, index % num_hands);
            assert_eq!(
                p.to_bits(),
                theirs[action * num_hands + relabelled[hand]].to_bits()
            );
        }
        on_paths += usize::from(tau == SuitPermutation::IDENTITY);
    }
    assert_eq!(on_paths, solver.average_strategy().nodes().count());
}

/// The stored values of the merged solver that are not the same bit for bit for hand `h` and
/// hand `σ(h)` at a merged player node, for `σ` in the node's stabilizer.
fn unlike_values(solver: &VectorCfr<MergedGame<'_>>) -> usize {
    let merged = solver.game();
    // Each member's relabelling of each player's hands, made once.
    let relabellings: Vec<(SuitPermutation, [Vec<usize>; 2])> = merged
        .group()
        .iter()
        .map(|sigma| {
            let hands =
                [Player::Oop, Player::Ip].map(|p| relabelled_hands(merged.full(), p, sigma));
            (sigma, hands)
        })
        .collect();
    let mut unlike = 0;
    for node in 0..merged.num_nodes() {
        let PublicNode::Player { player, .. } = merged.node(node) else {
            continue;
        };
        let stabilizer = merged.stabilizer(node as u32);
        for (_, hands) in relabellings.iter().filter(|(s, _)| stabilizer.contains(*s)) {
            let relabelled = &hands[player];
            let num_hands = relabelled.len();
            for values in [solver.regrets(node), solver.strategy_sum(node)] {
                let values = values.unwrap();
                for (index, &value) in values.iter().enumerate() {
                    let (action, hand) = (index / num_hands, index % num_hands);
                    let other = values[action * num_hands + relabelled[hand]];
                    unlike += usize::from(value.to_bits() != other.to_bits());
                }
            }
        }
    }
    unlike
}

/// Every stored value of a solver, as bits, node by node.
fn stored_bits<G: PublicGame>(solver: &VectorCfr<G>) -> Vec<(Vec<u32>, Vec<u32>)> {
    let bits = |values: &[f32]| values.iter().map(|v| v.to_bits()).collect();
    (0..solver.game().num_nodes())
        .filter_map(|node| {
            Some((
                bits(solver.regrets(node)?),
                bits(solver.strategy_sum(node)?),
            ))
        })
        .collect()
}

#[test]
fn the_merged_solver_keeps_the_hands_a_node_makes_alike_alike_bit_for_bit() {
    // The members of Γ that keep a merged node map its subtree onto itself, so hand h and hand
    // σ(h) must store the same regrets and strategy sums there: otherwise the reach entering a
    // chance node below stops being symmetric, and an image is read against a reach that is not
    // its own. Left to itself, the merged solver of `flop_monotone_wide` rounds two such hands
    // apart by iteration 8 under every schedule (from a last-bit difference in a terminal's or a
    // chance node's sum, which regret matching turns into a different strategy), and the narrow
    // spots never. The walk gives alike hands their class's first hand's value at terminals and
    // chance nodes, so they stay alike bit for bit: under every schedule, on one thread and on
    // four.
    let schedules = [
        Schedule::Vanilla,
        Schedule::CfrPlus,
        Schedule::Linear,
        dcfr(),
    ];
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(4)
        .build()
        .unwrap();
    let spots = std::iter::once("flop_monotone_wide").chain(SYMMETRIC);
    for name in spots {
        let game = PostflopGame::from_spot(&load(name)).unwrap();
        let iterations = 8;
        for schedule in schedules {
            let mut alone = VectorCfr::new(MergedGame::new(&game).unwrap(), schedule);
            alone.run(iterations, |_| {});
            let unlike = unlike_values(&alone);
            assert_eq!(
                unlike, 0,
                "{name}, {schedule:?}: {unlike} values are not alike"
            );
            let mut pooled = VectorCfr::new(MergedGame::new(&game).unwrap(), schedule);
            pool.install(|| pooled.run(iterations, |_| {}));
            assert!(
                stored_bits(&pooled) == stored_bits(&alone),
                "{name}, {schedule:?}: four threads store other values than one"
            );
        }
    }
}
