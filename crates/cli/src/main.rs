//! The `solver` binary: the solver's command line.
//!
//! Each command is a module of [`commands`], with its arguments and its `run`. What the commands
//! share has modules of its own: [`spot_input`] turns the options that name a spot into a
//! [`pk_postflop::Spot`], [`limits`] holds the limits a solve runs under and says what to do
//! when a spot is over one, [`format`] writes amounts, sizes and times and prints the spot, and
//! [`save`] is where a solve's solution is written.
//!
//! Only this crate prints. Everything a command reports goes to stdout. Errors go to stderr,
//! each as one `solver: <message>`, with exit code 1, or 2 for a command line that does not
//! parse.

use std::process::ExitCode;

use clap::error::ErrorKind;
use clap::{Parser, Subcommand};

mod commands;
mod format;
mod limits;
mod save;
mod spot_input;

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
}

fn main() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error) => return parse_error(&error),
    };
    let result = match cli.command {
        Command::SolvePostflop(args) => commands::solve_postflop::run(&args),
        Command::Estimate(args) => commands::estimate::run(&args),
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
