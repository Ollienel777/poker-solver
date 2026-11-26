//! `solver estimate`: prints the memory a solve of a spot would need, item by item, then the
//! memory that saving its solution would need, and whether `solve-postflop` would refuse it,
//! without building the game.
//!
//! A spot over the node limit is estimated as if the limit were lifted, so that the estimate
//! still says how much memory its tree would need. A refusal is the answer the command was asked
//! for, not an error: it prints it, with what to change, and exits with 0.

use anyhow::Result;
use clap::Args as ClapArgs;
use pk_game::Street;

use crate::format::{self, bytes, threads};
use crate::limits::{self, LimitArgs, Refusal};
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

    let estimates = limits::estimates(&input, &options, save_options.as_ref())?;
    if let Some(estimate) = &estimates.solve {
        format::print_sizes(&input, estimate);
        let storage = if options.compressed_storage {
            "16-bit storage"
        } else {
            "f32 storage"
        };
        let condition = if estimates.lifted(&options) {
            ", as if the node limit were lifted"
        } else {
            ""
        };
        println!();
        println!(
            "Memory a solve would need, with {storage} on {}{condition}, estimated before \
             anything is allocated:",
            threads(estimate.threads)
        );
        println!("{estimate}");
        if let (Some(save_options), Some(saving)) = (&save_options, &estimates.save) {
            println!();
            println!(
                "Memory saving its solution would need, in a file holding {}{condition}:",
                save::streets(&input.spot, save_options)
            );
            println!("{saving}");
        }
    }
    let over = estimates.over;

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
