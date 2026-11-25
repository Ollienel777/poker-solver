//! The turn and flop test spots under `reference/spots/turn/` and `reference/spots/flop/`, the
//! fixtures of the M4 gate.
//!
//! What `river_spots.rs` does for the river spots, on every street: every file loads,
//! validates, builds its betting tree and its game, and writes back exactly as it is on disk.
//! Every range weight is a quarter, so any solver reads it exactly. Every tree can be reproduced
//! by a solver that rounds otherwise or has a smaller minimum bet: each bet and raise, on every
//! street and every line, is a whole number of chips before rounding and at least 1bb, and is an
//! action of its own, never turned into all-in or merged with another. The set covers what it is
//! meant to stress. And the tables in `reference/README.md` match the files.

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use pk_cfr::PublicGame;
use pk_core::{DealtBoard, NUM_CARDS, NUM_SUITS, SuitGroup};
use pk_game::{Action, CHIPS_PER_BB, Chips, Node, Player, PostflopTree, RangeFile, Street};
use pk_postflop::{PostflopGame, Spot};

fn repo_path(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
}

/// The folders of the fixtures, each with the cards on its boards.
const FOLDERS: [(&str, usize); 2] = [("turn", 4), ("flop", 3)];

/// The spot files of `reference/spots/<folder>`, with the text each holds on disk, sorted by
/// path.
fn spot_files(folder: &str) -> Vec<(PathBuf, Spot, String)> {
    let root = repo_path(&format!("reference/spots/{folder}"));
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

fn spots(folder: &str) -> Vec<Spot> {
    spot_files(folder)
        .into_iter()
        .map(|(_, spot, _)| spot)
        .collect()
}

/// Every turn spot, then every flop spot.
fn all_spots() -> Vec<Spot> {
    FOLDERS
        .iter()
        .flat_map(|(folder, _)| spots(folder))
        .collect()
}

/// An amount in big blinds without trailing zeros: `5.5`, `94.75` or `20`.
fn bb(chips: Chips) -> String {
    let (whole, cents) = (chips / CHIPS_PER_BB, chips % CHIPS_PER_BB);
    match cents {
        0 => whole.to_string(),
        tenths if tenths % 10 == 0 => format!("{whole}.{}", tenths / 10),
        cents => format!("{whole}.{cents:02}"),
    }
}

/// A count with a comma every three digits: `191,844`.
fn thousands(count: u64) -> String {
    let digits = count.to_string();
    let mut text = String::new();
    for (i, digit) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            text.push(',');
        }
        text.push(digit);
    }
    text
}

/// The number of nodes of the public tree below `index`: the betting tree with a child at each
/// chance node for every card off the `cards` already on the board, counted here from the
/// betting tree alone.
fn public_nodes(tree: &PostflopTree, index: u32, cards: usize) -> u64 {
    match tree.node(index) {
        Node::Player { children, .. } => {
            1 + children
                .iter()
                .map(|&child| public_nodes(tree, child, cards))
                .sum::<u64>()
        }
        Node::Chance { child, .. } => {
            1 + (NUM_CARDS - cards) as u64 * public_nodes(tree, *child, cards + 1)
        }
        Node::Terminal { .. } => 1,
    }
}

/// The relabellings of the suits that keep the spot's board.
fn symmetries(spot: &Spot) -> SuitGroup {
    DealtBoard::new(spot.board).unwrap().symmetries()
}

/// The suits that the board's symmetries move, in classes of suits that can be exchanged for
/// one another, as `d, s` or `c, d, h`, or `none`.
fn suits_alike(spot: &Spot) -> String {
    let group = symmetries(spot);
    let classes: BTreeSet<Vec<u8>> = (0..NUM_SUITS as u8)
        .map(|suit| {
            group
                .iter()
                .map(|p| p.image(suit))
                .collect::<BTreeSet<u8>>()
                .into_iter()
                .collect::<Vec<u8>>()
        })
        .filter(|class| class.len() > 1)
        .collect();
    if classes.is_empty() {
        return "none".to_string();
    }
    let names: Vec<String> = classes
        .iter()
        .map(|class| {
            let suits: Vec<String> = class
                .iter()
                .map(|&suit| ['c', 'd', 'h', 's'][suit as usize].to_string())
                .collect();
            suits.join(", ")
        })
        .collect();
    names.join("; ")
}

// ---------------------------------------------------------------------------------------------
// The files

#[test]
fn every_turn_and_flop_spot_loads_builds_and_writes_back_identically() {
    let mut names = BTreeSet::new();
    for (folder, cards) in FOLDERS {
        let files = spot_files(folder);
        assert!(
            (3..=5).contains(&files.len()),
            "the {folder} set has 3 to 5 spots, not {}",
            files.len()
        );
        for (path, spot, text) in &files {
            let shown = path.display();
            let stem = path.file_stem().unwrap().to_str().unwrap();
            assert_eq!(spot.name, stem, "{shown}: `name` must match the file name");
            // The comparison names each dump after its spot, across both folders.
            assert!(
                names.insert(spot.name.clone()),
                "{shown}: a second `{stem}`"
            );
            assert_eq!(spot.board.len(), cards, "{shown}: a {folder} spot");
            spot.validate()
                .unwrap_or_else(|err| panic!("{shown}: {err}"));

            let tree = spot
                .build_tree()
                .unwrap_or_else(|err| panic!("{shown}: {err}"));
            assert_eq!(tree.counts().nodes(), tree.nodes().len(), "{shown}");
            let hands = spot.board_hands();
            for player in [Player::Oop, Player::Ip] {
                assert!(!hands.hands(player).is_empty(), "{shown}: {player}");
            }

            assert_eq!(
                &spot.to_toml(),
                text,
                "{shown} is not in the written layout"
            );
            let description = spot.description.to_lowercase();
            assert!(
                description.contains("illustrative"),
                "{shown}: the description must say the ranges are illustrative"
            );
            assert!(
                description.contains("2026-11-24"),
                "{shown}: the description must give the date the ranges were written"
            );
        }
    }
}

#[test]
fn every_game_builds_within_the_default_node_limit() {
    // `PostflopGame::from_spot` refuses a public tree of more than a million nodes, which the
    // gate's solves use. Each game is the public tree counted from the betting tree here.
    for spot in all_spots() {
        let tree = spot.build_tree().unwrap();
        let expected = public_nodes(&tree, PostflopTree::ROOT, spot.board.len());
        assert!(
            expected <= PostflopGame::DEFAULT_MAX_NODES,
            "{}: {expected} public nodes",
            spot.name
        );
        let game =
            PostflopGame::from_spot(&spot).unwrap_or_else(|err| panic!("{}: {err}", spot.name));
        assert_eq!(game.num_nodes() as u64, expected, "{}", spot.name);
    }
}

#[test]
fn every_range_weight_is_a_quarter_so_that_any_solver_reads_it_exactly() {
    // 0.25, 0.5, 0.75 and 1 are exact in `f32` and `f64` alike, so a reference solver that
    // parses the weights in another float type holds the same ranges.
    for spot in all_spots() {
        for player in [Player::Oop, Player::Ip] {
            for (combo, weight) in spot.range(player).iter() {
                assert_eq!(
                    (weight * 4.0).fract(),
                    0.0,
                    "{}: {combo} has weight {weight}",
                    spot.name
                );
            }
        }
    }
}

#[test]
fn the_ranges_keep_every_symmetry_of_the_board() {
    // Ranges written in hand classes are kept by every relabelling of the suits, so the cards
    // that a symmetry of the board exchanges lead to equal subgames. A range that named a
    // single suited combo would break that.
    for spot in all_spots() {
        let board = DealtBoard::new(spot.board).unwrap();
        assert_eq!(
            board.symmetries_with_ranges([&spot.oop_range, &spot.ip_range]),
            board.symmetries(),
            "{}",
            spot.name
        );
    }
}

// ---------------------------------------------------------------------------------------------
// Trees a reference solver can reproduce

/// The chips each player has put in, in all and on the current street, and the raises made on
/// the current street.
#[derive(Debug, Clone, Copy, Default)]
struct Betting {
    total: [Chips; 2],
    street: [Chips; 2],
    raises: u32,
}

/// What a walk of a spot's tree found.
#[derive(Debug, Default)]
struct Walked {
    /// The bets and raises checked against the sizes they came from.
    checked: usize,
    /// For each street (flop, turn, river), the street totals of the bets, raises and all-ins.
    amounts: [[BTreeSet<Chips>; 3]; 3],
    /// At each terminal, the pot both players matched: what the rake is taken from.
    matched: Vec<Chips>,
    /// The calls of an all-in before the river, after which the cards run out.
    allins_called_early: usize,
}

fn street_index(street: Street) -> usize {
    match street {
        Street::Flop => 0,
        Street::Turn => 1,
        Street::River => 2,
    }
}

/// Walks the tree from `index`, checking each node's actions against the street's sizing and
/// each wager against the size it came from.
fn walk(spot: &Spot, tree: &PostflopTree, index: u32, betting: Betting, walked: &mut Walked) {
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
            // Every size is an action of its own: none was lifted to a minimum, turned into
            // all-in or merged with another, so a solver with other rules for those builds
            // the same node. Only an all-in faced or the raise cap leaves fold and call alone.
            let expected = if faced == 0 {
                1 + sizing.bets.len() + usize::from(sizing.allin)
            } else if betting.total[them] == spot.effective_stack
                || betting.raises >= spot.tree.max_raises
            {
                2
            } else {
                2 + sizing.raises.len() + usize::from(sizing.allin)
            };
            assert_eq!(
                actions.len(),
                expected,
                "{shown}: {actions:?} on the {street}, from {betting:?}"
            );
            for (&action, &child) in actions.iter().zip(children) {
                // The size before any rounding, as a solver that sizes in fractions finds it.
                let exact = match action {
                    Action::Bet { fraction, .. } => {
                        assert_eq!(faced, 0, "{shown}: a bet faces no bet");
                        Some(fraction * f64::from(pot))
                    }
                    Action::Raise { multiple, .. } => Some(multiple * f64::from(faced)),
                    Action::AllIn { to } => {
                        // With `allin` off, an all-in could only come from a size the builder
                        // turned into one, which another solver might drop instead.
                        assert!(sizing.allin, "{shown}: an all-in on the {street}");
                        assert_eq!(
                            to,
                            spot.effective_stack - (betting.total[me] - betting.street[me]),
                            "{shown}: all-in is all that was left when the street began"
                        );
                        None
                    }
                    Action::Fold | Action::Check | Action::Call => None,
                };
                if let Some(exact) = exact {
                    let to = action.to().unwrap();
                    assert!(
                        (exact - f64::from(to)).abs() < 1e-6,
                        "{shown}: {action} on the {street} goes to {to} chips, but its size \
                         gives {exact}"
                    );
                    assert!(
                        to - betting.street[me] >= CHIPS_PER_BB,
                        "{shown}: {action} on the {street} puts in less than 1bb"
                    );
                    walked.checked += 1;
                }
                let slot = match action {
                    Action::Bet { .. } => Some(0),
                    Action::Raise { .. } => Some(1),
                    Action::AllIn { .. } => Some(2),
                    Action::Fold | Action::Check | Action::Call => None,
                };
                if let (Some(slot), Some(to)) = (slot, action.to()) {
                    walked.amounts[street_index(*street)][slot].insert(to);
                }

                let mut next = betting;
                if let Some(to) = action.to() {
                    next.total[me] += to - betting.street[me];
                    next.street[me] = to;
                    if faced > 0 {
                        next.raises += 1;
                    }
                } else if action == Action::Call {
                    next.total[me] += faced - betting.street[me];
                    next.street[me] = faced;
                    if *street != Street::River && next.total[me] == spot.effective_stack {
                        walked.allins_called_early += 1;
                    }
                }
                walk(spot, tree, child, next, walked);
            }
        }
        Node::Chance { child, .. } => {
            let next = Betting {
                street: [0, 0],
                raises: 0,
                ..betting
            };
            walk(spot, tree, *child, next, walked);
        }
        Node::Terminal { contributions, .. } => {
            // The walk kept the same books as the builder.
            assert_eq!(*contributions, betting.total, "{shown}");
            walked
                .matched
                .push(spot.pot + 2 * contributions[0].min(contributions[1]));
        }
    }
}

fn walked(spot: &Spot) -> Walked {
    let tree = spot.build_tree().unwrap();
    let mut walked = Walked::default();
    walk(
        spot,
        &tree,
        PostflopTree::ROOT,
        Betting::default(),
        &mut walked,
    );
    walked
}

#[test]
fn every_bet_and_raise_is_a_whole_number_of_chips_and_at_least_1bb_before_rounding() {
    for spot in all_spots() {
        // A threshold of 1 turns no size into all-in.
        assert_eq!(spot.tree.allin_threshold, 1.0, "{}", spot.name);
        let walked = walked(&spot);
        assert!(walked.checked > 0, "{}", spot.name);
        // Every street the spot plays has a bet.
        for street in Street::ALL.into_iter().filter(|&s| s >= spot.street()) {
            assert!(
                !walked.amounts[street_index(street)][0].is_empty(),
                "{}: no bet on the {street}",
                spot.name
            );
        }
    }
}

#[test]
fn every_raked_spot_takes_5_percent_with_a_cap_that_binds_in_some_pots_and_not_others() {
    // The configs cap the rake at 6bb (DECISIONS D16), which no pot of a single-raised flop
    // tree this small reaches, so each spot's cap is set where it binds in its larger pots and
    // not its smaller ones. No pot is raked exactly at the cap, where a solver that rounds the
    // rake differently could part.
    for (folder, _) in FOLDERS {
        let (mut free, mut raked) = (0usize, 0usize);
        for spot in spots(folder) {
            if spot.rake_percent == 0.0 {
                assert_eq!(spot.rake_cap, 0, "{}", spot.name);
                free += 1;
                continue;
            }
            raked += 1;
            assert_eq!(spot.rake_percent, 5.0, "{}", spot.name);
            let rakes: Vec<f64> = walked(&spot)
                .matched
                .iter()
                .map(|&pot| spot.rake_percent / 100.0 * f64::from(pot))
                .collect();
            let cap = f64::from(spot.rake_cap);
            assert!(
                rakes.iter().any(|&rake| rake > cap),
                "{}: the cap never binds",
                spot.name
            );
            assert!(
                rakes.iter().any(|&rake| rake < cap),
                "{}: the cap always binds",
                spot.name
            );
            assert!(
                rakes.iter().all(|&rake| rake != cap),
                "{}: a pot is raked at the cap exactly",
                spot.name
            );
        }
        assert!(
            free > 0 && raked > 0,
            "the {folder} spots are {free} rake-free and {raked} raked"
        );
    }
}

#[test]
fn the_set_covers_what_it_is_meant_to_stress() {
    let (turns, flops) = (spots("turn"), spots("flop"));
    let range_file = |name: &str| RangeFile::load(repo_path(name)).unwrap();
    let srp = range_file("reference/ranges/a/BTN_vs_BB_SRP.toml");
    let three_bet = range_file("reference/ranges/a/BB_vs_BTN_3BP.toml");

    // Every spot is on a BTN vs BB line at 100bb, in a single-raised pot or a 3-bet pot: the
    // chips in the pot and behind are those of the line's range file.
    for spot in turns.iter().chain(&flops) {
        let chips = spot.pot + 2 * spot.effective_stack;
        assert!(
            [&srp, &three_bet]
                .iter()
                .any(|line| chips == line.pot + 2 * line.effective_stack && spot.pot >= line.pot),
            "{}: not on either line",
            spot.name
        );
    }

    // The suits a later merging of isomorphic cards is checked on: a flop on which no two
    // suits are alike, one on which two are, and one on which three are.
    let alike: BTreeSet<usize> = flops
        .iter()
        .map(|spot| {
            let group = symmetries(spot);
            (0..NUM_SUITS as u8)
                .filter(|&suit| group.iter().any(|p| p.image(suit) != suit))
                .count()
        })
        .collect();
    for count in [0, 2, 3] {
        assert!(alike.contains(&count), "a flop with {count} suits alike");
    }

    // The flop of the gate, and the others with the same ranges: the repository's illustrative
    // single-raised-pot ranges, whole, at that line's pot and stack.
    let full_ranges = |spot: &&Spot| {
        spot.oop_range == srp.oop.range
            && spot.ip_range == srp.ip.range
            && spot.pot == srp.pot
            && spot.effective_stack == srp.effective_stack
    };
    assert!(
        flops.iter().filter(full_ranges).count() >= 3,
        "three flops with the full ranges"
    );
    assert!(
        flops.iter().filter(full_ranges).any(|spot| {
            [Street::Flop, Street::Turn, Street::River]
                .iter()
                .all(|&street| {
                    let sizing = spot.tree.street(street).unwrap();
                    sizing.bets == [0.5] && sizing.raises.is_empty() && !sizing.allin
                })
                && spot.tree.max_raises == 0
                && spot.rake_percent == 0.0
        }),
        "the gate's flop: the full ranges, a half-pot bet on each street, no raises, no rake"
    );
    let raises_on =
        |spot: &Spot, street: Street| !walked(spot).amounts[street_index(street)][1].is_empty();
    assert!(
        flops.iter().any(|spot| {
            let hands = spot.board_hands();
            raises_on(spot, Street::Flop)
                && [Player::Oop, Player::Ip]
                    .iter()
                    .all(|&player| (100..=130).contains(&hands.hands(player).len()))
        }),
        "a flop with a raise and narrower ranges, of 100 to 130 hands a side"
    );

    // The turn spots: what the flops leave out.
    let river_bets = |spot: &Spot| spot.tree.river.clone().unwrap().bets;
    assert!(
        turns
            .iter()
            .any(|spot| spot.tree.turn.clone().unwrap().bets.len() == 2
                && spot.rake_percent == 0.0
                && spot.oop_range == srp.oop.range
                && spot.pot == srp.pot),
        "a rake-free turn with two bet sizes after the flop was checked through, with BB's \
         whole range"
    );
    assert!(
        turns.iter().any(|spot| raises_on(spot, Street::Turn)
            && walked(spot).allins_called_early > 0
            && river_bets(spot).iter().any(|&f| f != 0.5 && f != 1.0)),
        "a turn with a raise, an all-in called before the river, and a river size that is \
         neither a half nor a whole pot"
    );
    assert!(
        turns.iter().any(|spot| spot.pot >= three_bet.pot
            && 2 * spot.effective_stack <= 5 * spot.pot
            && [Street::Turn, Street::River].iter().all(|&street| spot
                .tree
                .street(street)
                .unwrap()
                .allin)),
        "a 3-bet pot with a stack-to-pot ratio of 2.5 or less and all-in on both streets"
    );
    assert!(
        flops
            .iter()
            .any(|spot| spot.effective_stack >= 10 * spot.pot),
        "a stack-to-pot ratio of 10 or more"
    );
}

// ---------------------------------------------------------------------------------------------
// The README

/// The rows of the table in the section of `reference/README.md` headed `section` whose header
/// starts with `header`, as `(spot, cells after the first)`. A row whose first cell is empty
/// continues the spot of the row above.
fn readme_rows(section: &str, header: &str) -> Vec<(String, Vec<String>)> {
    let readme = fs::read_to_string(repo_path("reference/README.md"))
        .unwrap()
        .replace("\r\n", "\n");
    let section = &readme[readme
        .find(section)
        .unwrap_or_else(|| panic!("the README has `{section}`"))..];
    let table = &section[section
        .find(header)
        .unwrap_or_else(|| panic!("the README's section has `{header}`"))..];
    let mut name = String::new();
    table
        .lines()
        .take_while(|line| line.starts_with('|'))
        .skip(2)
        .map(|line| {
            let cells: Vec<String> = line
                .trim_matches('|')
                .split('|')
                .map(|cell| cell.trim().to_string())
                .collect();
            if !cells[0].is_empty() {
                name = cells[0]
                    .strip_prefix('`')
                    .and_then(|cell| cell.strip_suffix('`'))
                    .unwrap_or_else(|| panic!("`{}` names no spot", cells[0]))
                    .to_string();
            }
            (name.clone(), cells[1..].to_vec())
        })
        .collect()
}

const SECTION: &str = "## Turn and flop spots";

/// Every spot's name, once.
fn names() -> BTreeSet<String> {
    all_spots().into_iter().map(|spot| spot.name).collect()
}

#[test]
fn the_readme_table_of_turn_and_flop_spots_matches_the_files() {
    let rows = readme_rows(SECTION, "| Spot | Stresses |");
    let listed: BTreeSet<String> = rows.iter().map(|(name, _)| name.clone()).collect();
    assert_eq!(listed.len(), rows.len(), "the table lists each spot once");
    assert_eq!(listed, names());
    for spot in all_spots() {
        let (name, cells) = rows.iter().find(|(name, _)| *name == spot.name).unwrap();
        let spot = &spot;
        // Stresses, Board, Pot, Stack, Tree, Rake, Suits alike, Betting nodes, Public nodes,
        // OOP hands, IP hands.
        assert_eq!(cells.len(), 11, "{name}");
        let tree = spot.build_tree().unwrap();
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
            (6, suits_alike(spot)),
            (7, thousands(tree.nodes().len() as u64)),
            (
                8,
                thousands(public_nodes(&tree, PostflopTree::ROOT, spot.board.len())),
            ),
            (9, hands.hands(Player::Oop).len().to_string()),
            (10, hands.hands(Player::Ip).len().to_string()),
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
    // A row for each street a spot plays, in street order, the spot named on its first.
    let mut rows = readme_rows(SECTION, "| Spot | Street | Bets |");
    let mut listed = Vec::new();
    for (name, _) in &rows {
        if listed.last() != Some(name) {
            listed.push(name.clone());
        }
    }
    assert_eq!(
        listed.len(),
        listed.iter().collect::<BTreeSet<_>>().len(),
        "each spot's rows are together"
    );
    assert_eq!(listed.into_iter().collect::<BTreeSet<_>>(), names());
    // Rows of the same spot keep their order through a stable sort.
    rows.sort_by(|a, b| a.0.cmp(&b.0));
    let mut expected = Vec::new();
    let mut spots = all_spots();
    spots.sort_by(|a, b| a.name.cmp(&b.name));
    for spot in spots {
        let walked = walked(&spot);
        for street in Street::ALL.into_iter().filter(|&s| s >= spot.street()) {
            let cells: Vec<String> = walked.amounts[street_index(street)]
                .iter()
                .map(|amounts| {
                    if amounts.is_empty() {
                        "none".to_string()
                    } else {
                        let listed: Vec<String> = amounts.iter().map(|&to| bb(to)).collect();
                        format!("{}bb", listed.join(", "))
                    }
                })
                .collect();
            let mut row = vec![street.to_string()];
            row.extend(cells);
            expected.push((spot.name.clone(), row));
        }
    }
    assert_eq!(rows, expected);
}
