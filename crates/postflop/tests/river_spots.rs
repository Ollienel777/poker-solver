//! The river test spots under `reference/spots/river/`.
//!
//! Every file loads, validates, builds its tree and its hands, and writes back exactly as it is
//! on disk. Every tree can be reproduced by a solver that does not round: each bet and raise is
//! a whole number of chips before rounding. The toy game has the three properties its analytical
//! solution needs. And the tables in `reference/README.md` match the files.

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use pk_core::{CardSet, Combo, HandRank, evaluate};
use pk_game::{Action, CHIPS_PER_BB, Chips, Node, Player, PostflopTree, StreetSizing};
use pk_postflop::{HandList, Spot};

fn repo_path(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
}

/// Every spot file, with the text it holds on disk, sorted by path.
fn spot_files() -> Vec<(PathBuf, Spot, String)> {
    let root = repo_path("reference/spots/river");
    let mut paths: Vec<PathBuf> = fs::read_dir(&root)
        .unwrap_or_else(|err| panic!("{}: {err}", root.display()))
        .map(|entry| entry.unwrap().path())
        .collect();
    paths.sort();
    paths
        .into_iter()
        .map(|path| {
            assert_eq!(
                path.extension().and_then(|extension| extension.to_str()),
                Some("toml"),
                "{} is not a spot file",
                path.display()
            );
            let spot = Spot::load(&path).unwrap_or_else(|err| panic!("{}: {err}", path.display()));
            // Git writes LF here (see .gitattributes), but a checkout that turned it into CRLF
            // should not fail the layout check.
            let text = fs::read_to_string(&path).unwrap().replace("\r\n", "\n");
            (path, spot, text)
        })
        .collect()
}

fn toy_game() -> Spot {
    Spot::load(repo_path("reference/spots/river/toy_polarized.toml")).unwrap()
}

/// An amount in big blinds without trailing zeros: `5.5`, `97.25` or `20`.
fn bb(chips: Chips) -> String {
    let (whole, cents) = (chips / CHIPS_PER_BB, chips % CHIPS_PER_BB);
    match cents {
        0 => whole.to_string(),
        tenths if tenths % 10 == 0 => format!("{whole}.{}", tenths / 10),
        cents => format!("{whole}.{cents:02}"),
    }
}

// ---------------------------------------------------------------------------------------------
// The files

#[test]
fn every_river_spot_loads_builds_and_writes_back_identically() {
    let files = spot_files();
    assert!(
        (8..=10).contains(&files.len()),
        "the set has 8 to 10 spots, not {}",
        files.len()
    );
    for (path, spot, text) in &files {
        let shown = path.display();
        let stem = path.file_stem().unwrap().to_str().unwrap();
        assert_eq!(spot.name, stem, "{shown}: `name` must match the file name");
        assert_eq!(spot.board.len(), 5, "{shown}: a river spot has five cards");
        spot.validate()
            .unwrap_or_else(|err| panic!("{shown}: {err}"));

        let tree = spot
            .build_tree()
            .unwrap_or_else(|err| panic!("{shown}: {err}"));
        assert_eq!(tree.counts().river.nodes(), tree.nodes().len(), "{shown}");
        let hands = spot.board_hands();
        for player in [Player::Oop, Player::Ip] {
            assert!(!hands.hands(player).is_empty(), "{shown}: {player}");
        }

        assert_eq!(
            &spot.to_toml(),
            text,
            "{shown} is not in the written layout"
        );
        assert!(
            spot.description.to_lowercase().contains("illustrative"),
            "{shown}: the description must say the ranges are illustrative"
        );
    }
}

#[test]
fn every_range_weight_is_a_quarter_so_that_any_solver_reads_it_exactly() {
    // 0.25, 0.5, 0.75 and 1 are exact in `f32` and `f64` alike, so a reference solver that
    // parses the weights in another float type holds the same ranges.
    for (path, spot, _) in spot_files() {
        for player in [Player::Oop, Player::Ip] {
            for (combo, weight) in spot.range(player).iter() {
                assert_eq!(
                    (weight * 4.0).fract(),
                    0.0,
                    "{}: {combo} has weight {weight}",
                    path.display()
                );
            }
        }
    }
}

#[test]
fn about_half_the_spots_are_rake_free_and_the_rest_take_5_percent() {
    let (mut free, mut raked) = (0usize, 0usize);
    for (path, spot, _) in spot_files() {
        match (spot.rake_percent, spot.rake_cap) {
            (0.0, 0) => free += 1,
            // 5% capped at 3bb, a cap low enough to bind in pots of these sizes.
            (5.0, 300) => raked += 1,
            other => panic!("{}: rake {other:?}", path.display()),
        }
    }
    assert!(
        free.abs_diff(raked) <= 1,
        "{free} rake-free and {raked} raked"
    );
}

#[test]
fn the_set_covers_what_it_is_meant_to_stress() {
    let spots: Vec<Spot> = spot_files().into_iter().map(|(_, spot, _)| spot).collect();
    let any = |test: &dyn Fn(&Spot) -> bool| spots.iter().any(test);
    let river = |spot: &Spot| spot.tree.river.clone().unwrap();
    let ranks = |spot: &Spot| {
        spot.board
            .iter()
            .map(|card| card.rank())
            .collect::<Vec<_>>()
    };
    let suits = |spot: &Spot| {
        spot.board
            .iter()
            .map(|card| card.suit())
            .collect::<Vec<_>>()
    };
    let most_of = |values: Vec<u8>| {
        values
            .iter()
            .map(|v| values.iter().filter(|w| *w == v).count())
            .max()
            .unwrap()
    };

    assert!(any(&|spot| most_of(ranks(spot)) == 2), "a paired board");
    assert!(
        any(&|spot| most_of(suits(spot)) == 4),
        "four cards of a suit"
    );
    assert!(
        any(&|spot| spot.effective_stack <= spot.pot),
        "a stack-to-pot ratio of 1 or less"
    );
    assert!(
        any(&|spot| spot.effective_stack >= 10 * spot.pot),
        "a stack-to-pot ratio of 10 or more"
    );
    assert!(
        any(&|spot| river(spot).bets.len() == 1 && spot.tree.max_raises == 0),
        "a single bet size and no raises"
    );
    assert!(
        any(&|spot| river(spot).bets.len() == 2
            && !river(spot).raises.is_empty()
            && spot.tree.max_raises > 0),
        "two bet sizes and a raise"
    );
    assert!(any(&|spot| river(spot).allin), "an all-in option");
    assert!(any(&|spot| spot.name == "toy_polarized"), "the toy game");
}

// ---------------------------------------------------------------------------------------------
// Trees a reference solver can reproduce

/// The chips each player has put in, in all and on the current street.
#[derive(Debug, Clone, Copy, Default)]
struct Betting {
    total: [Chips; 2],
    street: [Chips; 2],
}

/// Walks the tree from `index`, checking each wager against the size it came from. Returns the
/// number of bets and raises checked.
fn check_wagers(spot: &Spot, tree: &PostflopTree, index: u32, betting: Betting) -> usize {
    let shown = &spot.name;
    match tree.node(index) {
        Node::Player {
            street,
            actor,
            actions,
            children,
        } => {
            let sizing = spot.tree.street(*street).unwrap();
            let (me, them) = (actor.index(), actor.opponent().index());
            let pot = spot.pot + betting.total[0] + betting.total[1];
            let faced = betting.street[them];
            let mut checked = 0;
            for (&action, &child) in actions.iter().zip(children) {
                // The size before any rounding, as a solver that sizes in fractions finds it.
                let exact = match action {
                    Action::Bet { fraction, .. } => {
                        assert_eq!(faced, 0, "{shown}: a bet faces no bet");
                        Some(fraction * f64::from(pot))
                    }
                    Action::Raise { multiple, .. } => Some(multiple * f64::from(faced)),
                    Action::AllIn { .. } => {
                        // With `allin` off, an all-in could only come from a size the builder
                        // turned into one, which another solver might drop instead.
                        assert!(sizing.allin, "{shown}: an all-in on a street without one");
                        None
                    }
                    Action::Fold | Action::Check | Action::Call => None,
                };
                if let Some(exact) = exact {
                    let to = action.to().unwrap();
                    assert!(
                        (exact - f64::from(to)).abs() < 1e-6,
                        "{shown}: {action} goes to {to} chips, but its size gives {exact}"
                    );
                    checked += 1;
                }

                let mut next = betting;
                if let Some(to) = action.to() {
                    next.total[me] += to - betting.street[me];
                    next.street[me] = to;
                } else if action == Action::Call {
                    next.total[me] += faced - betting.street[me];
                    next.street[me] = faced;
                }
                checked += check_wagers(spot, tree, child, next);
            }
            checked
        }
        Node::Chance { child, .. } => {
            let next = Betting {
                street: [0, 0],
                ..betting
            };
            check_wagers(spot, tree, *child, next)
        }
        Node::Terminal { contributions, .. } => {
            // The walk kept the same books as the builder.
            assert_eq!(*contributions, betting.total, "{shown}");
            0
        }
    }
}

#[test]
fn every_bet_and_raise_is_a_whole_number_of_chips_before_rounding() {
    for (path, spot, _) in spot_files() {
        // A threshold of 1 turns no size into all-in.
        assert_eq!(spot.tree.allin_threshold, 1.0, "{}", path.display());
        let tree = spot.build_tree().unwrap();
        let checked = check_wagers(&spot, &tree, PostflopTree::ROOT, Betting::default());
        assert!(checked > 0, "{}", path.display());
    }
}

// ---------------------------------------------------------------------------------------------
// The toy game

/// The combos of a hand list, as one set of cards.
fn cards_of(hands: &HandList) -> CardSet {
    hands
        .combos()
        .iter()
        .fold(CardSet::EMPTY, |cards, combo| cards | combo.cards())
}

#[test]
fn the_toy_game_has_what_its_analytical_solution_needs() {
    let spot = toy_game();
    // One bet of b = 0.5 pot, no raises, and no rake, so the equilibrium is the textbook one.
    let b = 0.5;
    assert_eq!(
        spot.tree.river,
        Some(StreetSizing {
            bets: vec![b],
            raises: vec![],
            allin: false
        })
    );
    assert_eq!(spot.tree.max_raises, 0);
    assert_eq!(spot.rake_percent, 0.0);

    let hands = spot.board_hands();
    let (bettor, caller) = (hands.hands(Player::Oop), hands.hands(Player::Ip));

    // 1. No card in one range is in the other, so every bettor hand meets every caller hand
    //    and card removal cannot tilt the frequencies.
    assert!(!cards_of(bettor).intersects(cards_of(caller)));

    // 2. Every nut combo beats every caller combo, which beats every air combo. The bettor's
    //    hands are split into nuts and air by the evaluator, and none may be left over.
    let strength = |combo: &Combo| -> HandRank { evaluate(spot.board | combo.cards()) };
    let calls: Vec<HandRank> = caller.combos().iter().map(strength).collect();
    let (mut nuts, mut air) = (Vec::new(), Vec::new());
    for (h, combo) in bettor.combos().iter().enumerate() {
        let s = strength(combo);
        if calls.iter().all(|&c| s > c) {
            nuts.push(h);
        } else if calls.iter().all(|&c| s < c) {
            air.push(h);
        } else {
            panic!("{combo} neither beats nor loses to every caller hand");
        }
    }
    for &n in &nuts {
        for (c, &call) in caller.combos().iter().zip(&calls) {
            assert!(strength(&bettor.combos()[n]) > call, "{c}");
        }
    }
    for &a in &air {
        for (c, &call) in caller.combos().iter().zip(&calls) {
            assert!(strength(&bettor.combos()[a]) < call, "{c}");
        }
    }
    // 3. The bettor has more air than the bluffs need, so some air checks and the caller's
    //    frequency is pinned by the bluffers' indifference. Bluffs B make up b / (1 + 2b) of
    //    the bets when B = N b / (1 + b), for nuts N.
    let weight = |list: &[usize]| list.iter().map(|&h| bettor.weights()[h]).sum::<f64>();
    let (nut_weight, air_weight) = (weight(&nuts), weight(&air));
    let bluffs = nut_weight * b / (1.0 + b);
    assert!(
        air_weight > bluffs,
        "{air_weight} of air for {bluffs} of bluffs"
    );

    // The numbers the description and the README give: 6 nut combos, 16 of air and 12
    // bluff-catchers, all at full weight, and 2 combos of bluffs.
    assert_eq!((nuts.len(), air.len(), caller.len()), (6, 16, 12));
    assert_eq!((nut_weight, air_weight), (6.0, 16.0));
    assert!(caller.weights().iter().all(|&w| w == 1.0));
    assert_eq!(bluffs, 2.0);
    assert_eq!(bluffs / (nut_weight + bluffs), b / (1.0 + 2.0 * b));

    // The description records b and both frequencies.
    assert_eq!(b / (1.0 + 2.0 * b), 1.0 / 4.0);
    assert_eq!(1.0 / (1.0 + b), 2.0 / 3.0);
    for text in ["b = 0.5", "b / (1 + 2b) = 1/4", "1 / (1 + b) = 2/3"] {
        assert!(spot.description.contains(text), "{text}");
    }
}

// ---------------------------------------------------------------------------------------------
// The documents

#[test]
fn the_architecture_example_loads_and_writes_back_identically() {
    let doc = fs::read_to_string(repo_path("docs/ARCHITECTURE.md"))
        .unwrap()
        .replace("\r\n", "\n");
    let section = &doc[doc.find("### Spot file").expect("the section exists")..];
    let start = section
        .find("```toml\n")
        .expect("the section has an example")
        + "```toml\n".len();
    let example = &section[start..start + section[start..].find("```").unwrap()];
    let spot = Spot::from_toml(example).unwrap_or_else(|err| panic!("{err}"));
    assert_eq!(spot.to_toml(), example);
    spot.build_tree().unwrap();
}

/// The rows of the table in `reference/README.md` that follows `heading` and whose first cell
/// names a spot in backticks, as `(name, cells after the first)`.
fn readme_rows(heading: &str) -> Vec<(String, Vec<String>)> {
    let readme = fs::read_to_string(repo_path("reference/README.md"))
        .unwrap()
        .replace("\r\n", "\n");
    let section = &readme[readme
        .find(heading)
        .unwrap_or_else(|| panic!("the README has `{heading}`"))..];
    section
        .lines()
        .skip_while(|line| !line.starts_with('|'))
        .take_while(|line| line.starts_with('|'))
        .filter_map(|line| {
            let cells: Vec<String> = line
                .trim_matches('|')
                .split('|')
                .map(|cell| cell.trim().to_string())
                .collect();
            let name = cells[0].strip_prefix('`')?.strip_suffix('`')?.to_string();
            Some((name, cells[1..].to_vec()))
        })
        .collect()
}

fn names(rows: &[(String, Vec<String>)]) -> BTreeSet<String> {
    rows.iter().map(|(name, _)| name.clone()).collect()
}

fn spots_by_name() -> Vec<(String, Spot)> {
    spot_files()
        .into_iter()
        .map(|(_, spot, _)| (spot.name.clone(), spot))
        .collect()
}

#[test]
fn the_readme_table_of_spots_matches_the_files() {
    let rows = readme_rows("| Spot | Stresses |");
    let spots = spots_by_name();
    let files: BTreeSet<String> = spots.iter().map(|(name, _)| name.clone()).collect();
    assert_eq!(names(&rows), files);
    for (name, spot) in &spots {
        let cells = &rows.iter().find(|(row, _)| row == name).unwrap().1;
        // Stresses, Board, Pot, Stack, Tree, Rake, Nodes, OOP hands, IP hands.
        assert_eq!(cells.len(), 9, "{name}");
        let hands = spot.board_hands();
        let rake = if spot.rake_percent == 0.0 {
            "none".to_string()
        } else {
            format!("{}%, cap {}bb", spot.rake_percent, bb(spot.rake_cap))
        };
        let expected = [
            (1, spot.board.to_string()),
            (2, format!("{}bb", bb(spot.pot))),
            (3, format!("{}bb", bb(spot.effective_stack))),
            (5, rake),
            (6, spot.build_tree().unwrap().nodes().len().to_string()),
            (7, hands.hands(Player::Oop).len().to_string()),
            (8, hands.hands(Player::Ip).len().to_string()),
        ];
        for (column, value) in expected {
            assert_eq!(
                cells[column].trim_matches('`'),
                value,
                "{name}, column {}",
                column + 2
            );
        }
    }
}

#[test]
fn the_readme_table_of_amounts_matches_the_trees() {
    let rows = readme_rows("| Spot | Bets |");
    let spots = spots_by_name();
    let files: BTreeSet<String> = spots.iter().map(|(name, _)| name.clone()).collect();
    assert_eq!(names(&rows), files);
    for (name, spot) in &spots {
        let cells = &rows.iter().find(|(row, _)| row == name).unwrap().1;
        assert_eq!(cells.len(), 3, "{name}");
        // The street totals of each kind of wager, smallest first.
        let mut amounts: [BTreeSet<Chips>; 3] = Default::default();
        for node in spot.build_tree().unwrap().nodes() {
            if let Node::Player { actions, .. } = node {
                for action in actions {
                    match *action {
                        Action::Bet { to, .. } => amounts[0].insert(to),
                        Action::Raise { to, .. } => amounts[1].insert(to),
                        Action::AllIn { to } => amounts[2].insert(to),
                        Action::Fold | Action::Check | Action::Call => false,
                    };
                }
            }
        }
        for (cell, amounts) in cells.iter().zip(&amounts) {
            let expected = if amounts.is_empty() {
                "none".to_string()
            } else {
                let listed: Vec<String> = amounts.iter().map(|&to| bb(to)).collect();
                format!("{}bb", listed.join(", "))
            };
            assert_eq!(cell, &expected, "{name}");
        }
    }
}
