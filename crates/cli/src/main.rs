//! The `solver` binary: the solver's command line.
//!
//! Each command is a module of [`commands`], with its arguments and its `run`. What the commands
//! share has modules of its own: [`spot_input`] turns the options that name a spot into a
//! [`pk_postflop::Spot`], [`limits`] holds the limits a solve runs under and says what to do
//! when a spot is over one, [`format`] writes amounts, sizes and times and prints the spot,
//! [`save`] is where a solve's solution is written, [`view`] is what `inspect` shows of a
//! solution, built as data before it is printed, [`json`] is that data's JSON form, and
//! [`server`] is what `serve` answers.
//!
//! Only this crate prints. Everything a command reports goes to stdout. Errors go to stderr,
//! each as one `solver: <message>`, with exit code 1, or 2 for a command line that does not
//! parse.

use std::process::ExitCode;

use clap::error::ErrorKind;
use clap::{Parser, Subcommand};

mod commands;
mod format;
mod json;
mod limits;
mod save;
mod server;
mod spot_input;
mod view;

/// A No-Limit Hold'em cash-game solver.
#[derive(Debug, Parser)]
#[command(name = "solver", bin_name = "solver", version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Solve a heads-up postflop spot, printing the solve's progress and its result.
    ///
    /// The spot comes from a range file and a board, or from a spot file. Before solving, the
    /// command prints the spot and the memory estimate, and refuses a spot whose public tree is
    /// over the node limit or whose estimate is over the memory limit. While solving, it prints
    /// a line per exploitability measurement. At the end it prints how the solve stopped, the
    /// exploitability, each player's value and the peak memory, and appends a record of the
    /// solve's resources to the log.
    SolvePostflop(commands::solve_postflop::Args),
    /// Print the memory a solve of a spot would need, item by item, without solving.
    ///
    /// It takes the same spot and the same limits as solve-postflop, builds the betting tree
    /// only, and says whether solve-postflop would refuse the spot and what to change if so.
    Estimate(commands::estimate::Args),
    /// Show a solution file: its spot and solve, and a node of its solution.
    ///
    /// Without --node it shows the root. A node shows who acts, the pot and the stacks, how
    /// often play reaches it and each range there, each action's frequency, a 13x13 grid of
    /// hand classes per action and one of the EVs, and, with --hand, the strategy and EV of each
    /// combo of one class. A node on a river the file leaves out is solved again from the ranges
    /// that reach it. --json prints the file and the node as JSON, as solver serve answers,
    /// --spot prints the file's spot as a spot file, and --dump writes the solution as a JSON
    /// solution dump.
    Inspect(commands::inspect::Args),
    /// Compare two solutions of one spot, or a solution and the reference solver's answer.
    ///
    /// Each solution is a solution file or a JSON solution dump, ours or another solver's. The
    /// comparison checks each strategy's exploitability, the values, each action's frequency at
    /// every node reached often enough, and the EVs, under the gates' tolerances, and prints
    /// what it could and could not check. It exits with 0 when every check passes and with 1
    /// when one does not. --reference compares a solution with what the reference solver that
    /// PK_REFERENCE_SOLVER names writes for its spot.
    Compare(commands::compare::Args),
    /// Serve a folder of solution files to a browser, on this machine only.
    ///
    /// It listens on 127.0.0.1 at --port, serves the viewer's page at / and answers the page's
    /// requests: the folder's solution files at /api/files, and a node of one at /api/node, as
    /// inspect --json prints it. Solutions stay loaded between requests, up to --max-memory, the
    /// least recently used dropped first. It prints a line per request, and runs until it is
    /// stopped.
    Serve(commands::serve::Args),
}

fn main() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error) => return parse_error(&error),
    };
    let result = match cli.command {
        Command::SolvePostflop(args) => commands::solve_postflop::run(&args),
        Command::Estimate(args) => commands::estimate::run(&args),
        Command::Inspect(args) => commands::inspect::run(&args),
        Command::Compare(args) => commands::compare::run(&args),
        Command::Serve(args) => commands::serve::run(&args),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("solver: {error:#}");
            ExitCode::FAILURE
        }
    }
}

/// Help and the version print as clap prints them. A command line that does not parse is an
/// error like any other, `solver: <message>`, followed by clap's usage line, with exit code 2.
fn parse_error(error: &clap::Error) -> ExitCode {
    let code = ExitCode::from(u8::try_from(error.exit_code()).unwrap_or(2));
    match error.kind() {
        ErrorKind::DisplayHelp
        | ErrorKind::DisplayVersion
        | ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand => {
            // Printing fails only when the stream is closed, and then there is no one to tell.
            let _ = error.print();
        }
        _ => {
            let text = error.render().to_string();
            let message = text.strip_prefix("error: ").unwrap_or(&text);
            eprint!("solver: {message}");
        }
    }
    code
}
