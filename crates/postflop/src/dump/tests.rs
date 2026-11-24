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
    let tree = spot.build_tree().unwrap();
    let decisions = decision_nodes(&tree);
    let players: Vec<u32> = (0..tree.nodes().len() as u32)
        .filter(|&n| matches!(tree.node(n), Node::Player { .. }))
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
            v["version"] = json!(2);
            v["surprise"] = json!(1);
        }),
        "version",
        "reads version 1 only",
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
