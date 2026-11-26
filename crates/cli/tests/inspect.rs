//! Runs `solver inspect` on solution files that `solver solve-postflop` writes, as a user would,
//! and checks what it prints against the library's numbers for the same files: the frequencies,
//! every cell of the grids, the EVs and the combos. A river left out of a file is checked
//! against the library's own solve of that river.
//!
//! M5's gate: a range file, `solve-postflop`, the file saved, reloaded, and `inspect` printing a
//! grid. The fast tests do it on a river board and on a small turn spot saved without its
//! river; `the_gate_flop_solved_saved_and_inspected_at_every_street` does it on the flop of the
//! gate, and is slow.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use pk_core::HandClass;
use pk_game::{Player, Street};
use pk_postflop::memory::format_bytes;
use pk_postflop::{
    Payoff, SavedSolution, Solution, SolutionDump, SolveOptions, Spot, node_values, read_header,
    solve,
};

mod common;

use common::*;

const ONE_BET: &str = "configs/trees/one_bet.toml";
const FLOP_SMALL: &str = "crates/postflop/tests/data/flop_small.toml";

/// The target and the iteration cap `inspect` solves a left-out river again to.
const RIVER_TARGET: f64 = 0.01;
const RIVER_ITERATIONS: u64 = 1_000;

// ---------------------------------------------------------------------------------------------
// Solving and reading what inspect printed

/// Solves a spot through the command line and saves it to `out`, holding the streets up to
/// `last_street`.
fn solve_to(spot: &[&str], out: &Path, last_street: &str) {
    let mut args = vec!["solve-postflop"];
    args.extend(spot);
    args.extend(["--no-log", "--out", arg(out), "--last-street", last_street]);
    solver(&args).succeeded();
}

/// `inspect` of `file`, with `options`, which must succeed.
fn inspect(file: &Path, options: &[&str]) -> Run {
    let mut args = vec!["inspect", arg(file)];
    args.extend(options);
    solver(&args).succeeded()
}

/// The grid printed under the first line that starts with `title`: each class's cell, for the
/// classes whose cell is not blank.
fn grid(run: &Run, title: &str) -> HashMap<HandClass, String> {
    let lines: Vec<&str> = run.stdout.lines().collect();
    let at = lines
        .iter()
        .position(|line| line.starts_with(title))
        .unwrap_or_else(|| panic!("no grid {title:?} in\n{}", run.stdout));
    let header = lines[at + 1];
    assert!(header.trim_start().starts_with('A'), "{header}");
    let width = (header.len() - 3) / 13;
    assert_eq!(header.len(), 3 + 13 * width, "{header}");
    let mut cells = HashMap::new();
    for (row, rank) in "AKQJT98765432".chars().enumerate() {
        let line = lines[at + 2 + row];
        assert!(line.starts_with(&format!("  {rank}")), "{line}");
        for column in 0..13 {
            let start = 3 + width * column;
            let end = (start + width).min(line.len());
            let cell = line.get(start..end).unwrap_or("").trim();
            if !cell.is_empty() {
                cells.insert(HandClass::from_index(13 * row + column), cell.to_string());
            }
        }
    }
    cells
}

/// The two frequencies printed on the line of the action `token`: taken, and of range.
fn action_line(run: &Run, token: &str) -> (String, String) {
    let lines: Vec<&str> = run.stdout.lines().collect();
    let header = lines
        .iter()
        .position(|line| line.starts_with("  action "))
        .unwrap_or_else(|| panic!("no actions in\n{}", run.stdout));
    let line = lines[header + 1..]
        .iter()
        .take_while(|line| line.starts_with("  "))
        .find(|line| line.split_whitespace().next() == Some(token))
        .unwrap_or_else(|| panic!("no action {token} in\n{}", run.stdout));
    let words: Vec<&str> = line.split_whitespace().collect();
    (
        words[words.len() - 2].to_string(),
        words[words.len() - 1].to_string(),
    )
}

/// The rows of the breakdown, by combo: every column after the combo.
fn breakdown(run: &Run) -> HashMap<String, Vec<String>> {
    let lines: Vec<&str> = run.stdout.lines().collect();
    let header = lines
        .iter()
        .position(|line| line.starts_with("  combo "))
        .unwrap_or_else(|| panic!("no breakdown in\n{}", run.stdout));
    lines[header + 1..]
        .iter()
        .take_while(|line| line.starts_with("  "))
        .map(|line| {
            let words: Vec<String> = line.split_whitespace().map(str::to_string).collect();
            (words[0].clone(), words[1..].to_vec())
        })
        .collect()
}

/// Everything after the file's description: the node.
fn node_part(run: &Run) -> &str {
    let start = run.stdout.find("\nNode ").expect("a node is shown");
    &run.stdout[start..]
}

// ---------------------------------------------------------------------------------------------
// What the library gives

/// A frequency as `inspect` prints it in its table.
fn percent(frequency: f64) -> String {
    format!("{:.2}%", 100.0 * frequency)
}

/// A frequency as a grid cell: a whole percent.
fn percent_cell(frequency: f64) -> String {
    format!("{}", (100.0 * frequency).round() + 0.0)
}

/// An EV in chips as a grid cell: big blinds to one decimal.
fn ev_cell(chips: f64) -> String {
    format!("{:.1}", ((10.0 * chips / 100.0).round() + 0.0) / 10.0)
}

/// What `inspect` shows at a decision, from the library: the actions with their two
/// frequencies, a grid per action, the grid of the EVs, each range in combos and how often play
/// reaches the node.
struct Expected {
    actions: Vec<(String, String, String)>,
    grids: Vec<HashMap<HandClass, String>>,
    evs: HashMap<HandClass, String>,
    combos: [f64; 2],
    reached: f64,
}

/// [`Expected`] at the player node `node` of `solution`, whose acting player's EV with hand `h`
/// is `ev(h)`, in chips from the start of the spot, and which play reaches with `reach_scale`
/// times the probability the solution's own game gives.
fn expected(
    solution: &Solution,
    node: u32,
    ev: &dyn Fn(usize) -> Option<f64>,
    reach_scale: f64,
) -> Expected {
    let game = solution.game();
    let actor = game.actor(node).expect("a decision");
    let reach = solution.reach(node);
    let combos = [Player::Oop, Player::Ip].map(|player| {
        let weights = solution.hands(player).weights();
        weights
            .iter()
            .zip(&reach[player.index()])
            .map(|(w, r)| w * r)
            .sum()
    });
    let taken = solution.action_frequencies(node).unwrap();
    let range = solution.range_view_frequencies(node).unwrap();
    let actions = game
        .actions(node)
        .iter()
        .enumerate()
        .map(|(a, action)| (action.to_string(), percent(taken[a]), percent(range[a])))
        .collect();
    let classes = solution.range_view_classes(node).unwrap();
    let grids = (0..taken.len())
        .map(|a| {
            classes
                .iter()
                .map(|class| (class.class, percent_cell(class.frequencies[a])))
                .collect()
        })
        .collect();
    let weights = solution.hands(actor).weights();
    let evs = classes
        .iter()
        .map(|class| {
            let (mut total, mut sum) = (0.0, 0.0);
            for &hand in &class.hands {
                let w = weights[hand] * reach[actor.index()][hand];
                if let (true, Some(ev)) = (w > 0.0, ev(hand)) {
                    total += w;
                    sum += w * ev;
                }
            }
            let cell = if total > 0.0 {
                ev_cell(sum / total)
            } else {
                "-".to_string()
            };
            (class.class, cell)
        })
        .collect();
    Expected {
        actions,
        grids,
        evs,
        combos,
        reached: reach_scale * solution.reach_probability(node),
    }
}

/// The EVs of `saved` at `node`, as the file holds them, by hand.
fn file_evs(saved: &SavedSolution, node: u32) -> impl Fn(usize) -> Option<f64> + '_ {
    move |hand| {
        let ev = saved.ev(node).unwrap()[hand];
        (!ev.is_nan()).then_some(f64::from(ev))
    }
}

/// Checks that `run`, `inspect` at a decision, shows `expected`: both frequencies of every
/// action, every cell of every grid, each range in combos, and how often play gets there.
fn check_decision(run: &Run, expected: &Expected) {
    let [oop, ip] = expected.combos;
    assert!(
        run.line("Ranges       ")
            .starts_with(&format!("Ranges       OOP {oop:.2} combos, IP {ip:.2}: ")),
        "{}",
        run.line("Ranges")
    );
    let reached = 100.0 * expected.reached;
    let reached_line = run.line("Reached      ");
    let printed: f64 = reached_line["Reached      ".len()..]
        .split('%')
        .next()
        .unwrap()
        .parse()
        .unwrap();
    // Two decimals from 1% up, three significant digits below.
    let close = if reached >= 1.0 {
        (printed - reached).abs() <= 0.0051
    } else {
        (printed - reached).abs() <= 0.006 * reached
    };
    assert!(close, "{reached_line}, against {reached}%");
    for (index, (token, taken, range)) in expected.actions.iter().enumerate() {
        assert_eq!(
            action_line(run, token),
            (taken.clone(), range.clone()),
            "action {token}"
        );
        let cells = grid(run, &format!("{token} ("));
        assert_eq!(cells, expected.grids[index], "the grid of {token}");
    }
    assert_eq!(grid(run, "EV in bb"), expected.evs, "the grid of the EVs");
}

// ---------------------------------------------------------------------------------------------
// A river spot from a range file

/// A river spot of the gate's range file, solved, saved and inspected: the file's description,
/// and at the root, after a bet and at the end of the hand, every number against the library's.
#[test]
fn a_river_solution_from_a_range_file_is_inspected_grid_by_grid() {
    let folder = scratch("inspect-river");
    let file = folder.join("river.pksol");
    solve_to(
        &[
            "--ranges",
            RANGES,
            "--board",
            "Ks7d2c9h3s",
            "--tree",
            RIVER_TREE,
        ],
        &file,
        "river",
    );
    let saved = SavedSolution::load(&file).unwrap();
    let header = saved.header();
    let solution = saved.solution();

    // The file and its spot.
    let run = inspect(&file, &[]);
    run.prints(&[
        &format!(
            "File         `{}`, {}, written by pk-postflop {}",
            file.display(),
            format_bytes(fs::metadata(&file).unwrap().len()),
            env!("CARGO_PKG_VERSION")
        ),
        "Holds        the river",
        "Spot         BTN_vs_BB_SRP_Ks9h7d3s2c",
        "Board        Ks9h7d3s2c, starting on the river",
        "Pot          5.5bb, with 97.5bb behind each player",
        "Rake         5% of the pot, capped at 6bb",
        &format!(
            "Tree         {} nodes, {} of them where a player acts",
            header.nodes, header.player_nodes
        ),
        "  river      bets 50% and 100% of the pot",
        "  rules      no raises",
        &format!(
            "Hands        OOP {}, IP {}, of the combos the board leaves",
            header.hands[0], header.hands[1]
        ),
        &format!(
            "Stopped      at the target, after {} iterations",
            header.iterations
        ),
        &format!(
            "Exploitable  {:.4}% of the pot, {:.4}bb per hand dealt, as stored, in 16 bits a \
             probability; {:.4}% as solved",
            header.exploitability.percent_of_pot,
            header.exploitability.chips / 100.0,
            header.solve_exploitability.percent_of_pot
        ),
        &format!(
            "Values       OOP {:.2}bb, IP {:.2}bb, per hand dealt",
            header.exploitability.values[0] / 100.0,
            header.exploitability.values[1] / 100.0
        ),
        "Node         the root, on the river, Ks9h7d3s2c",
        "Reached      100.00% of the hands dealt",
        "Acts         OOP",
        "  \"x\"     IP acts",
    ]);
    assert!(!run.stdout.contains("Players"), "a file knows no positions");
    assert!(!run.stdout.contains("River "), "the file holds the river");

    // The root: every frequency and every cell, from the file.
    let root = 0;
    let at_root = expected(solution, root, &file_evs(&saved, root), 1.0);
    check_decision(&run, &at_root);
    // A few cells by name, as a reader would look them up: a class in OOP's range, with its
    // frequencies summing to about 100, and the classes outside it left blank.
    let classes = solution.range_view_classes(root).unwrap();
    let class = classes[classes.len() / 2].class;
    let cells: Vec<f64> = ["x (", "b50 (", "b100 ("]
        .iter()
        .map(|title| grid(&run, title)[&class].parse().unwrap())
        .collect();
    assert!(
        (cells.iter().sum::<f64>() - 100.0).abs() <= 2.0,
        "{cells:?}"
    );
    let blank = HandClass::all()
        .find(|class| !classes.iter().any(|c| c.class == *class))
        .expect("OOP's range leaves out a class");
    assert!(!grid(&run, "x (").contains_key(&blank));

    // IP facing the half-pot bet, with the combos of one class.
    let node = solution.game().find("b50").unwrap();
    let classes = solution.range_view_classes(node).unwrap();
    let class = classes
        .iter()
        .find(|class| class.hands.len() > 1)
        .expect("a class of more than one combo")
        .class;
    let name = class.to_string();
    let run = inspect(&file, &["--node", "b50", "--hand", &name]);
    run.prints(&[
        "Node         `b50`, on the river, Ks9h7d3s2c",
        "Pot          8.25bb; OOP has put in 2.75bb and has 94.75bb behind, IP 0bb and 97.5bb",
        "Acts         IP",
        "  \"b50 f\"  the hand ends: IP folds, and OOP wins the pot",
        "  \"b50 c\"  the hand ends: at a showdown",
    ]);
    check_decision(
        &run,
        &expected(solution, node, &file_evs(&saved, node), 1.0),
    );
    let rows = breakdown(&run);
    let hands = solution.hands(Player::Ip);
    let reach = solution.reach(node);
    let evs = saved.ev(node).unwrap();
    let mut seen = 0;
    for combo in class.combos() {
        let Some(hand) = hands.position(combo) else {
            continue;
        };
        let row = &rows[&combo.to_string()];
        let strategy = solution.hand_strategy(node, hand).unwrap();
        let mut wanted = vec![format!("{:.3}", hands.weights()[hand] * reach[1][hand])];
        wanted.extend(strategy.iter().map(|&p| percent(f64::from(p))));
        wanted.push(format!("{:.2}bb", f64::from(evs[hand]) / 100.0));
        assert_eq!(row, &wanted, "{combo}");
        seen += 1;
    }
    assert_eq!(rows.len(), seen);
    let weight: f64 = class
        .combos()
        .filter_map(|combo| hands.position(combo))
        .map(|hand| hands.weights()[hand] * reach[1][hand])
        .sum();
    let line = run.line(&format!("{name}: "));
    assert!(
        line.starts_with(&format!(
            "{name}: {seen} combos among IP's hands here, {weight:.3} by their weight in the \
             range, "
        )),
        "{line}"
    );
    assert!(line.ends_with("% of it"), "{line}");

    // OOP facing a bet after checking, where its combos of one class reach the node by
    // different amounts, so that the EVs are averaged by weight.
    let node = solution.game().find("x b50").unwrap();
    let run = inspect(&file, &["--node", "x b50"]);
    check_decision(
        &run,
        &expected(solution, node, &file_evs(&saved, node), 1.0),
    );

    // The end of the hand.
    let run = inspect(&file, &["--node", "b50 f"]);
    let fold = solution.game().find("b50 f").unwrap();
    let Some(Payoff::Fold([oop, ip])) = solution.game().payoff(fold) else {
        panic!("a fold");
    };
    run.prints(&[
        "Node         `b50 f`, on the river, Ks9h7d3s2c",
        "Ends         IP folds, and OOP wins the pot",
        &format!(
            "Payoffs      OOP {:.2}bb, IP {:.2}bb, from the start of the spot, the rake taken",
            oop / 100.0,
            ip / 100.0 + 0.0
        ),
        "Next         nothing: the hand is over",
    ]);
    let run = inspect(&file, &["--node", "x x"]);
    run.prints(&["Ends         at a showdown"]);
    fs::remove_dir_all(&folder).unwrap();
}

// ---------------------------------------------------------------------------------------------
// A turn spot without its river

/// `turn_small` solved and saved twice, whole and without its river, in `folder`.
fn turn_files(folder: &Path) -> (PathBuf, PathBuf) {
    let (whole, partial) = (folder.join("whole.pksol"), folder.join("turn.pksol"));
    solve_to(&["--spot", TURN], &whole, "river");
    solve_to(&["--spot", TURN], &partial, "turn");
    (whole, partial)
}

/// The river of `saved` that starts at the node `start`, solved as `inspect` solves it.
fn river_solved(saved: &SavedSolution, start: u32) -> (pk_postflop::RiverSpot, Solution) {
    let river = saved.river_spot(start).unwrap();
    let options = SolveOptions {
        target: Some(RIVER_TARGET),
        max_iterations: RIVER_ITERATIONS,
        ..SolveOptions::default()
    };
    let solved = solve(&river.spot, &options, |_| {}).unwrap();
    (river, solved)
}

/// Checks `inspect` at the river node `path` of `partial`, a file without its river, against
/// the library's own solve of that river, which starts at `start`.
fn check_river_solved_again(partial: &Path, start: &str, path: &str, hand: Option<&str>) -> Run {
    let saved = SavedSolution::load(partial).unwrap();
    let game = saved.solution().game();
    let start_node = game.find(start).unwrap();
    let (river, solved) = river_solved(&saved, start_node);
    let rest: Vec<&str> = path
        .split_whitespace()
        .skip(start.split_whitespace().count())
        .collect();
    let node = solved.game().find(&rest.join(" ")).unwrap();
    let values = node_values(solved.game(), solved.strategy());
    let actor = solved.game().actor(node).unwrap();
    let put_in = f64::from(river.put_in[actor.index()]);
    let ev = |hand: usize| values.get(node).unwrap()[hand].map(|ev| ev - put_in);
    let reach_scale = saved.solution().reach_probability(start_node);

    let mut options = vec!["--node", path];
    if let Some(hand) = hand {
        options.extend(["--hand", hand]);
    }
    let run = inspect(partial, &options);
    let line = run.line("River        ");
    assert!(
        line.starts_with(&format!(
            "River        not in the file: solved again from the ranges that reach `{start}`, \
             to {:.4}% of its pot of ",
            solved.exploitability().percent_of_pot
        )),
        "{line}"
    );
    assert!(
        line.contains(&format!(
            "(the target 0.01%), in {} iterations and ",
            solved.iterations()
        )),
        "{line}"
    );
    assert!(solved.exploitability().percent_of_pot <= RIVER_TARGET);
    check_decision(&run, &expected(&solved, node, &ev, reach_scale));
    run
}

/// A turn spot saved without its river: a turn node shows as the whole file shows it, and a
/// river node is solved again, its numbers those of the library's solve of that river, its EVs
/// counted from the start of the spot.
#[test]
fn a_river_left_out_of_a_turn_file_is_solved_again_when_shown() {
    let folder = scratch("inspect-turn");
    let (whole, partial) = turn_files(&folder);
    let run = inspect(&partial, &[]);
    run.prints(&[
        "Holds        the turn, without the river, which is solved again when a node on it is \
         shown",
        "Board        Kh9c7d2s, starting on the turn",
    ]);

    // On the turn, both files hold the same strategy and the same EVs.
    for path in ["", "x", "b50"] {
        let from_partial = inspect(&partial, &["--node", path]);
        let from_whole = inspect(&whole, &["--node", path]);
        assert_eq!(node_part(&from_partial), node_part(&from_whole), "{path}");
        assert!(!from_partial.stdout.contains("River "));
        let saved = SavedSolution::load(&partial).unwrap();
        let node = saved.solution().game().find(path).unwrap();
        check_decision(
            &from_partial,
            &expected(saved.solution(), node, &file_evs(&saved, node), 1.0),
        );
    }

    // The river after both check the turn, at its first decision and its second.
    let run = check_river_solved_again(&partial, "x x 3c", "x x 3c", Some("AhTh"));
    run.prints(&[
        "Node         `x x 3c`, on the river, Kh9c7d3c2s",
        "Acts         OOP",
    ]);
    let rows = breakdown(&run);
    assert_eq!(rows.len(), 1, "AhTh is one combo");
    check_river_solved_again(&partial, "x x 3c", "x x 3c x", None);
    // After a bet and a call on the turn, facing the all-in that the river's bet turns into.
    let run = check_river_solved_again(&partial, "b50 c 8c", "b50 c 8c a", None);
    run.prints(&[
        "Pot          30bb; OOP has put in 15bb and has 0bb behind, IP 5bb and 10bb",
        "Acts         IP",
        "  \"b50 c 8c a c\"  the hand ends: at a showdown",
    ]);
    run.line("  f     fold ");
    run.line("  c     call 10bb ");
    // A river node the whole file holds is read from it, and solved again from the partial one.
    let from_whole = inspect(&whole, &["--node", "x x 3c"]);
    assert!(!from_whole.stdout.contains("River "));

    // A deal and the end of the hand on a river the file leaves out.
    let run = inspect(&partial, &["--node", "x x"]);
    let deals = run.line("Deals        ");
    assert!(
        deals.starts_with("Deals        the river: any of the 48 cards off the board, 2c 2d 2h"),
        "{deals}"
    );
    run.prints(&["Next         add the card that falls, as in --node \"x x 2c\""]);
    assert!(!run.stdout.contains("River "), "a deal needs no river");
    let run = inspect(&partial, &["--node", "x x 3c x x"]);
    run.prints(&[
        "Ends         at a showdown",
        "Next         nothing: the hand is over",
    ]);
    run.line("River        not in the file: solved again from the ranges that reach `x x 3c`");
    fs::remove_dir_all(&folder).unwrap();
}

/// A flop file without its turn can show the flop and its deals, and refuses a node after
/// them, saying what the file holds and how to get the rest.
#[test]
fn a_node_on_a_turn_left_out_is_refused_with_what_to_do() {
    let folder = scratch("inspect-flop");
    let file = folder.join("flop.pksol");
    solve_to(&["--spot", FLOP_SMALL], &file, "flop");
    let run = inspect(&file, &["--node", "x x"]);
    run.line("Deals        the turn: any of the 49 cards off the board");
    for path in ["x x 8h", "x x 8h x x 3c"] {
        let run = solver(&["inspect", arg(&file), "--node", path]);
        assert_eq!(
            run.failed(1),
            format!(
                "--node `{path}` is after the flop, the last street `{0}` holds, and only a \
                 river can be solved again; solve the spot again with solve-postflop \
                 --last-street turn or river, from the spot that `solver inspect {0} --spot` \
                 prints",
                file.display()
            )
        );
        assert_eq!(run.stdout, "", "nothing is printed before the error");
    }
    fs::remove_dir_all(&folder).unwrap();
}

// ---------------------------------------------------------------------------------------------
// The spot and the dump

#[test]
fn the_spot_prints_as_a_spot_file_that_solves_again() {
    let folder = scratch("inspect-spot");
    let (_, partial) = turn_files(&folder);
    let run = inspect(&partial, &["--spot"]);
    let header = read_header(&partial).unwrap();
    assert_eq!(run.stdout, header.spot.to_toml());
    assert_eq!(Spot::from_toml(&run.stdout).unwrap(), header.spot);
    assert_eq!(header.spot, Spot::load(root().join(TURN)).unwrap());
    // Redirected to a file, it is a spot file the command line takes.
    let spot = folder.join("spot.toml");
    fs::write(&spot, &run.stdout).unwrap();
    let estimate = solver(&["estimate", "--spot", arg(&spot)]).succeeded();
    estimate.line("A solve would run: ");
    fs::remove_dir_all(&folder).unwrap();
}

/// `--dump` writes the library's dump of the file, with the EVs the file holds: of the streets
/// it holds by default, or of fewer.
#[test]
fn the_dump_is_the_files_solution_with_its_evs() {
    let folder = scratch("inspect-dump");
    let (whole, partial) = turn_files(&folder);

    // The file without its river dumps its turn, a partial dump.
    let out = folder.join("turn.json");
    let run = inspect(&partial, &["--dump", arg(&out)]);
    let dumping = run.line("Dumping      ");
    assert!(
        dumping.starts_with(&format!(
            "Dumping      `{}`, holding the turn, with the EVs the file holds: about ",
            out.display()
        )),
        "{dumping}"
    );
    let text = fs::read_to_string(&out).unwrap();
    run.line(&format!(
        "Dumped       {} in ",
        format_bytes(text.len() as u64)
    ));
    run.prints(&[
        "Partial      the dump holds the first streets only, as its `complete: false` says",
    ]);
    let partial_saved = SavedSolution::load(&partial).unwrap();
    assert_eq!(text, SolutionDump::from_saved(&partial_saved, 1).to_json());
    let dump = SolutionDump::from_json(&text).unwrap();
    assert!(!dump.complete);
    assert_eq!(dump.streets(), 1);

    // The whole file dumps every street, with its own EVs: those the library measures again
    // for its strategy, rounded to f32.
    let out_whole = folder.join("whole.json");
    let run = inspect(&whole, &["--dump", arg(&out_whole)]);
    assert!(!run.stdout.contains("Partial"));
    assert!(SolutionDump::load(&out_whole).unwrap().complete);
    let whole_saved = SavedSolution::load(&whole).unwrap();
    let dump = SolutionDump::from_saved(&whole_saved, 2);
    assert_eq!(fs::read_to_string(&out_whole).unwrap(), dump.to_json());
    let measured = SolutionDump::from_solution(whole_saved.solution());
    assert_eq!(dump.nodes.len(), measured.nodes.len());
    assert_eq!(dump.values, measured.values);
    for (ours, again) in dump.nodes.iter().zip(&measured.nodes) {
        assert_eq!(ours.path, again.path);
        assert_eq!(ours.strategy, again.strategy);
        let (ours, again) = (ours.ev.as_ref().unwrap(), again.ev.as_ref().unwrap());
        for ((combo, ev), (_, measured)) in ours.iter().zip(again) {
            assert_eq!(
                ev.map(|ev| ev as f32),
                measured.map(|ev| ev as f32),
                "{combo}"
            );
        }
    }

    // Its first street is the partial file's dump, byte for byte.
    let first = folder.join("first.json");
    inspect(&whole, &["--dump", arg(&first), "--streets", "1"]);
    assert_eq!(fs::read_to_string(&first).unwrap(), text);

    // The dump is not written over unless asked, nor of more streets than the file holds.
    let run = solver(&["inspect", arg(&partial), "--dump", arg(&out)]);
    assert_eq!(
        run.failed(1),
        format!(
            "--dump `{}` already exists; pass --force to overwrite it, or give another path",
            out.display()
        )
    );
    inspect(&partial, &["--dump", arg(&out), "--force"]);
    let run = solver(&[
        "inspect",
        arg(&partial),
        "--dump",
        arg(&folder.join("more.json")),
        "--streets",
        "2",
    ]);
    assert_eq!(
        run.failed(1),
        format!(
            "--streets 2: `{}` holds the turn only, 1 street from the spot's first; give 1",
            partial.display()
        )
    );
    let missing = folder.join("no-such-folder").join("x.json");
    let message = solver(&["inspect", arg(&partial), "--dump", arg(&missing)]).failed(1);
    assert!(
        message.contains("no-such-folder` does not exist; create it"),
        "{message}"
    );
    let message = solver(&["inspect", arg(&partial), "--streets", "1"]).failed(2);
    assert!(message.contains("--dump <FILE>"), "{message}");
    // Nothing is left half-written.
    assert!(!folder.join("turn.json.part").exists());
    fs::remove_dir_all(&folder).unwrap();
}

// ---------------------------------------------------------------------------------------------
// The JSON

/// `inspect --json` of `file`, with `options`, read back.
fn inspect_json(file: &Path, options: &[&str]) -> serde_json::Value {
    let mut all = vec!["--json"];
    all.extend(options);
    let run = inspect(file, &all);
    serde_json::from_str(&run.stdout).expect("inspect --json prints JSON")
}

/// `inspect --json` of a river file holds the library's numbers: the file's facts from its
/// header, and at a decision, with and without a breakdown, and at the end of the hand, every
/// number exactly.
#[test]
fn the_json_of_a_river_file_holds_the_librarys_numbers() {
    let folder = scratch("inspect-json-river");
    let file = folder.join("river.pksol");
    solve_to(
        &[
            "--ranges",
            RANGES,
            "--board",
            "Ks7d2c9h3s",
            "--tree",
            RIVER_TREE,
        ],
        &file,
        "river",
    );
    let saved = SavedSolution::load(&file).unwrap();
    let header = saved.header();
    let solution = saved.solution();

    let answer = inspect_json(&file, &[]);
    let about = &answer["file"];
    assert_eq!(about["name"], "river.pksol");
    assert_eq!(about["bytes"], fs::metadata(&file).unwrap().len());
    assert_eq!(about["spot"]["name"], "BTN_vs_BB_SRP_Ks9h7d3s2c");
    assert_eq!(
        about["spot"]["board"],
        serde_json::json!(["Ks", "9h", "7d", "3s", "2c"])
    );
    assert_eq!(about["spot"]["pot"], 550);
    assert_eq!(about["spot"]["stack"], 9750);
    assert_eq!(about["spot"]["rake_cap"], 600);
    assert_eq!(about["streets"], serde_json::json!(["river"]));
    assert_eq!(about["complete"], true);
    assert_eq!(about["nodes"], header.nodes);
    assert_eq!(about["hands"], serde_json::json!(header.hands));
    assert_eq!(about["iterations"], header.iterations);
    assert_eq!(
        about["exploitability"]["percent_of_pot"].as_f64(),
        Some(header.exploitability.percent_of_pot)
    );
    assert_eq!(f64s(&about["values"]), header.exploitability.values);

    let node = &answer["node"];
    assert_eq!(node["path"], "");
    assert_eq!(node["line"], serde_json::json!([]));
    assert_eq!(node["pot"], 550);
    assert!(node["river"].is_null());
    check_json_decision(node, solution, 0, &file_evs(&saved, 0), 1.0);

    // IP facing the half-pot bet, with the combos of a class.
    let at = solution.game().find("b50").unwrap();
    let class = solution.range_view_classes(at).unwrap()[0]
        .class
        .to_string();
    let answer = inspect_json(&file, &["--node", "b50", "--hand", &class]);
    let node = &answer["node"];
    assert_eq!(node["put_in"], serde_json::json!([275, 0]));
    assert_eq!(node["behind"], serde_json::json!([9475, 9750]));
    assert_eq!(node["line"][0]["actor"], "oop");
    assert_eq!(node["hand"]["query"], class.as_str());
    assert!(!node["hand"]["rows"].as_array().unwrap().is_empty());
    check_json_decision(node, solution, at, &file_evs(&saved, at), 1.0);
    let fold = &node["actions"][0];
    assert_eq!(fold["token"], "f");
    assert_eq!(
        fold["next"],
        serde_json::json!({"kind": "ends", "outcome": "fold", "winner": "oop"})
    );

    // The end of the hand.
    let answer = inspect_json(&file, &["--node", "b50 f"]);
    let end = solution.game().find("b50 f").unwrap();
    let Some(Payoff::Fold(values)) = solution.game().payoff(end) else {
        panic!("a fold");
    };
    assert_eq!(answer["node"]["kind"], "end");
    assert_eq!(answer["node"]["winner"], "oop");
    assert_eq!(f64s(&answer["node"]["payoff"]["values"]), values);
    fs::remove_dir_all(&folder).unwrap();
}

/// `inspect --json` of a turn file without its river holds the library's numbers: a turn
/// decision from the file, a deal with each card's path, and a river decision solved again,
/// against the library's own solve of that river.
#[test]
fn the_json_of_a_turn_file_without_its_river_holds_the_librarys_numbers() {
    let folder = scratch("inspect-json-turn");
    let (_, partial) = turn_files(&folder);
    let saved = SavedSolution::load(&partial).unwrap();
    let game = saved.solution().game();

    let answer = inspect_json(&partial, &["--node", "b50"]);
    assert_eq!(answer["file"]["complete"], false);
    let at = game.find("b50").unwrap();
    check_json_decision(
        &answer["node"],
        saved.solution(),
        at,
        &file_evs(&saved, at),
        1.0,
    );

    let answer = inspect_json(&partial, &["--node", "x x"]);
    let node = &answer["node"];
    assert_eq!(node["kind"], "deal");
    assert_eq!(node["street"], "river");
    let deal = game.find("x x").unwrap();
    let cards: Vec<(String, String)> = node["cards"]
        .as_array()
        .unwrap()
        .iter()
        .map(|card| {
            (
                card["card"].as_str().unwrap().to_string(),
                card["path"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    let expected: Vec<(String, String)> = game
        .outcomes(deal)
        .iter()
        .map(|card| (card.to_string(), format!("x x {card}")))
        .collect();
    assert_eq!(cards, expected);

    let (start, path) = ("x x 3c", "x x 3c x");
    let start_node = game.find(start).unwrap();
    let (river, solved) = river_solved(&saved, start_node);
    let at = solved.game().find("x").unwrap();
    let values = node_values(solved.game(), solved.strategy());
    let put_in = f64::from(river.put_in[1]);
    let ev = |hand: usize| values.get(at).unwrap()[hand].map(|ev| ev - put_in);
    let answer = inspect_json(&partial, &["--node", path, "--hand", "AhTh"]);
    let node = &answer["node"];
    assert_eq!(
        node["board"],
        serde_json::json!(["Kh", "9c", "7d", "2s", "3c"])
    );
    assert_eq!(node["river"]["start"], start);
    assert_eq!(node["river"]["iterations"], solved.iterations());
    assert_eq!(
        node["river"]["exploitability"]["percent_of_pot"].as_f64(),
        Some(solved.exploitability().percent_of_pot)
    );
    assert_eq!(node["river"]["target"].as_f64(), Some(RIVER_TARGET));
    check_json_decision(
        node,
        &solved,
        at,
        &ev,
        saved.solution().reach_probability(start_node),
    );
    fs::remove_dir_all(&folder).unwrap();
}

// ---------------------------------------------------------------------------------------------
// Errors

#[test]
fn a_path_that_does_not_lead_to_a_node_says_where_and_what_could_follow() {
    let folder = scratch("inspect-paths");
    let (_, partial) = turn_files(&folder);
    let error = |path: &str| {
        let run = solver(&["inspect", arg(&partial), "--node", path]);
        assert_eq!(run.stdout, "");
        run.failed(1)
    };
    assert_eq!(
        error("x b75"),
        "--node `x b75`: `b75` is not an action after `x`; the actions there are `x`, `b50` and `a`"
    );
    assert_eq!(
        error("X B75"),
        "--node `x b75`: `b75` is not an action after `x`; the actions there are `x`, `b50` and `a`"
    );
    let message = error("x x 2s");
    assert!(
        message.starts_with(
            "--node `x x 2s`: `2s` cannot fall after `x x`: the 2s is on the board, Kh9c7d2s; \
             the cards that can fall there are the 48 off the board Kh9c7d2s: 2c 2d 2h 3c "
        ),
        "{message}"
    );
    assert!(message.ends_with(" As"), "{message}");
    let message = error("x x 2x");
    assert!(
        message.starts_with("--node `x x 2x`: `2x` is not a card; `x x` deals the river"),
        "{message}"
    );
    assert_eq!(
        error("b50 f x"),
        "--node `b50 f x`: `x` comes after the end of the hand, at `b50 f`, where IP folds"
    );

    // A hand that is neither a class nor a combo does not parse.
    let message = solver(&["inspect", arg(&partial), "--hand", "AK"]).failed(2);
    assert!(
        message.starts_with(
            "invalid value 'AK' for '--hand <HAND>': give a hand class, such as AKs, AKo or 77, \
             or a combo, such as AhKh"
        ),
        "{message}"
    );
    // At a node where no one acts, a hand has no breakdown, which is said.
    let run = inspect(&partial, &["--node", "b50 f", "--hand", "AhTh"]);
    run.prints(&["No breakdown of AhTh: no one acts once the hand is over."]);
    fs::remove_dir_all(&folder).unwrap();
}

#[test]
fn a_file_that_is_missing_or_not_a_solution_file_is_named() {
    let folder = scratch("inspect-files");
    let run = solver(&["inspect", "no/such/file.pksol"]);
    let message = run.failed(1);
    assert!(
        message.starts_with("could not read the solution file `no/such/file.pksol`: "),
        "{message}"
    );
    assert!(message.ends_with("; check the path"), "{message}");
    let text = folder.join("spot.pksol");
    fs::write(&text, "format = 1\n").unwrap();
    assert_eq!(
        solver(&["inspect", arg(&text)]).failed(1),
        format!(
            "`{}` is not a solution file: it does not start with `PKSOL`; give a file that \
             solve-postflop --out wrote",
            text.display()
        )
    );
    fs::remove_dir_all(&folder).unwrap();
}

// ---------------------------------------------------------------------------------------------
// The gate

/// M5's gate: the gate's range file on the flop Ks7d2c under `configs/trees/one_bet.toml`,
/// solved by `solve-postflop` and saved whole and saved to the turn, then inspected at the root,
/// at a turn node and at a river node of each file, every number against the library's. The
/// river the turn file leaves out is solved again. Run in release with `--nocapture` to see
/// what each command printed.
#[test]
#[ignore = "slow"]
fn the_gate_flop_solved_saved_and_inspected_at_every_street() {
    let folder = scratch("inspect-gate");
    let (whole, partial) = (folder.join("whole.pksol"), folder.join("turn.pksol"));
    let spot = ["--ranges", RANGES, "--board", "Ks7d2c", "--tree", ONE_BET];
    solve_to(&spot, &whole, "river");
    solve_to(&spot, &partial, "turn");
    let size = |path: &Path| fs::metadata(path).unwrap().len();
    println!(
        "whole: {}, to the turn: {}",
        format_bytes(size(&whole)),
        format_bytes(size(&partial))
    );
    assert!(size(&whole) > 50 * size(&partial));

    // The root, from either file: the root of the whole file reads none of its river.
    let started = Instant::now();
    let from_whole = inspect(&whole, &[]);
    println!("the root of the whole file in {:?}", started.elapsed());
    let from_partial = inspect(&partial, &[]);
    assert_eq!(node_part(&from_whole), node_part(&from_partial));
    let saved = SavedSolution::load_streets(&whole, Street::Turn).unwrap();
    check_decision(
        &from_whole,
        &expected(saved.solution(), 0, &file_evs(&saved, 0), 1.0),
    );
    from_whole.prints(&[
        "Spot         BTN_vs_BB_SRP_Ks7d2c",
        "Holds        the flop, the turn and the river",
        "Node         the root, on the flop, Ks7d2c",
    ]);
    from_partial.prints(&[
        "Holds        the flop and the turn, without the river, which is solved again when a \
         node on it is shown",
    ]);

    // A turn node, from either file.
    let turn = "x b50 c 8h";
    let from_whole = inspect(&whole, &["--node", turn, "--hand", "KQo"]);
    let from_partial = inspect(&partial, &["--node", turn, "--hand", "KQo"]);
    assert_eq!(node_part(&from_whole), node_part(&from_partial));
    let node = saved.solution().game().find(turn).unwrap();
    check_decision(
        &from_whole,
        &expected(saved.solution(), node, &file_evs(&saved, node), 1.0),
    );
    from_whole.prints(&["Node         `x b50 c 8h`, on the turn, Ks8h7d2c"]);
    breakdown(&from_whole);

    // A river node: read from the whole file, and solved again from the one without it.
    let river = "x b50 c 8h x x 3c";
    let started = Instant::now();
    let from_whole = inspect(&whole, &["--node", river]);
    println!("a river node of the whole file in {:?}", started.elapsed());
    assert!(!from_whole.stdout.contains("River "));
    let complete = SavedSolution::load(&whole).unwrap();
    let node = complete.solution().game().find(river).unwrap();
    check_decision(
        &from_whole,
        &expected(complete.solution(), node, &file_evs(&complete, node), 1.0),
    );
    drop(complete);
    let started = Instant::now();
    let from_partial = check_river_solved_again(&partial, river, river, Some("87s"));
    println!(
        "a river node of the file without its river, solved again, in {:?}",
        started.elapsed()
    );
    from_partial.prints(&["Node         `x b50 c 8h x x 3c`, on the river, Ks8h7d3c2c"]);
    breakdown(&from_partial);
    fs::remove_dir_all(&folder).unwrap();
}
