//! `solver solve-postflop`: solves a spot, printing the spot, the memory estimate, a line per
//! exploitability measurement and the result, and appends a record of the solve's resources to
//! the resource log.
//!
//! Before anything is solved it checks the solution's destination and the streets the file is
//! to hold, so that a solve of many minutes is not lost to a mistyped folder, and refuses a spot
//! over a limit, saying what to change. The memory limit holds the save as it holds the solve,
//! since a save can need more than its solve did. At the end it writes the solution file through
//! [`save::save`], then appends the record.

use std::path::PathBuf;
use std::time::Instant;

use anyhow::{Result, anyhow};
use clap::Args as ClapArgs;
use pk_game::{Player, Street};
use pk_postflop::{
    Progress, Solution, SolveOptions, SolveRecord, StopReason, append_record, estimate_memory,
    estimate_save_memory, peak_resident_bytes, solve,
};

use crate::format::{self, bb_value, bytes, duration, field, percent, thousands, threads};
use crate::limits::{self, LimitArgs, Refusal};
use crate::save;
use crate::spot_input::{self, SpotArgs, SpotInput};

/// Where the resource log goes unless `--log` or `--no-log` says otherwise.
pub const DEFAULT_LOG: &str = "runs/log.jsonl";

/// The options of `solver solve-postflop`.
#[derive(Debug, ClapArgs)]
pub struct Args {
    #[command(flatten)]
    pub spot: SpotArgs,

    /// Stop once the exploitability is at or below this percent of the pot
    #[arg(
        long,
        value_name = "PERCENT",
        default_value = "0.3",
        value_parser = parse_target,
        help_heading = "Solving"
    )]
    pub target: f64,

    /// Stop after this many iterations, whatever the exploitability
    #[arg(
        long,
        value_name = "N",
        default_value = "10000",
        value_parser = parse_iterations,
        help_heading = "Solving"
    )]
    pub max_iters: u64,

    #[command(flatten)]
    pub limits: LimitArgs,

    /// The solution file to write; a solve takes this or --no-out
    ///
    /// Its folder must exist, and the file must not, unless --force is given; both are checked
    /// before the solve starts.
    #[arg(
        long,
        value_name = "FILE",
        required_unless_present = "no_out",
        help_heading = "Output"
    )]
    pub out: Option<PathBuf>,

    /// Solve without writing a solution file
    #[arg(long, conflicts_with = "out", help_heading = "Output")]
    pub no_out: bool,

    /// Overwrite the --out file if it exists
    #[arg(long, conflicts_with = "no_out", help_heading = "Output")]
    pub force: bool,

    /// The last street the solution file holds
    ///
    /// `turn` leaves the river out, which is nearly all of a flop solution's file; a river
    /// that a file leaves out is solved again when it is looked at. The file always holds the
    /// street the board starts on.
    #[arg(
        long,
        value_name = "STREET",
        default_value = "river",
        value_parser = save::parse_street,
        conflicts_with = "no_out",
        help_heading = "Output"
    )]
    pub last_street: Street,

    /// The resource log, to which a record of the solve is appended as a line of JSON
    #[arg(long, value_name = "FILE", default_value = DEFAULT_LOG, help_heading = "Output")]
    pub log: PathBuf,

    /// Append no record to the resource log
    #[arg(long, conflicts_with = "log", help_heading = "Output")]
    pub no_log: bool,
}

impl Args {
    /// The solve's options: these limits and stopping rules, the library's defaults otherwise.
    pub fn options(&self) -> SolveOptions {
        SolveOptions {
            target: Some(self.target),
            max_iterations: self.max_iters,
            ..self.limits.options()
        }
    }
}

fn parse_target(text: &str) -> Result<f64, String> {
    text.trim()
        .trim_end_matches('%')
        .parse::<f64>()
        .map_err(|_| TARGET_HELP.to_string())
        .and_then(check_target)
}

const TARGET_HELP: &str = "give a percent of the pot from 0 up, such as 0.3";

/// A target as a solve takes it: a finite percent of the pot, 0 or more.
pub fn check_target(target: f64) -> Result<f64, String> {
    if target.is_finite() && target >= 0.0 {
        Ok(target)
    } else {
        Err(TARGET_HELP.to_string())
    }
}

fn parse_iterations(text: &str) -> Result<u64, String> {
    text.trim()
        .replace(['_', ','], "")
        .parse()
        .map_err(|_| "give a whole number of iterations, such as 10000".to_string())
}

pub fn run(args: &Args) -> Result<()> {
    let input = spot_input::load(&args.spot)?;
    // Where the solution goes and what its file holds, checked before anything is solved.
    let out = match &args.out {
        Some(out) => {
            save::check_destination(out, args.force)?;
            Some((out, save::options(&input.spot, args.last_street)?))
        }
        None => None,
    };
    let options = args.options();

    format::print_spot(&input);
    let estimate = estimate_memory(&input.spot, &options)
        .map_err(|error| limits::explain(&input, &options, error))?;
    format::print_sizes(&input, &estimate);
    // What saving the solution will hold, which can be more than the solve does.
    let saving = out
        .as_ref()
        .map(|(_, save_options)| estimate_save_memory(&input.spot, &options, save_options))
        .transpose()
        .map_err(|error| limits::explain(&input, &options, error))?;
    let limit = bytes(options.max_memory);
    let memory = match &saving {
        Some(saving) => format!(
            "about {} to solve, the most {}, and {} to save, by the estimates; the limit is \
             {limit}",
            bytes(estimate.total),
            estimate.peak_phase,
            bytes(saving.total)
        ),
        None => format!(
            "about {} by the estimate, the most {}; the limit is {limit}",
            bytes(estimate.total),
            estimate.peak_phase
        ),
    };
    field("Memory", memory);
    let over = limits::over_memory(&options, &estimate, saving.as_ref());
    if !over.is_empty() {
        return Err(Refusal::new(&input, &options, &over).error());
    }

    let storage = if options.compressed_storage {
        "16-bit storage"
    } else {
        "f32 storage"
    };
    println!();
    println!(
        "Solving to {}% of the pot, for at most {} iterations, on {}, with {storage}",
        args.target,
        thousands(options.max_iterations),
        threads(estimate.threads)
    );
    println!();
    println!(
        "{:>11}  {:>12}  {:>10}  {:>12}",
        "iterations", "exploitable", "elapsed", "peak memory"
    );
    let solution = solve(&input.spot, &options, print_progress)
        .map_err(|error| limits::explain(&input, &options, error))?;
    let peak = peak_resident_bytes();

    println!();
    print_result(&input, &options, &solution);
    match peak {
        Some(peak) => field(
            "Peak memory",
            format!(
                "{} resident, against an estimate of {}",
                bytes(peak),
                bytes(estimate.total)
            ),
        ),
        None => field(
            "Peak memory",
            "unknown: the operating system does not report it",
        ),
    }

    // The solution first, since it is what the solve was for; then the record, even if the
    // solution could not be written.
    let saved = out.map(|(out, save_options)| {
        field(
            "Saving",
            format!(
                "`{}`, holding {}",
                out.display(),
                save::streets(&input.spot, &save_options)
            ),
        );
        let start = Instant::now();
        let report = save::save(&solution, out, &save_options)?;
        field(
            "Saved",
            format!(
                "{} in {}; as stored, in 16 bits a probability, exploitable for {} of the pot",
                bytes(report.bytes),
                duration(start.elapsed()),
                percent(report.exploitability.percent_of_pot)
            ),
        );
        // The save's memory, when it took the process past the solve's peak.
        if let (Some(solved), Some(saved), Some(saving)) = (peak, peak_resident_bytes(), &saving)
            && saved > solved
        {
            field(
                "  its memory",
                format!(
                    "{} resident at the most, against an estimate of {}",
                    bytes(saved),
                    bytes(saving.total)
                ),
            );
        }
        Ok::<(), anyhow::Error>(())
    });
    let logged = (!args.no_log).then(|| {
        let record = SolveRecord::new(&input.spot, &options, &solution, &estimate, peak);
        append_record(&args.log, &record).map_err(|error| {
            anyhow!(
                "could not append the solve's record to the resource log `{}`: {error}; give \
                 another file with --log, or pass --no-log",
                args.log.display()
            )
        })?;
        field(
            "Log",
            format!("appended the solve's record to `{}`", args.log.display()),
        );
        Ok::<(), anyhow::Error>(())
    });
    match (saved, logged) {
        (Some(Err(save)), Some(Err(log))) => Err(anyhow!("{save:#}; and {log:#}")),
        (Some(Err(error)), _) | (_, Some(Err(error))) => Err(error),
        _ => Ok(()),
    }
}

fn print_progress(progress: &Progress) {
    let memory = peak_resident_bytes().map_or_else(|| "unknown".to_string(), bytes);
    println!(
        "{:>11}  {:>12}  {:>10}  {:>12}",
        thousands(progress.iterations),
        percent(progress.exploitability.percent_of_pot),
        duration(progress.elapsed),
        memory
    );
}

/// How the solve stopped, its exploitability and each player's value.
fn print_result(input: &SpotInput, options: &SolveOptions, solution: &Solution) {
    let exploitability = solution.exploitability();
    let iterations = thousands(solution.iterations());
    let time = duration(solution.elapsed());
    let stopped = match solution.stop() {
        StopReason::Target => format!("at the target, after {iterations} iterations, in {time}"),
        StopReason::IterationCap => format!("at the cap of {iterations} iterations, in {time}"),
        StopReason::TimeCap => format!("at the time cap, after {iterations} iterations, in {time}"),
    };
    field("Stopped", stopped);
    let mut exploitable = format!(
        "{} of the pot, {} per hand dealt",
        percent(exploitability.percent_of_pot),
        bb_value(exploitability.chips, 4)
    );
    if let Some(target) = options.target
        && exploitability.percent_of_pot > target
    {
        exploitable.push_str(&format!(", above the target of {target}%"));
    }
    field("Exploitable", exploitable);
    let values = solution.values();
    field(
        "Values",
        format!(
            "{} {}, {} {}, per hand dealt",
            input.player(Player::Oop),
            bb_value(values[0], 2),
            input.player(Player::Ip),
            bb_value(values[1], 2)
        ),
    );
}
