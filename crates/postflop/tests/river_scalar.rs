//! An independent scalar check of the river solver: deal, card removal, payoffs, rake,
//! normalisation, expected values, best responses and one CFR iteration.
//!
//! For two small spots, each with rake and with ranges that share combos and cards, the test
//! builds the same game as a scalar [`pk_cfr::Game`] from first principles, sharing none of
//! the river game's code but the betting tree and the hand evaluator:
//!
//! - chance deals each pair of hands, one from each range, that share no card with each other
//!   or with the board, with probability proportional to the product of their range weights;
//! - the betting follows the tree, and an information set is the acting player, the tree node
//!   and the player's combo;
//! - a terminal's payoffs come from its contributions, the pot and the rake, worked out here
//!   from what happens to the chips, and at a showdown from evaluating the two hands.
//!
//! Then the vector side ([`RiverGame`], [`VectorCfr`], the vector best response and expected
//! values, and [`exploitability`]) must agree with the scalar side ([`Cfr`],
//! [`expected_values`], [`best_response`]): the deal probability of every pair, the payoff of
//! every pair at every terminal, each player's value and best-response value under several
//! profiles, and every stored value after one iteration from the same stored state. The scalar
//! values also sum to what [`expected_rake`] says they must.

use pk_cfr::{
    Cfr, Dcfr, Game, Node as ScalarNode, Profile, PublicGame, PublicNode, PublicProfile, Schedule,
    VectorCfr, best_response, expected_values, vector_best_response, vector_expected_values,
};
use pk_core::{CardSet, Combo, evaluate};
use pk_game::{Chips, Node, Outcome, Player, PostflopTree};
use pk_postflop::{RiverGame, Spot, expected_rake, exploitability};
use rand_chacha::ChaCha8Rng;
use rand_chacha::rand_core::{Rng, SeedableRng};

/// A raked spot whose ranges share two combos (AhAd and KhQd) and several cards (the Qc of
/// KcQc and QcJc, the 6h of 7c6h and 8h6h, the Th of JhTh and ThTc), with ties at showdown
/// (two aces, three KQ, two 98) and range weights of a quarter to 1. The rake is 5% capped at
/// 1bb: a showdown or fold with no bet called pays 0.5bb, and every pot with a bet called hits
/// the cap.
const SHARED: &str = r#"format = 1
name = "shared"
description = "Illustrative ranges, for the scalar check."
board = "Qh9c7d4s2h"
pot_bb = 10.0
effective_stack_bb = 25.0
oop_range = "AhAd,AcAs:0.5,KhQd,KcQc:0.75,9s8s,JhTh,7c6h:0.25,5h3h:0.5"
ip_range = "AhAd:0.5,KsQs,KhQd:0.25,QcJc,9d8d:0.75,8h6h,44:0.5,ThTc"

[rake]
percent = 5.0
cap_bb = 1.0

[tree]
max_raises = 1
allin_threshold = 1.0
river = { bets = [0.5], raises = [3.0], allin = false }
"#;

/// Four hearts on the board, so that hearts make flushes ranked by the heart held, and a
/// heart in one hand takes flushes out of the other range (the Ah, Jh and 5h are each in both
/// ranges); a shared combo (QhJh), a tie (AsKc against AdKd); an all-in, two raises, and a
/// rake cap of 0.5bb that the larger pots hit while the 6bb root pot does not.
const FLUSH: &str = r#"format = 1
name = "flush"
description = "Illustrative ranges, for the scalar check."
board = "Kh9h6h4h2c"
pot_bb = 6.0
effective_stack_bb = 20.0
oop_range = "AhQs,Th8s,QhJh:0.5,KsKd,9c9d:0.25,5h5d,AsKc:0.75"
ip_range = "Jh9c,AhKs:0.5,8h7h,KcQc,66:0.25,AdKd,5h5c:0.5,QhJh:0.25"

[rake]
percent = 5.0
cap_bb = 0.5

[tree]
max_raises = 2
allin_threshold = 1.0
river = { bets = [1.0], raises = [2.0], allin = true }
"#;

fn spots() -> Vec<Spot> {
    [SHARED, FLUSH]
        .iter()
        .map(|text| Spot::from_toml(text).unwrap())
        .collect()
}

// ---------------------------------------------------------------------------------------------
// The scalar game

/// An information set: the acting player, the tree node and the player's combo index.
type Key = (usize, u32, usize);

/// A history: before the deal, or a pair of hands (indices into `ScalarRiver::hands`) at a
/// node of the tree.
#[derive(Debug, Clone)]
enum State {
    Deal,
    Play { hands: [usize; 2], node: u32 },
}

/// The river spot as a scalar game, built from first principles.
#[derive(Debug, Clone)]
struct ScalarRiver {
    tree: PostflopTree,
    board: CardSet,
    pot: f64,
    rake_percent: f64,
    rake_cap: f64,
    /// Each player's combos with a positive weight and no card on the board, and their weights.
    hands: [Vec<(Combo, f64)>; 2],
    /// Every pair of hands that can be dealt, and its probability.
    deals: Vec<([usize; 2], f64)>,
}

impl ScalarRiver {
    fn new(spot: &Spot) -> ScalarRiver {
        let board = spot.board;
        let hands = [&spot.oop_range, &spot.ip_range].map(|range| {
            range
                .iter()
                .filter(|(combo, _)| (combo.cards() & board).is_empty())
                .map(|(combo, weight)| (combo, f64::from(weight)))
                .collect::<Vec<_>>()
        });
        let mut deals = Vec::new();
        for (a, &(first, w)) in hands[0].iter().enumerate() {
            for (b, &(second, v)) in hands[1].iter().enumerate() {
                if (first.cards() & second.cards()).is_empty() {
                    deals.push(([a, b], w * v));
                }
            }
        }
        let total: f64 = deals.iter().map(|(_, weight)| weight).sum();
        for (_, weight) in &mut deals {
            *weight /= total;
        }
        ScalarRiver {
            tree: spot.build_tree().unwrap(),
            board,
            pot: chips(spot.pot),
            rake_percent: spot.rake_percent,
            rake_cap: chips(spot.rake_cap),
            hands,
            deals,
        }
    }

    /// Each player's net change in chips from the start of the river. A player who folds
    /// loses what it put in. The winner of a fold takes back the part of its last bet that was
    /// not called, so both players have put in the smaller contribution, which is also both
    /// contributions at a showdown. The house takes its rake from that pot, and the rest goes
    /// to the winner, or half to each on a tie.
    fn utilities(&self, hands: [usize; 2], outcome: Outcome, put_in: [Chips; 2]) -> Vec<f64> {
        let called = chips(put_in[0].min(put_in[1]));
        let pot = self.pot + 2.0 * called;
        let rake = (pot * self.rake_percent / 100.0).min(self.rake_cap);
        let share = match outcome {
            Outcome::Fold { winner } => {
                let mut share = [0.0; 2];
                share[winner.index()] = 1.0;
                share
            }
            Outcome::Showdown => {
                assert_eq!(put_in[0], put_in[1]);
                let [first, second] =
                    [0, 1].map(|p| evaluate(self.board | self.hands[p][hands[p]].0.cards()));
                match first.cmp(&second) {
                    std::cmp::Ordering::Greater => [1.0, 0.0],
                    std::cmp::Ordering::Less => [0.0, 1.0],
                    std::cmp::Ordering::Equal => [0.5, 0.5],
                }
            }
        };
        share.iter().map(|s| s * (pot - rake) - called).collect()
    }
}

fn chips(amount: Chips) -> f64 {
    f64::from(amount)
}

impl Game for ScalarRiver {
    type State = State;
    type InfoSet = Key;

    fn num_players(&self) -> usize {
        2
    }

    fn root(&self) -> State {
        State::Deal
    }

    fn node(&self, state: &State) -> ScalarNode<Key> {
        match *state {
            State::Deal => ScalarNode::Chance {
                probabilities: self.deals.iter().map(|&(_, p)| p).collect(),
            },
            State::Play { hands, node } => match self.tree.node(node) {
                Node::Player { actor, actions, .. } => {
                    let player = actor.index();
                    let combo = self.hands[player][hands[player]].0;
                    ScalarNode::Player {
                        player,
                        info_set: (player, node, combo.index()),
                        num_actions: actions.len(),
                    }
                }
                Node::Terminal {
                    outcome,
                    contributions,
                    ..
                } => ScalarNode::Terminal {
                    utilities: self.utilities(hands, *outcome, *contributions),
                },
                Node::Chance { .. } => panic!("a river tree has no chance node"),
            },
        }
    }

    fn next(&self, state: &State, index: usize) -> State {
        match *state {
            State::Deal => State::Play {
                hands: self.deals[index].0,
                node: PostflopTree::ROOT,
            },
            State::Play { hands, node } => State::Play {
                hands,
                node: self.tree.node(node).children()[index],
            },
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Between the two sides

fn player_of(index: usize) -> Player {
    [Player::Oop, Player::Ip][index]
}

/// Every information set of the vector side, as its node, hand and scalar key.
fn entries(game: &RiverGame) -> Vec<(usize, usize, Key)> {
    let mut entries = Vec::new();
    for node in 0..game.num_nodes() {
        if let PublicNode::Player { player, .. } = game.node(node) {
            for (hand, combo) in game.hands(player_of(player)).combos().iter().enumerate() {
                entries.push((node, hand, (player, node as u32, combo.index())));
            }
        }
    }
    entries
}

fn public_profile(game: &RiverGame, scalar: &Profile<Key>) -> PublicProfile {
    PublicProfile::from_fn(game, |node, hand| {
        let PublicNode::Player { player, .. } = game.node(node) else {
            unreachable!("strategies are for player nodes");
        };
        let combo = game.hands(player_of(player)).combos()[hand];
        scalar[&(player, node as u32, combo.index())].to_vec()
    })
}

/// The scalar profile of `public`: the strategy of every information set at a node where
/// `public` has one.
fn scalar_profile(game: &RiverGame, public: &PublicProfile) -> Profile<Key> {
    entries(game)
        .into_iter()
        .filter_map(|(node, hand, key)| Some((key, public.hand(node, hand)?)))
        .collect()
}

/// The values of one hand in a node's `[actions × hands]` slice.
fn column(values: &[f32], hand: usize, num_hands: usize) -> Vec<f32> {
    values
        .iter()
        .skip(hand)
        .step_by(num_hands)
        .copied()
        .collect()
}

/// The largest difference between two profiles' action probabilities.
fn largest_difference(expected: &Profile<Key>, actual: &Profile<Key>) -> f64 {
    assert_eq!(expected.len(), actual.len());
    let mut largest = 0.0f64;
    for (key, strategy) in expected.iter() {
        for (&a, &b) in strategy.iter().zip(&actual[key]) {
            largest = largest.max((f64::from(a) - f64::from(b)).abs());
        }
    }
    largest
}

/// The two sides, for one spot.
struct Sides {
    name: String,
    scalar: ScalarRiver,
    game: RiverGame,
}

fn sides() -> Vec<Sides> {
    spots()
        .iter()
        .map(|spot| Sides {
            name: spot.name.clone(),
            scalar: ScalarRiver::new(spot),
            game: RiverGame::new(spot).unwrap(),
        })
        .collect()
}

// ---------------------------------------------------------------------------------------------
// The game itself

#[test]
fn the_spots_cover_shared_combos_shared_cards_ties_and_rake_with_and_without_the_cap() {
    for sides in sides() {
        let ScalarRiver { hands, deals, .. } = &sides.scalar;
        let name = &sides.name;
        let combos = |p: usize| -> Vec<Combo> { hands[p].iter().map(|&(c, _)| c).collect() };
        let shared_combos = combos(0).iter().filter(|c| combos(1).contains(c)).count();
        let pairs = hands[0].len() * hands[1].len();
        assert!(shared_combos >= 1, "{name}");
        // Pairs that share a card but are not the same combo.
        let blocked = pairs - deals.len() - shared_combos;
        assert!(blocked >= 2, "{name}: {blocked}");
        // Every hand can be dealt with some hand of the other range, so every vector
        // information set is one of the scalar game's.
        let scalar = Cfr::new(sides.scalar.clone(), Schedule::Vanilla).average_strategy();
        assert_eq!(scalar.len(), entries(&sides.game).len(), "{name}");
        // Ties, and the rake with and without its cap.
        let mut ties = 0;
        let mut capped = [false, false];
        for (node, terminal) in sides.scalar.tree.nodes().iter().enumerate() {
            let Node::Terminal {
                outcome,
                contributions,
                ..
            } = terminal
            else {
                continue;
            };
            for &(pair, _) in deals {
                let utilities = sides.scalar.utilities(pair, *outcome, *contributions);
                let rake = sides.scalar.pot - utilities[0] - utilities[1];
                assert!(rake > 0.0, "{name}, node {node}");
                capped[usize::from(rake == sides.scalar.rake_cap)] = true;
                if *outcome == Outcome::Showdown && utilities[0] == utilities[1] {
                    ties += 1;
                }
            }
        }
        assert!(ties > 0, "{name}");
        assert_eq!(capped, [true, true], "{name}");
    }
}

#[test]
fn the_deal_and_every_payoff_match_the_scalar_game() {
    // Each pair of hands has the scalar game's probability, the pairs that share a card have
    // none, and at every terminal, with the opponent's reach all on one hand, each hand's
    // value is the scalar payoff against it. Measured, the probabilities agree within 1 ulp,
    // and the payoffs exactly.
    for sides in sides() {
        let (game, scalar) = (&sides.game, &sides.scalar);
        let name = &sides.name;
        let lists = [0, 1].map(|p| game.hands(player_of(p)));
        for p in 0..2 {
            let combos: Vec<Combo> = scalar.hands[p].iter().map(|&(c, _)| c).collect();
            assert_eq!(lists[p].combos(), combos.as_slice(), "{name}");
        }
        let mut total = 0.0;
        for &([a, b], probability) in &scalar.deals {
            let product = game.deal(0)[a] * game.deal(1)[b];
            assert!(
                (product - probability).abs() <= 2.0 * f64::EPSILON * probability,
                "{name}: {product} against {probability}"
            );
            total += product;
        }
        assert!((total - 1.0).abs() < 1e-14, "{name}: {total}");

        let mut checked = 0;
        for node in 0..game.num_nodes() {
            if game.node(node) != PublicNode::Terminal {
                continue;
            }
            for &(pair, _) in &scalar.deals {
                let state = State::Play {
                    hands: pair,
                    node: node as u32,
                };
                let ScalarNode::Terminal { utilities } = scalar.node(&state) else {
                    panic!("{name}: node {node} is a terminal");
                };
                for player in 0..2 {
                    let opponent = 1 - player;
                    let mut reach = vec![0.0; game.num_hands(opponent)];
                    reach[pair[opponent]] = 1.0;
                    let mut values = vec![f64::NAN; game.num_hands(player)];
                    game.terminal_values(node, player, &reach, &mut values);
                    assert_eq!(
                        values[pair[player]], utilities[player],
                        "{name}, node {node}, {pair:?}, player {player}"
                    );
                    // A hand that shares a card with the opponent's meets none of its reach.
                    let theirs = lists[opponent].combos()[pair[opponent]];
                    for (h, combo) in lists[player].combos().iter().enumerate() {
                        if !(combo.cards() & theirs.cards()).is_empty() {
                            assert_eq!(values[h], 0.0, "{name}: {combo} against {theirs}");
                        }
                    }
                    checked += 1;
                }
            }
        }
        assert!(checked > 100, "{name}: {checked}");
    }
}

// ---------------------------------------------------------------------------------------------
// Values under fixed profiles

/// A random profile over the scalar game's information sets, with about one probability in
/// five exactly zero.
fn random_profile(shapes: &Profile<Key>, rng: &mut ChaCha8Rng) -> Profile<Key> {
    shapes
        .iter()
        .map(|(key, strategy)| {
            let mut weights: Vec<f64> = (0..strategy.len())
                .map(|_| {
                    let unit = (rng.next_u64() >> 11) as f64 / (1u64 << 53) as f64;
                    if unit < 0.2 { 0.0 } else { unit }
                })
                .collect();
            if weights.iter().all(|&w| w == 0.0) {
                weights[0] = 1.0;
            }
            let total: f64 = weights.iter().sum();
            (*key, weights.iter().map(|w| (w / total) as f32).collect())
        })
        .collect()
}

/// The pure profile that plays the action `choose(number of actions)` everywhere.
fn pure(shapes: &Profile<Key>, choose: impl Fn(usize) -> usize) -> Profile<Key> {
    shapes
        .iter()
        .map(|(key, strategy)| {
            let chosen = choose(strategy.len());
            let row = (0..strategy.len())
                .map(|a| f32::from(a == chosen))
                .collect();
            (*key, row)
        })
        .collect()
}

/// Profiles to compare on: uniform, random with zeros, pure, and CFR's strategies early on.
fn profiles(scalar: &ScalarRiver) -> Vec<(&'static str, Profile<Key>)> {
    let shapes = Cfr::new(scalar.clone(), Schedule::Vanilla).average_strategy();
    let mut rng = ChaCha8Rng::seed_from_u64(27);
    let solved = |schedule: Schedule, iterations: u64, average: bool| {
        let mut cfr = Cfr::new(scalar.clone(), schedule);
        cfr.run(iterations, |_| {});
        if average {
            cfr.average_strategy()
        } else {
            cfr.current_strategy()
        }
    };
    let dcfr = Schedule::Discounted(Dcfr::default());
    vec![
        ("uniform", shapes.clone()),
        ("random 1", random_profile(&shapes, &mut rng)),
        ("random 2", random_profile(&shapes, &mut rng)),
        ("random 3", random_profile(&shapes, &mut rng)),
        ("always check or fold", pure(&shapes, |_| 0)),
        ("always the largest wager", pure(&shapes, |n| n - 1)),
        ("always the smallest bet or a call", pure(&shapes, |_| 1)),
        ("DCFR average after 30", solved(dcfr, 30, true)),
        ("CFR+ current after 7", solved(Schedule::CfrPlus, 7, false)),
    ]
}

#[test]
fn values_and_best_responses_match_the_scalar_game() {
    // For each profile: both players' expected values, both best-response values, the value
    // of playing the vector best response in the scalar game, and the exploitability; and the
    // sum of the scalar values against `pot · M − E[rake]` from `expected_rake`. Measured, the
    // largest differences are 6.8e-13 chips for the values, 3.4e-13 for the best responses,
    // 2.8e-13 for the exploitability and 4.5e-13 for the value sum, in games whose values run
    // to hundreds of chips.
    let mut largest = [0.0f64; 4];
    for sides in sides() {
        let (game, scalar) = (&sides.game, &sides.scalar);
        let name = &sides.name;
        for (profile_name, profile) in profiles(scalar) {
            let what = format!("{name}, {profile_name}");
            let public = public_profile(game, &profile);
            let scalar_values = expected_values(scalar, &profile);
            let vector_values = vector_expected_values(game, &public);
            // The scalar values sum to what the expected rake says they must, the pot weighted
            // by the probability of ending less the expected rake.
            let expected = expected_rake(game, &public);
            let difference = (scalar_values[0] + scalar_values[1] - expected.value_sum()).abs();
            largest[3] = largest[3].max(difference);
            assert!(
                difference < 1e-9,
                "{what}: {scalar_values:?} against {expected:?}"
            );
            let mut scalar_responses = [0.0; 2];
            for player in 0..2 {
                let difference = (vector_values[player] - scalar_values[player]).abs();
                largest[0] = largest[0].max(difference);
                assert!(
                    difference < 1e-9,
                    "{what}, player {player}: {vector_values:?} against {scalar_values:?}"
                );

                let expected = best_response(scalar, &profile, player);
                let actual = vector_best_response(game, &public, player);
                let difference = (actual.value - expected.value).abs();
                largest[1] = largest[1].max(difference);
                assert!(
                    difference < 1e-9,
                    "{what}, player {player}: {} against {}",
                    actual.value,
                    expected.value
                );
                scalar_responses[player] = expected.value;

                // The vector response is worth its value when the scalar game plays it.
                let mut merged = profile.clone();
                let response = scalar_profile(game, &actual.strategy);
                assert!(response.iter().all(|(key, _)| key.0 == player));
                assert_eq!(
                    response.len(),
                    profile.iter().filter(|(key, _)| key.0 == player).count()
                );
                for (key, row) in response.iter() {
                    merged.insert(*key, row.to_vec());
                }
                let played = expected_values(scalar, &merged)[player];
                assert!(
                    (played - expected.value).abs() < 1e-9,
                    "{what}, player {player}: played {played}"
                );
            }
            let measured = exploitability(game, &public);
            let expected = ((scalar_responses[0] - scalar_values[0])
                + (scalar_responses[1] - scalar_values[1]))
                / 2.0;
            let difference = (measured.chips - expected).abs();
            largest[2] = largest[2].max(difference);
            assert!(
                difference < 1e-9,
                "{what}: {} against {expected}",
                measured.chips
            );
            assert_eq!(measured.values, vector_values);
            assert!(
                (measured.percent_of_pot - 100.0 * expected / scalar.pot).abs() < 1e-9,
                "{what}"
            );
            // None of these profiles is an equilibrium.
            assert!(measured.percent_of_pot > 0.1, "{what}: {measured:?}");
        }
    }
    println!(
        "largest differences: values {:e}, best responses {:e}, exploitability {:e}, value \
         sum {:e}",
        largest[0], largest[1], largest[2], largest[3]
    );
}

#[test]
fn a_solved_test_spot_has_the_exploitability_the_scalar_game_measures() {
    // Two of the reference spots, solved to 0.3% of the pot: a raked one with an all-in, and
    // the four-flush board, where card removal decides who holds the better flush. The
    // scalar game measures the solution's values and best responses afresh. Measured, they
    // agree within 1.7e-11 chips.
    for name in ["paired_low_spr", "four_flush"] {
        let path = format!(
            "{}/../../reference/spots/river/{name}.toml",
            env!("CARGO_MANIFEST_DIR")
        );
        let spot = Spot::load(path).unwrap();
        let solution =
            pk_postflop::solve(&spot, &pk_postflop::SolveOptions::default(), |_| {}).unwrap();
        let scalar = ScalarRiver::new(&spot);
        let profile = scalar_profile(solution.game(), solution.strategy());
        let values = expected_values(&scalar, &profile);
        let responses = [0, 1].map(|player| best_response(&scalar, &profile, player).value);
        let measured = solution.exploitability();
        for player in 0..2 {
            assert!(
                (values[player] - measured.values[player]).abs() < 1e-9,
                "{name}: {values:?} against {measured:?}"
            );
            assert!(
                (responses[player] - measured.best_responses[player]).abs() < 1e-9,
                "{name}: {responses:?} against {measured:?}"
            );
        }
        let chips = ((responses[0] - values[0]) + (responses[1] - values[1])) / 2.0;
        assert!((chips - measured.chips).abs() < 1e-9, "{name}");
        assert!(
            100.0 * chips / f64::from(spot.pot) <= 0.3,
            "{name}: {chips}"
        );
        println!(
            "{name}: values differ by {:e} and {:e}, best responses by {:e} and {:e}",
            values[0] - measured.values[0],
            values[1] - measured.values[1],
            responses[0] - measured.best_responses[0],
            responses[1] - measured.best_responses[1]
        );
    }
}

// ---------------------------------------------------------------------------------------------
// One iteration from the same state

/// The worst comparison over a run of one-step iterations.
#[derive(Debug, Default)]
struct Worst {
    /// The largest difference of a stored value, relative to the largest magnitude in its row.
    stored: f64,
    /// The number of stored values that differ at all.
    different: usize,
    /// The number of stored values compared.
    compared: usize,
    average: f64,
    current: f64,
}

/// Runs `iterations` iterations of `schedule` in the scalar solver and, before each one, sets
/// the vector solver's stored values to the scalar solver's, so that both run the same
/// iteration from the same state; then compares every stored value and both strategies.
fn one_step(sides: &Sides, schedule: Schedule, iterations: u64) -> Worst {
    let game = &sides.game;
    let entries = entries(game);
    let mut scalar = Cfr::new(sides.scalar.clone(), schedule);
    let mut vector = VectorCfr::new(game.clone(), schedule);
    let mut worst = Worst::default();
    for _ in 0..iterations {
        // Set the vector solver's state from the scalar solver's.
        for node in 0..game.num_nodes() {
            let PublicNode::Player {
                player,
                num_actions,
            } = game.node(node)
            else {
                continue;
            };
            let num_hands = game.num_hands(player);
            let mut regrets = vec![0.0; num_actions * num_hands];
            let mut sums = regrets.clone();
            for (hand, combo) in game.hands(player_of(player)).combos().iter().enumerate() {
                let key = (player, node as u32, combo.index());
                for (values, from) in [
                    (&mut regrets, scalar.regrets(&key).unwrap()),
                    (&mut sums, scalar.strategy_sum(&key).unwrap()),
                ] {
                    for (action, &value) in from.iter().enumerate() {
                        values[action * num_hands + hand] = value;
                    }
                }
            }
            vector.set_stored(node, &regrets, &sums);
        }
        scalar.iterate();
        vector.iterate();
        for &(node, hand, key) in &entries {
            let num_hands = game.num_hands(key.0);
            for (expected, actual) in [
                (scalar.regrets(&key), vector.regrets(node)),
                (scalar.strategy_sum(&key), vector.strategy_sum(node)),
            ] {
                let (expected, actual) =
                    (expected.unwrap(), column(actual.unwrap(), hand, num_hands));
                let scale = expected
                    .iter()
                    .chain(&actual)
                    .fold(0.0f64, |m, &x| m.max(f64::from(x).abs()));
                for (&e, &a) in expected.iter().zip(&actual) {
                    worst.compared += 1;
                    if e.to_bits() != a.to_bits() {
                        worst.different += 1;
                        let relative = (f64::from(e) - f64::from(a)).abs() / scale;
                        worst.stored = worst.stored.max(relative);
                    }
                }
            }
        }
        worst.average = worst.average.max(largest_difference(
            &scalar.average_strategy(),
            &scalar_profile(game, &vector.average_strategy()),
        ));
        worst.current = worst.current.max(largest_difference(
            &scalar.current_strategy(),
            &scalar_profile(game, &vector.current_strategy()),
        ));
    }
    worst
}

#[test]
fn one_iteration_from_the_same_state_matches_the_scalar_solver() {
    // 200 iterations of each schedule, each from the scalar solver's state. Measured, at most
    // 212 of the 50,400 or 108,800 stored values compared differ at all, each within 8.7e-8 of
    // the largest in its row (one f32 ulp); the average strategies are identical and the
    // current strategies within 8.5e-17.
    let schedules = [
        Schedule::Vanilla,
        Schedule::CfrPlus,
        Schedule::Linear,
        Schedule::Discounted(Dcfr::default()),
    ];
    for sides in sides() {
        for schedule in schedules {
            let worst = one_step(&sides, schedule, 200);
            println!("{}, {schedule:?}: {worst:?}", sides.name);
            for (what, difference) in [
                ("stored values", worst.stored),
                ("average strategy", worst.average),
                ("current strategy", worst.current),
            ] {
                assert!(
                    difference <= 1e-6,
                    "{}, {schedule:?}: {what} differ by {difference:e}",
                    sides.name
                );
            }
            // The comparison is not vacuous: the solvers learn, and most values match exactly.
            assert!(worst.compared > 10_000, "{worst:?}");
            assert!(worst.different * 10 < worst.compared, "{worst:?}");
        }
    }
}
