use std::collections::BTreeMap;

use proptest::prelude::*;

use super::*;
use crate::config::StreetSizing;
use crate::rules::{commit_limit, round_chips};

// ---------------------------------------------------------------------------------------------
// Helpers

fn street(bets: &[f64], raises: &[f64], allin: bool) -> StreetSizing {
    StreetSizing {
        bets: bets.to_vec(),
        raises: raises.to_vec(),
        allin,
    }
}

/// A tree with the same sizing on every street, checked against every rule by [`check`].
fn build(
    root: Street,
    pot: Chips,
    stack: Chips,
    sizing: StreetSizing,
    max_raises: u32,
    allin_threshold: f64,
) -> PostflopTree {
    let sizing = PostflopSizing::uniform(sizing, max_raises, allin_threshold);
    let tree = PostflopTree::build(TreeSpec::new(root, pot, stack, sizing)).unwrap();
    check(&tree);
    tree
}

/// Follows a path of action tokens from the root, passing through chance nodes on the way, and
/// returns the node it ends on.
fn at(tree: &PostflopTree, path: &str) -> u32 {
    let mut index = PostflopTree::ROOT;
    for token in path.split_whitespace() {
        while let Node::Chance { child, .. } = *tree.node(index) {
            index = child;
        }
        let Node::Player {
            actions, children, ..
        } = tree.node(index)
        else {
            panic!("`{path}`: `{token}` comes after the end of the hand");
        };
        let slot = actions
            .iter()
            .position(|action| action.to_string() == token)
            .unwrap_or_else(|| panic!("`{path}`: no `{token}` among {actions:?}"));
        index = children[slot];
    }
    index
}

/// The actions at a player node as tokens, with each wager's street total: `x b25=100 a=700`.
/// At a chance node, the menu of the decision that follows the card.
fn menu(tree: &PostflopTree, mut index: u32) -> String {
    while let Node::Chance { child, .. } = *tree.node(index) {
        index = child;
    }
    let Node::Player { actions, .. } = tree.node(index) else {
        panic!("node {index} is not a player node: {:?}", tree.node(index));
    };
    let tokens: Vec<String> = actions
        .iter()
        .map(|action| match action.to() {
            Some(to) => format!("{action}={to}"),
            None => action.to_string(),
        })
        .collect();
    tokens.join(" ")
}

fn counts(
    player: usize,
    actions: usize,
    chance: usize,
    fold: usize,
    showdown: usize,
) -> StreetCounts {
    StreetCounts {
        player,
        actions,
        chance,
        fold,
        showdown,
    }
}

const NONE: StreetCounts = StreetCounts {
    player: 0,
    actions: 0,
    chance: 0,
    fold: 0,
    showdown: 0,
};

// ---------------------------------------------------------------------------------------------
// The checker: every rule of the module documentation, at every node.

/// The betting so far, replayed from the root by the checker.
#[derive(Debug, Clone, Copy)]
struct Replay {
    street: Street,
    actor: Player,
    total: [Chips; 2],
    street_bet: [Chips; 2],
    /// The last bet or raise increment on the street, exactly as the rule says it.
    last_increment: Chips,
    raises: u32,
}

impl Replay {
    fn start(street: Street, total: [Chips; 2]) -> Replay {
        Replay {
            street,
            actor: Player::Oop,
            total,
            street_bet: [0, 0],
            last_increment: 0,
            raises: 0,
        }
    }
}

/// What the checker expects to find at a node.
#[derive(Debug, Clone)]
enum Expect {
    Decision(Replay),
    Chance { street: Street, total: [Chips; 2] },
    Node(Node),
}

/// What follows the end of the betting on `street` with both players having put in `total`.
fn end_of_street(street: Street, total: [Chips; 2], path: &str) -> Expect {
    assert_eq!(
        total[0], total[1],
        "{path}: the street ends with unequal totals"
    );
    match street.next() {
        None => Expect::Node(Node::Terminal {
            street: Street::River,
            outcome: Outcome::Showdown,
            contributions: total,
        }),
        Some(next) => Expect::Chance {
            street: next,
            total,
        },
    }
}

/// The actions the rules call for at a decision, worked out from the rules one size at a time.
fn expected_actions(spec: &TreeSpec, s: &Replay) -> Vec<Action> {
    let me = s.actor.index();
    let opponent = s.actor.opponent().index();
    let sizing = spec.sizing.street(s.street);
    let behind = spec.stack - s.total[me];
    let allin_to = s.street_bet[me] + behind;
    let limit = commit_limit(spec.stack, spec.sizing.allin_threshold);
    let before_street = s.total[me] - s.street_bet[me];

    // Street total -> the smallest size that gives it, or `None` for all-in.
    let mut menu: BTreeMap<Chips, Option<f64>> = BTreeMap::new();
    let mut offer = |to: Chips, size: Option<f64>| {
        let kept = menu.entry(to).or_insert(size);
        if let (Some(kept_size), Some(size)) = (*kept, size) {
            *kept = Some(kept_size.min(size));
        }
    };
    let faced = s.street_bet[opponent];
    let facing = faced > s.street_bet[me];
    if !facing {
        let pot = spec.pot + s.total[0] + s.total[1];
        for &fraction in &sizing.bets {
            let amount = round_chips(fraction * f64::from(pot)).max(CHIPS_PER_BB);
            if amount >= behind || before_street + amount > limit {
                offer(allin_to, None);
            } else {
                offer(amount, Some(fraction));
            }
        }
        if sizing.allin {
            offer(allin_to, None);
        }
    } else if s.raises < spec.sizing.max_raises && allin_to > faced {
        let min_raise_to = faced + s.last_increment;
        for &multiple in &sizing.raises {
            let to = round_chips(multiple * f64::from(faced)).max(min_raise_to);
            if to >= allin_to || before_street + to > limit {
                offer(allin_to, None);
            } else {
                offer(to, Some(multiple));
            }
        }
        if sizing.allin {
            offer(allin_to, None);
        }
    }

    let first: &[Action] = if facing {
        &[Action::Fold, Action::Call]
    } else {
        &[Action::Check]
    };
    let wagers = menu.into_iter().map(|(to, size)| match (size, facing) {
        (None, _) => Action::AllIn { to },
        (Some(fraction), false) => Action::Bet { to, fraction },
        (Some(multiple), true) => Action::Raise { to, multiple },
    });
    first.iter().copied().chain(wagers).collect()
}

/// Checks the legality of each action at a decision directly, without working out the menu.
fn check_legal(spec: &TreeSpec, s: &Replay, actions: &[Action], path: &str) {
    let me = s.actor.index();
    let opponent = s.actor.opponent().index();
    let sizing = spec.sizing.street(s.street);
    let behind = spec.stack - s.total[me];
    let allin_to = s.street_bet[me] + behind;
    let threshold = spec.sizing.allin_threshold * f64::from(spec.stack);
    let faced = s.street_bet[opponent];
    let facing = faced > s.street_bet[me];
    let committed = |to: Chips| f64::from(s.total[me] - s.street_bet[me] + to);

    if facing {
        assert_eq!(actions[..2], [Action::Fold, Action::Call], "{path}");
    } else {
        assert_eq!(actions[0], Action::Check, "{path}");
        assert!(behind > 0, "{path}: a decision with nothing to call or bet");
        assert!(actions.len() >= 2, "{path}: no bet offered");
    }
    let wagers = &actions[if facing { 2 } else { 1 }..];
    if facing && !wagers.is_empty() {
        assert!(
            s.raises < spec.sizing.max_raises,
            "{path}: raise past the cap"
        );
    }
    let mut previous: Option<Chips> = None;
    for &action in wagers {
        let to = action
            .to()
            .unwrap_or_else(|| panic!("{path}: {action:?} out of place"));
        assert!(
            previous < Some(to),
            "{path}: amounts out of order or repeated"
        );
        previous = Some(to);
        assert!(
            to > faced,
            "{path}: {action:?} is not more than the bet faced"
        );
        match action {
            Action::Bet { fraction, .. } => {
                assert!(!facing, "{path}: a bet facing a bet");
                assert!(sizing.bets.contains(&fraction), "{path}: unknown size");
                assert!(to >= CHIPS_PER_BB, "{path}: a bet below 1bb");
                assert!(to < allin_to, "{path}: a bet of every chip is not all-in");
                assert!(
                    committed(to) <= threshold * (1.0 + 1e-9),
                    "{path}: past threshold"
                );
            }
            Action::Raise { multiple, .. } => {
                assert!(facing, "{path}: a raise with no bet to face");
                assert!(sizing.raises.contains(&multiple), "{path}: unknown size");
                assert!(
                    to >= faced + s.last_increment,
                    "{path}: below the minimum raise"
                );
                assert!(to < allin_to, "{path}: a raise of every chip is not all-in");
                assert!(
                    committed(to) <= threshold * (1.0 + 1e-9),
                    "{path}: past threshold"
                );
            }
            Action::AllIn { .. } => {
                assert_eq!(to, allin_to, "{path}: all-in is every chip");
                assert_eq!(Some(&action), wagers.last(), "{path}: all-in is last");
            }
            _ => panic!("{path}: {action:?} out of place"),
        }
    }
}

/// Replays every line from the root and checks every rule, the node counts and the layout.
fn check(tree: &PostflopTree) {
    let spec = tree.spec();
    let nodes = tree.nodes();
    assert!(u32::try_from(nodes.len()).is_ok());
    let mut seen = vec![false; nodes.len()];

    let start = Replay::start(spec.street, [0, 0]);
    let mut work = vec![(PostflopTree::ROOT, Expect::Decision(start), String::new())];
    while let Some((index, expect, path)) = work.pop() {
        let node = &nodes[index as usize];
        assert!(
            !seen[index as usize],
            "{path}: node {index} has two parents"
        );
        seen[index as usize] = true;
        for &child in node.children() {
            assert!(
                index < child && (child as usize) < nodes.len(),
                "{path}: bad child"
            );
        }
        match expect {
            Expect::Node(expected) => assert_eq!(*node, expected, "{path}"),
            Expect::Chance { street, total } => {
                let &Node::Chance {
                    street: dealt,
                    child,
                } = node
                else {
                    panic!("{path}: expected a chance node, found {node:?}");
                };
                assert_eq!(dealt, street, "{path}");
                let next = if total[0] == spec.stack {
                    end_of_street(street, total, &path)
                } else {
                    Expect::Decision(Replay::start(street, total))
                };
                work.push((child, next, format!("{path} {street}")));
            }
            Expect::Decision(s) => {
                let Node::Player {
                    street,
                    actor,
                    actions,
                    children,
                } = node
                else {
                    panic!("{path}: expected a decision, found {node:?}");
                };
                assert_eq!((*street, *actor), (s.street, s.actor), "{path}");
                assert_eq!(actions.len(), children.len(), "{path}");
                check_legal(spec, &s, actions, &path);
                assert_eq!(*actions, expected_actions(spec, &s), "{path}");

                let me = s.actor.index();
                let opponent = s.actor.opponent();
                for (&action, &child) in actions.iter().zip(children) {
                    let path = format!("{path} {action}");
                    let next = match action {
                        Action::Fold => Expect::Node(Node::Terminal {
                            street: s.street,
                            outcome: Outcome::Fold { winner: opponent },
                            contributions: s.total,
                        }),
                        Action::Check if s.actor == Player::Oop => Expect::Decision(Replay {
                            actor: Player::Ip,
                            ..s
                        }),
                        Action::Check => end_of_street(s.street, s.total, &path),
                        Action::Call => {
                            let mut total = s.total;
                            total[me] += s.street_bet[opponent.index()] - s.street_bet[me];
                            end_of_street(s.street, total, &path)
                        }
                        wager => {
                            let to = wager.to().unwrap();
                            let faced = s.street_bet[opponent.index()];
                            let mut n = s;
                            n.total[me] += to - s.street_bet[me];
                            assert!(n.total[me] <= spec.stack, "{path}: more than the stack");
                            n.street_bet[me] = to;
                            n.last_increment = to - faced;
                            if faced > s.street_bet[me] {
                                n.raises += 1;
                            }
                            assert!(n.raises <= spec.sizing.max_raises, "{path}: past the cap");
                            n.actor = opponent;
                            Expect::Decision(n)
                        }
                    };
                    work.push((child, next, path));
                }
            }
        }
    }
    assert!(
        seen.iter().all(|&s| s),
        "every node is reachable from the root"
    );

    // Pre-order: each node's first child follows it, and each later child follows the subtree
    // of the one before.
    let mut size = vec![1usize; nodes.len()];
    for index in (0..nodes.len()).rev() {
        size[index] += nodes[index]
            .children()
            .iter()
            .map(|&c| size[c as usize])
            .sum::<usize>();
    }
    assert_eq!(size[0], nodes.len());
    for (index, node) in nodes.iter().enumerate() {
        let mut next = index + 1;
        for &child in node.children() {
            assert_eq!(child as usize, next, "node {index} is not in pre-order");
            next += size[child as usize];
        }
    }

    // The counts add up to the arena.
    let counts = tree.counts();
    assert_eq!(counts.nodes(), nodes.len());
    let edges: usize = Street::ALL
        .iter()
        .map(|&s| counts.street(s).actions + counts.street(s).chance)
        .sum();
    assert_eq!(
        edges,
        nodes.len() - 1,
        "a tree has one edge per node but the root"
    );
}

// ---------------------------------------------------------------------------------------------
// Trees counted by hand

#[test]
fn a_river_tree_with_one_raise_counted_by_hand() {
    // River, pot 1000, stack 10000, bets [0.5], raises [3], no all-in flag, one raise allowed.
    //
    // No bet changes the pot before the street's first bet, so every bet is 0.5 x 1000 = 500.
    // Every raise is to 3 x 500 = 1500, above the minimum raise to 500 + 500 = 1000. After one
    // raise the cap leaves fold or call.
    //
    //   root OOP: x b50                  player
    //     x: IP: x b50                   player
    //       x: showdown                  showdown
    //       b50: OOP: f c r3             player, fold, showdown (the call)
    //         r3: IP: f c                player, fold, showdown
    //     b50: IP: f c r3                player, fold, showdown
    //       r3: OOP: f c                 player, fold, showdown
    //
    // Player nodes 6, folds 4, showdowns 1 + 4 = 5: 15 nodes.
    // Actions 2 + 2 + 3 + 2 + 3 + 2 = 14.
    let tree = build(
        Street::River,
        1000,
        10_000,
        street(&[0.5], &[3.0], false),
        1,
        1.0,
    );
    let c = tree.counts();
    assert_eq!(c.river, counts(6, 14, 0, 4, 5));
    assert_eq!((c.flop, c.turn), (NONE, NONE));
    assert_eq!(c.nodes(), 15);

    assert_eq!(menu(&tree, at(&tree, "")), "x b50=500");
    assert_eq!(menu(&tree, at(&tree, "x")), "x b50=500");
    assert_eq!(menu(&tree, at(&tree, "x b50")), "f c r3=1500");
    assert_eq!(menu(&tree, at(&tree, "b50 r3")), "f c");
    assert_eq!(
        *tree.node(at(&tree, "b50 r3 f")),
        Node::Terminal {
            street: Street::River,
            outcome: Outcome::Fold { winner: Player::Ip },
            contributions: [500, 1500],
        }
    );
    assert_eq!(
        *tree.node(at(&tree, "x b50 r3 c")),
        Node::Terminal {
            street: Street::River,
            outcome: Outcome::Showdown,
            contributions: [1500, 1500],
        }
    );
}

#[test]
fn the_threshold_turns_a_size_into_all_in_counted_by_hand() {
    // River, pot 1000, stack 1500, bets [0.5, 1], raises [2], all-in flag, two raises allowed,
    // threshold 0.6. The limit is 0.6 x 1500 = 900 chips put in.
    //
    // With no bet to face: 0.5 x 1000 = 500 stays (500 <= 900). 1 x 1000 = 1000 is below the
    // 1500 stack but past 900, so it becomes all-in, the same action as the flag's all-in.
    // Menu: x b50 a.
    // Facing 500: 2 x 500 = 1000, the minimum raise exactly, is past 900, so all-in again; with
    // the flag that is one action. Menu: f c a.
    // Facing all-in: a call uses every chip, so f c.
    //
    //   root OOP: x b50 a                player
    //     x: IP: x b50 a                 player
    //       x: showdown                  showdown
    //       b50: OOP: f c a              player, fold, showdown
    //         a: IP: f c                 player, fold, showdown
    //       a: OOP: f c                  player, fold, showdown
    //     b50: IP: f c a                 player, fold, showdown
    //       a: OOP: f c                  player, fold, showdown
    //     a: IP: f c                     player, fold, showdown
    //
    // Player nodes 8, folds 6, showdowns 1 + 6 = 7: 21 nodes.
    // Actions 3 + 3 + 3 + 2 + 2 + 3 + 2 + 2 = 20.
    let tree = build(
        Street::River,
        1000,
        1500,
        street(&[0.5, 1.0], &[2.0], true),
        2,
        0.6,
    );
    assert_eq!(tree.counts().river, counts(8, 20, 0, 6, 7));
    assert_eq!(tree.counts().nodes(), 21);

    assert_eq!(menu(&tree, at(&tree, "")), "x b50=500 a=1500");
    assert_eq!(menu(&tree, at(&tree, "x")), "x b50=500 a=1500");
    assert_eq!(menu(&tree, at(&tree, "b50")), "f c a=1500");
    assert_eq!(menu(&tree, at(&tree, "x b50 a")), "f c");

    // With a threshold of 1 the pot-sized bet and the 2x raise both stand.
    let tree = build(
        Street::River,
        1000,
        1500,
        street(&[0.5, 1.0], &[2.0], true),
        2,
        1.0,
    );
    assert_eq!(menu(&tree, at(&tree, "")), "x b50=500 b100=1000 a=1500");
    assert_eq!(menu(&tree, at(&tree, "b50")), "f c r2=1000 a=1500");
}

#[test]
fn the_stack_turns_a_size_into_all_in_and_small_sizes_are_lifted_counted_by_hand() {
    // River, pot 200, stack 700, bets [0.25, 4], raises [1.5], no all-in flag, two raises
    // allowed, threshold 1.
    //
    // With no bet to face: 0.25 x 200 = 50 is lifted to the 1bb minimum, 100. 4 x 200 = 800 is
    // more than the 700 left, so it becomes all-in. Menu: x b25 a, though the flag is off.
    // Facing 100: 1.5 x 100 = 150 is lifted to the minimum raise, 100 + 100 = 200.
    // Facing the raise to 200: 1.5 x 200 = 300, exactly the minimum, 200 + 100 = 300.
    // Facing the raise to 300: two raises made, so f c.
    // Facing all-in: f c.
    //
    // Facing 100 the subtree is
    //   F100: f c r1.5                   player, fold, showdown
    //     r1.5: F200: f c r1.5           player, fold, showdown
    //       r1.5: F300: f c              player, fold, showdown
    // which is 3 players, 3 folds, 3 showdowns. Facing all-in, FA, is 1 of each but showdown.
    //
    //   root OOP: x b25 a                player
    //     x: IP: x b25 a                 player
    //       x: showdown                  showdown
    //       b25: F100                    3 players, 3 folds, 3 showdowns
    //       a: FA                        1 player, 1 fold, 1 showdown
    //     b25: F100                      3 players, 3 folds, 3 showdowns
    //     a: FA                          1 player, 1 fold, 1 showdown
    //
    // Player nodes 1 + 1 + 3 + 1 + 3 + 1 = 10, folds 3 + 1 + 3 + 1 = 8,
    // showdowns 1 + 3 + 1 + 3 + 1 = 9: 27 nodes.
    // Actions: 3 at each no-bet node, 3 at F100 and F200, 2 at F300 and FA:
    // 3 + 3 + (3 + 3 + 2) + 2 + (3 + 3 + 2) + 2 = 26.
    let tree = build(
        Street::River,
        200,
        700,
        street(&[0.25, 4.0], &[1.5], false),
        2,
        1.0,
    );
    assert_eq!(tree.counts().river, counts(10, 26, 0, 8, 9));
    assert_eq!(tree.counts().nodes(), 27);

    assert_eq!(menu(&tree, at(&tree, "")), "x b25=100 a=700");
    assert_eq!(menu(&tree, at(&tree, "b25")), "f c r1.5=200");
    assert_eq!(menu(&tree, at(&tree, "b25 r1.5")), "f c r1.5=300");
    assert_eq!(menu(&tree, at(&tree, "x b25 r1.5 r1.5")), "f c");
    assert_eq!(menu(&tree, at(&tree, "x a")), "f c");
}

#[test]
fn a_turn_tree_deals_the_river_on_every_line_that_closes_counted_by_hand() {
    // Turn, pot 100, stack 1000, bets [1], no raises, no all-in flag on either street.
    //
    // A street here is: OOP x or bet; IP behind a check x or bet; facing a bet, f or c.
    //   OOP: x b100                      player
    //     x: IP: x b100                  player
    //       x: (street closes)
    //       b100: OOP: f c               player, fold, (c closes)
    //     b100: IP: f c                  player, fold, (c closes)
    // That is 4 players, 2 folds and 3 lines that close the street, with 8 actions.
    //
    // On the turn each closing line leads to a chance node for the river: 3 chance nodes. On
    // the river each closing line is a showdown, so a river street is 4 players, 8 actions,
    // 2 folds and 3 showdowns, and there are 3 of them. The pot is 100 after x x and 300 after
    // a bet and a call; either way the pot-sized bet is below the stack and stands.
    //
    //   turn: 4 players, 8 actions, 2 folds.
    //   river: 3 chance, 12 players, 24 actions, 6 folds, 9 showdowns.
    //   4 + 2 + 3 + 12 + 6 + 9 = 36 nodes.
    let tree = build(Street::Turn, 100, 1000, street(&[1.0], &[], false), 0, 1.0);
    let c = tree.counts();
    assert_eq!(c.flop, NONE);
    assert_eq!(c.turn, counts(4, 8, 0, 2, 0));
    assert_eq!(c.river, counts(12, 24, 3, 6, 9));
    assert_eq!(c.nodes(), 36);

    assert!(matches!(
        *tree.node(at(&tree, "x x")),
        Node::Chance {
            street: Street::River,
            ..
        }
    ));
    assert_eq!(menu(&tree, at(&tree, "x x x")), "x b100=100");
    assert_eq!(menu(&tree, at(&tree, "b100 c")), "x b100=300");
    assert_eq!(menu(&tree, at(&tree, "x b100 c x")), "x b100=300");
    // 100 each on the turn, then OOP's river bet of 300 into 300 is folded to.
    assert_eq!(
        *tree.node(at(&tree, "b100 c b100 f")),
        Node::Terminal {
            street: Street::River,
            outcome: Outcome::Fold {
                winner: Player::Oop
            },
            contributions: [400, 100],
        }
    );
}

#[test]
fn players_all_in_on_the_turn_go_through_the_river_card_to_showdown_counted_by_hand() {
    // Turn, pot 300, stack 50, bets [1], no raises, all-in flag.
    //
    // A pot-sized bet of 300 is more than the 50 left, so it becomes all-in, the same action as
    // the flag's. Facing all-in, f c. So each street is:
    //   OOP: x a                         player
    //     x: IP: x a                     player
    //       x: (street closes)
    //       a: OOP: f c                  player, fold, (c: both all-in)
    //     a: IP: f c                     player, fold, (c: both all-in)
    // 4 players, 8 actions, 2 folds.
    //
    // Turn: 4 players, 2 folds, and 3 closing lines, each to a river chance node. After x x the
    // river is played like the turn: 4 players, 2 folds, 3 showdowns. After either all-in and
    // call, the chance node leads straight to a showdown: 2 more showdowns.
    //
    //   turn: 4 players, 8 actions, 2 folds.
    //   river: 3 chance, 4 players, 8 actions, 2 folds, 3 + 2 = 5 showdowns.
    //   4 + 2 + 3 + 4 + 2 + 5 = 20 nodes.
    let tree = build(Street::Turn, 300, 50, street(&[1.0], &[], true), 1, 1.0);
    let c = tree.counts();
    assert_eq!(c.turn, counts(4, 8, 0, 2, 0));
    assert_eq!(c.river, counts(4, 8, 3, 2, 5));
    assert_eq!(c.nodes(), 20);

    assert_eq!(menu(&tree, at(&tree, "")), "x a=50");
    let dealt = at(&tree, "a c");
    let Node::Chance {
        street: Street::River,
        child,
    } = *tree.node(dealt)
    else {
        panic!("an all-in call on the turn deals the river");
    };
    assert_eq!(
        *tree.node(child),
        Node::Terminal {
            street: Street::River,
            outcome: Outcome::Showdown,
            contributions: [50, 50],
        }
    );
}

#[test]
fn a_flop_tree_with_all_ins_before_the_river_counted_by_hand() {
    // Flop, pot 100, stack 150, bets [1], no raise sizes, all-in flag, one raise allowed.
    //
    // A street that starts with pot P and S left each, where 100 <= P < S, is:
    //   OOP: x bP a                          player
    //     x: IP: x bP a                      player
    //       x: closes at (P, S)
    //       bP: OOP: f c a                   player, fold, c closes at (3P, S - P)
    //         a: IP: f c                     player, fold, c all-in
    //       a: OOP: f c                      player, fold, c all-in
    //     bP: IP: f c a                      player, fold, c closes at (3P, S - P)
    //       a: OOP: f c                      player, fold, c all-in
    //     a: IP: f c                         player, fold, c all-in
    // Facing bP, all-in is a raise to S, short of the minimum raise to 2P: allowed. Facing
    // all-in, a call uses every chip. Call this street G: 8 players, 20 actions, 6 folds, and
    // its line ends are 1 at (P, S), 2 at (3P, S - P) and 4 all-in.
    //
    // A street that starts with pot 300 and 50 left, H: the pot-sized bet is past the stack and
    // becomes all-in, so OOP: x a; IP after x: x a; facing all-in: f c. 4 players, 8 actions,
    // 2 folds; line ends 1 at (300, 50) and 2 all-in.
    //
    // The flop is G with P = 100, S = 150. Its 7 line ends each deal the turn:
    //   (100, 150) -> turn G again: 8 players, 6 folds, 7 line ends dealing the river:
    //       (100, 150) -> river G, every end a showdown: 8 players, 6 folds, 7 showdowns.
    //       2 x (300, 50) -> river H: 4 players, 2 folds, 3 showdowns each.
    //       4 x all-in -> showdown.
    //   2 x (300, 50) -> turn H: 4 players, 2 folds, 3 line ends dealing the river:
    //       (300, 50) -> river H: 4 players, 2 folds, 3 showdowns.
    //       2 x all-in -> showdown.
    //   4 x all-in -> river chance node -> showdown.
    //
    //   flop:  8 players, 20 actions, 6 folds.
    //   turn:  7 chance; players 8 + 2 x 4 = 16; actions 20 + 2 x 8 = 36; folds 6 + 2 x 2 = 10.
    //   river: chance 7 + 2 x 3 + 4 = 17;
    //          players 8 + 2 x 4 (in turn G) + 2 x 4 (in the two turn H) = 24;
    //          actions 20 + 2 x 8 + 2 x 8 = 52; folds 6 + 2 x 2 + 2 x 2 = 14;
    //          showdowns (7 + 2 x 3 + 4) + 2 x (3 + 2) + 4 = 31.
    //   14 + 33 + 86 = 133 nodes.
    let tree = build(Street::Flop, 100, 150, street(&[1.0], &[], true), 1, 1.0);
    let c = tree.counts();
    assert_eq!(c.flop, counts(8, 20, 0, 6, 0));
    assert_eq!(c.turn, counts(16, 36, 7, 10, 0));
    assert_eq!(c.river, counts(24, 52, 17, 14, 31));
    assert_eq!(c.nodes(), 133);

    assert_eq!(menu(&tree, at(&tree, "")), "x b100=100 a=150");
    assert_eq!(menu(&tree, at(&tree, "x b100")), "f c a=150");
    assert_eq!(menu(&tree, at(&tree, "x b100 a")), "f c");
    assert_eq!(menu(&tree, at(&tree, "x x x")), "x b100=100 a=150");
    assert_eq!(menu(&tree, at(&tree, "b100 c x")), "x a=50");
    assert_eq!(menu(&tree, at(&tree, "x x b100 c x")), "x a=50");

    // An all-in called on the flop deals the turn and the river with no decision between.
    let turn = at(&tree, "a c");
    let Node::Chance {
        street: Street::Turn,
        child: river,
    } = *tree.node(turn)
    else {
        panic!("an all-in call on the flop deals the turn");
    };
    let Node::Chance {
        street: Street::River,
        child: showdown,
    } = *tree.node(river)
    else {
        panic!("and then the river");
    };
    assert_eq!(
        *tree.node(showdown),
        Node::Terminal {
            street: Street::River,
            outcome: Outcome::Showdown,
            contributions: [150, 150],
        }
    );
}

#[test]
fn a_tree_past_the_node_limit_is_refused() {
    // The flop tree with all-ins above has 133 nodes, counted by hand.
    let sizing = PostflopSizing::uniform(street(&[1.0], &[], true), 1, 1.0);
    let spec = |max_nodes| TreeSpec {
        max_nodes,
        ..TreeSpec::new(Street::Flop, 100, 150, sizing.clone())
    };
    assert_eq!(PostflopTree::build(spec(133)).unwrap().nodes().len(), 133);
    assert_eq!(
        PostflopTree::build(spec(132)),
        Err(TreeError::TooLarge { limit: 132 })
    );
    assert_eq!(
        PostflopTree::build(spec(1)),
        Err(TreeError::TooLarge { limit: 1 })
    );
}

#[test]
fn a_runaway_sizing_stops_at_the_limit() {
    // A million big blinds deep with min-raises and a cap of 10,000 raises: the river alone has
    // far more lines than any limit allows. The builder stops at the limit, holding no more
    // than that many nodes.
    let sizing = PostflopSizing::uniform(street(&[0.01, 0.02, 0.03], &[1.01], true), 10_000, 1.0);
    let spec = TreeSpec {
        max_nodes: 10_000,
        ..TreeSpec::new(Street::Flop, 100, 100_000_000, sizing)
    };
    assert_eq!(
        PostflopTree::build(spec),
        Err(TreeError::TooLarge { limit: 10_000 })
    );
}

// ---------------------------------------------------------------------------------------------
// Single rules

#[test]
fn the_threshold_counts_chips_put_in_on_earlier_streets() {
    // Flop, pot 400, stack 1000, bets [0.25], threshold 0.2: at most 200 put in, in all.
    let tree = build(Street::Flop, 400, 1000, street(&[0.25], &[], false), 0, 0.2);
    // On the flop, 0.25 x 400 = 100 stands.
    assert_eq!(menu(&tree, at(&tree, "")), "x b25=100");
    // After a check through, the turn pot is still 400 and the same bet stands.
    assert_eq!(menu(&tree, at(&tree, "x x x")), "x b25=100");
    // After a bet and a call the pot is 600: 0.25 x 600 = 150 would make 250 put in, so it is
    // all-in, though 150 alone is under 200.
    assert_eq!(menu(&tree, at(&tree, "b25 c x")), "x a=900");
}

#[test]
fn sizes_that_round_to_the_same_amount_are_one_action() {
    // At a pot of 150: 0.1 -> 15, 0.33 -> 49.5 -> 50 and 0.5 -> 75 are all lifted to 100, and
    // 0.67 -> 100.5 rounds up to 101. The smallest size names the merged bet.
    let sizing = street(&[0.5, 0.67, 0.33, 0.1], &[], true);
    let tree = build(Street::River, 150, 10_000, sizing, 0, 1.0);
    assert_eq!(menu(&tree, at(&tree, "")), "x b10=100 b67=101 a=10000");
}

#[test]
fn the_order_of_sizes_in_the_config_does_not_change_the_tree() {
    let ordered = build(
        Street::Turn,
        550,
        9750,
        street(&[0.33, 0.75], &[2.5, 3.0], true),
        3,
        0.67,
    );
    let shuffled = build(
        Street::Turn,
        550,
        9750,
        street(&[0.75, 0.33], &[3.0, 2.5], true),
        3,
        0.67,
    );
    assert_eq!(ordered.nodes(), shuffled.nodes());
}

#[test]
fn a_zero_pot_lifts_every_bet_to_one_big_blind() {
    let tree = build(
        Street::River,
        0,
        1000,
        street(&[0.33, 0.75], &[3.0], false),
        1,
        1.0,
    );
    assert_eq!(menu(&tree, at(&tree, "")), "x b33=100");
    assert_eq!(menu(&tree, at(&tree, "b33")), "f c r3=300");
}

#[test]
fn no_raises_once_the_cap_is_reached() {
    // Raises to 3x with the cap at 0, 1 and 2, deep enough that nothing goes all-in.
    let sizing = street(&[1.0], &[3.0], false);
    let none = build(Street::River, 100, 1_000_000, sizing.clone(), 0, 1.0);
    assert_eq!(menu(&none, at(&none, "b100")), "f c");
    let one = build(Street::River, 100, 1_000_000, sizing.clone(), 1, 1.0);
    assert_eq!(menu(&one, at(&one, "b100")), "f c r3=300");
    assert_eq!(menu(&one, at(&one, "b100 r3")), "f c");
    let two = build(Street::River, 100, 1_000_000, sizing, 2, 1.0);
    assert_eq!(menu(&two, at(&two, "b100 r3")), "f c r3=900");
    assert_eq!(menu(&two, at(&two, "b100 r3 r3")), "f c");
}

#[test]
fn the_cap_stops_all_in_raises_too() {
    // With one raise allowed, the all-in flag offers all-in beside the 3x raise. Once the 3x
    // raise is made, the cap leaves fold or call, flag or not. (Heads-up, an all-in raise can
    // never be re-raised anyway, so counting it toward the cap only matters to a longer table.)
    let sizing = street(&[1.0], &[3.0], true);
    let tree = build(Street::River, 100, 1000, sizing.clone(), 1, 1.0);
    assert_eq!(menu(&tree, at(&tree, "b100")), "f c r3=300 a=1000");
    assert_eq!(menu(&tree, at(&tree, "b100 r3")), "f c");
    // With no raises allowed, the flag still offers the all-in bet, but no raise.
    let tree = build(Street::River, 100, 1000, sizing, 0, 1.0);
    assert_eq!(menu(&tree, at(&tree, "")), "x b100=100 a=1000");
    assert_eq!(menu(&tree, at(&tree, "b100")), "f c");
}

#[test]
fn actions_print_the_token_of_their_size() {
    let tokens: Vec<String> = [
        Action::Fold,
        Action::Check,
        Action::Call,
        Action::Bet {
            to: 330,
            fraction: 0.33,
        },
        Action::Bet {
            to: 1500,
            fraction: 1.5,
        },
        Action::Bet {
            to: 125,
            fraction: 0.125,
        },
        Action::Raise {
            to: 900,
            multiple: 3.0,
        },
        Action::Raise {
            to: 750,
            multiple: 2.5,
        },
        Action::AllIn { to: 9750 },
    ]
    .iter()
    .map(Action::to_string)
    .collect();
    assert_eq!(
        tokens,
        ["f", "x", "c", "b33", "b150", "b12.5", "r3", "r2.5", "a"]
    );
}

fn rejected_field(spec: TreeSpec) -> String {
    match PostflopTree::build(spec) {
        Err(TreeError::Invalid { field, .. }) => field,
        other => panic!("expected an invalid-input error, got {other:?}"),
    }
}

#[test]
fn inputs_that_make_no_sense_are_rejected() {
    let sizing = PostflopSizing::uniform(street(&[0.5], &[3.0], true), 3, 0.67);
    let good = TreeSpec::new(Street::Flop, 550, 9750, sizing);
    assert!(PostflopTree::build(good.clone()).is_ok());

    let no_room = TreeSpec {
        max_nodes: 0,
        ..good.clone()
    };
    assert_eq!(rejected_field(no_room), "max_nodes");

    let broke = TreeSpec {
        stack: 0,
        ..good.clone()
    };
    assert_eq!(rejected_field(broke), "stack");

    let too_big = TreeSpec {
        pot: 2,
        stack: Chips::MAX / 2,
        ..good.clone()
    };
    assert_eq!(rejected_field(too_big), "pot");
    let just_fits = TreeSpec {
        pot: 1,
        stack: Chips::MAX / 2,
        sizing: PostflopSizing::uniform(street(&[10.0], &[], false), 0, 1.0),
        ..good.clone()
    };
    assert!(PostflopTree::build(just_fits).is_ok());

    let mut no_bets = good.clone();
    no_bets.sizing.turn.bets.clear();
    assert_eq!(rejected_field(no_bets), "sizing.turn.bets");
    let mut small_raise = good.clone();
    small_raise.sizing.river.raises = vec![1.0];
    assert_eq!(rejected_field(small_raise), "sizing.river.raises");
    for threshold in [0.0, -0.5, 1.01, f64::NAN] {
        let mut bad = good.clone();
        bad.sizing.allin_threshold = threshold;
        assert_eq!(rejected_field(bad), "sizing.allin_threshold");
    }

    // Percents typed where a fraction or a multiple belongs.
    let mut percent_bets = good.clone();
    percent_bets.sizing.river.bets = vec![33.0, 75.0];
    assert_eq!(rejected_field(percent_bets), "sizing.river.bets");
    let mut percent_raise = good.clone();
    percent_raise.sizing.flop.raises = vec![250.0];
    assert_eq!(rejected_field(percent_raise), "sizing.flop.raises");
    let mut largest = good.clone();
    largest.sizing.river.bets = vec![crate::MAX_BET_FRACTION];
    largest.sizing.river.raises = vec![crate::MAX_RAISE_MULTIPLE];
    assert!(PostflopTree::build(largest).is_ok());
}

#[test]
fn config_sizing_builds_trees_from_every_street() {
    let path = format!("{}/../../configs/a.toml", env!("CARGO_MANIFEST_DIR"));
    let config = crate::GameConfig::load(&path).unwrap();
    for root in Street::ALL {
        // A single-raised pot: 5.5bb in the middle and 97.25bb behind.
        let spec = TreeSpec::new(root, 550, 9725, config.postflop.clone());
        let tree = PostflopTree::build(spec).unwrap();
        check(&tree);
        // 0.33 x 550 = 181.5 -> 182 and 0.75 x 550 = 412.5 -> 413; only the river offers all-in.
        let expected = match root {
            Street::River => "x b33=182 b75=413 a=9725",
            _ => "x b33=182 b75=413",
        };
        assert_eq!(menu(&tree, PostflopTree::ROOT), expected);
    }
}

// ---------------------------------------------------------------------------------------------
// Node paths

/// Every node a path can name, with its path written out by walking the tree: each player
/// node's children, each under its action's token, down to the terminals and the chance nodes
/// that end a path. In pre-order.
fn named_nodes(tree: &PostflopTree) -> Vec<(u32, String)> {
    let mut named = Vec::new();
    let mut stack = vec![(PostflopTree::ROOT, Vec::<String>::new())];
    while let Some((index, path)) = stack.pop() {
        if let Node::Player {
            actions, children, ..
        } = tree.node(index)
        {
            for (action, &child) in actions.iter().zip(children).rev() {
                let mut next = path.clone();
                next.push(action.to_string());
                stack.push((child, next));
            }
        }
        named.push((index, path.join(" ")));
    }
    named
}

fn strings(tokens: &[&str]) -> Vec<String> {
    tokens.iter().map(|token| token.to_string()).collect()
}

#[test]
fn every_node_is_found_by_its_path() {
    // River trees with two bet sizes, raises, and all-in by the flag, by the threshold and by
    // the stack: every node has a path. On a turn tree, paths name the turn's nodes and the
    // chance nodes that end them.
    let rivers = [
        build(
            Street::River,
            1000,
            1500,
            street(&[0.5, 1.0], &[2.0], true),
            2,
            0.6,
        ),
        build(
            Street::River,
            200,
            700,
            street(&[0.25, 4.0], &[1.5], false),
            2,
            1.0,
        ),
        build(
            Street::River,
            1000,
            10_000,
            street(&[0.5, 1.0], &[3.0], false),
            1,
            1.0,
        ),
    ];
    for tree in &rivers {
        let named = named_nodes(tree);
        assert_eq!(named.len(), tree.nodes().len());
        for (index, path) in named {
            assert_eq!(tree.find(&path), Ok(index), "`{path}`");
        }
    }
    let turn = build(Street::Turn, 100, 1000, street(&[1.0], &[], false), 0, 1.0);
    let named = named_nodes(&turn);
    // 4 player nodes, 2 folds and 3 chance nodes, as counted by hand above.
    assert_eq!(named.len(), 9);
    for (index, path) in named {
        assert_eq!(turn.find(&path), Ok(index), "`{path}`");
    }
    assert!(matches!(
        turn.node(turn.find("b100 c").unwrap()),
        Node::Chance {
            street: Street::River,
            ..
        }
    ));
    // The empty path is the root, and any whitespace separates tokens.
    let tree = &rivers[2];
    assert_eq!(tree.find(""), Ok(PostflopTree::ROOT));
    assert_eq!(tree.find("   "), Ok(PostflopTree::ROOT));
    assert_eq!(tree.find("  x\tb50  r3 "), tree.find("x b50 r3"));
    assert_eq!(tree.find("x b50 r3"), Ok(at(tree, "x b50 r3")));
}

#[test]
fn a_merged_size_is_found_by_the_smallest_sizes_token_and_all_in_by_a() {
    // At a pot of 150, the sizes 0.1, 0.33 and 0.5 are all lifted to 100 chips, one action named
    // by the smallest: `x b10 b67 a`, as `sizes_that_round_to_the_same_amount_are_one_action`
    // counts. The other two sizes' tokens name nothing.
    let sizing = street(&[0.5, 0.67, 0.33, 0.1], &[], true);
    let tree = build(Street::River, 150, 10_000, sizing, 0, 1.0);
    let root = tree.node(PostflopTree::ROOT).children();
    assert_eq!(tree.find("b10"), Ok(root[1]));
    assert_eq!(tree.find("b67"), Ok(root[2]));
    assert_eq!(tree.find("a"), Ok(root[3]));
    for token in ["b33", "b50"] {
        let error = tree.find(token).unwrap_err();
        assert_eq!(
            error,
            PathError {
                before: Vec::new(),
                token: token.to_string(),
                kind: PathErrorKind::NoAction {
                    actions: strings(&["x", "b10", "b67", "a"]),
                },
            }
        );
        assert_eq!(
            error.to_string(),
            format!(
                "`{token}` is not an action at the root; the actions there are `x`, `b10`, `b67` \
                 and `a`"
            )
        );
    }
    // A size that would use more than the stack becomes all-in: `a`, not its own token.
    let tree = build(
        Street::River,
        200,
        700,
        street(&[0.25, 4.0], &[1.5], false),
        2,
        1.0,
    );
    assert_eq!(
        tree.find("a"),
        Ok(tree.node(PostflopTree::ROOT).children()[2])
    );
    let error = tree.find("x b400").unwrap_err();
    assert_eq!(error.before, ["x"]);
    assert_eq!(
        error.kind,
        PathErrorKind::NoAction {
            actions: strings(&["x", "b25", "a"]),
        }
    );
}

#[test]
fn a_path_that_leads_nowhere_names_the_token_and_what_is_there() {
    // River, pot 1000, stack 10000, bets [0.5], raises [3], one raise: as counted by hand above.
    let tree = build(
        Street::River,
        1000,
        10_000,
        street(&[0.5], &[3.0], false),
        1,
        1.0,
    );
    // A token that is not an action there. Only the first bad token is reported.
    let error = tree.find("x b75 c").unwrap_err();
    assert_eq!(
        error,
        PathError {
            before: strings(&["x"]),
            token: "b75".to_string(),
            kind: PathErrorKind::NoAction {
                actions: strings(&["x", "b50"]),
            },
        }
    );
    assert_eq!(
        error.to_string(),
        "`b75` is not an action after `x`; the actions there are `x` and `b50`"
    );
    // Tokens are matched exactly, and a raise cap leaves no raise.
    let error = tree.find("X").unwrap_err();
    assert_eq!(
        error.to_string(),
        "`X` is not an action at the root; the actions there are `x` and `b50`"
    );
    assert_eq!(
        tree.find("b50 r3 r3").unwrap_err().to_string(),
        "`r3` is not an action after `b50 r3`; the actions there are `f` and `c`"
    );
    // A token after a fold or a showdown.
    let error = tree.find("b50 f c").unwrap_err();
    assert_eq!(
        error,
        PathError {
            before: strings(&["b50", "f"]),
            token: "c".to_string(),
            kind: PathErrorKind::HandOver {
                outcome: Outcome::Fold {
                    winner: Player::Oop
                },
            },
        }
    );
    assert_eq!(
        error.to_string(),
        "`c` comes after the end of the hand, at `b50 f`, where IP folds"
    );
    assert_eq!(
        tree.find("x x x").unwrap_err().to_string(),
        "`x` comes after the end of the hand, at `x x`, a showdown"
    );
    // A token after a chance node.
    let turn = build(Street::Turn, 100, 1000, street(&[1.0], &[], false), 0, 1.0);
    let error = turn.find("x x b100").unwrap_err();
    assert_eq!(
        error,
        PathError {
            before: strings(&["x", "x"]),
            token: "b100".to_string(),
            kind: PathErrorKind::Chance {
                street: Street::River,
            },
        }
    );
    assert_eq!(
        error.to_string(),
        "`b100` comes after `x x`, which deals the river, and a path does not go past a chance \
         node yet"
    );
}

// ---------------------------------------------------------------------------------------------
// Random sizings

fn bet_size() -> impl Strategy<Value = f64> {
    prop_oneof![
        Just(0.25),
        Just(0.33),
        Just(0.5),
        Just(0.75),
        Just(1.0),
        Just(1.5),
        0.01f64..3.0,
    ]
}

fn raise_size() -> impl Strategy<Value = f64> {
    prop_oneof![Just(2.0), Just(2.5), Just(3.0), 1.01f64..5.0]
}

fn street_sizing(bets: usize, raises: usize) -> impl Strategy<Value = StreetSizing> {
    (
        prop::collection::vec(bet_size(), 1..=bets),
        prop::collection::vec(raise_size(), 0..=raises),
        any::<bool>(),
    )
        .prop_map(|(bets, raises, allin)| StreetSizing {
            bets,
            raises,
            allin,
        })
}

/// The node limit for random trees. The sizes below keep almost every tree to a few hundred
/// nodes, and the limit makes any larger one a cheap, expected error instead of a large build.
const RANDOM_MAX_NODES: u32 = 5_000;

/// Random specs with few sizes and a low raise cap, fewer still the more streets the tree has.
fn spec() -> impl Strategy<Value = TreeSpec> {
    prop_oneof![Just(Street::Flop), Just(Street::Turn), Just(Street::River)].prop_flat_map(|root| {
        // Bet sizes, raise sizes and the raise cap, at most.
        let (bets, raises, max_raises): (usize, usize, u32) = match root {
            Street::Flop => (2, 1, 1),
            Street::Turn => (2, 1, 2),
            Street::River => (3, 2, 3),
        };
        (
            street_sizing(bets, raises),
            street_sizing(bets, raises),
            street_sizing(bets, raises),
            0..=max_raises,
            prop_oneof![Just(1.0), Just(0.67), 0.01f64..=1.0],
            prop_oneof![Just(0u32), 0u32..5000],
            prop_oneof![1u32..300, 1u32..20_000, 1u32..200_000],
        )
            .prop_map(
                move |(flop, turn, river, max_raises, allin_threshold, pot, stack)| TreeSpec {
                    street: root,
                    pot,
                    stack,
                    sizing: PostflopSizing {
                        flop,
                        turn,
                        river,
                        max_raises,
                        allin_threshold,
                    },
                    max_nodes: RANDOM_MAX_NODES,
                },
            )
    })
}

proptest! {
    // Every tree is small, so 2,000 cases take well under a second.
    #![proptest_config(ProptestConfig::with_cases(2000))]

    #[test]
    fn random_trees_keep_every_rule(spec in spec()) {
        match PostflopTree::build(spec) {
            Ok(tree) => {
                prop_assert!(tree.nodes().len() <= RANDOM_MAX_NODES as usize);
                check(&tree);
            }
            // Expected now and then; the limit itself is tested on a tree counted by hand.
            Err(TreeError::TooLarge { limit }) => prop_assert_eq!(limit, RANDOM_MAX_NODES),
            Err(error) => prop_assert!(false, "a valid spec was refused: {}", error),
        }
    }

    #[test]
    fn random_trees_find_every_node_by_its_path(spec in spec()) {
        // Also checks that no two actions at a node print the same token.
        if let Ok(tree) = PostflopTree::build(spec) {
            for (index, path) in named_nodes(&tree) {
                prop_assert_eq!(tree.find(&path), Ok(index), "`{}`", path);
            }
        }
    }
}
