use pk_core::{Card, Combo, HandClass, NUM_CARDS, NUM_CLASSES, NUM_COMBOS};
use pk_game::{Action, Node};
use proptest::prelude::*;

use super::*;

// ---------------------------------------------------------------------------------------------
// Helpers

/// A valid spot, in the layout the writer produces. It is also the example in the module
/// documentation. The error tests change one thing in it.
const BASE: &str = r#"format = 1
name = "example"
description = "Illustrative ranges, not from a reference."
board = "Ks9h7d4c2s"
pot_bb = 20.0
effective_stack_bb = 80.0
oop_range = "TT-QQ,AKs,KQs,K9s,AKo:0.5"
ip_range = "KK,99,AKs,KJs,QJs,T8s:0.5"

[rake]
percent = 5.0
cap_bb = 3.0

[tree]
max_raises = 1
allin_threshold = 1.0
river = { bets = [0.5, 1.0], raises = [3.0], allin = false }
"#;

fn read(text: &str) -> Spot {
    Spot::from_toml(text).unwrap_or_else(|err| panic!("{text}\nshould load: {err}"))
}

fn base() -> Spot {
    read(BASE)
}

fn cards(text: &str) -> CardSet {
    text.parse().unwrap()
}

fn range(text: &str) -> Range {
    text.parse()
        .unwrap_or_else(|err| panic!("{text:?} should parse: {err}"))
}

fn street(bets: &[f64], raises: &[f64], allin: bool) -> StreetSizing {
    StreetSizing {
        bets: bets.to_vec(),
        raises: raises.to_vec(),
        allin,
    }
}

/// `BASE` with the first `from` replaced by `to`.
fn base_with(from: &str, to: &str) -> String {
    assert!(BASE.contains(from), "test setup: {from:?} is not in BASE");
    BASE.replacen(from, to, 1)
}

/// The field and reason of the error that loading `text` gives.
fn rejected(text: &str) -> (String, String) {
    match Spot::from_toml(text) {
        Err(SpotError::Invalid { field, reason }) => {
            println!("`{field}` {reason}");
            (field, reason)
        }
        other => panic!("expected an invalid-spot error, got {other:?} for\n{text}"),
    }
}

/// The field that `BASE` is rejected for once `from` is replaced by `to`.
fn rejected_field(from: &str, to: &str) -> String {
    rejected(&base_with(from, to)).0
}

/// The field and reason of the error that validating `spot` gives.
fn invalid_field(spot: &Spot) -> (String, String) {
    match spot.validate() {
        Err(SpotError::Invalid { field, reason }) => {
            println!("`{field}` {reason}");
            (field, reason)
        }
        other => panic!("expected an invalid-spot error, got {other:?} for {spot:?}"),
    }
}

/// Writes the spot, reads it back and writes it again, and checks that nothing changed.
fn assert_round_trip(spot: &Spot) {
    spot.validate()
        .unwrap_or_else(|err| panic!("{spot:?} should be valid: {err}"));
    let text = spot.to_toml();
    let reread = read(&text);
    assert_eq!(&reread, spot, "{text}");
    assert_eq!(reread.to_toml(), text);
}

/// The base spot moved to a flop or turn board, with a sizing for each street it plays.
fn earlier_spot(board: &str) -> Spot {
    let mut spot = Spot {
        board: cards(board),
        ..base()
    };
    match spot.board.len() {
        3 => {
            spot.tree.flop = Some(street(&[0.33], &[], false));
            spot.tree.turn = Some(street(&[0.75], &[3.0], false));
        }
        4 => spot.tree.turn = Some(street(&[0.75], &[3.0], true)),
        _ => unreachable!("test setup"),
    }
    spot
}

// ---------------------------------------------------------------------------------------------
// Reading and writing

#[test]
fn the_base_spot_reads_as_written_and_writes_back_unchanged() {
    let spot = base();
    assert_eq!(spot.name, "example");
    assert_eq!(
        spot.description,
        "Illustrative ranges, not from a reference."
    );
    assert_eq!(spot.board, cards("Ks9h7d4c2s"));
    assert_eq!(spot.pot, 2000);
    assert_eq!(spot.effective_stack, 8000);
    assert_eq!(spot.oop_range, range("TT-QQ,AKs,AKo:0.5,KQs,K9s"));
    assert_eq!(spot.ip_range, range("99,KK,AKs,KJs,QJs,T8s:0.5"));
    assert_eq!(spot.rake_percent, 5.0);
    assert_eq!(spot.rake_cap, 300);
    assert_eq!(
        spot.tree,
        TreeSizing {
            max_raises: 1,
            allin_threshold: 1.0,
            flop: None,
            turn: None,
            river: Some(street(&[0.5, 1.0], &[3.0], false)),
        }
    );
    assert_eq!(spot.to_toml(), BASE);
    assert_eq!(spot.to_string(), BASE);
}

#[test]
fn the_module_documentation_shows_the_base_spot() {
    let source = include_str!("../spot.rs");
    let start = source.find("//! ```toml\n").expect("the example exists") + "//! ```toml\n".len();
    let end = start + source[start..].find("//! ```").unwrap();
    let example: String = source[start..end]
        .lines()
        .map(|line| line.strip_prefix("//!").unwrap().trim_start_matches(' '))
        .map(|line| format!("{line}\n"))
        .collect();
    assert_eq!(example, BASE);
}

#[test]
fn a_hand_typed_file_reads_the_same_and_writes_in_the_canonical_layout() {
    // Keys out of order, comments, a literal string, a board in any order and case with
    // spaces, ranges typed in no particular order, integers for amounts and sizes, and the
    // river sizing as a table of its own.
    let typed = r#"
# Typed by hand.
format = 1
description = 'Illustrative ranges, not from a reference.'
name = "example"
board = "2s 4c 7D 9h ks"
effective_stack_bb = 80   # behind
pot_bb = 20
ip_range = "T8s:0.5, QJs, KJs, AKs, 99, KK"
oop_range = "AKo:0.5, K9s, KQs, AKs, QQ, JJ, TT"

[tree]
allin_threshold = 1
max_raises = 1

[tree.river]
allin = false
raises = [3]
bets = [0.5, 1]

[rake]
cap_bb = 3
percent = 5
"#;
    let spot = read(typed);
    assert_eq!(spot, base());
    assert_eq!(spot.to_toml(), BASE);

    // A size typed with extra digits is written in its shortest form.
    let long = read(&base_with("bets = [0.5, 1.0]", "bets = [0.500, 1.000000]"));
    assert_eq!(long.to_toml(), BASE);
}

#[test]
fn writing_then_reading_gives_an_equal_spot() {
    let base = base();
    let spots = [
        base.clone(),
        // Rake-free, no raises, and one combo a side.
        Spot {
            oop_range: range("AhAd"),
            ip_range: range("QcQd:0.25"),
            rake_percent: 0.0,
            rake_cap: 0,
            tree: TreeSizing {
                max_raises: 0,
                river: Some(street(&[0.75], &[], false)),
                ..base.tree.clone()
            },
            ..base.clone()
        },
        // Amounts that need both decimal places, the largest amounts, and a rake percent that is
        // not a round number.
        Spot {
            pot: 1,
            effective_stack: MAX_CHIPS,
            rake_percent: 4.75,
            rake_cap: 7,
            ..base.clone()
        },
        Spot {
            pot: MAX_CHIPS,
            effective_stack: 1225,
            rake_percent: 99.999,
            rake_cap: MAX_CHIPS,
            ..base.clone()
        },
        // Sizes with no short decimal form, and many of them.
        Spot {
            tree: TreeSizing {
                max_raises: u32::MAX,
                allin_threshold: 0.67,
                river: Some(street(
                    &[0.1 + 0.2, 1e-7, 0.33, 2.0 / 3.0, MAX_BET_FRACTION],
                    &[1.0 + f64::EPSILON, 2.5, MAX_RAISE_MULTIPLE],
                    true,
                )),
                ..base.tree.clone()
            },
            ..base.clone()
        },
        // Weights at several levels, and a full range.
        Spot {
            oop_range: range("AA:0.125,KK:0.3333,AKs,AsKs:0.75,AhKd:0.01,72o:1e-3"),
            ip_range: Range::full(),
            ..base.clone()
        },
        // Text that needs escaping.
        Spot {
            name: "a \"quoted\" name".into(),
            description: "back\\slash, two\nlines\tand a tab\u{7f}\u{1}, ünïcödé # not a comment"
                .into(),
            ..base.clone()
        },
        // Flop and turn spots.
        earlier_spot("Ks9h7d"),
        earlier_spot("Ks9h7d4c"),
    ];
    for spot in &spots {
        assert_round_trip(spot);
    }
}

#[test]
fn numbers_are_written_as_the_shortest_exact_decimal() {
    for (number, text) in [
        (0.0, "0.0"),
        (-0.0, "0.0"),
        (0.5, "0.5"),
        (0.75, "0.75"),
        (1.0, "1.0"),
        (3.0, "3.0"),
        (0.33, "0.33"),
        (0.1 + 0.2, "0.30000000000000004"),
        (1e-7, "1e-7"),
        (1e21, "1e21"),
    ] {
        assert_eq!(float(number), text);
        let parsed: Table = toml::from_str(&format!("x = {text}")).unwrap();
        assert_eq!(parsed["x"].as_float(), Some(number), "{text}");
    }
    assert_eq!(floats(&[]), "[]");
    assert_eq!(floats(&[0.33, 0.75]), "[0.33, 0.75]");

    for (chips, text) in [(1, "0.01"), (50, "0.5"), (1225, "12.25"), (10_000, "100.0")] {
        assert_eq!(decimal_bb(chips), text);
    }
}

fn any_weight() -> impl Strategy<Value = f32> {
    prop_oneof![
        4 => Just(0.0f32),
        2 => Just(1.0f32),
        1 => Just(0.5f32),
        1 => 0.0f32..=1.0f32,
    ]
}

/// A range with weights by class and a few combos set on their own. It may be empty.
fn any_range() -> impl Strategy<Value = Range> {
    let classes = prop::collection::vec(any_weight(), NUM_CLASSES);
    let combos = prop::collection::vec((0..NUM_COMBOS, any_weight()), 0..6);
    (classes, combos).prop_map(|(classes, combos)| {
        let mut range = Range::empty();
        for (index, weight) in classes.into_iter().enumerate() {
            range.set_class(HandClass::from_index(index), weight);
        }
        for (index, weight) in combos {
            range.set(Combo::from_index(index), weight);
        }
        range
    })
}

/// Any text at all, after a letter so that it is never blank.
fn any_text() -> impl Strategy<Value = String> {
    any::<String>().prop_map(|text| format!("x{text}"))
}

fn any_bet() -> impl Strategy<Value = f64> {
    prop_oneof![
        4 => 0.01f64..5.0,
        1 => Just(0.33),
        1 => Just(1e-7),
        1 => Just(MAX_BET_FRACTION),
        1 => Just(0.1 + 0.2),
    ]
}

fn any_raise() -> impl Strategy<Value = f64> {
    prop_oneof![
        4 => (1.0f64..10.0).prop_map(|multiple| multiple + 0.5),
        1 => Just(1.0 + f64::EPSILON),
        1 => Just(2.5),
        1 => Just(MAX_RAISE_MULTIPLE),
    ]
}

fn any_street() -> impl Strategy<Value = StreetSizing> {
    (
        prop::collection::vec(any_bet(), 1..4),
        prop::collection::vec(any_raise(), 0..3),
        any::<bool>(),
    )
        .prop_map(|(bets, raises, allin)| StreetSizing {
            bets,
            raises,
            allin,
        })
}

fn any_spot() -> impl Strategy<Value = Spot> {
    let cards: Vec<usize> = (0..NUM_CARDS).collect();
    let board = prop::sample::subsequence(cards, 3..=5).prop_shuffle();
    // Four positions among the cards left off the board, at least 47 of them, for a pair of
    // hands that can always be dealt together.
    let free = prop::sample::subsequence((0..47usize).collect::<Vec<_>>(), 4).prop_shuffle();
    let texts = (any_text(), any_text());
    let amounts = (1..=MAX_CHIPS, 1..=MAX_CHIPS);
    let ranges = (any_range(), any_range(), free, 0.01f32..=1.0, 0.01f32..=1.0);
    let rake = (
        prop_oneof![Just(0.0), Just(5.0), 0.0f64..100.0],
        0..=MAX_CHIPS,
    );
    let threshold = prop_oneof![Just(1.0), Just(0.67), f64::MIN_POSITIVE..=1.0];
    let tree = (
        any::<u32>(),
        threshold,
        any_street(),
        any_street(),
        any_street(),
    );
    (texts, board, amounts, ranges, rake, tree).prop_map(
        |(
            (name, description),
            board,
            (pot, effective_stack),
            (mut oop_range, mut ip_range, free, oop_weight, ip_weight),
            (rake_percent, rake_cap),
            (max_raises, allin_threshold, flop, turn, river),
        )| {
            let board: CardSet = board.into_iter().map(Card::from_index).collect();
            let off_board: Vec<Card> = Card::all().filter(|&c| !board.contains(c)).collect();
            let card = |i: usize| off_board[free[i]];
            oop_range.set(Combo::new(card(0), card(1)), oop_weight);
            ip_range.set(Combo::new(card(2), card(3)), ip_weight);
            let start = street_of(board).unwrap();
            let rake_cap = if rake_percent > 0.0 {
                rake_cap.max(1)
            } else {
                rake_cap
            };
            Spot {
                name,
                description,
                board,
                pot,
                effective_stack,
                oop_range,
                ip_range,
                rake_percent,
                rake_cap,
                tree: TreeSizing {
                    max_raises,
                    allin_threshold,
                    flop: (start == Street::Flop).then_some(flop),
                    turn: (start <= Street::Turn).then_some(turn),
                    river: Some(river),
                },
            }
        },
    )
}

proptest! {
    #[test]
    fn any_valid_spot_round_trips(spot in any_spot()) {
        prop_assert!(spot.validate().is_ok(), "{:?}", spot.validate());
        let text = spot.to_toml();
        let reread = Spot::from_toml(&text);
        prop_assert!(reread.is_ok(), "{:?}\n{}", reread, text);
        let reread = reread.unwrap();
        prop_assert_eq!(&reread, &spot);
        prop_assert_eq!(reread.to_toml(), text);
    }
}

// ---------------------------------------------------------------------------------------------
// What the solver takes from a spot

#[test]
fn the_street_follows_the_board() {
    assert_eq!(base().street(), Street::River);
    assert_eq!(earlier_spot("Ks9h7d4c").street(), Street::Turn);
    assert_eq!(earlier_spot("Ks9h7d").street(), Street::Flop);
}

#[test]
#[should_panic(expected = "3, 4 or 5 cards, not 6")]
fn a_board_of_six_cards_has_no_street() {
    let spot = Spot {
        board: cards("Ks9h7d4c2s3s"),
        ..base()
    };
    spot.street();
}

#[test]
fn the_tree_spec_holds_the_spot_and_its_sizing() {
    let spot = base();
    let spec = spot.tree_spec();
    assert_eq!(spec.street, Street::River);
    assert_eq!(spec.pot, 2000);
    assert_eq!(spec.stack, 8000);
    assert_eq!(spec.max_nodes, TreeSpec::DEFAULT_MAX_NODES);
    let river = street(&[0.5, 1.0], &[3.0], false);
    assert_eq!(spec.sizing.river, river);
    // The flop and turn are never played. They hold copies of the river.
    assert_eq!(spec.sizing.flop, river);
    assert_eq!(spec.sizing.turn, river);
    assert_eq!(spec.sizing.max_raises, 1);
    assert_eq!(spec.sizing.allin_threshold, 1.0);

    let turn = earlier_spot("Ks9h7d4c");
    let spec = turn.tree_spec();
    assert_eq!(spec.street, Street::Turn);
    assert_eq!(spec.sizing.flop, street(&[0.75], &[3.0], true));
    assert_eq!(Some(&spec.sizing.turn), turn.tree.turn.as_ref());
    assert_eq!(Some(&spec.sizing.river), turn.tree.river.as_ref());

    let flop = earlier_spot("Ks9h7d");
    let spec = flop.tree_spec();
    assert_eq!(spec.street, Street::Flop);
    assert_eq!(Some(&spec.sizing.flop), flop.tree.flop.as_ref());
    assert_eq!(Some(&spec.sizing.turn), flop.tree.turn.as_ref());
}

#[test]
fn a_sizing_with_a_street_missing_gives_no_postflop_sizing() {
    let tree = earlier_spot("Ks9h7d4c").tree;
    assert!(tree.postflop_sizing(Street::Turn).is_some());
    assert!(tree.postflop_sizing(Street::River).is_some());
    assert!(tree.postflop_sizing(Street::Flop).is_none());
    let no_river = TreeSizing {
        river: None,
        ..tree
    };
    assert!(no_river.postflop_sizing(Street::Turn).is_none());
}

#[test]
#[should_panic(expected = "needs a sizing for every street it plays")]
fn the_tree_spec_needs_every_street_the_spot_plays() {
    let spot = Spot {
        board: cards("Ks9h7d4c"),
        ..base()
    };
    spot.tree_spec();
}

#[test]
fn the_tree_is_built_from_the_spot() {
    let tree = base().build_tree().unwrap();
    assert_eq!(tree.spec(), &base().tree_spec());
    // OOP checks or bets half the pot or the pot.
    match tree.node(PostflopTree::ROOT) {
        Node::Player { actions, .. } => assert_eq!(
            actions,
            &[
                Action::Check,
                Action::Bet {
                    to: 1000,
                    fraction: 0.5
                },
                Action::Bet {
                    to: 2000,
                    fraction: 1.0
                },
            ]
        ),
        other => panic!("the root is {other:?}"),
    }
    assert!(earlier_spot("Ks9h7d4c").build_tree().is_ok());
}

#[test]
fn the_board_hands_and_the_rake_come_from_the_spot() {
    let spot = base();
    let hands = spot.board_hands();
    assert_eq!(hands.board(), spot.board);
    // TT-QQ is 18 combos, AKs 3 and KQs 3 once the Ks is on the board, K9s 2 once the 9h is
    // too, and AKo 9.
    assert_eq!(hands.hands(Player::Oop).len(), 18 + 3 + 3 + 2 + 9);
    // KK 3, 99 3, AKs 3, KJs 3, QJs 4 and T8s 4.
    assert_eq!(hands.hands(Player::Ip).len(), 3 + 3 + 3 + 3 + 4 + 4);
    assert_eq!(spot.range(Player::Oop), &spot.oop_range);
    assert_eq!(spot.range(Player::Ip), &spot.ip_range);

    assert_eq!(
        spot.rake(),
        Rake {
            percent: 5.0,
            cap: 300,
            no_flop_no_drop: true
        }
    );
}

// ---------------------------------------------------------------------------------------------
// Load errors, one test per kind

#[test]
fn the_format_must_be_1() {
    let (field, reason) = rejected(&base_with("format = 1", "format = 2"));
    assert_eq!(field, "format");
    assert!(reason.contains("format 1 only"), "{reason}");
    assert_eq!(rejected_field("format = 1", "format = \"1\""), "format");
    assert_eq!(rejected_field("format = 1", "format = 1.0"), "format");
    assert_eq!(rejected_field("format = 1\n", ""), "format");
    // The version comes first, so a file from a later format is reported as one, not by the
    // keys that format adds.
    assert_eq!(
        rejected_field("format = 1", "format = 2\nseed = 7"),
        "format"
    );
}

#[test]
fn every_field_is_required() {
    for (line, field) in [
        ("name = \"example\"\n", "name"),
        (
            "description = \"Illustrative ranges, not from a reference.\"\n",
            "description",
        ),
        ("board = \"Ks9h7d4c2s\"\n", "board"),
        ("pot_bb = 20.0\n", "pot_bb"),
        ("effective_stack_bb = 80.0\n", "effective_stack_bb"),
        ("oop_range = \"TT-QQ,AKs,KQs,K9s,AKo:0.5\"\n", "oop_range"),
        ("ip_range = \"KK,99,AKs,KJs,QJs,T8s:0.5\"\n", "ip_range"),
        ("percent = 5.0\n", "rake.percent"),
        ("cap_bb = 3.0\n", "rake.cap_bb"),
        ("max_raises = 1\n", "tree.max_raises"),
        ("allin_threshold = 1.0\n", "tree.allin_threshold"),
    ] {
        let (rejected_as, reason) = rejected(&base_with(line, ""));
        assert_eq!(rejected_as, field);
        assert_eq!(reason, "is missing");
    }
    for (bets, field) in [
        ("bets = [0.5, 1.0], ", "tree.river.bets"),
        ("raises = [3.0], ", "tree.river.raises"),
        (", allin = false", "tree.river.allin"),
    ] {
        assert_eq!(
            rejected(&base_with(bets, "")),
            (field.into(), "is missing".into())
        );
    }

    assert_eq!(
        rejected(&base_with("[rake]\npercent = 5.0\ncap_bb = 3.0\n", "")).0,
        "rake"
    );
    let no_tree = &BASE[..BASE.find("\n[tree]").unwrap()];
    assert_eq!(rejected(no_tree).0, "tree");
}

#[test]
fn unknown_keys_are_rejected() {
    let (field, reason) = rejected(&base_with("pot_bb", "seed = 7\npot_bb"));
    assert_eq!(field, "seed");
    assert!(reason.contains("`effective_stack_bb`"), "{reason}");
    let (field, reason) = rejected(&base_with("cap_bb", "no_flop_no_drop = true\ncap_bb"));
    assert_eq!(field, "rake.no_flop_no_drop");
    assert!(reason.contains("of `[rake]`"), "{reason}");
    assert_eq!(
        rejected_field("max_raises", "max_nodes = 10\nmax_raises"),
        "tree.max_nodes"
    );
    let (field, reason) = rejected(&base_with("allin = false", "allin = false, check = true"));
    assert_eq!(field, "tree.river.check");
    assert!(reason.contains("of `tree.river`"), "{reason}");
    // A street that is not one of the three is unknown too.
    assert_eq!(
        rejected_field("river = {", "preflop = { bets = [1.0] }\nriver = {"),
        "tree.preflop"
    );
    // A misspelled key is reported as itself, not as the key it was meant to be.
    assert_eq!(
        rejected_field("effective_stack_bb", "efective_stack_bb"),
        "efective_stack_bb"
    );
    // Keys are case-sensitive.
    assert_eq!(rejected_field("[rake]", "[Rake]"), "Rake");
}

#[test]
fn values_of_the_wrong_type_are_rejected() {
    for (from, to, field) in [
        ("name = \"example\"", "name = 1", "name"),
        ("board = \"Ks9h7d4c2s\"", "board = [\"Ks\"]", "board"),
        ("pot_bb = 20.0", "pot_bb = \"20\"", "pot_bb"),
        (
            "oop_range = \"TT-QQ,AKs,KQs,K9s,AKo:0.5\"",
            "oop_range = [\"TT-QQ\"]",
            "oop_range",
        ),
        ("percent = 5.0", "percent = \"5%\"", "rake.percent"),
        ("cap_bb = 3.0", "cap_bb = true", "rake.cap_bb"),
        ("max_raises = 1", "max_raises = 1.0", "tree.max_raises"),
        (
            "allin_threshold = 1.0",
            "allin_threshold = \"all\"",
            "tree.allin_threshold",
        ),
        ("bets = [0.5, 1.0]", "bets = 0.5", "tree.river.bets"),
        (
            "bets = [0.5, 1.0]",
            "bets = [0.5, \"1\"]",
            "tree.river.bets",
        ),
        ("raises = [3.0]", "raises = [[3.0]]", "tree.river.raises"),
        ("allin = false", "allin = 0", "tree.river.allin"),
    ] {
        let (rejected_as, reason) = rejected(&base_with(from, to));
        assert_eq!(rejected_as, field, "{to}");
        assert!(reason.contains("must"), "{reason}");
    }

    let flat = BASE[..BASE.find("\n[rake]").unwrap()].to_string() + "rake = 5.0\n";
    let flat = flat + &BASE[BASE.find("\n[tree]").unwrap()..];
    let (field, reason) = rejected(&flat);
    assert_eq!(field, "rake");
    assert!(reason.contains("must be a table"), "{reason}");
    assert_eq!(
        rejected_field(
            "river = { bets = [0.5, 1.0], raises = [3.0], allin = false }",
            "river = [0.5, 1.0]"
        ),
        "tree.river"
    );
}

#[test]
fn the_name_and_description_must_not_be_blank() {
    let (field, reason) = rejected(&base_with("name = \"example\"", "name = \"\""));
    assert_eq!((field.as_str(), reason.as_str()), ("name", "is blank"));
    assert_eq!(
        rejected_field(
            "description = \"Illustrative ranges, not from a reference.\"",
            "description = \" \t\""
        ),
        "description"
    );
}

#[test]
fn the_board_must_be_3_to_5_distinct_cards() {
    for (bad, message) in [
        ("Ks9h7d4c2x", "`2x` is not a card"),
        ("Ks9h7d4c2", "is not a list of cards"),
        ("Ks9h7d4cKs", "card `Ks` appears more than once"),
        ("Ks9h", "has 2 cards"),
        ("", "has 0 cards"),
        ("Ks9h7d4c2s3s", "has 6 cards"),
    ] {
        let (field, reason) = rejected(&base_with("Ks9h7d4c2s", bad));
        assert_eq!(field, "board", "{bad}");
        assert!(reason.contains(message), "{reason}");
    }
}

#[test]
fn amounts_must_be_positive_whole_chips() {
    for bad in [
        "20.005",
        "0",
        "0.0",
        "-0.0",
        "1e-9",
        "-20.0",
        "nan",
        "inf",
        "1000000.01",
    ] {
        assert_eq!(
            rejected_field("pot_bb = 20.0", &format!("pot_bb = {bad}")),
            "pot_bb",
            "{bad}"
        );
    }
    for bad in ["80.001", "0", "-1", "1e7"] {
        let line = format!("effective_stack_bb = {bad}");
        assert_eq!(
            rejected_field("effective_stack_bb = 80.0", &line),
            "effective_stack_bb",
            "{bad}"
        );
    }
    let (_, reason) = rejected(&base_with("pot_bb = 20.0", "pot_bb = 20.005"));
    assert!(reason.contains("not a whole number of chips"), "{reason}");
    let (_, reason) = rejected(&base_with("pot_bb = 20.0", "pot_bb = 0"));
    assert!(reason.contains("more than 0"), "{reason}");

    // The smallest and largest amounts load.
    assert_eq!(read(&base_with("pot_bb = 20.0", "pot_bb = 0.01")).pot, 1);
    let largest = base_with("effective_stack_bb = 80.0", "effective_stack_bb = 1000000");
    assert_eq!(read(&largest).effective_stack, MAX_CHIPS);
}

#[test]
fn ranges_must_parse() {
    // The range parser's own message follows the field.
    for bad in ["AA:1.5", "AKx", "A2s-K5s", "AA,,KK"] {
        let (field, reason) = rejected(&base_with("TT-QQ,AKs,KQs,K9s,AKo:0.5", bad));
        assert_eq!(field, "oop_range");
        let message = bad.parse::<Range>().unwrap_err().to_string();
        assert_eq!(reason, format!("is not a valid range: {message}"));
    }
    assert_eq!(
        rejected_field("KK,99,AKs,KJs,QJs,T8s:0.5", "KK:2"),
        "ip_range"
    );
}

#[test]
fn a_range_must_hold_a_hand_the_board_leaves_possible() {
    for empty in ["", "  ", "AA:0"] {
        let (field, reason) = rejected(&base_with("KK,99,AKs,KJs,QJs,T8s:0.5", empty));
        assert_eq!(field, "ip_range");
        assert_eq!(reason, "holds no hands");
    }
    // Every combo holds the Ks or the 9h, which are on the board.
    for blocked in ["KsQs,KsKh", "Ks9s,9h9d,9hKh"] {
        let (field, reason) = rejected(&base_with("TT-QQ,AKs,KQs,K9s,AKo:0.5", blocked));
        assert_eq!(field, "oop_range");
        assert!(reason.contains("on this board"), "{reason}");
    }
}

#[test]
fn some_pair_of_hands_must_be_dealable_together() {
    // Each IP combo shares a card with the one OOP combo.
    let oop = base_with("TT-QQ,AKs,KQs,K9s,AKo:0.5", "AhAd");
    let text = oop.replacen("KK,99,AKs,KJs,QJs,T8s:0.5", "AhKh,AdQd,AhAc", 1);
    let (field, reason) = rejected(&text);
    assert_eq!(field, "ip_range");
    assert!(reason.contains("shares a card"), "{reason}");

    // One combo that misses both OOP cards is enough.
    let text = oop.replacen("KK,99,AKs,KJs,QJs,T8s:0.5", "AhKh,AdQd,QcJc", 1);
    read(&text);
    // The same combo in both ranges blocks itself.
    let text = oop.replacen("KK,99,AKs,KJs,QJs,T8s:0.5", "AhAd", 1);
    assert_eq!(rejected(&text).0, "ip_range");
}

#[test]
fn the_rake_is_checked() {
    for bad in ["-1.0", "100.0", "150", "nan", "inf"] {
        assert_eq!(
            rejected_field("percent = 5.0", &format!("percent = {bad}")),
            "rake.percent",
            "{bad}"
        );
    }
    for bad in ["3.001", "-3.0", "1000000.01"] {
        assert_eq!(
            rejected_field("cap_bb = 3.0", &format!("cap_bb = {bad}")),
            "rake.cap_bb",
            "{bad}"
        );
    }
    // A cap of 0 under a positive percent would take nothing.
    let (field, reason) = rejected(&base_with("cap_bb = 3.0", "cap_bb = 0.0"));
    assert_eq!(field, "rake.cap_bb");
    assert!(reason.contains("rake-free spot"), "{reason}");

    // A rake-free spot may keep any cap.
    let free = read(&base_with("percent = 5.0", "percent = 0.0"));
    assert_eq!((free.rake_percent, free.rake_cap), (0.0, 300));
    let free =
        read(&base_with("percent = 5.0", "percent = 0").replacen("cap_bb = 3.0", "cap_bb = 0", 1));
    assert_eq!((free.rake_percent, free.rake_cap), (0.0, 0));
}

#[test]
fn the_tree_rules_are_checked() {
    for bad in ["0.0", "-0.5", "1.01", "nan"] {
        assert_eq!(
            rejected_field("allin_threshold = 1.0", &format!("allin_threshold = {bad}")),
            "tree.allin_threshold"
        );
    }
    for bad in ["-1", "4294967296"] {
        let (field, reason) =
            rejected(&base_with("max_raises = 1", &format!("max_raises = {bad}")));
        assert_eq!(field, "tree.max_raises");
        assert!(reason.contains("from 0 to 4294967295"), "{reason}");
    }
    // No raises at all, and a threshold just above 0, are allowed.
    let text = base_with("max_raises = 1", "max_raises = 0");
    assert_eq!(read(&text).tree.max_raises, 0);
    let text = base_with("allin_threshold = 1.0", "allin_threshold = 0.01");
    assert_eq!(read(&text).tree.allin_threshold, 0.01);
}

#[test]
fn the_sizes_are_checked() {
    for (from, to, field, message) in [
        (
            "bets = [0.5, 1.0]",
            "bets = []",
            "tree.river.bets",
            "is empty",
        ),
        (
            "bets = [0.5, 1.0]",
            "bets = [0.5, 0]",
            "tree.river.bets",
            "has 0",
        ),
        (
            "bets = [0.5, 1.0]",
            "bets = [-0.5]",
            "tree.river.bets",
            "has -0.5",
        ),
        (
            "bets = [0.5, 1.0]",
            "bets = [inf]",
            "tree.river.bets",
            "has inf",
        ),
        (
            "bets = [0.5, 1.0]",
            "bets = [nan]",
            "tree.river.bets",
            "has NaN",
        ),
        (
            "bets = [0.5, 1.0]",
            "bets = [33.0, 75.0]",
            "tree.river.bets",
            "has 33",
        ),
        (
            "raises = [3.0]",
            "raises = [250.0]",
            "tree.river.raises",
            "has 250",
        ),
        (
            "raises = [3.0]",
            "raises = [1.0]",
            "tree.river.raises",
            "has 1",
        ),
        (
            "raises = [3.0]",
            "raises = [3.0, 0.5]",
            "tree.river.raises",
            "has 0.5",
        ),
        (
            "raises = [3.0]",
            "raises = [nan]",
            "tree.river.raises",
            "has NaN",
        ),
    ] {
        let (rejected_as, reason) = rejected(&base_with(from, to));
        assert_eq!(rejected_as, field, "{to}");
        assert!(reason.contains(message), "{reason}");
    }
    // No raise sizes is allowed: facing a bet, a player then folds or calls, or goes all-in if
    // the street offers it.
    let text = base_with("raises = [3.0]", "raises = []");
    assert!(read(&text).tree.river.unwrap().raises.is_empty());
}

#[test]
fn the_spot_sizes_exactly_the_streets_it_plays() {
    let river = "river = { bets = [0.5, 1.0], raises = [3.0], allin = false }";
    let turn = "turn = { bets = [0.75], raises = [], allin = false }";

    // A river spot sizes the river only.
    let (field, reason) = rejected(&base_with(river, &format!("{turn}\n{river}")));
    assert_eq!(field, "tree.turn");
    assert!(reason.contains("never plays the turn"), "{reason}");
    let flop = "flop = { bets = [0.33], raises = [], allin = false }";
    assert_eq!(
        rejected_field(river, &format!("{flop}\n{river}")),
        "tree.flop"
    );
    let (field, reason) = rejected(&base_with(river, ""));
    assert_eq!(field, "tree.river");
    assert!(reason.contains("plays the river"), "{reason}");

    // A turn spot needs the turn and the river.
    let turn_board = base_with("Ks9h7d4c2s", "Ks9h7d4c");
    let text = turn_board.replacen(river, &format!("{turn}\n{river}"), 1);
    assert_eq!(read(&text).street(), Street::Turn);
    let (field, reason) = rejected(&turn_board);
    assert_eq!(field, "tree.turn");
    assert!(reason.contains("starts on the turn"), "{reason}");
    assert_eq!(
        rejected(&turn_board.replacen(river, turn, 1)).0,
        "tree.river"
    );

    // A flop spot needs all three.
    let flop_board = text.replacen("Ks9h7d4c", "Ks9h7d", 1);
    assert_eq!(rejected(&flop_board).0, "tree.flop");
    let all = flop_board.replacen(turn, &format!("{flop}\n{turn}"), 1);
    assert_eq!(read(&all).street(), Street::Flop);
}

#[test]
fn the_size_checks_agree_with_the_tree_builder() {
    let base = base();
    let sizings = [
        (street(&[0.5], &[], false), 1.0),
        (street(&[0.01, 50.0], &[1.0001, 2.0], true), 1e-9),
        (street(&[], &[3.0], false), 1.0),
        (street(&[0.0], &[3.0], false), 1.0),
        (street(&[-0.5], &[], false), 1.0),
        (street(&[f64::NAN], &[], false), 1.0),
        (street(&[f64::INFINITY], &[], false), 1.0),
        (street(&[0.5], &[1.0], false), 1.0),
        (street(&[0.5], &[0.5], false), 1.0),
        (street(&[0.5], &[f64::INFINITY], false), 1.0),
        (street(&[0.5], &[3.0], false), 0.0),
        (street(&[0.5], &[3.0], false), 1.0 + 1e-9),
        (street(&[0.5], &[3.0], false), f64::NAN),
    ];
    for (river, allin_threshold) in sizings {
        let spot = Spot {
            tree: TreeSizing {
                max_raises: 1,
                allin_threshold,
                river: Some(river.clone()),
                ..base.tree.clone()
            },
            ..base.clone()
        };
        let sizing = PostflopSizing::uniform(river, 1, allin_threshold);
        let built = PostflopTree::build(TreeSpec::new(Street::River, 2000, 8000, sizing));
        assert_eq!(
            spot.validate().is_ok(),
            built.is_ok(),
            "{:?} and {:?}",
            spot.validate(),
            built.as_ref().map(|tree| tree.counts())
        );
    }
}

#[test]
fn malformed_toml_is_a_toml_error() {
    let repeated_table = base_with("[tree]", "[rake]");
    for text in [
        "format = ",
        "format = 1\nformat = 1",
        "[rake\npercent = 5.0",
        &repeated_table,
    ] {
        assert!(
            matches!(Spot::from_toml(text), Err(SpotError::Toml(_))),
            "{text}"
        );
    }
}

#[test]
fn a_missing_file_is_an_io_error() {
    let error = Spot::load("reference/spots/does-not-exist.toml").unwrap_err();
    assert!(matches!(error, SpotError::Io { .. }));
    assert!(error.to_string().contains("does-not-exist.toml"));
}

#[test]
fn errors_name_the_field_in_their_message() {
    let text = base_with("raises = [3.0]", "raises = [0.9]");
    assert_eq!(
        Spot::from_toml(&text).unwrap_err().to_string(),
        "`tree.river.raises` has 0.9; a raise size must be more than 1"
    );
}

#[test]
fn errors_come_in_the_order_of_the_layout() {
    // A blank name, a bad pot and a missing street: the name comes first.
    let text = base_with("name = \"example\"", "name = \"\"")
        .replacen("pot_bb = 20.0", "pot_bb = 0", 1)
        .replacen("river = {", "turn = {", 1);
    assert_eq!(rejected(&text).0, "name");
    // A table's keys are checked when the table is reached, so a bad board comes before an
    // unknown key in `[tree]`.
    let text = base_with("Ks9h7d4c2s", "Ks9h").replacen("max_raises", "max_rases", 1);
    assert_eq!(rejected(&text).0, "board");
}

// ---------------------------------------------------------------------------------------------
// Validating a spot built in code

#[test]
fn a_spot_built_in_code_gets_the_same_checks() {
    let base = base();
    base.validate().unwrap();
    let river = base.tree.river.clone();
    let cases = [
        (
            Spot {
                name: " ".into(),
                ..base.clone()
            },
            "name",
        ),
        (
            Spot {
                description: String::new(),
                ..base.clone()
            },
            "description",
        ),
        (
            Spot {
                board: cards("Ks9h"),
                ..base.clone()
            },
            "board",
        ),
        (
            Spot {
                board: cards("Ks9h7d4c2s3s"),
                ..base.clone()
            },
            "board",
        ),
        (
            Spot {
                pot: 0,
                ..base.clone()
            },
            "pot_bb",
        ),
        (
            Spot {
                effective_stack: MAX_CHIPS + 1,
                ..base.clone()
            },
            "effective_stack_bb",
        ),
        (
            Spot {
                oop_range: Range::empty(),
                ..base.clone()
            },
            "oop_range",
        ),
        (
            Spot {
                ip_range: range("Ks9s"),
                ..base.clone()
            },
            "ip_range",
        ),
        (
            Spot {
                oop_range: range("AhAd"),
                ip_range: range("AhKh"),
                ..base.clone()
            },
            "ip_range",
        ),
        (
            Spot {
                rake_percent: -0.5,
                ..base.clone()
            },
            "rake.percent",
        ),
        (
            Spot {
                rake_cap: MAX_CHIPS + 1,
                ..base.clone()
            },
            "rake.cap_bb",
        ),
        (
            Spot {
                rake_cap: 0,
                ..base.clone()
            },
            "rake.cap_bb",
        ),
        (
            Spot {
                tree: TreeSizing {
                    allin_threshold: 0.0,
                    ..base.tree.clone()
                },
                ..base.clone()
            },
            "tree.allin_threshold",
        ),
        (
            Spot {
                tree: TreeSizing {
                    turn: river.clone(),
                    ..base.tree.clone()
                },
                ..base.clone()
            },
            "tree.turn",
        ),
        (
            Spot {
                tree: TreeSizing {
                    river: None,
                    ..base.tree.clone()
                },
                ..base.clone()
            },
            "tree.river",
        ),
        (
            Spot {
                tree: TreeSizing {
                    river: Some(street(&[0.5], &[1.0], false)),
                    ..base.tree.clone()
                },
                ..base.clone()
            },
            "tree.river.raises",
        ),
    ];
    for (spot, field) in cases {
        assert_eq!(invalid_field(&spot).0, field);
    }
}

// ---------------------------------------------------------------------------------------------
// Tree files

/// A tree file of all three streets. It is also the example in the module documentation.
const TREE_FILE: &str = r#"max_raises = 0
allin_threshold = 1.0
flop = { bets = [0.5], raises = [], allin = false }
turn = { bets = [0.5], raises = [], allin = false }
river = { bets = [0.5], raises = [], allin = false }
"#;

/// The field and reason of the error that reading `text` as a tree file gives.
fn rejected_tree(text: &str) -> (String, String) {
    match TreeSizing::from_toml(text) {
        Err(SpotError::Invalid { field, reason }) => {
            println!("`{field}` {reason}");
            (field, reason)
        }
        other => panic!("expected an invalid-tree error, got {other:?} for\n{text}"),
    }
}

#[test]
fn a_tree_file_reads_as_the_tree_table_of_a_spot_file() {
    let sizing = TreeSizing::from_toml(TREE_FILE).unwrap();
    let half = street(&[0.5], &[], false);
    assert_eq!(
        sizing,
        TreeSizing {
            max_raises: 0,
            allin_threshold: 1.0,
            flop: Some(half.clone()),
            turn: Some(half.clone()),
            river: Some(half),
        }
    );
    // The same keys under `[tree]` in a spot file give the same sizing.
    let flop_spot = base_with("Ks9h7d4c2s", "Ks9h7d").replacen(
        "max_raises = 1\nallin_threshold = 1.0\nriver = { bets = [0.5, 1.0], raises = [3.0], allin = false }\n",
        TREE_FILE,
        1,
    );
    assert_eq!(read(&flop_spot).tree, sizing);
    // So does the base spot's tree, and a tree file written as tables.
    let river_only = "max_raises = 1\nallin_threshold = 1.0\n\n[river]\nbets = [0.5, 1.0]\nraises = [3.0]\nallin = false\n";
    assert_eq!(TreeSizing::from_toml(river_only).unwrap(), base().tree);
}

#[test]
fn the_module_documentation_shows_the_tree_file() {
    let source = include_str!("../spot.rs");
    let doc: String = source
        .lines()
        .skip_while(|line| *line != "//! # Tree files")
        .skip_while(|line| *line != "//! ```toml")
        .skip(1)
        .take_while(|line| *line != "//! ```")
        .map(|line| format!("{}\n", line.trim_start_matches("//!").trim_start()))
        .collect();
    assert_eq!(doc, TREE_FILE);
}

#[test]
fn a_tree_file_sizes_the_streets_from_its_first_on() {
    let flop = "flop = { bets = [0.5], raises = [], allin = false }\n";
    let turn = "turn = { bets = [0.5], raises = [], allin = false }\n";
    let river = "river = { bets = [0.5], raises = [], allin = false }\n";
    let turn_and_river = TREE_FILE.replacen(flop, "", 1);
    let sizing = TreeSizing::from_toml(&turn_and_river).unwrap();
    assert!(sizing.flop.is_none() && sizing.turn.is_some() && sizing.river.is_some());
    let river_alone = turn_and_river.replacen(turn, "", 1);
    assert!(TreeSizing::from_toml(&river_alone).unwrap().turn.is_none());

    let (field, reason) = rejected_tree(&TREE_FILE.replacen(turn, "", 1));
    assert_eq!(field, "turn");
    assert!(reason.contains("sizes the flop"), "{reason}");
    let (field, reason) = rejected_tree(&TREE_FILE.replacen(river, "", 1));
    assert_eq!(field, "river");
    assert!(reason.contains("sizes the turn"), "{reason}");
    let (field, reason) = rejected_tree(&river_alone.replacen(river, "", 1));
    assert_eq!(field, "river");
    assert!(reason.contains("every spot plays the river"), "{reason}");
}

#[test]
fn a_tree_file_gets_the_checks_of_the_tree_table_with_its_own_field_names() {
    for (from, to, field) in [
        (
            "allin_threshold = 1.0",
            "allin_threshold = 0.0",
            "allin_threshold",
        ),
        ("max_raises = 0", "max_raises = -1", "max_raises"),
        ("max_raises = 0\n", "", "max_raises"),
        ("flop = { bets = [0.5]", "flop = { bets = []", "flop.bets"),
        (
            "turn = { bets = [0.5]",
            "turn = { bets = [33.0]",
            "turn.bets",
        ),
        (
            "raises = [], allin = false }\n",
            "raises = [0.9], allin = false }\n",
            "flop.raises",
        ),
        ("allin = false }\n", "allin = \"no\" }\n", "flop.allin"),
        (
            "river = { bets",
            "river = { sizes = [0.5], bets",
            "river.sizes",
        ),
        ("max_raises", "max_rases", "max_rases"),
    ] {
        assert!(TREE_FILE.contains(from), "test setup: {from:?}");
        assert_eq!(
            rejected_tree(&TREE_FILE.replacen(from, to, 1)).0,
            field,
            "{to}"
        );
    }
    // A spot file's keys, or the `[tree]` line kept, are named as such.
    let (field, reason) = rejected_tree(BASE);
    assert_eq!(field, "board");
    assert!(reason.contains("not a tree-file key"), "{reason}");
    let (field, reason) = rejected_tree(&format!("[tree]\n{TREE_FILE}"));
    assert_eq!(field, "tree");
    assert!(reason.contains("without the `[tree]` line"), "{reason}");
    assert!(matches!(
        TreeSizing::from_toml("max_raises = "),
        Err(SpotError::Toml(_))
    ));
    let error = TreeSizing::load("reference/trees/does-not-exist.toml").unwrap_err();
    assert!(matches!(error, SpotError::Io { .. }));
    assert!(error.to_string().contains("does-not-exist.toml"));
}
