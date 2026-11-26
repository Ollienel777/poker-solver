//! `solver inspect`: describes a solution file and shows a node of its solution: who acts, the
//! pot and the stacks, how often play gets there, each action's frequency, a 13x13 grid of hand
//! classes for each action and one of the EVs, and, for one class or combo, each combo's
//! strategy and EV. A node on a river the file leaves out is shown from that river solved
//! again. `--json` prints the same as JSON, as `solver serve` answers ([`crate::json`]),
//! `--spot` prints the file's spot as a spot file, and `--dump` writes the solution as a JSON
//! solution dump.
//!
//! It reads the file's sections up to the street the node is on and no further
//! ([`SavedSolution::load_streets`]), so that the flop and the turn of a large flop solution
//! come up without its river. What it shows is built as data by [`crate::view`] and printed
//! here.

use std::fs;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Result, anyhow, bail};
use clap::Args as ClapArgs;
use pk_core::NUM_CLASSES;
use pk_game::{CHIPS_PER_BB, Outcome, Street};
use pk_postflop::{
    Payoff, SaveOptions, SavedSolution, SolutionDump, SolutionFileError, StopReason,
};

use crate::format::{self, bb, bb_value, bytes, duration, field, list, percent, share, thousands};
use crate::json::NodeAnswer;
use crate::save;
use crate::view::{
    self, ActionView, Decision, FileView, HandQuery, Missing, Next, NodeKind, NodeView,
};

/// The largest dump written without `--streets`, by its estimate: 1 GB. A complete dump of a
/// flop solution with real ranges is larger, and is written only when `--streets` asks for
/// every street (DECISIONS P77).
pub const DUMP_LIMIT: u64 = 1_000_000_000;

/// The options of `solver inspect`.
#[derive(Debug, ClapArgs)]
pub struct Args {
    /// The solution file, as solve-postflop --out writes it
    #[arg(value_name = "FILE")]
    pub file: PathBuf,

    /// The node to show, by the actions and cards that lead to it, such as "x b50 c 8h"
    /// [default: the root]
    ///
    /// The tokens are x (check), c (call), f (fold), bN (a bet of N% of the pot), rN (a raise
    /// to N times the bet), a (all-in), and a card, such as 8h, for the card that falls, each
    /// separated by a space.
    #[arg(long, value_name = "PATH", conflicts_with_all = ["spot", "dump"])]
    pub node: Option<String>,

    /// A hand class, such as AKs, 77 or T9o, or a combo, such as AhKh, to show combo by combo
    /// at the node
    #[arg(
        long,
        value_name = "HAND",
        value_parser = parse_hand,
        conflicts_with_all = ["spot", "dump"]
    )]
    pub hand: Option<HandQuery>,

    /// Print the file and the node as JSON, as solver serve answers, in place of the text
    #[arg(long, conflicts_with_all = ["spot", "dump"])]
    pub json: bool,

    /// Print the file's spot as a spot file, and nothing else
    #[arg(long, conflicts_with = "dump")]
    pub spot: bool,

    /// Write the solution to this file as a JSON solution dump
    ///
    /// Its folder must exist, and the file must not, unless --force is given.
    #[arg(long, value_name = "FILE", help_heading = "Dump")]
    pub dump: Option<PathBuf>,

    /// The streets the dump holds, from the spot's first [default: those the file holds]
    ///
    /// A dump of more than 1 GB by its estimate, as a complete flop dump with real ranges is,
    /// is written only when --streets asks for every street.
    #[arg(
        long,
        value_name = "N",
        requires = "dump",
        value_parser = clap::value_parser!(u8).range(1..=3),
        help_heading = "Dump"
    )]
    pub streets: Option<u8>,

    /// Overwrite the --dump file if it exists
    #[arg(long, requires = "dump", help_heading = "Dump")]
    pub force: bool,
}

pub fn parse_hand(text: &str) -> Result<HandQuery, String> {
    HandQuery::parse(text).ok_or_else(|| {
        "give a hand class, such as AKs, AKo or 77, or a combo, such as AhKh".to_string()
    })
}

pub fn run(args: &Args) -> Result<()> {
    let file = &args.file;
    let info = FileView::read(file).map_err(|error| file_error(file, error))?;
    if args.spot {
        print!("{}", info.header.spot.to_toml());
        return Ok(());
    }
    if let Some(out) = &args.dump {
        return dump(args, &info, out);
    }
    let path = args.node.as_deref().unwrap_or("");
    let last = view::street_of_path(info.header.spot.street(), path);
    let saved = SavedSolution::load_streets(file, last).map_err(|error| file_error(file, error))?;
    let node = view::find(&saved, path)?;
    let shown = view::node_view(&saved, file, node, args.hand)?;
    if args.json {
        let answer = NodeAnswer::new(&info, &shown);
        println!(
            "{}",
            serde_json::to_string_pretty(&answer).expect("a node's view is JSON")
        );
        return Ok(());
    }
    print_file(&info);
    println!();
    print_node(&shown, args.hand);
    Ok(())
}

/// A solution file's error as the command reports it, naming the file and saying what to do.
pub fn file_error(path: &Path, error: SolutionFileError) -> anyhow::Error {
    let shown = path.display();
    match error {
        SolutionFileError::Io { source, .. } => {
            anyhow!("could not read the solution file `{shown}`: {source}; check the path")
        }
        SolutionFileError::NotASolutionFile { .. } => anyhow!(
            "`{shown}` is not a solution file: it does not start with `PKSOL`; give a file that \
             solve-postflop --out wrote"
        ),
        error @ (SolutionFileError::TreeMismatch { .. } | SolutionFileError::Version { .. }) => {
            anyhow!("{error}")
        }
        error => anyhow!("{error}; the file is damaged: solve the spot again"),
    }
}

/// The file: where it is and what it holds, the spot, and how the solve ended.
fn print_file(info: &FileView) {
    let header = &info.header;
    let spot = &header.spot;
    field(
        "File",
        format!(
            "`{}`, {}, written by {}",
            info.path.display(),
            bytes(info.bytes),
            header.written_by()
        ),
    );
    let options = SaveOptions {
        last_street: header.last_street(),
    };
    let mut holds = save::streets(spot, &options);
    if header.last_street() == Street::Turn {
        holds.push_str(", which is solved again when a node on it is shown");
    }
    field("Holds", holds);
    let tree = format!(
        "{} nodes, {} of them where a player acts",
        thousands(u64::from(header.nodes)),
        thousands(u64::from(header.player_nodes))
    );
    format::print_spot_lines(spot, None, None, &tree);
    field(
        "Hands",
        format!(
            "OOP {}, IP {}, of the combos the board leaves",
            thousands(u64::from(header.hands[0])),
            thousands(u64::from(header.hands[1]))
        ),
    );
    let iterations = thousands(header.iterations);
    field(
        "Stopped",
        match header.stop {
            StopReason::Target => format!("at the target, after {iterations} iterations"),
            StopReason::IterationCap => format!("at the cap of {iterations} iterations"),
            StopReason::TimeCap => format!("at the time cap, after {iterations} iterations"),
        },
    );
    let stored = &header.exploitability;
    field(
        "Exploitable",
        format!(
            "{} of the pot, {} per hand dealt, as stored, in 16 bits a probability; {} as solved",
            percent(stored.percent_of_pot),
            bb_value(stored.chips, 4),
            percent(header.solve_exploitability.percent_of_pot)
        ),
    );
    field(
        "Values",
        format!(
            "OOP {}, IP {}, per hand dealt",
            bb_value(stored.values[0], 2),
            bb_value(stored.values[1], 2)
        ),
    );
}

/// The node, as the view holds it.
fn print_node(node: &NodeView, query: Option<HandQuery>) {
    let place = if node.path.is_empty() {
        "the root".to_string()
    } else {
        format!("`{}`", node.path)
    };
    field(
        "Node",
        format!("{place}, on the {}, {}", node.street, node.board),
    );
    let pot = bb(node.pot);
    let [oop, ip] = node.put_in;
    field(
        "Pot",
        if oop == ip && oop == 0 {
            format!("{pot}, with {} behind each player", bb(node.behind[0]))
        } else if oop == ip {
            format!(
                "{pot}, with {} behind each player, who have each put in {}",
                bb(node.behind[0]),
                bb(oop)
            )
        } else {
            format!(
                "{pot}; OOP has put in {} and has {} behind, IP {} and {}",
                bb(oop),
                bb(node.behind[0]),
                bb(ip),
                bb(node.behind[1])
            )
        },
    );
    field(
        "Reached",
        format!("{} of the hands dealt", share(node.reached)),
    );
    field(
        "Ranges",
        format!(
            "OOP {:.2} combos, IP {:.2}: each hand's range weight times its own reach",
            node.combos[0], node.combos[1]
        ),
    );
    if let Some(river) = &node.river {
        let stopped = match river.stop {
            StopReason::Target => "",
            StopReason::IterationCap | StopReason::TimeCap => {
                ", stopped by its cap above the target"
            }
        };
        field(
            "River",
            format!(
                "not in the file: solved again from the ranges that reach `{}`, to {} of its \
                 pot of {} (the target {}%{stopped}), in {} iterations and {}",
                river.start,
                percent(river.exploitability.percent_of_pot),
                bb(river.pot),
                river.target,
                thousands(river.iterations),
                duration(river.elapsed)
            ),
        );
    }
    match &node.kind {
        NodeKind::Decision(decision) => print_decision(node, decision),
        NodeKind::Deal { street, cards } => {
            let names: Vec<String> = cards.iter().map(ToString::to_string).collect();
            field(
                "Deals",
                format!(
                    "the {street}: any of the {} cards off the board, {}",
                    cards.len(),
                    names.join(" ")
                ),
            );
            field(
                "Next",
                format!(
                    "add the card that falls, as in --node \"{}\"",
                    joined(&node.path, &names[0])
                ),
            );
            if let Some(query) = query {
                println!();
                println!(
                    "No breakdown of {}: no one acts at a deal.",
                    query_name(query)
                );
            }
        }
        NodeKind::End { outcome, payoff } => {
            field("Ends", ending(*outcome));
            field("Payoffs", payoffs(payoff));
            field("Next", "nothing: the hand is over");
            if let Some(query) = query {
                println!();
                println!(
                    "No breakdown of {}: no one acts once the hand is over.",
                    query_name(query)
                );
            }
        }
    }
}

/// `path` with `token` after it.
fn joined(path: &str, token: &str) -> String {
    if path.is_empty() {
        token.to_string()
    } else {
        format!("{path} {token}")
    }
}

fn query_name(query: HandQuery) -> String {
    match query {
        HandQuery::Class(class) => class.to_string(),
        HandQuery::Combo(combo) => combo.to_string(),
    }
}

/// How a hand ends at a terminal.
fn ending(outcome: Outcome) -> String {
    match outcome {
        Outcome::Fold { winner } => {
            format!("{} folds, and {winner} wins the pot", winner.opponent())
        }
        Outcome::Showdown => "at a showdown".to_string(),
    }
}

/// A terminal's payoffs, in big blinds from the start of the spot.
fn payoffs(payoff: &Payoff) -> String {
    // A payoff of no chips is written 0, whichever side of zero it is on.
    let amount = |chips: f64| bb_value(chips + 0.0, 2);
    match *payoff {
        Payoff::Fold([oop, ip]) => format!(
            "OOP {}, IP {}, from the start of the spot, the rake taken",
            amount(oop),
            amount(ip)
        ),
        Payoff::Showdown(showdown) => format!(
            "the better hand {}, the worse {}, a split {} each, from the start of the spot, the \
             rake taken",
            amount(showdown.win),
            amount(showdown.lose),
            amount(showdown.tie)
        ),
    }
}

/// What an action does, in big blinds: `check`, `bet 2.75bb`, `call 2.75bb` (what the call puts
/// in), `raise to 8.25bb` or `all-in to 97.5bb` (the street total).
fn words(action: &ActionView) -> String {
    use pk_game::Action;
    match action.action {
        Action::Fold => "fold".to_string(),
        Action::Check => "check".to_string(),
        Action::Call => format!("call {}", bb(action.adds)),
        Action::Bet { .. } => format!("bet {}", bb(action.to)),
        Action::Raise { .. } => format!("raise to {}", bb(action.to)),
        Action::AllIn { .. } => format!("all-in to {}", bb(action.to)),
    }
}

/// A frequency as a percent to two decimals, or `-` where there is none.
fn frequency(frequency: Option<f64>) -> String {
    frequency.map_or_else(|| "-".to_string(), |f| format!("{:.2}%", 100.0 * f))
}

/// A decision: who acts, each action's frequency, a grid per action and one of the EVs, the
/// breakdown asked for, and the nodes that follow.
fn print_decision(node: &NodeView, decision: &Decision) {
    let actor = decision.actor;
    let opponent = actor.opponent();
    field("Acts", actor);
    println!();
    let labels: Vec<String> = decision
        .actions
        .iter()
        .map(|action| format!("{:<6}{}", action.token, words(action)))
        .collect();
    let width = labels.iter().map(String::len).max().unwrap_or(0).max(6);
    println!("  {:<width$}  {:>8}  {:>8}", "action", "taken", "of range");
    for (action, label) in decision.actions.iter().zip(&labels) {
        println!(
            "  {label:<width$}  {:>8}  {:>8}",
            frequency(action.frequency),
            frequency(action.range_frequency)
        );
    }
    println!(
        "Taken: how often {actor} takes the action once play is here, card removal by \
         {opponent}'s range included."
    );
    println!(
        "Of range: its frequency over {actor}'s range alone, each combo by its weight there, as \
         the grids show it."
    );

    if decision.classes.is_empty() {
        println!();
        println!("{actor} never plays to this node, so it has no range here.");
    } else {
        println!();
        println!(
            "The grids have a cell per hand class: pairs on the diagonal, suited hands above it,"
        );
        println!("offsuit hands below, and a blank for a class not in {actor}'s range here.");
        for (index, action) in decision.actions.iter().enumerate() {
            let mut cells: Vec<Option<String>> = vec![None; NUM_CLASSES];
            for class in &decision.classes {
                let percent = (100.0 * class.frequencies[index]).round() + 0.0;
                cells[class.class.index()] = Some(format!("{percent}"));
            }
            println!();
            println!(
                "{} ({}): the percent of each class's combos in {actor}'s range that take it; {} \
                 of the range",
                action.token,
                words(action),
                frequency(action.range_frequency)
            );
            for line in format::grid(&cells, 4) {
                println!("{line}");
            }
        }
        let mut cells: Vec<Option<String>> = vec![None; NUM_CLASSES];
        for class in &decision.classes {
            cells[class.class.index()] = Some(match class.ev {
                Some(ev) => ev_cell(ev),
                None => "-".to_string(),
            });
        }
        println!();
        println!(
            "EV in bb from the start of the spot: each class's EVs averaged over its combos in \
             {actor}'s range, by weight"
        );
        for line in format::grid(&cells, 6) {
            println!("{line}");
        }
    }

    if let Some(breakdown) = &decision.breakdown {
        print_breakdown(decision, breakdown);
    }

    println!();
    println!("Next, for --node:");
    let width = decision
        .actions
        .iter()
        .map(|action| joined(&node.path, &action.token).len() + 2)
        .max()
        .unwrap_or(0);
    for action in &decision.actions {
        let what = match action.next {
            Next::Acts(player) => format!("{player} acts"),
            Next::Deals { street, example } => format!(
                "deals the {street}: add the card that falls, as in \"{}\"",
                joined(&joined(&node.path, &action.token), &example.to_string())
            ),
            Next::Ends(outcome) => format!("the hand ends: {}", ending(outcome)),
        };
        let shown = format!("\"{}\"", joined(&node.path, &action.token));
        println!("  {shown:<width$}  {what}");
    }
}

/// An EV in chips as a grid cell: big blinds to one decimal, `0.0` for a value that rounds to
/// zero either side.
fn ev_cell(chips: f64) -> String {
    let tenths = (10.0 * chips / f64::from(CHIPS_PER_BB)).round() + 0.0;
    format!("{:.1}", tenths / 10.0)
}

/// The combos of a class or combo: each one's weight in the range, its strategy and its EV.
fn print_breakdown(decision: &Decision, breakdown: &view::Breakdown) {
    let actor = decision.actor;
    let name = query_name(breakdown.query);
    println!();
    // A class in the range has its cells in the grids, with its combos and its share there.
    let class = match breakdown.query {
        HandQuery::Class(class) => decision.classes.iter().find(|row| row.class == class),
        HandQuery::Combo(_) => None,
    };
    match breakdown.rows.len() {
        0 => println!("{name}: none of its combos is among {actor}'s hands here."),
        count => {
            let combos = match count {
                1 => "1 combo".to_string(),
                count => format!("{count} combos"),
            };
            let (weight, of_range) = match class {
                Some(row) => (row.combos, format!(", {} of it", share(row.share))),
                None => (
                    breakdown.rows.iter().map(|row| row.weight).sum::<f64>() + 0.0,
                    String::new(),
                ),
            };
            println!(
                "{name}: {combos} among {actor}'s hands here, {weight:.3} by their weight in the \
                 range{of_range}"
            );
            println!("Each weight is the combo's range weight times its own reach.");
        }
    }
    if !breakdown.rows.is_empty() {
        let tokens: Vec<&str> = decision.actions.iter().map(|a| a.token.as_str()).collect();
        let widths: Vec<usize> = tokens.iter().map(|t| t.len().max(7)).collect();
        let mut head = format!("  {:<6}  {:>7}", "combo", "weight");
        for (token, width) in tokens.iter().zip(&widths) {
            head.push_str(&format!("  {token:>width$}"));
        }
        head.push_str(&format!("  {:>9}", "EV"));
        println!("{head}");
        for row in &breakdown.rows {
            let mut line = format!("  {:<6}  {:>7.3}", row.combo.to_string(), row.weight);
            for (p, width) in row.strategy.iter().zip(&widths) {
                line.push_str(&format!("  {:>width$}", format!("{:.2}%", 100.0 * p)));
            }
            let ev = row.ev.map_or_else(|| "-".to_string(), |ev| bb_value(ev, 2));
            line.push_str(&format!("  {ev:>9}"));
            println!("{line}");
        }
    }
    let mut others: Vec<String> = Vec::new();
    for (reason, words) in [
        (Missing::OnBoard, "hold a card of the board"),
        (Missing::NotInRange, "are not in the range here"),
    ] {
        let combos: Vec<String> = breakdown
            .missing
            .iter()
            .filter(|(_, why)| *why == reason)
            .map(|(combo, _)| combo.to_string())
            .collect();
        if !combos.is_empty() {
            let words = if combos.len() == 1 {
                words.replace("hold ", "holds ").replace("are ", "is ")
            } else {
                words.to_string()
            };
            others.push(format!("{} {words}", list(&combos)));
        }
    }
    if !others.is_empty() {
        println!("No row: {}.", others.join("; "));
    }
}

/// Writes the solution as a JSON solution dump.
fn dump(args: &Args, info: &FileView, out: &Path) -> Result<()> {
    check_destination(out, args.force)?;
    let header = &info.header;
    let first = header.spot.street();
    let held = header.streets.len();
    let held_names: Vec<String> = header.streets.iter().map(|s| format!("the {s}")).collect();
    let streets = match args.streets {
        Some(n) if usize::from(n) > held => bail!(
            "--streets {n}: `{}` holds {} only, {held} street{} from the spot's first; give {}",
            info.path.display(),
            list(&held_names),
            if held == 1 { "" } else { "s" },
            match held {
                1 => "1".to_string(),
                2 => "1 or 2".to_string(),
                held => format!("1 to {held}"),
            }
        ),
        Some(n) => usize::from(n),
        None => held,
    };
    let estimate = dump_estimate(info, streets);
    if args.streets.is_none() && estimate > DUMP_LIMIT {
        let fewer = streets - 1;
        let instead = if fewer == 0 {
            String::new()
        } else {
            format!(
                "--streets {fewer} for {} (about {}), or ",
                list(&held_names[..fewer]),
                bytes(dump_estimate(info, fewer))
            )
        };
        bail!(
            "--dump `{}`: a dump of every street the file holds, {}, would be about {} of JSON, \
             and take several times that in memory to write; pass {instead}--streets {streets} \
             to write it all",
            out.display(),
            list(&held_names),
            bytes(estimate)
        );
    }
    let last = header.streets[streets - 1];
    print_file(info);
    println!();
    let kept = list(&held_names[..streets]);
    field(
        "Dumping",
        format!(
            "`{}`, holding {kept}, with the EVs the file holds: about {} of JSON",
            out.display(),
            bytes(estimate)
        ),
    );
    let start = Instant::now();
    let saved = SavedSolution::load_streets(&info.path, last)
        .map_err(|error| file_error(&info.path, error))?;
    let text = SolutionDump::from_saved(&saved, streets).to_json();
    drop(saved);
    write_atomically(out, text.as_bytes())?;
    field(
        "Dumped",
        format!(
            "{} in {}",
            bytes(text.len() as u64),
            duration(start.elapsed())
        ),
    );
    if streets < streets_played(first) {
        field(
            "Partial",
            "the dump holds the first streets only, as its `complete: false` says",
        );
    }
    Ok(())
}

/// The number of streets a spot that starts on `first` plays.
fn streets_played(first: Street) -> usize {
    Street::ALL
        .iter()
        .filter(|&&street| street >= first)
        .count()
}

/// The size of a dump of the first `streets` streets of the file, estimated from the raw
/// lengths of their sections: a hand's row at a node, its strategy and its EV, takes about
/// [`ROW_BYTES`] bytes and each probability in it [`PROBABILITY_BYTES`] more. It counts the
/// rows of hands that hold a card dealt on the way, which a dump leaves out, so it is a little
/// high.
fn dump_estimate(info: &FileView, streets: usize) -> u64 {
    let header = &info.header;
    let mut total = 0;
    for street in &header.streets[..streets] {
        let raw = |name: String| {
            header
                .sections
                .iter()
                .find(|section| section.name == name)
                .map_or(0, |section| section.raw_length)
        };
        let rows = raw(format!("ev.{street}")) / 4;
        let probabilities = raw(format!("strategy.{street}")) / 2;
        total += ROW_BYTES * rows + PROBABILITY_BYTES * probabilities;
    }
    total
}

/// What a dump takes for a hand's row at a node, besides its probabilities: its name twice,
/// the indent and punctuation of its strategy row, and its EV. With [`PROBABILITY_BYTES`], it
/// puts the dump of the flop and the turn of `BTN_vs_BB_SRP` on Ks7d2c, 15.6 MB, at 16.6 MB.
const ROW_BYTES: u64 = 40;

/// What a dump takes for each probability of a row, separator included.
const PROBABILITY_BYTES: u64 = 12;

/// Checks that the dump can be written to `path`: its folder exists, and the file does not,
/// unless `force` allows overwriting it.
fn check_destination(path: &Path, force: bool) -> Result<()> {
    let shown = path.display();
    let folder = match path.parent() {
        Some(folder) if !folder.as_os_str().is_empty() => folder,
        _ => Path::new("."),
    };
    if !folder.is_dir() {
        if folder.exists() {
            bail!(
                "--dump `{shown}`: `{}` is not a folder; give a path in a folder",
                folder.display()
            );
        }
        bail!(
            "--dump `{shown}`: the folder `{}` does not exist; create it, or give a path in a \
             folder that does",
            folder.display()
        );
    }
    match fs::metadata(path) {
        Ok(metadata) if metadata.is_dir() => {
            bail!("--dump `{shown}` is a folder; give the path of a file to write")
        }
        Ok(_) if !force => bail!(
            "--dump `{shown}` already exists; pass --force to overwrite it, or give another path"
        ),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => bail!("--dump `{shown}`: {error}"),
    }
}

/// Writes `bytes` to `path` through `<path>.part`, renamed once it is complete, so that a run
/// cut short leaves no half-written dump under the final name.
fn write_atomically(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut part = path.as_os_str().to_owned();
    part.push(".part");
    let part = PathBuf::from(part);
    let written = fs::File::create(&part).and_then(|mut file| {
        file.write_all(bytes)?;
        file.sync_all()
    });
    if let Err(error) = written.and_then(|()| fs::rename(&part, path)) {
        let _ = fs::remove_file(&part);
        bail!("--dump `{}` could not be written: {error}", path.display());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use pk_game::Player;

    use super::*;

    #[test]
    fn an_ev_cell_is_big_blinds_to_one_decimal() {
        assert_eq!(ev_cell(412.0), "4.1");
        assert_eq!(ev_cell(-275.0), "-2.8");
        assert_eq!(ev_cell(-4.0), "0.0");
        assert_eq!(ev_cell(9750.0), "97.5");
    }

    #[test]
    fn a_player_and_an_ending_read_as_words() {
        assert_eq!(
            ending(Outcome::Fold { winner: Player::Ip }),
            "OOP folds, and IP wins the pot"
        );
        assert_eq!(ending(Outcome::Showdown), "at a showdown");
        assert_eq!(joined("", "x"), "x");
        assert_eq!(joined("x b50", "c"), "x b50 c");
    }
}
