//! Solving from the page: the inputs a solve can be made from, the estimate of a solve before it
//! starts, and the solve itself, which runs on a thread of its own while the server goes on
//! answering, one at a time.
//!
//! A solve from the page is `solve-postflop --ranges F --board B [--tree T]` with its file in the
//! served folder: the spot is read by [`spot_input::load`], its estimates and refusals are
//! [`limits::estimates`] and [`Refusal`], its destination and streets are checked by [`save`],
//! and its solution is written by [`save::save`], so that the page and the command line refuse
//! the same spots with the same words. Its progress is the library's progress callback, which
//! [`Job`] keeps; nothing here prints but a line when a solve ends.
//!
//! A solve is cancelled at its next measurement of exploitability: the callback unwinds the
//! solve's thread with [`panic::resume_unwind`], which drops what the solve holds on the way out
//! and runs no panic hook, and the thread catches it. Nothing is saved.

use std::fs;
use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

use pk_game::{RangeFile, Street};
use pk_postflop::{
    Exploitability, Progress, SaveOptions, SolveOptions, SolveRecord, StopReason, append_record,
    peak_resident_bytes, solve,
};
use serde::{Deserialize, Serialize};

use super::{Answer, EXTENSION, Reply, has_extension, plain};
use crate::commands::solve_postflop::check_target;
use crate::format::{bytes, street_sizing, tree_rules};
use crate::json::{ExploitabilityJson, SpotJson, stop as stop_name};
use crate::limits::{self, Estimates, Over, Refusal};
use crate::save;
use crate::spot_input::{self, SpotArgs, SpotInput, TreeSource, config_tree};

/// The folder of range files `serve` offers unless `--ranges` names another, laid out
/// `<config>/<formation>.toml`.
pub const DEFAULT_RANGES: &str = "reference/ranges";

/// The folder of tree files `serve` offers unless `--trees` names another.
pub const DEFAULT_TREES: &str = "configs/trees";

/// Where a solve from the page takes its inputs from, and what it runs under besides the
/// server's memory limit.
#[derive(Debug, Clone)]
pub struct Inputs {
    /// The folder of range files, `<config>/<formation>.toml`.
    pub ranges: PathBuf,
    /// The folder of tree files.
    pub trees: PathBuf,
    /// The node limit, as `solve-postflop --max-nodes`.
    pub max_nodes: u64,
    /// The resource log a solve's record is appended to, or `None` for none.
    pub log: Option<PathBuf>,
}

impl Default for Inputs {
    fn default() -> Inputs {
        Inputs {
            ranges: PathBuf::from(DEFAULT_RANGES),
            trees: PathBuf::from(DEFAULT_TREES),
            max_nodes: pk_postflop::PostflopGame::DEFAULT_MAX_NODES,
            log: None,
        }
    }
}

// ---------------------------------------------------------------------------------------------
// The inputs

/// `/api/inputs`: the range files and tree files a solve can be made from.
pub fn inputs(inputs: &Inputs, max_memory: u64) -> Reply {
    #[derive(Serialize)]
    struct List {
        ranges_folder: String,
        trees_folder: String,
        ranges: Vec<RangeEntry>,
        trees: Vec<TreeEntry>,
        max_memory: u64,
        max_nodes: u64,
    }
    #[derive(Serialize)]
    struct RangeEntry {
        name: String,
        config: Option<String>,
        formation: Option<String>,
        line: Option<String>,
        positions: Option<[String; 2]>,
        pot: Option<u32>,
        stack: Option<u32>,
        combos: Option<[f64; 2]>,
        config_tree: Option<String>,
        error: Option<String>,
    }
    #[derive(Serialize)]
    struct TreeEntry {
        name: String,
        summary: Option<String>,
        error: Option<String>,
    }

    let mut ranges = Vec::new();
    for config in sorted_entries(&inputs.ranges, true) {
        if !plain(&config) {
            continue;
        }
        for file in sorted_entries(&inputs.ranges.join(&config), false) {
            if !plain(&file) || !is_toml(&file) {
                continue;
            }
            let name = format!("{config}/{file}");
            let path = inputs.ranges.join(&config).join(&file);
            let mut entry = RangeEntry {
                name,
                config: None,
                formation: None,
                line: None,
                positions: None,
                pot: None,
                stack: None,
                combos: None,
                config_tree: None,
                error: None,
            };
            match RangeFile::load(&path) {
                Err(error) => entry.error = Some(format!("{error}")),
                Ok(range) => {
                    let combos = |side: &pk_game::PlayerRange| {
                        side.range.iter().map(|(_, w)| f64::from(w)).sum::<f64>()
                    };
                    entry.combos = Some([combos(&range.oop), combos(&range.ip)]);
                    entry.positions =
                        Some([range.oop.position, range.ip.position].map(|p| p.to_string()));
                    entry.pot = Some(range.pot);
                    entry.stack = Some(range.effective_stack);
                    match spot_input::read_config(&range, &path, None) {
                        Ok((config, _)) => {
                            entry.config_tree = Some(summary(&config_tree(&config.postflop)));
                        }
                        Err(error) => entry.error = Some(format!("{error:#}")),
                    }
                    entry.config = Some(range.config);
                    entry.formation = Some(range.formation);
                    entry.line = Some(range.line);
                }
            }
            ranges.push(entry);
        }
    }
    let trees = sorted_entries(&inputs.trees, false)
        .into_iter()
        .filter(|name| plain(name) && is_toml(name))
        .map(
            |name| match pk_postflop::TreeSizing::load(inputs.trees.join(&name)) {
                Ok(tree) => TreeEntry {
                    name,
                    summary: Some(summary(&tree)),
                    error: None,
                },
                Err(error) => TreeEntry {
                    name,
                    summary: None,
                    error: Some(format!("{error}")),
                },
            },
        )
        .collect();
    Reply::json(&List {
        ranges_folder: inputs.ranges.display().to_string(),
        trees_folder: inputs.trees.display().to_string(),
        ranges,
        trees,
        max_memory,
        max_nodes: inputs.max_nodes,
    })
}

/// The names in `folder`, sorted: of its folders if `folders`, else of its files. None if it
/// cannot be read.
fn sorted_entries(folder: &Path, folders: bool) -> Vec<String> {
    let Ok(entries) = fs::read_dir(folder) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .filter_map(Result::ok)
        .filter(|entry| {
            entry.file_type().is_ok_and(|kind| {
                if folders {
                    kind.is_dir()
                } else {
                    kind.is_file()
                }
            })
        })
        .filter_map(|entry| entry.file_name().into_string().ok())
        .collect();
    names.sort();
    names
}

fn is_toml(name: &str) -> bool {
    Path::new(name)
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("toml"))
}

/// A tree's sizing on one line: `flop: bets 50% of the pot / turn: ... / no raises`.
fn summary(tree: &pk_postflop::TreeSizing) -> String {
    let mut parts: Vec<String> = Street::ALL
        .into_iter()
        .filter_map(|street| {
            tree.street(street)
                .map(|sizing| format!("{street}: {}", street_sizing(sizing, tree.max_raises)))
        })
        .collect();
    parts.push(tree_rules(tree.max_raises, tree.allin_threshold));
    parts.join(" / ")
}

// ---------------------------------------------------------------------------------------------
// A solve asked for, checked and estimated

/// A solve as the page asks for it, the body of `/api/estimate` and `/api/solve`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SolveRequest {
    /// A range file of the ranges folder, `<config>/<formation>.toml`.
    pub ranges: String,
    /// 3, 4 or 5 cards, such as `Ks7d2c`.
    pub board: String,
    /// A tree file of the trees folder; `null` or `""` for the config's own tree.
    #[serde(default)]
    pub tree: Option<String>,
    /// The target, a percent of the pot: 0.3 by default.
    #[serde(default = "default_target")]
    pub target: f64,
    /// The iteration cap: 10,000 by default.
    #[serde(default = "default_iterations")]
    pub max_iterations: u64,
    /// The last street the file holds, `flop`, `turn` or `river`: the river by default.
    #[serde(default)]
    pub last_street: Option<String>,
    /// The solution file's name in the served folder: the spot's name by default, `.pksol`
    /// added if it has no extension.
    #[serde(default)]
    pub out: Option<String>,
    /// Overwrite the file if it exists.
    #[serde(default)]
    pub force: bool,
    /// Store the regrets and strategy sums in 16 bits, as `--compressed`.
    #[serde(default)]
    pub compressed: bool,
}

fn default_target() -> f64 {
    0.3
}

fn default_iterations() -> u64 {
    10_000
}

/// A value of the request, with `""` read as none.
fn given(value: &Option<String>) -> Option<&str> {
    value
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty())
}

impl SolveRequest {
    /// Reads a request's body.
    pub fn parse(body: &[u8]) -> Answer<SolveRequest> {
        serde_json::from_slice(body).map_err(|error| {
            Reply::error(
                400,
                format!(
                    "the request is not a solve: {error}; send JSON with `ranges` and `board`, \
                     and any of `tree`, `target`, `max_iterations`, `last_street`, `out`, \
                     `force` and `compressed`"
                ),
            )
        })
    }
}

/// A solve checked and estimated, ready to start.
pub struct Prepared {
    pub request: SolveRequest,
    pub input: SpotInput,
    pub options: SolveOptions,
    pub save: SaveOptions,
    /// The solution file's name in the served folder, and its path.
    pub out: String,
    pub path: PathBuf,
    /// Whether a file of that name is already there.
    pub exists: bool,
    pub estimates: Estimates,
}

impl Prepared {
    /// What the solve reserves of the memory limit while it runs: the larger of the solve's
    /// estimate and its save's.
    pub fn memory(&self) -> u64 {
        let solve = self.estimates.solve.as_ref().map_or(0, |e| e.total);
        let save = self.estimates.save.as_ref().map_or(0, |e| e.total);
        solve.max(save)
    }

    /// Why the solve is refused, if it is.
    pub fn refusal(&self) -> Option<Refusal> {
        (!self.estimates.over.is_empty())
            .then(|| Refusal::new(&self.input, &self.options, &self.estimates.over))
    }
}

/// A command-line error in the page's words: `--board `x`: ...` as `board `x`: ...`.
fn words(message: String) -> String {
    match message.strip_prefix("--") {
        Some(rest) => {
            let (option, rest) = rest.split_once(' ').unwrap_or((rest, ""));
            format!("{} {rest}", option.replace('-', " "))
        }
        None => message,
    }
}

/// Checks a solve asked for in `folder` and estimates it under `inputs` and `max_memory`. A
/// name that is not plain is refused with 400, one that names no file with 404, and any other
/// input that a solve would refuse with 400 and the command line's words. A spot over a limit
/// is not an error here: [`Prepared::refusal`] says so.
pub fn prepare(
    folder: &Path,
    inputs: &Inputs,
    max_memory: u64,
    request: SolveRequest,
) -> Answer<Prepared> {
    let ranges = range_path(inputs, &request.ranges)?;
    let tree = match given(&request.tree) {
        None => None,
        Some(name) => Some(tree_path(inputs, name)?),
    };
    spot_input::parse_board(&request.board)
        .map_err(|error| Reply::error(400, words(format!("{error:#}"))))?;
    let target = check_target(request.target)
        .map_err(|reason| Reply::error(400, format!("target {}: {reason}", request.target)))?;
    let last_street = match given(&request.last_street) {
        None => Street::River,
        Some(text) => save::parse_street(text)
            .map_err(|reason| Reply::error(400, format!("last street `{text}`: {reason}")))?,
    };
    let args = SpotArgs {
        ranges: Some(ranges),
        board: Some(request.board.clone()),
        tree,
        config: None,
        spot: None,
    };
    let input =
        spot_input::load(&args).map_err(|error| Reply::error(400, words(format!("{error:#}"))))?;
    let save = save::options(&input.spot, last_street)
        .map_err(|error| Reply::error(400, words(format!("{error:#}"))))?;
    let out = match given(&request.out) {
        None => format!("{}.{EXTENSION}", input.spot.name),
        Some(name) if Path::new(name).extension().is_none() => format!("{name}.{EXTENSION}"),
        Some(name) => name.to_string(),
    };
    if !plain(&out) || !has_extension(&out) {
        return Err(Reply::error(
            400,
            format!(
                "out `{out}`: give a file name for the solution, with no folder in it, ending in \
                 .{EXTENSION}"
            ),
        ));
    }
    let path = folder.join(&out);
    if path.is_dir() {
        return Err(Reply::error(
            400,
            format!("out `{out}` is a folder; give the name of a file to write"),
        ));
    }
    let exists = path.exists();
    let options = SolveOptions {
        target: Some(target),
        max_iterations: request.max_iterations,
        max_memory,
        max_nodes: inputs.max_nodes,
        compressed_storage: request.compressed,
        ..SolveOptions::default()
    };
    let estimates = limits::estimates(&input, &options, Some(&save))
        .map_err(|error| Reply::error(400, words(format!("{error:#}"))))?;
    Ok(Prepared {
        request,
        input,
        options,
        save,
        out,
        path,
        exists,
        estimates,
    })
}

/// The path of the range file `name`, `<config>/<formation>.toml`.
fn range_path(inputs: &Inputs, name: &str) -> Answer<PathBuf> {
    let parts = name.split_once('/');
    let Some((config, file)) = parts.filter(|(config, file)| plain(config) && plain(file)) else {
        return Err(Reply::error(
            400,
            format!(
                "ranges `{name}`: give a range file of the ranges folder as <config>/<file>.toml, \
                 as /api/inputs lists it"
            ),
        ));
    };
    let path = inputs.ranges.join(config).join(file);
    if !is_toml(file) || !path.is_file() {
        return Err(Reply::error(
            404,
            format!(
                "ranges `{name}`: there is no range file of that name in `{}`; /api/inputs lists \
                 them",
                inputs.ranges.display()
            ),
        ));
    }
    Ok(path)
}

/// The path of the tree file `name`.
fn tree_path(inputs: &Inputs, name: &str) -> Answer<PathBuf> {
    if !plain(name) {
        return Err(Reply::error(
            400,
            format!(
                "tree `{name}`: give a tree file of the trees folder, with no folder in it, as \
                 /api/inputs lists it"
            ),
        ));
    }
    let path = inputs.trees.join(name);
    if !is_toml(name) || !path.is_file() {
        return Err(Reply::error(
            404,
            format!(
                "tree `{name}`: there is no tree file of that name in `{}`; /api/inputs lists them",
                inputs.trees.display()
            ),
        ));
    }
    Ok(path)
}

/// What `/api/estimate` answers, and what a refused start carries.
#[derive(Debug, Serialize)]
pub struct EstimateJson {
    pub spot: SpotJson,
    /// Each player's position, OOP's first.
    pub positions: Option<[String; 2]>,
    /// Where the tree came from: `"config"` or the tree file's name.
    pub tree: String,
    /// The solution file the solve would write, whether it is there already, and the streets
    /// it would hold.
    pub out: String,
    pub exists: bool,
    pub streets: Vec<&'static str>,
    /// Each player's hands, and the trees' nodes; `null` when the betting tree is over the node
    /// limit and nothing could be estimated.
    pub hands: Option<[u64; 2]>,
    pub nodes: Option<NodesJson>,
    pub memory: MemoryJson,
    /// Why a solve would be refused, or `null` if it would run.
    pub refusal: Option<RefusalJson>,
}

#[derive(Debug, Serialize)]
pub struct NodesJson {
    pub betting: u64,
    pub public: u64,
    pub player: u64,
    /// The merged tree's nodes when the solve merges by suit symmetry, else `null`.
    pub merged: Option<u64>,
}

/// Memory in bytes, 10^9 to a GB.
#[derive(Debug, Serialize)]
pub struct MemoryJson {
    /// The solve's estimate and its save's, `null` if they could not be made.
    pub solve: Option<u64>,
    pub save: Option<u64>,
    /// The limit, `--max-memory`; what loaded solutions hold of it now, which are dropped to
    /// make room for a solve; and what a running solve holds of it.
    pub limit: u64,
    pub loaded: u64,
    pub solving: u64,
    /// Whether the estimates were made as if the node limit were lifted.
    pub lifted: bool,
    pub threads: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct RefusalJson {
    /// Each limit the spot is over.
    pub over: Vec<OverJson>,
    /// What is over which limit, a sentence each, and what to change, as `estimate` prints them.
    pub reasons: Vec<String>,
    pub advice: String,
    /// The refusal as `solve-postflop` gives it, one line.
    pub message: String,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OverJson {
    PublicTree { nodes: u64, limit: u64 },
    BettingTree { limit: u32 },
    Memory { estimate: u64 },
    SaveMemory { estimate: u64 },
}

impl EstimateJson {
    pub fn new(prepared: &Prepared, loaded: u64, solving: u64) -> EstimateJson {
        let input = &prepared.input;
        let solve = prepared.estimates.solve.as_ref();
        let streets = Street::ALL
            .into_iter()
            .filter(|&s| s >= input.spot.street() && s <= prepared.save.last_street)
            .map(Street::name)
            .collect();
        let refusal = prepared.refusal().map(|refusal| RefusalJson {
            over: prepared
                .estimates
                .over
                .iter()
                .map(|over| match *over {
                    Over::PublicTree { nodes, limit } => OverJson::PublicTree { nodes, limit },
                    Over::BettingTree { limit } => OverJson::BettingTree { limit },
                    Over::Memory { estimate } => OverJson::Memory { estimate },
                    Over::SaveMemory { estimate, .. } => OverJson::SaveMemory { estimate },
                })
                .collect(),
            message: format!("{:#}", refusal.error()),
            reasons: refusal.reasons,
            advice: refusal.advice,
        });
        EstimateJson {
            spot: SpotJson::new(&input.spot),
            positions: input.positions().map(|p| p.map(|p| p.to_string())),
            tree: match &input.tree {
                TreeSource::TreeFile(path) => path
                    .file_name()
                    .map_or_else(String::new, |n| n.to_string_lossy().into_owned()),
                TreeSource::Config { .. } | TreeSource::SpotFile => "config".to_string(),
            },
            out: prepared.out.clone(),
            exists: prepared.exists,
            streets,
            hands: solve.map(|e| e.hands),
            nodes: solve.map(|e| NodesJson {
                betting: e.betting_nodes,
                public: e.public_nodes,
                player: e.player_nodes,
                merged: e.merged.then_some(e.merged_nodes),
            }),
            memory: MemoryJson {
                solve: solve.map(|e| e.total),
                save: prepared.estimates.save.as_ref().map(|e| e.total),
                limit: prepared.options.max_memory,
                loaded,
                solving,
                lifted: prepared.estimates.lifted(&prepared.options),
                threads: solve.map(|e| e.threads),
            },
            refusal,
        }
    }
}

// ---------------------------------------------------------------------------------------------
// The solve and its state

/// One measurement of a running solve.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Measured {
    pub iterations: u64,
    pub percent_of_pot: f64,
    pub chips: f64,
    /// Since the solve started.
    pub seconds: f64,
}

/// A solve started from the page: what it is, and what it has measured so far.
#[derive(Debug, Clone)]
pub struct Run {
    pub spot: String,
    pub file: String,
    pub ranges: String,
    pub board: String,
    pub tree: String,
    pub target: f64,
    pub max_iterations: u64,
    pub last_street: Street,
    /// What it reserves of the memory limit.
    pub memory: u64,
    started: Instant,
    /// How long it ran, once it has ended.
    ended: Option<Duration>,
    measurements: Vec<Measured>,
}

impl Run {
    pub fn new(prepared: &Prepared, estimate: &EstimateJson) -> Run {
        Run {
            spot: prepared.input.spot.name.clone(),
            file: prepared.out.clone(),
            ranges: prepared.request.ranges.clone(),
            board: prepared.input.spot.board.to_string(),
            tree: estimate.tree.clone(),
            target: prepared.options.target.unwrap_or(0.0),
            max_iterations: prepared.options.max_iterations,
            last_street: prepared.save.last_street,
            memory: prepared.memory(),
            started: Instant::now(),
            ended: None,
            measurements: Vec::new(),
        }
    }

    fn elapsed(&self) -> Duration {
        self.ended.unwrap_or_else(|| self.started.elapsed())
    }
}

/// How a solve ended well: its file and what it measured.
#[derive(Debug, Clone, PartialEq)]
pub struct Finished {
    pub file: String,
    pub iterations: u64,
    pub stop: StopReason,
    /// The exploitability of the strategy as stored, and as solved.
    pub stored: Exploitability,
    pub solved: Exploitability,
    pub bytes: u64,
    /// A note when the solve's record could not be appended to the log.
    pub warning: Option<String>,
}

/// How a solve ended.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    Done(Finished),
    Failed(String),
    Cancelled,
}

/// Where the server's one solve is.
#[derive(Debug, Clone, PartialEq)]
enum Phase {
    Idle,
    Running,
    Saving,
    Ended(Outcome),
}

/// The server's solve: none yet, one running or saving, or the last one's end, which stays
/// until the next starts.
#[derive(Debug)]
pub struct Job {
    phase: Phase,
    run: Option<Run>,
    cancel: Arc<AtomicBool>,
}

impl Default for Job {
    fn default() -> Job {
        Job {
            phase: Phase::Idle,
            run: None,
            cancel: Arc::new(AtomicBool::new(false)),
        }
    }
}

/// The payload a cancelled solve's thread unwinds with.
struct Cancelled;

impl Job {
    /// Whether a solve is running or saving.
    pub fn busy(&self) -> bool {
        matches!(self.phase, Phase::Running | Phase::Saving)
    }

    /// Why another solve cannot start now.
    pub fn busy_message(&self) -> String {
        let run = self.run.as_ref().expect("a busy job has a run");
        format!(
            "a solve of {} into `{}` is {} already, and the server runs one at a time; wait for \
             it to end, or cancel it",
            run.spot,
            run.file,
            if self.phase == Phase::Saving {
                "being saved"
            } else {
                "running"
            }
        )
    }

    /// Starts `run`, and gives the flag that cancels it. Refuses while another solve is running
    /// or saving.
    pub fn begin(&mut self, run: Run) -> Result<Arc<AtomicBool>, String> {
        if self.busy() {
            return Err(self.busy_message());
        }
        self.phase = Phase::Running;
        self.run = Some(run);
        self.cancel = Arc::new(AtomicBool::new(false));
        Ok(Arc::clone(&self.cancel))
    }

    /// Keeps a measurement of the running solve.
    pub fn measured(&mut self, progress: &Progress) {
        if let (Phase::Running, Some(run)) = (&self.phase, &mut self.run) {
            run.measurements.push(Measured {
                iterations: progress.iterations,
                percent_of_pot: progress.exploitability.percent_of_pot,
                chips: progress.exploitability.chips,
                seconds: progress.elapsed.as_secs_f64(),
            });
        }
    }

    /// The running solve has finished solving and is being saved.
    pub fn saving(&mut self) {
        if self.phase == Phase::Running {
            self.phase = Phase::Saving;
        }
    }

    /// The solve has ended.
    pub fn finish(&mut self, outcome: Outcome) {
        if !self.busy() {
            return;
        }
        if let Some(run) = &mut self.run {
            run.ended = Some(run.started.elapsed());
        }
        self.phase = Phase::Ended(outcome);
    }

    /// Asks the running solve to stop at its next measurement. Refuses when none is running,
    /// and once it is being saved.
    pub fn cancel(&mut self) -> Result<(), String> {
        match self.phase {
            Phase::Running => {
                self.cancel.store(true, Ordering::SeqCst);
                Ok(())
            }
            Phase::Saving => Err(
                "the solve has finished and is being saved, which cannot be cancelled".to_string(),
            ),
            _ => Err("no solve is running".to_string()),
        }
    }

    /// The state as `/api/solve` gives it.
    pub fn status(&self) -> StatusJson {
        let state = match &self.phase {
            Phase::Idle => "idle",
            Phase::Running => "running",
            Phase::Saving => "saving",
            Phase::Ended(Outcome::Done(_)) => "done",
            Phase::Ended(Outcome::Failed(_)) => "failed",
            Phase::Ended(Outcome::Cancelled) => "cancelled",
        };
        let solve = self.run.as_ref().map(|run| RunJson {
            spot: run.spot.clone(),
            file: run.file.clone(),
            ranges: run.ranges.clone(),
            board: run.board.clone(),
            tree: run.tree.clone(),
            target: run.target,
            max_iterations: run.max_iterations,
            last_street: run.last_street.name(),
            memory: run.memory,
            seconds: run.elapsed().as_secs_f64(),
            iterations: run.measurements.last().map_or(0, |m| m.iterations),
            measurements: run.measurements.clone(),
        });
        let (result, message) = match &self.phase {
            Phase::Ended(Outcome::Done(done)) => (
                Some(ResultJson {
                    file: done.file.clone(),
                    iterations: done.iterations,
                    stop: stop_name(done.stop),
                    exploitability: ExploitabilityJson::new(&done.stored),
                    solve_exploitability: ExploitabilityJson::new(&done.solved),
                    bytes: done.bytes,
                    warning: done.warning.clone(),
                }),
                None,
            ),
            Phase::Ended(Outcome::Failed(message)) => (None, Some(message.clone())),
            _ => (None, None),
        };
        StatusJson {
            state,
            cancelling: self.phase == Phase::Running && self.cancel.load(Ordering::SeqCst),
            solve,
            result,
            message,
        }
    }
}

/// What `/api/solve` answers.
#[derive(Debug, Serialize)]
pub struct StatusJson {
    /// `"idle"`, `"running"`, `"saving"`, `"done"`, `"failed"` or `"cancelled"`.
    pub state: &'static str,
    /// Whether a cancel has been asked for and the solve has not reached its next measurement.
    pub cancelling: bool,
    /// The solve running, or the last one, or `null` before the first.
    pub solve: Option<RunJson>,
    /// What a solve that is done wrote; else `null`.
    pub result: Option<ResultJson>,
    /// Why a failed solve failed; else `null`.
    pub message: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct RunJson {
    pub spot: String,
    pub file: String,
    pub ranges: String,
    pub board: String,
    pub tree: String,
    pub target: f64,
    pub max_iterations: u64,
    pub last_street: &'static str,
    pub memory: u64,
    /// Since it started, or how long it ran.
    pub seconds: f64,
    /// The iterations at its last measurement.
    pub iterations: u64,
    pub measurements: Vec<Measured>,
}

#[derive(Debug, Serialize)]
pub struct ResultJson {
    pub file: String,
    pub iterations: u64,
    pub stop: &'static str,
    pub exploitability: ExploitabilityJson,
    pub solve_exploitability: ExploitabilityJson,
    pub bytes: u64,
    pub warning: Option<String>,
}

/// The job, whatever a panic left it as: its state is replaced whole by each change.
pub fn lock(job: &Mutex<Job>) -> MutexGuard<'_, Job> {
    job.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Runs `prepared` on a thread of its own, keeping its progress in `job`, which has begun it
/// with `cancel`, and appending its record to `log`; `release` gives back its memory when it
/// ends, before the job says so.
pub fn spawn(
    prepared: Prepared,
    job: Arc<Mutex<Job>>,
    cancel: Arc<AtomicBool>,
    log: Option<PathBuf>,
    release: impl FnOnce() + Send + 'static,
) {
    let thread_job = Arc::clone(&job);
    let started = thread::Builder::new()
        .name("solve".to_string())
        .spawn(move || {
            let job = thread_job;
            let ran = panic::catch_unwind(AssertUnwindSafe(|| {
                run(&prepared, &job, &cancel, log.as_deref())
            }));
            let outcome = match ran {
                Ok(Ok(finished)) => Outcome::Done(finished),
                Ok(Err(message)) => Outcome::Failed(message),
                Err(payload) if payload.is::<Cancelled>() => Outcome::Cancelled,
                Err(payload) => Outcome::Failed(format!(
                    "the solve failed: {}; nothing was saved, and the server goes on",
                    super::panic_message(payload.as_ref())
                )),
            };
            release();
            let line = match &outcome {
                Outcome::Done(done) => format!(
                    "  solve done: `{}`, {} iterations, {}",
                    done.file,
                    done.iterations,
                    bytes(done.bytes)
                ),
                Outcome::Failed(message) => format!("  solve failed: {message}"),
                Outcome::Cancelled => "  solve cancelled".to_string(),
            };
            println!("{line}");
            lock(&job).finish(outcome);
        });
    if let Err(error) = started {
        lock(&job).finish(Outcome::Failed(format!(
            "the solve could not start a thread: {error}"
        )));
    }
}

/// The solve, its save and its record.
fn run(
    prepared: &Prepared,
    job: &Mutex<Job>,
    cancel: &AtomicBool,
    log: Option<&Path>,
) -> Result<Finished, String> {
    let input = &prepared.input;
    let options = &prepared.options;
    let solution = solve(&input.spot, options, |progress| {
        lock(job).measured(progress);
        if cancel.load(Ordering::SeqCst) {
            panic::resume_unwind(Box::new(Cancelled));
        }
    })
    .map_err(|error| format!("{:#}", limits::explain(input, options, error)))?;
    let peak = peak_resident_bytes();
    lock(job).saving();
    let report = save::save(&solution, &prepared.path, &prepared.save)
        .map_err(|error| format!("{error:#}"))?;
    let warning = match (log, prepared.estimates.solve.as_ref()) {
        (Some(log), Some(estimate)) => {
            let record = SolveRecord::new(&input.spot, options, &solution, estimate, peak);
            append_record(log, &record).err().map(|error| {
                format!(
                    "the solution was saved, but its record could not be appended to the \
                     resource log `{}`: {error}",
                    log.display()
                )
            })
        }
        _ => None,
    };
    Ok(Finished {
        file: prepared.out.clone(),
        iterations: solution.iterations(),
        stop: solution.stop(),
        stored: report.exploitability,
        solved: *solution.exploitability(),
        bytes: report.bytes,
        warning,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a_run() -> Run {
        Run {
            spot: "spot".to_string(),
            file: "spot.pksol".to_string(),
            ranges: "a/spot.toml".to_string(),
            board: "Ks7d2c".to_string(),
            tree: "config".to_string(),
            target: 0.3,
            max_iterations: 100,
            last_street: Street::River,
            memory: 10,
            started: Instant::now(),
            ended: None,
            measurements: Vec::new(),
        }
    }

    fn progress(iterations: u64) -> Progress {
        Progress {
            iterations,
            elapsed: Duration::from_millis(iterations),
            exploitability: Exploitability {
                values: [0.0; 2],
                best_responses: [0.0; 2],
                chips: 1.0,
                percent_of_pot: 0.5,
            },
        }
    }

    fn done() -> Outcome {
        let none = progress(0).exploitability;
        Outcome::Done(Finished {
            file: "spot.pksol".to_string(),
            iterations: 20,
            stop: StopReason::Target,
            stored: none,
            solved: none,
            bytes: 100,
            warning: None,
        })
    }

    #[test]
    fn a_solve_goes_from_idle_through_running_and_saving_to_done() {
        let mut job = Job::default();
        assert_eq!(job.status().state, "idle");
        assert!(job.status().solve.is_none());
        assert_eq!(job.cancel(), Err("no solve is running".to_string()));
        // A measurement or an end with no solve running changes nothing.
        job.measured(&progress(10));
        job.finish(Outcome::Cancelled);
        assert_eq!(job.status().state, "idle");

        let cancel = job.begin(a_run()).unwrap();
        assert!(!cancel.load(Ordering::SeqCst));
        assert_eq!(job.status().state, "running");
        job.measured(&progress(10));
        job.measured(&progress(20));
        let status = job.status();
        let solve = status.solve.unwrap();
        assert_eq!(solve.iterations, 20);
        assert_eq!(solve.measurements.len(), 2);
        assert_eq!(solve.measurements[0].iterations, 10);
        // One at a time.
        assert!(
            job.begin(a_run())
                .unwrap_err()
                .contains("is running already")
        );

        job.saving();
        assert_eq!(job.status().state, "saving");
        job.measured(&progress(30));
        assert_eq!(job.status().solve.unwrap().measurements.len(), 2);
        assert!(
            job.begin(a_run())
                .unwrap_err()
                .contains("is being saved already")
        );
        assert!(job.cancel().unwrap_err().contains("cannot be cancelled"));

        job.finish(done());
        let status = job.status();
        assert_eq!(status.state, "done");
        assert_eq!(status.result.unwrap().file, "spot.pksol");
        assert!(!job.busy());
        // The end stays until the next solve starts, which may start now.
        job.begin(a_run()).unwrap();
        assert_eq!(job.status().state, "running");
        assert!(job.status().solve.unwrap().measurements.is_empty());
    }

    #[test]
    fn a_solve_can_be_cancelled_or_fail_and_the_next_one_starts() {
        let mut job = Job::default();
        let cancel = job.begin(a_run()).unwrap();
        job.cancel().unwrap();
        assert!(cancel.load(Ordering::SeqCst));
        assert!(job.status().cancelling);
        assert_eq!(job.status().state, "running");
        job.finish(Outcome::Cancelled);
        assert_eq!(job.status().state, "cancelled");
        assert!(!job.status().cancelling);

        // A new solve has a flag of its own.
        let again = job.begin(a_run()).unwrap();
        assert!(!again.load(Ordering::SeqCst));
        assert!(cancel.load(Ordering::SeqCst));
        job.finish(Outcome::Failed("it broke".to_string()));
        let status = job.status();
        assert_eq!(status.state, "failed");
        assert_eq!(status.message.as_deref(), Some("it broke"));
        assert!(status.solve.unwrap().seconds >= 0.0);
        assert!(job.begin(a_run()).is_ok());
    }

    #[test]
    fn an_option_in_the_command_lines_words_reads_as_the_pages() {
        assert_eq!(
            words("--board `Ks7d` has 2 cards".to_string()),
            "board `Ks7d` has 2 cards"
        );
        assert_eq!(
            words("--last-street `flop`: no".to_string()),
            "last street `flop`: no"
        );
        assert_eq!(words("the range file".to_string()), "the range file");
    }
}
