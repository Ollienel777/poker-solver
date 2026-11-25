use serde_json::{Value, json};

use super::*;
use crate::solve::{SolveOptions, solve};
use crate::spot::Spot;

fn load(name: &str) -> Spot {
    let path = format!(
        "{}/../../reference/spots/river/{name}.toml",
        env!("CARGO_MANIFEST_DIR")
    );
    Spot::load(path).unwrap()
}

/// The dump of the toy game after a few iterations, as JSON. Its nodes are the root, IP's
/// node after a check, OOP's facing a bet after checking, and IP's facing a bet.
fn toy_text() -> String {
    let options = SolveOptions {
        target: None,
        max_iterations: 5,
        ..SolveOptions::default()
    };
    let solution = solve(&load("toy_polarized"), &options, |_| {}).unwrap();
    SolutionDump::from_solution(&solution).to_json()
}

/// The toy dump's JSON with `change` made to it, read back: the error, which there must be.
fn error_after(change: impl FnOnce(&mut Value)) -> DumpError {
    let mut value: Value = serde_json::from_str(&toy_text()).unwrap();
    change(&mut value);
    SolutionDump::from_json(&value.to_string()).unwrap_err()
}

fn assert_invalid(error: DumpError, field: &str, reason: &str) {
    match error {
        DumpError::Invalid {
            field: found,
            reason: why,
        } => {
            assert_eq!(found, field, "{why}");
            assert!(why.contains(reason), "`{field}`: {why}");
        }
        other => panic!("`{field}`: {other}"),
    }
}

/// The index in a dump's JSON of the node at `path`.
fn index_of(value: &Value, path: Value) -> usize {
    value["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .position(|node| node["path"] == path)
        .unwrap()
}

#[test]
fn a_dump_reads_back_as_written() {
    // Every number but the probabilities reads back exactly. A probability, an f32, is written
    // as the shortest decimal that reads back as that f32, and so reads back as the f64
    // nearest that decimal: equal once rounded to f32.
    let solution = solve(&load("dry_ip_polarized"), &SolveOptions::default(), |_| {}).unwrap();
    let mut dump = SolutionDump::from_solution(&solution);
    dump.spot.name = Some("dry_ip_polarized".to_string());
    let text = dump.to_json();
    let back = SolutionDump::from_json(&text).unwrap();
    assert_eq!(back.solver, dump.solver);
    assert_eq!(back.spot, dump.spot);
    assert_eq!(back.values, dump.values);
    assert_eq!(back.nodes.len(), dump.nodes.len());
    let narrow = |row: &[f64]| -> Vec<u32> { row.iter().map(|&p| (p as f32).to_bits()).collect() };
    for (read, written) in back.nodes.iter().zip(&dump.nodes) {
        assert_eq!(read.path, written.path);
        assert_eq!(read.player, written.player);
        assert_eq!(read.actions, written.actions);
        assert_eq!(read.ev, written.ev);
        for ((a, x), (b, y)) in read.strategy.iter().zip(&written.strategy) {
            assert_eq!(a, b);
            assert_eq!(narrow(x), narrow(y));
        }
    }
    // Written again, the text is the same.
    assert_eq!(back.to_json(), text);
    // The layout: the keys in order.
    assert!(text.starts_with("{\n  \"format\": \"pk-solution-dump\",\n  \"version\": 1,\n"));
    let order = ["\"solver\"", "\"spot\"", "\"values\"", "\"nodes\""];
    let at: Vec<usize> = order.iter().map(|key| text.find(key).unwrap()).collect();
    assert!(at.windows(2).all(|w| w[0] < w[1]), "{at:?}");
}

#[test]
fn the_example_in_the_module_documentation_is_a_valid_dump() {
    let source = include_str!("../dump.rs");
    let open = "//! ```json\n";
    let start = source.find(open).unwrap() + open.len();
    let end = start + source[start..].find("//! ```\n").unwrap();
    let text: Vec<&str> = source[start..end]
        .lines()
        .map(|line| line.strip_prefix("//!").unwrap().trim_start_matches(' '))
        .collect();
    let dump = SolutionDump::from_json(&text.join("\n")).unwrap();
    assert_eq!(dump.nodes.len(), 2);
    assert_eq!(dump.nodes[1].path, [DumpAction::Bet(500)]);
    assert_eq!(dump.spot.hands[1].len(), 2);
}

#[test]
fn our_actions_are_named_by_kind_and_street_total() {
    // On the spot with all-in at every decision: an all-in is a bet with no bet to face and a
    // raise facing one, a call's amount is the bet it calls, and the amounts are the street
    // totals `reference/README.md` lists for the spot.
    let spot = load("deep_high_spr");
    let game = PostflopGame::new(&spot).unwrap();
    let decisions = decisions(&game);
    let players: Vec<u32> = (0..game.tree().nodes().len() as u32)
        .filter(|&n| matches!(game.tree().node(n), pk_game::Node::Player { .. }))
        .collect();
    let nodes: Vec<u32> = decisions.iter().map(|d| d.node).collect();
    assert_eq!(nodes, players, "every player node, in the tree's order");
    assert!(decisions[0].path.is_empty());
    assert_eq!(decisions[0].player, Player::Oop);
    let mut bets = HashSet::new();
    let mut raises = HashSet::new();
    for decision in &decisions {
        // On the river the players take turns, so the last action is the opponent's.
        let facing = decision.path.last().and_then(|a| a.amount());
        for action in &decision.actions {
            match *action {
                DumpAction::Bet(amount) => {
                    assert_eq!(facing, None, "{decision:?}");
                    bets.insert(amount);
                }
                DumpAction::Raise(amount) => {
                    assert!(facing.is_some_and(|f| amount > f), "{decision:?}");
                    raises.insert(amount);
                }
                DumpAction::Call(amount) => assert_eq!(Some(amount), facing, "{decision:?}"),
                DumpAction::Fold => assert!(facing.is_some(), "{decision:?}"),
                DumpAction::Check => assert_eq!(facing, None, "{decision:?}"),
                DumpAction::Deal(_) => panic!("a river node offers no deal: {decision:?}"),
            }
        }
    }
    let sorted = |set: HashSet<Chips>| {
        let mut list: Vec<Chips> = set.into_iter().collect();
        list.sort();
        list
    };
    let stack = spot.effective_stack;
    assert_eq!(sorted(bets), [275, 550, stack]);
    assert_eq!(sorted(raises), [825, 1650, 2475, 4950, 7425, stack]);
}

#[test]
fn the_format_and_version_are_checked_first() {
    assert_invalid(
        error_after(|v| v["format"] = json!("pk-solution")),
        "format",
        "must be `pk-solution-dump`",
    );
    assert_invalid(
        error_after(|v| {
            v["version"] = json!(3);
            v["surprise"] = json!(1);
        }),
        "version",
        "reads versions 1 and 2 only",
    );
    assert_invalid(
        error_after(|v| {
            v.as_object_mut().unwrap().remove("version");
        }),
        "version",
        "is missing",
    );
    assert_invalid(
        error_after(|v| v["surprise"] = json!(1)),
        "surprise",
        "is not a key of a dump",
    );
}

#[test]
fn text_that_is_not_json_or_repeats_a_key_is_refused() {
    assert!(matches!(
        SolutionDump::from_json("{\"format\": "),
        Err(DumpError::Json(_))
    ));
    let text = toy_text().replacen("\"version\": 1,", "\"version\": 1,\n  \"version\": 1,", 1);
    let error = SolutionDump::from_json(&text).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("the key `version` appears twice"),
        "{error}"
    );
}

type Change = Box<dyn FnOnce(&mut Value)>;

#[test]
fn every_fault_of_structure_or_type_names_its_field() {
    let cases: Vec<(Change, &str, &str)> = vec![
        (
            Box::new(|v| {
                v.as_object_mut().unwrap().remove("nodes");
            }),
            "nodes",
            "is missing",
        ),
        (
            Box::new(|v| v["solver"]["name"] = json!(3)),
            "solver.name",
            "is a number; it must be a string",
        ),
        (
            Box::new(|v| v["solver"]["iterations"] = json!(2.5)),
            "solver.iterations",
            "a whole number",
        ),
        (
            Box::new(|v| v["spot"]["pot"] = json!(1000.5)),
            "spot.pot",
            "a whole number of chips",
        ),
        (
            Box::new(|v| v["spot"]["pot"] = json!(-1000)),
            "spot.pot",
            "a whole number of chips",
        ),
        (
            Box::new(|v| v["spot"]["board"] = json!("Ks9h6d3c2x")),
            "spot.board",
            "is not a valid board",
        ),
        (
            Box::new(|v| v["spot"]["rake"]["cut"] = json!(5)),
            "spot.rake.cut",
            "is not a key of `spot.rake`",
        ),
        (
            Box::new(|v| v["spot"]["hands"]["oop"]["Xx9d"] = json!(1.0)),
            "spot.hands.oop.Xx9d",
            "is not a hand",
        ),
        (
            Box::new(|v| v["nodes"][0]["player"] = json!("button")),
            "nodes[0].player",
            "must be `oop` or `ip`",
        ),
        (
            Box::new(|v| v["nodes"][0]["actions"][1] = json!({"kind": "allin", "amount": 2000})),
            "nodes[0].actions[1].kind",
            "An all-in is a bet or a raise of the whole stack",
        ),
        (
            Box::new(|v| v["nodes"][0]["actions"][0] = json!({"kind": "check", "amount": 0})),
            "nodes[0].actions[0].amount",
            "a check has no amount",
        ),
        (
            Box::new(|v| v["nodes"][0]["actions"][1] = json!({"kind": "bet"})),
            "nodes[0].actions[1].amount",
            "the acting player's total for the street",
        ),
        (
            Box::new(|v| v["nodes"][0]["strategy"]["9s9d"] = json!("0.5")),
            "nodes[0].strategy.9s9d",
            "an array of probabilities",
        ),
    ];
    for (change, field, reason) in cases {
        assert_invalid(error_after(change), field, reason);
    }
}

#[test]
fn every_fault_of_value_names_its_field() {
    let cases: Vec<(Change, &str, &str)> = vec![
        (
            Box::new(|v| v["solver"]["name"] = json!(" ")),
            "solver.name",
            "is blank",
        ),
        (
            Box::new(|v| v["spot"]["board"] = json!("Ks9h6d3c")),
            "spot.board",
            "river spots only",
        ),
        (
            Box::new(|v| v["spot"]["pot"] = json!(0)),
            "spot.pot",
            "it must be more than 0",
        ),
        (
            Box::new(|v| v["spot"]["rake"]["percent"] = json!(100)),
            "spot.rake.percent",
            "less than 100",
        ),
        (
            Box::new(|v| v["spot"]["hands"]["oop"]["9s9d"] = json!(0.0)),
            "spot.hands.oop.9s9d",
            "has weight 0",
        ),
        (
            Box::new(|v| v["spot"]["hands"]["oop"]["Ks9s"] = json!(1.0)),
            "spot.hands.oop.Ks9s",
            "holds a card of the board",
        ),
        (
            // The same hand with its cards in the other order.
            Box::new(|v| v["spot"]["hands"]["oop"]["9d9s"] = json!(1.0)),
            "spot.hands.oop.9s9d",
            "is listed twice",
        ),
        (Box::new(|v| v["nodes"] = json!([])), "nodes", "is empty"),
        (
            Box::new(|v| {
                let copy = v["nodes"][0].clone();
                v["nodes"].as_array_mut().unwrap().push(copy);
            }),
            "nodes[4].path",
            "the path of `nodes[0]` too",
        ),
        (
            Box::new(|v| v["nodes"][0]["actions"][1] = json!({"kind": "check"})),
            "nodes[0].actions[1]",
            "comes twice",
        ),
        (
            Box::new(|v| v["nodes"][0]["actions"][1] = json!({"kind": "bet", "amount": 0})),
            "nodes[0].actions[1].amount",
            "more than 0",
        ),
        (
            Box::new(|v| v["nodes"][0]["strategy"]["9s9d"] = json!([0.5, 0.4])),
            "nodes[0].strategy.9s9d",
            "sums to 0.9",
        ),
        (
            Box::new(|v| v["nodes"][0]["strategy"]["9s9d"] = json!([1.25, -0.25])),
            "nodes[0].strategy.9s9d[0]",
            "a probability is from 0 to 1",
        ),
        (
            Box::new(|v| v["nodes"][0]["strategy"]["9s9d"] = json!([1.0])),
            "nodes[0].strategy.9s9d",
            "has 1 probabilities, and the node offers 2 actions",
        ),
        (
            Box::new(|v| {
                let rows = v["nodes"][0]["strategy"].as_object_mut().unwrap();
                rows.remove("9s9d");
            }),
            "nodes[0].strategy",
            "has no row for 9s9d, of OOP's hands in `spot.hands.oop` (the root)",
        ),
        (
            Box::new(|v| v["nodes"][0]["strategy"]["KhQh"] = json!([0.5, 0.5])),
            "nodes[0].strategy.KhQh",
            "is not one of OOP's hands",
        ),
        (
            Box::new(|v| {
                v["nodes"][1].as_object_mut().unwrap().remove("ev");
            }),
            "nodes[1].ev",
            "give them at every node or at none",
        ),
        (
            Box::new(|v| {
                v["nodes"][0]["ev"].as_object_mut().unwrap().remove("9s9d");
            }),
            "nodes[0].ev",
            "has no row for 9s9d",
        ),
    ];
    for (change, field, reason) in cases {
        assert_invalid(error_after(change), field, reason);
    }
}

#[test]
fn optional_keys_may_be_left_out_or_null() {
    let mut value: Value = serde_json::from_str(&toy_text()).unwrap();
    value["values"] = Value::Null;
    value["solver"] = json!({"name": "x"});
    for node in value["nodes"].as_array_mut().unwrap() {
        node.as_object_mut().unwrap().remove("ev");
    }
    let dump = SolutionDump::from_json(&value.to_string()).unwrap();
    assert_eq!(dump.values, None);
    assert_eq!(dump.solver.iterations, None);
    assert!(dump.nodes.iter().all(|node| node.ev.is_none()));
    // Players and kinds in any case, and whole amounts written with a fraction of zero.
    let mut value: Value = serde_json::from_str(&toy_text()).unwrap();
    value["spot"]["pot"] = json!(1000.0);
    value["nodes"][0]["player"] = json!("OOP");
    let check = index_of(&value, json!([{"kind": "check"}]));
    value["nodes"][check]["path"] = json!([{"kind": "CHECK"}]);
    let dump = SolutionDump::from_json(&value.to_string()).unwrap();
    assert_eq!(dump.spot.pot, 1000);
    assert_eq!(dump.nodes[0].player, Player::Oop);
    assert_eq!(dump.nodes[check].path, [DumpAction::Check]);
}

#[test]
fn a_dump_built_in_code_is_validated_as_text_is() {
    let mut dump = SolutionDump::from_json(&toy_text()).unwrap();
    assert!(dump.validate().is_ok());
    dump.nodes[0].strategy[0].1 = vec![f64::NAN, 1.0];
    let field = format!("nodes[0].strategy.{}[0]", dump.nodes[0].strategy[0].0);
    assert_invalid(dump.validate().unwrap_err(), &field, "is NaN");
}

// ---------------------------------------------------------------------------------------------
// Version 2

fn load_data(name: &str) -> Spot {
    Spot::load(format!(
        "{}/tests/data/{name}.toml",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

/// A short solve of a spot of `tests/data`.
fn short(name: &str, iterations: u64) -> Solution {
    let options = SolveOptions {
        target: None,
        max_iterations: iterations,
        check_every: iterations.max(1),
        ..SolveOptions::default()
    };
    solve(&load_data(name), &options, |_| {}).unwrap()
}

/// The complete dump of the turn spot after a few iterations, as JSON. Its board is
/// `Kh9c7d2s`, its pot 1,000 and its stacks 1,500; the turn offers a bet of 500, a raise to
/// 1,500 and all-in.
fn turn_text() -> String {
    SolutionDump::from_solution(&short("turn_small", 3)).to_json()
}

/// The turn dump's JSON with `change` made to it, read back: the error, which there must be.
fn turn_error(change: impl FnOnce(&mut Value)) -> DumpError {
    let mut value: Value = serde_json::from_str(&turn_text()).unwrap();
    change(&mut value);
    SolutionDump::from_json(&value.to_string()).unwrap_err()
}

/// Asserts that `error` names a field that ends with `field`, for a reason that holds `reason`.
fn assert_invalid_at(error: DumpError, field: &str, reason: &str) {
    match error {
        DumpError::Invalid {
            field: found,
            reason: why,
        } => {
            assert!(found.ends_with(field), "`{found}` for `{field}`: {why}");
            assert!(why.contains(reason), "`{field}`: {why}");
        }
        other => panic!("`{field}`: {other}"),
    }
}

fn check_step() -> Value {
    json!({"kind": "check"})
}

fn bet(amount: u32) -> Value {
    json!({"kind": "bet", "amount": amount})
}

fn call(amount: u32) -> Value {
    json!({"kind": "call", "amount": amount})
}

fn deal(card: &str) -> Value {
    json!({"kind": "deal", "card": card})
}

/// The index of the river node after both check the turn and the 3c falls.
fn river_node(value: &Value) -> usize {
    index_of(value, json!([check_step(), check_step(), deal("3c")]))
}

#[test]
fn a_flop_or_turn_dump_is_version_2_with_complete_after_the_spot() {
    let text = turn_text();
    assert!(text.starts_with("{\n  \"format\": \"pk-solution-dump\",\n  \"version\": 2,\n"));
    let order = [
        "\"solver\"",
        "\"spot\"",
        "\"complete\": true",
        "\"values\"",
        "\"nodes\"",
    ];
    let at: Vec<usize> = order.iter().map(|key| text.find(key).unwrap()).collect();
    assert!(at.windows(2).all(|w| w[0] < w[1]), "{at:?}");
    assert!(text.contains("{\"kind\": \"deal\", \"card\": \"3c\"}"));
    let dump = SolutionDump::from_json(&text).unwrap();
    assert_eq!(
        (dump.version(), dump.streets(), dump.complete),
        (2, 2, true)
    );
    assert_eq!(dump.to_json(), text);
    // A river dump has no `complete`, and is version 1.
    assert!(!toy_text().contains("complete"));
}

#[test]
fn the_version_2_example_in_the_module_documentation_is_a_valid_dump() {
    let source = include_str!("../dump.rs");
    let open = "//! ```json\n";
    let first = source.find(open).unwrap() + open.len();
    let start = first + source[first..].find(open).unwrap() + open.len();
    let end = start + source[start..].find("//! ```\n").unwrap();
    let text: Vec<&str> = source[start..end]
        .lines()
        .map(|line| line.strip_prefix("//!").unwrap().trim_start_matches(' '))
        .collect();
    let dump = SolutionDump::from_json(&text.join("\n")).unwrap();
    assert_eq!(
        (dump.version(), dump.streets(), dump.complete),
        (2, 2, true)
    );
    assert_eq!(dump.nodes.len(), 3);
    let river = &dump.nodes[2];
    assert_eq!(river.dealt(), CardSet::single("9s".parse().unwrap()));
    assert_eq!(river.strategy.len(), 2);
    assert_eq!(river.ev.as_ref().unwrap()[1].1, None);
}

#[test]
fn every_fault_of_a_version_2_header_names_its_field() {
    let cases: Vec<(Change, &str, &str)> = vec![
        (
            Box::new(|v| {
                v.as_object_mut().unwrap().remove("complete");
            }),
            "complete",
            "is missing; a version 2 dump says whether it lists every decision node",
        ),
        (
            Box::new(|v| v["complete"] = json!("yes")),
            "complete",
            "is a string; it must be `true` or `false`",
        ),
        (
            Box::new(|v| {
                v["version"] = json!(1);
                v.as_object_mut().unwrap().remove("complete");
            }),
            "spot.board",
            "has 4 cards; version 1 holds river spots only",
        ),
        (
            Box::new(|v| v["spot"]["board"] = json!("Kh9c7d2s3c")),
            "spot.board",
            "has 5 cards; version 2 holds flop and turn spots",
        ),
    ];
    for (change, field, reason) in cases {
        assert_invalid(turn_error(change), field, reason);
    }
    // A river dump has no `complete` key.
    assert_invalid(
        error_after(|v| v["complete"] = json!(true)),
        "complete",
        "is not a key of a dump",
    );
}

#[test]
fn every_fault_of_a_deal_names_its_field() {
    let cases: Vec<(Change, &str, &str)> = vec![
        (
            Box::new(|v| {
                let at = river_node(v);
                v["nodes"][at]["path"][2]["amount"] = json!(0);
            }),
            "path[2].amount",
            "a deal has no amount",
        ),
        (
            Box::new(|v| {
                let at = river_node(v);
                v["nodes"][at]["path"][2]
                    .as_object_mut()
                    .unwrap()
                    .remove("card");
            }),
            "path[2].card",
            "is missing; a deal names the card dealt",
        ),
        (
            Box::new(|v| {
                let at = river_node(v);
                v["nodes"][at]["path"][2]["card"] = json!("3x");
            }),
            "path[2].card",
            "is `3x`, which is not a card",
        ),
        (
            Box::new(|v| {
                let at = river_node(v);
                v["nodes"][at]["path"][0]["card"] = json!("3c");
            }),
            "path[0].card",
            "only a deal has a card",
        ),
        (
            Box::new(|v| {
                let at = river_node(v);
                v["nodes"][at]["path"][0] = json!({"kind": "allin", "amount": 1500});
            }),
            "path[0].kind",
            "or in a path `deal`",
        ),
        (
            Box::new(|v| {
                let at = river_node(v);
                v["nodes"][at]["actions"][0] = deal("3c");
            }),
            "actions[0].kind",
            "is `deal`, but a deal is not an action a player takes",
        ),
    ];
    for (change, field, reason) in cases {
        assert_invalid_at(turn_error(change), field, reason);
    }
}

#[test]
fn a_path_that_breaks_the_betting_is_refused_where_it_breaks() {
    // Each path replaces that of the river node after both check and the 3c falls, so that
    // only the path is wrong.
    let x = check_step;
    let fold = json!({"kind": "fold"});
    let raise = |amount: u32| json!({"kind": "raise", "amount": amount});
    let cases: Vec<(Value, &str, &str)> = vec![
        (
            json!([deal("3c")]),
            "path[0]",
            "a path starts with the first action on the spot's turn",
        ),
        (
            json!([x(), deal("3c"), x()]),
            "path[1]",
            "is the deal of the 3c, but the turn is not over",
        ),
        (
            json!([x(), x(), x()]),
            "path[2]",
            "the turn closed at `path[1]` with the check behind",
        ),
        (
            json!([x(), x(), deal("Kh")]),
            "path[2]",
            "deals the Kh, which is on the board",
        ),
        (
            json!([x(), x()]),
            "path",
            "ends with the check behind, which closes the turn",
        ),
        (
            json!([bet(500), fold]),
            "path",
            "ends where the hand is over, after a fold",
        ),
        (
            json!([bet(500), fold, deal("3c")]),
            "path[2]",
            "after the end of the hand at `path[1]`, a fold",
        ),
        (
            json!([bet(1500), call(1500)]),
            "path",
            "ends with the call of an all-in",
        ),
        (
            json!([bet(1500), call(1500), deal("3c")]),
            "path[2]",
            "after the call of an all-in at `path[1]`",
        ),
        (json!([fold]), "path[0]", "is a fold, but OOP faces no bet"),
        (
            json!([bet(500), x()]),
            "path[1]",
            "is a check, but IP faces a bet of 500",
        ),
        (
            json!([bet(500), call(400)]),
            "path[1]",
            "is a call of 400, but the bet faced is 500",
        ),
        (
            json!([x(), call(500)]),
            "path[1]",
            "is a call, but IP faces no bet",
        ),
        (
            json!([bet(500), bet(1000)]),
            "path[1]",
            "is a bet, but IP faces a bet of 500",
        ),
        (
            json!([bet(500), raise(500)]),
            "path[1]",
            "is a raise to 500, which is not above the bet faced, 500",
        ),
        (
            json!([x(), raise(500)]),
            "path[1]",
            "is a raise, but IP faces no bet",
        ),
        (
            json!([bet(1600)]),
            "path[0]",
            "is a bet of 1600, more than the 1500 chips OOP had left when the turn began",
        ),
        (
            json!([bet(500), call(500), deal("3c"), bet(1500)]),
            "path[3]",
            "is a bet of 1500, more than the 1000 chips OOP had left when the river began \
             (`spot.effective_stack` less the 500 it put in on earlier streets)",
        ),
    ];
    for (path, field, reason) in cases {
        let error = turn_error(|v| {
            let at = river_node(v);
            v["nodes"][at]["path"] = path.clone();
        });
        assert_invalid_at(error, &format!("].{field}"), reason);
    }
}

#[test]
fn a_node_whose_actions_break_the_betting_is_refused() {
    let cases: Vec<(Value, &str)> = vec![
        (
            json!([{"kind": "fold"}, check_step()]),
            "offers a fold, but OOP faces no bet",
        ),
        (
            json!([check_step(), bet(2000)]),
            "offers a bet of 2000, more than the 1500 chips OOP had left",
        ),
        (
            json!([check_step(), call(500)]),
            "offers a call, but OOP faces no bet",
        ),
    ];
    for (actions, reason) in cases {
        let error = turn_error(|v| {
            let width = actions.as_array().unwrap().len();
            v["nodes"][0]["actions"] = actions.clone();
            let rows = v["nodes"][0]["strategy"].as_object_mut().unwrap();
            for row in rows.values_mut() {
                *row = json!(vec![1.0 / width as f64; width]);
            }
        });
        match error {
            DumpError::Invalid { field, reason: why } => {
                assert!(field.starts_with("nodes[0].actions["), "{field}: {why}");
                assert!(why.contains(reason), "{actions}: {why}");
            }
            other => panic!("{actions}: {other}"),
        }
    }
}

#[test]
fn a_dump_not_closed_under_prefixes_or_whose_complete_is_wrong_is_refused() {
    assert_invalid(
        turn_error(|v| {
            v["nodes"].as_array_mut().unwrap().remove(0);
        }),
        "nodes",
        "has no node at the root",
    );
    // Without IP's node after a check, every node below it leads through a missing node.
    let error = turn_error(|v| {
        let at = index_of(v, json!([check_step()]));
        v["nodes"].as_array_mut().unwrap().remove(at);
    });
    assert_invalid_at(
        error,
        "].path",
        "leads through [check], the decision node where",
    );
    // The turn alone, said to be complete; the river kept, said not to be.
    assert_invalid(
        turn_error(|v| {
            v["nodes"].as_array_mut().unwrap().retain(|node| {
                let path = node["path"].as_array().unwrap();
                path.iter().all(|step| step["kind"] != "deal")
            });
        }),
        "complete",
        "is true, but no path deals more than 0 cards, so the dump holds the first 1 of the 2 \
         streets of a turn spot",
    );
    assert_invalid(
        turn_error(|v| v["complete"] = json!(false)),
        "complete",
        "deals 1 card, so the dump holds every street of the turn spot",
    );
}

#[test]
fn a_card_dealt_twice_on_a_path_is_refused() {
    // On a flop spot, the river may not be the turn card again.
    let mut dump = SolutionDump::from_solution(&short("flop_tiny", 0));
    let turn: Card = "Ah".parse().unwrap();
    let at = dump
        .nodes
        .iter()
        .position(|n| n.street_index() == 2 && n.path[2] == DumpAction::Deal(turn))
        .unwrap();
    let river = dump.nodes[at]
        .path
        .iter()
        .rposition(|step| step.is_deal())
        .unwrap();
    dump.nodes[at].path[river] = DumpAction::Deal(turn);
    assert_invalid(
        dump.validate().unwrap_err(),
        &format!("nodes[{at}].path[{river}]"),
        "deals the Ah, which was dealt at `path[2]`",
    );
}

#[test]
fn a_partial_dump_reads_back_as_written() {
    let dump = SolutionDump::from_solution_streets(&short("turn_small", 3), 1);
    assert!(!dump.complete);
    assert_eq!(dump.streets(), 1);
    assert!(dump.nodes.iter().all(|n| n.dealt().is_empty()));
    let text = dump.to_json();
    assert!(text.contains("\n  \"complete\": false,\n"));
    let back = SolutionDump::from_json(&text).unwrap();
    assert_eq!(back.to_json(), text);
}

#[test]
#[should_panic(expected = "holds 1 to 2 of them, not 3")]
fn a_dump_of_more_streets_than_the_spot_plays_is_refused() {
    SolutionDump::from_solution_streets(&short("turn_small", 0), 3);
}
