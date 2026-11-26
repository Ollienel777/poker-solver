//! The limits a solve runs under, the options that set them, and what to do when a spot is over
//! one.
//!
//! `solve-postflop` and `estimate` take the same limits, so that an estimate is of the solve the
//! same options would run, and they refuse a spot, or say a solve would be refused, by the same
//! rules as the library: a betting or public tree over the node limit, and a memory estimate
//! over the memory limit. The memory limit holds the save of a solve's solution as it holds the
//! solve: a save can need more memory than its solve did, and a solve that fits is lost if its
//! save does not. Each refusal says what to change.

use anyhow::anyhow;
use clap::Args;
use pk_game::{Street, TreeError};
use pk_postflop::{
    MemoryEstimate, SaveMemoryEstimate, SaveOptions, SolveError, SolveOptions, estimate_memory,
    estimate_save_memory,
};

use crate::format::{bytes, thousands};
use crate::spot_input::{SpotInput, TreeSource};

/// A tree file of one half-pot bet a street and no raises, shipped with the configs.
pub const ONE_BET_TREE: &str = "configs/trees/one_bet.toml";

/// The options that set a solve's limits and storage.
#[derive(Debug, Clone, Args)]
pub struct LimitArgs {
    /// The threads to solve on; the results are the same for every count [default: all the
    /// machine offers]
    #[arg(long, value_name = "N", value_parser = parse_threads, help_heading = "Limits")]
    pub threads: Option<usize>,

    /// Refuse a solve whose memory estimate is over this many gigabytes (10^9 bytes)
    #[arg(
        long,
        value_name = "GB",
        default_value = "10",
        value_parser = parse_gigabytes,
        help_heading = "Limits"
    )]
    pub max_memory: u64,

    /// Refuse a betting or public tree of more nodes than this
    #[arg(
        long,
        value_name = "N",
        default_value = "1000000",
        value_parser = parse_nodes,
        help_heading = "Limits"
    )]
    pub max_nodes: u64,

    /// Store the regrets and strategy sums in 16 bits rather than as f32
    ///
    /// It halves their memory, at a cost in time, and a solve stops improving at a few
    /// thousandths of a percent of the pot where an f32 solve goes on.
    #[arg(long, help_heading = "Limits")]
    pub compressed: bool,
}

impl LimitArgs {
    /// The library's default options with these limits.
    pub fn options(&self) -> SolveOptions {
        let defaults = SolveOptions::default();
        SolveOptions {
            threads: self.threads.unwrap_or(defaults.threads),
            max_memory: self.max_memory,
            max_nodes: self.max_nodes,
            compressed_storage: self.compressed,
            ..defaults
        }
    }
}

fn parse_threads(text: &str) -> Result<usize, String> {
    match text.trim().parse::<usize>() {
        Ok(0) => Err("a solve runs on 1 thread or more".to_string()),
        Ok(threads) => Ok(threads),
        Err(_) => Err("give a number of threads, such as 1 or 8".to_string()),
    }
}

pub fn parse_gigabytes(text: &str) -> Result<u64, String> {
    match text.trim().parse::<f64>() {
        Ok(gb) if gb.is_finite() && gb > 0.0 => Ok((gb * 1e9) as u64),
        _ => Err("give a size in gigabytes, more than 0, such as 10 or 2.5".to_string()),
    }
}

pub fn parse_nodes(text: &str) -> Result<u64, String> {
    match text.trim().replace(['_', ','], "").parse::<u64>() {
        Ok(0) => Err("a tree has at least its root, so the limit is 1 or more".to_string()),
        Ok(nodes) => Ok(nodes),
        Err(_) => Err("give a whole number of nodes, such as 1000000".to_string()),
    }
}

/// A limit a spot is over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Over {
    /// The public tree would have `nodes` nodes, more than `limit`.
    PublicTree { nodes: u64, limit: u64 },
    /// The betting tree has more than `limit` nodes.
    BettingTree { limit: u32 },
    /// The memory estimate is `estimate` bytes, more than the limit in the options.
    Memory { estimate: u64 },
    /// Saving the solution, in a file that holds the streets up to `last_street`, would need
    /// `estimate` bytes by its estimate, more than the limit in the options.
    SaveMemory { estimate: u64, last_street: Street },
}

/// The memory limits a solve with these estimates is over: the solve's own, and its save's when
/// the solution is to be saved.
pub fn over_memory(
    options: &SolveOptions,
    solve: &MemoryEstimate,
    save: Option<&SaveMemoryEstimate>,
) -> Vec<Over> {
    let mut over = Vec::new();
    if solve.total > options.max_memory {
        over.push(Over::Memory {
            estimate: solve.total,
        });
    }
    if let Some(save) = save
        && save.total > options.max_memory
    {
        over.push(Over::SaveMemory {
            estimate: save.total,
            last_street: save.last_street,
        });
    }
    over
}

impl Over {
    /// The limit a solve error stands for, if it is one.
    pub fn of(error: &SolveError) -> Option<Over> {
        match *error {
            SolveError::PublicTreeTooLarge { nodes, limit } => {
                Some(Over::PublicTree { nodes, limit })
            }
            SolveError::Tree(TreeError::TooLarge { limit }) => Some(Over::BettingTree { limit }),
            SolveError::MemoryTooLarge { estimate, .. } => Some(Over::Memory { estimate }),
            _ => None,
        }
    }
}

/// Why a spot is refused, and what to change.
#[derive(Debug, Clone, PartialEq)]
pub struct Refusal {
    /// What is over which limit, one for each limit: `the public tree, with a child for every
    /// card that can fall, would have 9,789,026 nodes, more than the limit of 1,000,000`.
    pub reasons: Vec<String>,
    /// What to change, the likeliest first.
    pub advice: String,
}

impl Refusal {
    /// Why a spot over each of `over` is refused under `options`, and what to change.
    ///
    /// The advice is to size the tree with fewer bets and raises, in the file it came from, or
    /// to raise the limits it is over if the machine has the memory; and, first, to store the
    /// regrets and strategy sums in 16 bits when the solve's memory is the only limit it is
    /// over, or to leave the river out of the solution file when the save's is, if a second
    /// estimate says that would bring it under. A save's memory does not depend on the
    /// storage, nor a solve's on what its file holds.
    pub fn new(input: &SpotInput, options: &SolveOptions, over: &[Over]) -> Refusal {
        let reasons = over
            .iter()
            .map(|over| match *over {
                Over::PublicTree { nodes, limit } => format!(
                    "the public tree, with a child for every card that can fall, would have {} \
                     nodes, more than the limit of {}",
                    thousands(nodes),
                    thousands(limit)
                ),
                Over::BettingTree { limit } => format!(
                    "the betting tree has more nodes than the limit of {}",
                    thousands(limit.into())
                ),
                Over::Memory { estimate } => format!(
                    "the solve would need about {} of memory by its estimate, more than the \
                     limit of {}",
                    bytes(estimate),
                    bytes(options.max_memory)
                ),
                Over::SaveMemory { estimate, .. } => format!(
                    "saving the solution would need about {} of memory by its estimate, more \
                     than the limit of {}",
                    bytes(estimate),
                    bytes(options.max_memory)
                ),
            })
            .collect();

        let solving = over.iter().any(|over| matches!(over, Over::Memory { .. }));
        let saving = over.iter().find_map(|over| match *over {
            Over::SaveMemory { last_street, .. } => Some(last_street),
            _ => None,
        });
        let memory = solving || saving.is_some();
        let nodes = over
            .iter()
            .any(|over| matches!(over, Over::PublicTree { .. } | Over::BettingTree { .. }));
        let mut advice = Vec::new();
        if let Some(Street::River) = saving
            && !solving
            && !nodes
            && input.spot.street() < Street::River
        {
            let without_river = SaveOptions {
                last_street: Street::Turn,
            };
            if let Ok(smaller) = estimate_save_memory(&input.spot, options, &without_river)
                && smaller.total <= options.max_memory
            {
                advice.push(format!(
                    "pass --last-street turn to leave the river out of the solution file, \
                     which brings the save's estimate to about {}",
                    bytes(smaller.total)
                ));
            }
        }
        if solving && saving.is_none() && !nodes && !options.compressed_storage {
            let compressed = SolveOptions {
                compressed_storage: true,
                ..*options
            };
            if let Ok(half) = estimate_memory(&input.spot, &compressed)
                && half.total <= options.max_memory
            {
                advice.push(format!(
                    "pass --compressed to store the regrets and strategy sums in 16 bits, which \
                     brings the estimate to about {}",
                    bytes(half.total)
                ));
            }
        }
        advice.push(fewer_sizes(input));
        advice.push(
            match (nodes, memory) {
                (true, false) => {
                    "raise --max-nodes, if the machine has the memory that `solver estimate` gives"
                }
                (false, _) => "raise --max-memory, if the machine has the memory",
                (true, true) => "raise --max-nodes and --max-memory, if the machine has the memory",
            }
            .to_string(),
        );
        Refusal {
            reasons,
            advice: advice.join("; or "),
        }
    }

    /// The refusal as `solve-postflop`'s error.
    pub fn error(&self) -> anyhow::Error {
        anyhow!(
            "{}, so the solve is refused; {}",
            self.reasons.join("; and "),
            self.advice
        )
    }
}

/// The advice for a tree with too many nodes or too much memory: fewer sizes, in the file the
/// tree came from.
fn fewer_sizes(input: &SpotInput) -> String {
    match &input.tree {
        TreeSource::Config { name, .. } => format!(
            "pass --tree with a tree file of fewer bet and raise sizes than config {name}'s \
             [postflop] table, such as {ONE_BET_TREE} (one half-pot bet a street, no raises)"
        ),
        TreeSource::TreeFile(path) => format!(
            "give the tree file `{}` fewer bet and raise sizes, or a lower max_raises",
            path.display()
        ),
        TreeSource::SpotFile => "give the spot file's [tree] table fewer bet and raise sizes, or \
                                 a lower max_raises"
            .to_string(),
    }
}

/// The estimates of a solve of `input`'s spot under `options`, and of saving its solution with
/// `save` if it is to be saved, with every limit they are over: what `estimate` prints and what
/// `solver serve` answers before a solve from the page starts.
#[derive(Debug, Clone)]
pub struct Estimates {
    /// The options the estimates are made under: `options`, or `options` with the node limit
    /// lifted to the public tree's size when the public tree is over it, so that the estimate
    /// still says what the tree would need.
    pub estimated: SolveOptions,
    /// The solve's estimate; `None` when the betting tree is over the node limit, which leaves
    /// nothing to estimate.
    pub solve: Option<MemoryEstimate>,
    /// The save's estimate, when the solution is to be saved and the solve has an estimate.
    pub save: Option<SaveMemoryEstimate>,
    /// The limits the spot is over; empty if a solve would run.
    pub over: Vec<Over>,
}

impl Estimates {
    /// Whether the node limit was lifted to estimate the spot.
    pub fn lifted(&self, options: &SolveOptions) -> bool {
        self.estimated.max_nodes != options.max_nodes
    }
}

/// [`Estimates`] of `input`'s spot under `options`, with `save` the options of its solution file
/// if it is to be saved. A solve error that is not a limit, such as a save before the spot's
/// first street, is an error.
pub fn estimates(
    input: &SpotInput,
    options: &SolveOptions,
    save: Option<&SaveOptions>,
) -> anyhow::Result<Estimates> {
    let mut over = Vec::new();
    let mut estimated = *options;
    let solve = match estimate_memory(&input.spot, options) {
        Ok(estimate) => Some(estimate),
        Err(SolveError::PublicTreeTooLarge { nodes, limit }) => {
            over.push(Over::PublicTree { nodes, limit });
            estimated = SolveOptions {
                max_nodes: nodes,
                ..*options
            };
            estimate_memory(&input.spot, &estimated).ok()
        }
        Err(SolveError::Tree(TreeError::TooLarge { limit })) => {
            over.push(Over::BettingTree { limit });
            None
        }
        Err(error) => return Err(explain(input, options, error)),
    };
    let saving = match (&solve, save) {
        (Some(_), Some(save)) => Some(
            estimate_save_memory(&input.spot, &estimated, save)
                .map_err(|error| explain(input, options, error))?,
        ),
        _ => None,
    };
    if let Some(estimate) = &solve {
        over.extend(over_memory(options, estimate, saving.as_ref()));
    }
    Ok(Estimates {
        estimated,
        solve,
        save: saving,
        over,
    })
}

/// A solve error as the command reports it: a refusal with what to change, or the error.
pub fn explain(input: &SpotInput, options: &SolveOptions, error: SolveError) -> anyhow::Error {
    match Over::of(&error) {
        Some(over) => Refusal::new(input, options, &[over]).error(),
        None => anyhow!("{error}"),
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use pk_postflop::{Spot, TreeSizing};

    use super::*;
    use crate::spot_input::Source;

    fn repo(path: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(path)
    }

    /// A test spot as a spot file gives it.
    fn input(path: &str) -> SpotInput {
        SpotInput {
            spot: Spot::load(repo(path)).unwrap(),
            source: Source::SpotFile(path.into()),
            tree: TreeSource::SpotFile,
        }
    }

    #[test]
    fn the_limits_set_the_options_and_leave_the_rest_at_the_defaults() {
        let args = LimitArgs {
            threads: Some(3),
            max_memory: 2_500_000_000,
            max_nodes: 50_000,
            compressed: true,
        };
        let options = args.options();
        assert_eq!(options.threads, 3);
        assert_eq!(options.max_memory, 2_500_000_000);
        assert_eq!(options.max_nodes, 50_000);
        assert!(options.compressed_storage);
        let defaults = SolveOptions::default();
        assert_eq!(options.target, defaults.target);
        assert_eq!(options.max_iterations, defaults.max_iterations);
        let unset = LimitArgs {
            threads: None,
            ..args
        };
        assert_eq!(unset.options().threads, defaults.threads);
    }

    #[test]
    fn the_values_are_parsed_with_messages_that_say_what_to_give() {
        assert_eq!(parse_threads("8"), Ok(8));
        assert!(parse_threads("0").unwrap_err().contains("1 thread or more"));
        assert!(parse_threads("all").unwrap_err().contains("such as 1 or 8"));
        assert_eq!(parse_gigabytes("10"), Ok(10_000_000_000));
        assert_eq!(parse_gigabytes("2.5"), Ok(2_500_000_000));
        assert!(parse_gigabytes("0").is_err());
        assert!(parse_gigabytes("ten").is_err());
        assert_eq!(parse_nodes("1_000_000"), Ok(1_000_000));
        assert_eq!(parse_nodes("2,000,000"), Ok(2_000_000));
        assert!(parse_nodes("0").is_err());
    }

    #[test]
    fn a_spot_over_the_memory_limit_is_told_of_16_bit_storage_when_it_would_fit() {
        let input = input("reference/spots/turn/checked_flop_two_bets.toml");
        let options = SolveOptions {
            threads: 1,
            ..SolveOptions::default()
        };
        let f32 = estimate_memory(&input.spot, &options).unwrap().total;
        let compressed = SolveOptions {
            compressed_storage: true,
            ..options
        };
        let half = estimate_memory(&input.spot, &compressed).unwrap().total;
        assert!(half < f32);

        // A limit between the two estimates: 16-bit storage fits, and is the first advice.
        let between = SolveOptions {
            max_memory: (half + f32) / 2,
            ..options
        };
        let refusal = Refusal::new(&input, &between, &[Over::Memory { estimate: f32 }]);
        println!("{}", refusal.error());
        assert_eq!(refusal.reasons.len(), 1);
        assert!(refusal.reasons[0].contains(&format!("about {} of memory", bytes(f32))));
        assert!(refusal.advice.starts_with(&format!(
            "pass --compressed to store the regrets and strategy sums in 16 bits, which brings \
             the estimate to about {}",
            bytes(half)
        )));
        assert!(
            refusal
                .advice
                .contains("give the spot file's [tree] table fewer")
        );
        assert!(
            refusal
                .advice
                .ends_with("raise --max-memory, if the machine has the memory")
        );

        // Below both, or already compressed, it is not offered.
        let below = SolveOptions {
            max_memory: half / 2,
            ..options
        };
        let refusal = Refusal::new(&input, &below, &[Over::Memory { estimate: f32 }]);
        assert!(!refusal.advice.contains("--compressed"));
        let refusal = Refusal::new(
            &input,
            &SolveOptions {
                compressed_storage: true,
                ..between
            },
            &[Over::Memory { estimate: half }],
        );
        assert!(!refusal.advice.contains("--compressed"));
    }

    #[test]
    fn a_save_over_the_memory_limit_is_told_to_leave_out_the_river_when_that_would_fit() {
        let input = input("reference/spots/turn/checked_flop_two_bets.toml");
        let options = SolveOptions {
            threads: 1,
            ..SolveOptions::default()
        };
        let saving = |last_street| {
            estimate_save_memory(&input.spot, &options, &SaveOptions { last_street }).unwrap()
        };
        let (whole, partial) = (saving(Street::River), saving(Street::Turn));
        assert!(partial.total < whole.total);
        let solve = estimate_memory(&input.spot, &options).unwrap();

        // A limit between the two saves: the solve's estimate is not in question, and leaving
        // out the river is the first advice. 16-bit storage, which a save does not use, is not.
        let between = SolveOptions {
            max_memory: (partial.total + whole.total) / 2,
            ..options
        };
        let over = [Over::SaveMemory {
            estimate: whole.total,
            last_street: Street::River,
        }];
        let refusal = Refusal::new(&input, &between, &over);
        println!("{}", refusal.error());
        assert_eq!(refusal.reasons.len(), 1);
        assert!(refusal.reasons[0].starts_with(&format!(
            "saving the solution would need about {} of memory by its estimate",
            bytes(whole.total)
        )));
        assert!(refusal.advice.starts_with(&format!(
            "pass --last-street turn to leave the river out of the solution file, which brings \
             the save's estimate to about {}",
            bytes(partial.total)
        )));
        assert!(!refusal.advice.contains("--compressed"));
        assert!(
            refusal
                .advice
                .ends_with("raise --max-memory, if the machine has the memory")
        );

        // Below both saves, or for a file that already leaves the river out, it is not offered.
        let below = SolveOptions {
            max_memory: partial.total / 2,
            ..options
        };
        let refusal = Refusal::new(&input, &below, &over);
        assert!(!refusal.advice.contains("--last-street"));
        let without_river = [Over::SaveMemory {
            estimate: partial.total,
            last_street: Street::Turn,
        }];
        let refusal = Refusal::new(&input, &between, &without_river);
        assert!(!refusal.advice.contains("--last-street"));

        // The estimates give the limits a solve is over: each of the two that is.
        assert_eq!(
            over_memory(&between, &solve, Some(&whole)),
            if solve.total > between.max_memory {
                vec![
                    Over::Memory {
                        estimate: solve.total,
                    },
                    over[0],
                ]
            } else {
                over.to_vec()
            }
        );
        assert_eq!(over_memory(&options, &solve, Some(&whole)), []);
        assert_eq!(
            over_memory(&below, &solve, None),
            [Over::Memory {
                estimate: solve.total
            }]
        );
        // Over both, neither of the two first pieces of advice is given: each mends one only.
        let both = [
            Over::Memory {
                estimate: solve.total,
            },
            over[0],
        ];
        let refusal = Refusal::new(&input, &between, &both);
        assert_eq!(refusal.reasons.len(), 2);
        assert!(!refusal.advice.contains("--last-street"));
        assert!(!refusal.advice.contains("--compressed"));
    }

    #[test]
    fn a_tree_over_the_node_limit_is_told_to_take_fewer_sizes_from_where_it_came() {
        let mut input = input("reference/spots/turn/checked_flop_two_bets.toml");
        let options = SolveOptions::default();
        let over = [Over::PublicTree {
            nodes: 9_789_026,
            limit: 1_000_000,
        }];
        input.tree = TreeSource::Config {
            name: "A".into(),
            path: "configs/a.toml".into(),
        };
        let refusal = Refusal::new(&input, &options, &over);
        assert_eq!(
            refusal.error().to_string(),
            "the public tree, with a child for every card that can fall, would have 9,789,026 \
             nodes, more than the limit of 1,000,000, so the solve is refused; pass --tree with \
             a tree file of fewer bet and raise sizes than config A's [postflop] table, such as \
             configs/trees/one_bet.toml (one half-pot bet a street, no raises); or raise \
             --max-nodes, if the machine has the memory that `solver estimate` gives"
        );
        input.tree = TreeSource::TreeFile("big.toml".into());
        let refusal = Refusal::new(&input, &options, &over);
        assert!(
            refusal
                .advice
                .starts_with("give the tree file `big.toml` fewer bet and raise")
        );

        // Over both limits: one piece of advice for the two.
        let both = [
            over[0],
            Over::Memory {
                estimate: 56_250_000_000,
            },
        ];
        let refusal = Refusal::new(&input, &options, &both);
        assert_eq!(refusal.reasons.len(), 2);
        assert!(
            refusal
                .error()
                .to_string()
                .contains("1,000,000; and the solve would need")
        );
        assert!(
            refusal
                .advice
                .ends_with("raise --max-nodes and --max-memory, if the machine has the memory")
        );
    }

    #[test]
    fn the_one_bet_tree_is_where_the_advice_says() {
        let tree = TreeSizing::load(repo(ONE_BET_TREE)).unwrap();
        assert_eq!(tree.max_raises, 0);
        for sizing in [&tree.flop, &tree.turn, &tree.river] {
            assert_eq!(sizing.as_ref().unwrap().bets, [0.5]);
        }
    }
}
