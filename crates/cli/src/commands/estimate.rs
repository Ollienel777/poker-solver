//! `solver estimate`: prints the memory a solve of a spot would need, item by item, then the
//! memory that saving its solution would need, and whether `solve-postflop` would refuse it,
//! without building the game.
//!
//! A spot over the node limit is estimated as if the limit were lifted, so that the estimate
//! still says how much memory its tree would need. A refusal is the answer the command was asked
//! for, not an error: it prints it, with what to change, and exits with 0.

use anyhow::Result;
use clap::Args as ClapArgs;
use pk_game::{Street, TreeError};
use pk_postflop::{SolveError, SolveOptions, estimate_memory, estimate_save_memory};

use crate::format::{self, bytes, threads};
use crate::limits::{self, LimitArgs, Over, Refusal};
use crate::save;
use crate::spot_input::{self, SpotArgs};

/// The options of `solver estimate`: the spot, the limits and storage of the solve, and what
/// its solution file would hold.
#[derive(Debug, ClapArgs)]
pub struct Args {
    #[command(flatten)]
    pub spot: SpotArgs,

    #[command(flatten)]
    pub limits: LimitArgs,

    /// Estimate a solve that writes no solution file, as `solve-postflop --no-out` runs it
    #[arg(long, help_heading = "Output")]
    pub no_out: bool,

    /// The last street the solution file would hold, as for `solve-postflop`
    #[arg(
        long,
        value_name = "STREET",
        default_value = "river",
        value_parser = save::parse_street,
        conflicts_with = "no_out",
        help_heading = "Output"
    )]
    pub last_street: Street,
}

pub fn run(args: &Args) -> Result<()> {
    let input = spot_input::load(&args.spot)?;
    let options = args.limits.options();
    let save_options = if args.no_out {
        None
    } else {
        Some(save::options(&input.spot, args.last_street)?)
    };
    format::print_spot(&input);

    let mut over = Vec::new();
    // The options the estimates are made under: these, or these with the node limit lifted.
    let mut estimated = options;
    let estimate = match estimate_memory(&input.spot, &options) {
        Ok(estimate) => Some(estimate),
        Err(SolveError::PublicTreeTooLarge { nodes, limit }) => {
            over.push(Over::PublicTree { nodes, limit });
            estimated = SolveOptions {
                max_nodes: nodes,
                ..options
            };
            estimate_memory(&input.spot, &estimated).ok()
        }
        Err(SolveError::Tree(TreeError::TooLarge { limit })) => {
            over.push(Over::BettingTree { limit });
            None
        }
        Err(error) => return Err(limits::explain(&input, &options, error)),
    };

    if let Some(estimate) = &estimate {
        format::print_sizes(&input, estimate);
        let storage = if options.compressed_storage {
            "16-bit storage"
        } else {
            "f32 storage"
        };
        let condition = if estimated.max_nodes == options.max_nodes {
            ""
        } else {
            ", as if the node limit were lifted"
        };
        println!();
        println!(
            "Memory a solve would need, with {storage} on {}{condition}, estimated before \
             anything is allocated:",
            threads(estimate.threads)
        );
        println!("{estimate}");
        let saving = match &save_options {
            Some(save_options) => {
                let saving = estimate_save_memory(&input.spot, &estimated, save_options)
                    .map_err(|error| limits::explain(&input, &options, error))?;
                println!();
                println!(
                    "Memory saving its solution would need, in a file holding {}{condition}:",
                    save::streets(&input.spot, save_options)
                );
                println!("{saving}");
                Some(saving)
            }
            None => None,
        };
        over.extend(limits::over_memory(&options, estimate, saving.as_ref()));
    }

    println!();
    if over.is_empty() {
        let within = if save_options.is_some() {
            "the solve's estimate and its save's are"
        } else {
            "the estimate is"
        };
        println!(
            "A solve would run: {within} within the limit of {}.",
            bytes(options.max_memory)
        );
        return Ok(());
    }
    let refusal = Refusal::new(&input, &options, &over);
    match refusal.reasons.as_slice() {
        [reason] => println!("A solve would be refused: {reason}."),
        reasons => {
            println!("A solve would be refused:");
            for reason in reasons {
                println!("- {reason}");
            }
        }
    }
    println!("To solve it, {}.", refusal.advice);
    Ok(())
}
