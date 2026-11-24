use pk_game::Node;

use super::*;
use crate::solve::{Solution, SolveOptions, solve};

fn load(name: &str) -> Spot {
    let path = format!(
        "{}/../../reference/spots/river/{name}.toml",
        env!("CARGO_MANIFEST_DIR")
    );
    Spot::load(path).unwrap()
}

fn solved(spot: &Spot, iterations: u64) -> Solution {
    let options = SolveOptions {
        target: None,
        max_iterations: iterations,
        check_every: iterations.max(1),
        ..SolveOptions::default()
    };
    solve(spot, &options, |_| {}).unwrap()
}

fn mapped(solution: &Solution) -> MappedSolution {
    MappedSolution::from_solution(solution)
}

#[test]
fn the_defaults_are_the_tolerance_proposed_for_the_gate() {
    let t = Tolerances::default();
    assert_eq!(
        (t.exploitability, t.value, t.frequency, t.min_reach),
        (0.3, 0.2, 0.02, 0.01)
    );
    assert_eq!((t.ev, t.reported_ev), (5.0, 0.1));
}

#[test]
fn a_solution_compared_with_itself_passes_with_nothing_apart() {
    let spot = load("dry_ip_polarized");
    let solution = solved(&spot, 300);
    let comparison = compare_mapped(
        &mapped(&solution),
        &mapped(&solution),
        &Tolerances::default(),
    );
    assert!(comparison.passes(), "{comparison}");
    assert_eq!(comparison.pot, spot.pot);
    let players = solution
        .tree()
        .nodes()
        .iter()
        .filter(|n| matches!(n, Node::Player { .. }))
        .count();
    assert_eq!(comparison.nodes.len(), players);
    assert_eq!(comparison.root_player, Player::Oop);
    let summary = comparison.frequencies;
    assert_eq!(summary.weighted_mean, 0.0);
    assert_eq!(summary.largest.map(|l| l.difference), Some(0.0));
    assert!(summary.checked >= 1 && summary.checked <= comparison.nodes.len());
    for node in &comparison.nodes {
        assert_eq!(node.differences().map(|d| d.iter().sum::<f64>()), Some(0.0));
    }
}

#[test]
fn node_and_class_frequencies_are_each_strategys_range_view() {
    // Both sides through the same functions: the range view of each strategy, at every node;
    // and at the root, each class's frequency is its combos' mean, by range weight.
    let spot = load("four_flush");
    let (a, b) = (solved(&spot, 40), solved(&spot, 400));
    let comparison = compare_mapped(&mapped(&a), &mapped(&b), &Tolerances::default());
    for node in &comparison.nodes {
        assert_eq!(node.frequencies[0], a.range_view_frequencies(node.node));
        assert_eq!(node.frequencies[1], b.range_view_frequencies(node.node));
    }
    let oop = a.hands(Player::Oop);
    let total: f64 = oop.weights().iter().sum();
    for class in &comparison.classes {
        let hands: Vec<usize> = (0..oop.len())
            .filter(|&h| HandClass::of(oop.combos()[h]) == class.class)
            .collect();
        assert_eq!(hands.len(), class.combos);
        let weight: f64 = hands.iter().map(|&h| oop.weights()[h]).sum();
        assert!((class.share - weight / total).abs() < 1e-12);
        for (side, solution) in [&a, &b].into_iter().enumerate() {
            let bets: f64 = hands
                .iter()
                .map(|&h| oop.weights()[h] * f64::from(solution.hand_strategy(0, h).unwrap()[1]))
                .sum();
            assert!(
                (class.frequencies[side][1] - bets / weight).abs() < 1e-12,
                "{}",
                class.class
            );
        }
    }
    let shares: f64 = comparison.classes.iter().map(|c| c.share).sum();
    assert!((shares - 1.0).abs() < 1e-12);
}

#[test]
fn the_reach_of_a_node_is_the_chance_that_a_deal_plays_to_it() {
    // At the root it is 1; after the root, it is shared out among the root's actions.
    let spot = load("wet_wide_two_bets");
    let solution = solved(&spot, 100);
    let comparison = compare_mapped(
        &mapped(&solution),
        &mapped(&solution),
        &Tolerances::default(),
    );
    let root = &comparison.nodes[0];
    assert!((root.reach[0] - 1.0).abs() < 1e-12);
    let frequencies = solution.action_frequencies(0).unwrap();
    for (action, frequency) in frequencies.iter().enumerate() {
        let child = solution.tree().node(0).children()[action];
        if let Some(node) = comparison.nodes.iter().find(|n| n.node == child) {
            assert!(
                (node.reach[0] - frequency).abs() < 1e-12,
                "{}: {} against {frequency}",
                PathText(&node.path),
                node.reach[0]
            );
        }
    }
}

#[test]
fn a_poor_strategy_fails_and_the_text_says_where() {
    let spot = load("dry_ip_polarized");
    let (good, poor) = (solved(&spot, 300), solved(&spot, 0));
    let comparison = compare_mapped(&mapped(&good), &mapped(&poor), &Tolerances::default());
    assert!(!comparison.passes());
    let failed: Vec<Check> = comparison.failures.iter().map(Failure::check).collect();
    assert!(failed.contains(&Check::Exploitability));
    assert!(failed.contains(&Check::Frequency));
    let text = comparison.to_string();
    for expected in [
        "Comparison of A = pk-postflop (300 iterations) and B = pk-postflop (0 iterations): ",
        "checks fail",
        "Exploitability, measured here (limit 0.3% of the pot):",
        "Values at the root, in chips per hand dealt (limit 0.2% of the pot apart):",
        "decision nodes reached more than 1% of the time (limit 2 points apart)",
        "as a range view shows it",
        "The root, OOP's",
        "Failures:\n- exploitability: B is exploitable by",
        "- frequency: at [] (reached 100.0% / 100.0%), check is played",
    ] {
        assert!(text.contains(expected), "{expected}\n{text}");
    }
}

#[test]
#[should_panic(expected = "not of the same spot")]
fn solutions_of_two_spots_are_not_compared() {
    let (a, b) = (load("toy_polarized"), load("dry_ip_polarized"));
    compare_mapped(
        &mapped(&solved(&a, 1)),
        &mapped(&solved(&b, 1)),
        &Tolerances::default(),
    );
}
