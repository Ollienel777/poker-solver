//! Solves a river spot file and prints what the solver found: the spot, the solve's progress,
//! the result, and the strategy at one node, as a range view and as a 13x13 grid of hand
//! classes for each action.
//!
//! ```text
//! cargo run --release -p pk-postflop --example solve_spot -- reference/spots/river/toy_polarized.toml
//! cargo run --release -p pk-postflop --example solve_spot -- <spot> --node "x b50" --target 0.1
//! ```
//!
//! This is a way to try the library, not the `solver` command line, which comes in M5. Every
//! number it prints is one of the library's read-outs; the example only formats them.

use std::process::ExitCode;

use pk_core::NUM_CLASSES;
use pk_game::{Action, CHIPS_PER_BB, Chips, Node, Outcome, Player};
use pk_postflop::{ClassView, Progress, Solution, SolveOptions, Spot, StopReason, solve};

const USAGE: &str =
    "usage: solve_spot <spot file> [--node \"x b50\"] [--target <percent>] [--iterations <cap>]";

/// The ranks of the grid's rows and columns, in grid order.
const RANKS: &str = "AKQJT98765432";

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("solve_spot: {message}");
            ExitCode::FAILURE
        }
    }
}

struct Args {
    spot: String,
    /// The node path, its tokens separated by one space: empty for the root.
    path: String,
    options: SolveOptions,
}

fn run() -> Result<(), String> {
    let args = parse_args(std::env::args().skip(1))?;
    let spot = Spot::load(&args.spot).map_err(|e| e.to_string())?;
    // The solve builds the same tree. Finding the node first means a mistyped path costs no
    // solve.
    let tree = spot.build_tree().map_err(|e| e.to_string())?;
    let path = args.path;
    let node = tree.find(&path).map_err(|e| format!("--node: {e}"))?;
    if let Node::Terminal { outcome, .. } = tree.node(node) {
        return Err(format!(
            "--node: `{path}` {}; give a node where a player acts",
            ending(*outcome)
        ));
    }

    let hands = spot.board_hands();
    let rake = if spot.rake_percent == 0.0 {
        "no rake".to_string()
    } else {
        format!(
            "{}% rake, capped at {}",
            spot.rake_percent,
            bb(spot.rake_cap)
        )
    };
    println!(
        "Spot {}: board {}, pot {}, stack {}, {rake}",
        spot.name,
        spot.board,
        bb(spot.pot),
        bb(spot.effective_stack)
    );
    println!(
        "OOP has {} hands and IP {}; the tree has {} nodes",
        hands.hands(Player::Oop).len(),
        hands.hands(Player::Ip).len(),
        tree.nodes().len()
    );

    let options = args.options;
    match options.target {
        Some(target) => println!(
            "\nSolving to {target}% of the pot, for at most {} iterations",
            options.max_iterations
        ),
        None => println!("\nSolving for {} iterations", options.max_iterations),
    }
    let solution = solve(&spot, &options, print_progress).map_err(|e| e.to_string())?;

    let stop = match solution.stop() {
        StopReason::Target => "reached the target",
        StopReason::IterationCap => "reached the iteration cap",
        StopReason::TimeCap => "reached the time cap",
    };
    println!(
        "\nStopped: {stop} after {} iterations, in {:.3}s",
        solution.iterations(),
        solution.elapsed().as_secs_f64()
    );
    let [oop, ip] = solution.values();
    println!(
        "Exploitable by {:.4}% of the pot; values per hand dealt: OOP {:.2}bb, IP {:.2}bb",
        solution.exploitability().percent_of_pot,
        oop / f64::from(CHIPS_PER_BB),
        ip / f64::from(CHIPS_PER_BB)
    );

    print_node(&solution, node, &path);
    Ok(())
}

fn print_progress(progress: &Progress) {
    println!(
        "  {:>6} iterations  {:>8.3}s  exploitable by {:>8.4}% of the pot",
        progress.iterations,
        progress.elapsed.as_secs_f64(),
        progress.exploitability.percent_of_pot
    );
}

/// The strategy at a player node, then its children.
fn print_node(solution: &Solution, node: u32, path: &str) {
    let tree = solution.tree();
    let Node::Player {
        actor,
        actions,
        children,
        ..
    } = tree.node(node)
    else {
        unreachable!("the node was checked to be a player node");
    };
    let name = if path.is_empty() { "the root" } else { path };
    println!(
        "\nAt {name}, {actor} acts; play reaches it {:.2}% of the time",
        100.0 * solution.reach_probability(node)
    );
    let (Some(frequencies), Some(classes)) = (
        solution.range_view_frequencies(node),
        solution.range_view_classes(node),
    ) else {
        println!("{actor} never plays to this node, so it has no range here");
        return;
    };
    let labels: Vec<String> = actions
        .iter()
        .map(|&action| label(solution, node, action))
        .collect();
    println!("Each action, and how often {actor}'s range takes it (the range view):");
    let width = labels.iter().map(String::len).max().unwrap_or(0);
    for (label, frequency) in labels.iter().zip(&frequencies) {
        println!("  {label:<width$} {:>7.2}%", 100.0 * frequency);
    }
    println!(
        "\nEach action's frequency by hand class in {actor}'s range, in percent (blank: not in \
         the range)"
    );
    for (index, label) in labels.iter().enumerate() {
        println!("\n{label}");
        print_grid(&classes, index);
    }

    println!("\nThe nodes after {name}, for --node:");
    for (action, &child) in actions.iter().zip(children) {
        let child_path = if path.is_empty() {
            action.to_string()
        } else {
            format!("{path} {action}")
        };
        let what = match tree.node(child) {
            Node::Player { actor, .. } => format!("{actor} acts"),
            Node::Terminal { outcome, .. } => ending(*outcome),
            Node::Chance { street, .. } => format!("deals the {street}"),
        };
        println!("  {:<24} {what}", format!("\"{child_path}\""));
    }
}

/// An action's token, what it does, and the acting player's street total after it, such as
/// `b50   bet to 500 chips (5bb)`.
fn label(solution: &Solution, node: u32, action: Action) -> String {
    let words = match action {
        Action::Fold => "fold".to_string(),
        Action::Check => "check".to_string(),
        Action::Call => format!("call {}", chips(called(solution, node))),
        Action::Bet { to, .. } => format!("bet to {}", chips(to)),
        Action::Raise { to, .. } => format!("raise to {}", chips(to)),
        Action::AllIn { to } => format!("all-in to {}", chips(to)),
    };
    format!("{:<6}{words}", action.to_string())
}

/// The street total a call at `node` matches: on the river, the bet or raise that led there.
fn called(solution: &Solution, node: u32) -> Chips {
    let (parent, index) = solution
        .game()
        .path(node)
        .pop()
        .expect("a call faces a bet");
    let Node::Player { actions, .. } = solution.tree().node(parent) else {
        unreachable!("a path passes through player nodes only");
    };
    actions[index].to().expect("a call faces a bet or raise")
}

/// The 13x13 grid of one action's frequency for each class, as a whole percent: pairs on the
/// diagonal, suited hands above it and offsuit hands below.
fn print_grid(classes: &[ClassView], action: usize) {
    let mut cells: [Option<f64>; NUM_CLASSES] = [None; NUM_CLASSES];
    for class in classes {
        cells[class.class.index()] = Some(class.frequencies[action]);
    }
    let header: String = RANKS.chars().map(|rank| format!("{rank:>4}")).collect();
    println!("  {header}");
    for (row, rank) in RANKS.chars().enumerate() {
        let line: String = cells[13 * row..13 * (row + 1)]
            .iter()
            .map(|cell| match cell {
                Some(frequency) => format!("{:>4}", (100.0 * frequency).round()),
                None => "    ".to_string(),
            })
            .collect();
        println!("  {rank}{}", line.trim_end());
    }
}

/// How a terminal ends the hand, after a node path.
fn ending(outcome: Outcome) -> String {
    match outcome {
        Outcome::Fold { winner } => format!("ends the hand: {} folds", winner.opponent()),
        Outcome::Showdown => "ends the hand at a showdown".to_string(),
    }
}

fn bb(chips: Chips) -> String {
    format!("{}bb", f64::from(chips) / f64::from(CHIPS_PER_BB))
}

fn chips(amount: Chips) -> String {
    format!("{amount} chips ({})", bb(amount))
}

fn parse_args(mut args: impl Iterator<Item = String>) -> Result<Args, String> {
    let mut spot = None;
    let mut path = String::new();
    let mut options = SolveOptions::default();
    while let Some(arg) = args.next() {
        let mut value = |wanted: &str| {
            args.next()
                .ok_or_else(|| format!("{arg} needs a value: {wanted}"))
        };
        match arg.as_str() {
            "--node" => {
                let text = value("a node path such as \"x b50\"")?;
                path = text.split_whitespace().collect::<Vec<_>>().join(" ");
            }
            "--target" => {
                let text = value("a percent of the pot such as 0.3")?;
                let target = text.parse().map_err(|_| {
                    format!(
                        "--target `{text}` is not a number; give a percent of the pot, such as 0.3"
                    )
                })?;
                options.target = Some(target);
            }
            "--iterations" => {
                let text = value("the most iterations to run, such as 10000")?;
                options.max_iterations = text.parse().map_err(|_| {
                    format!("--iterations `{text}` is not a whole number; give a cap such as 10000")
                })?;
            }
            "-h" | "--help" => return Err(USAGE.to_string()),
            flag if flag.starts_with("--") => {
                return Err(format!(
                    "`{flag}` is not an option; the options are --node, --target and --iterations"
                ));
            }
            _ if spot.is_none() => spot = Some(arg.clone()),
            _ => return Err(format!("`{arg}` is a second spot file; give one")),
        }
    }
    let spot = spot.ok_or_else(|| format!("no spot file given; {USAGE}"))?;
    Ok(Args {
        spot,
        path,
        options,
    })
}
