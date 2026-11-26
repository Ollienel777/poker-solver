//! `solver compare`: compares two solutions of one spot, each a solution file or a JSON
//! solution dump, ours or another solver's, or a solution and the reference solver's answer to
//! its spot, node by node, and gives the verdict as its exit code.
//!
//! The comparison is the library's ([`pk_postflop::compare`]): [`compare_mapped`] of two
//! [`MappedSolution`]s under the default [`Tolerances`], which are the gates' (DECISIONS D20,
//! D21). This module reads the two solutions, maps a dump onto the spot it answers, checks
//! that both are of one game, runs the reference when asked, and prints which solution is
//! which, the tolerances, what could and could not be checked, the comparison and a verdict.
//!
//! A solution file is mapped with the EVs it holds ([`MappedSolution::from_saved`]), so that a
//! file without its river is compared on the streets it holds, EVs included. A file is told
//! from a dump by what it holds: a solution file starts with `PKSOL`, and a dump is JSON.
//!
//! The command exits with 0 when every check passes and with 1 when one does not, the
//! comparison on stdout and one line on stderr, so that a script can act on the verdict
//! (DECISIONS P80).

use std::fs::{self, File};
use std::io::{ErrorKind, Read};
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Result, anyhow, bail};
use clap::Args as ClapArgs;
use pk_core::Combo;
use pk_game::{Player, Street};
use pk_postflop::compare::Check;
use pk_postflop::external::SOLVER_ENV;
use pk_postflop::{
    Comparison, ExternalSolver, ImportError, MappedSolution, PostflopGame, SavedSolution,
    SolutionDump, SolutionFileError, Spot, SpotError, Tolerances, Unchecked, compare_mapped,
};

use crate::format::{bytes, duration, field, list, percent, street_sizing, thousands, tree_rules};

/// The options of `solver compare`.
#[derive(Debug, ClapArgs)]
pub struct Args {
    /// Solution A: a solution file, or a JSON solution dump
    #[arg(value_name = "A")]
    pub a: PathBuf,

    /// Solution B: a solution file, or a JSON solution dump; or pass --reference instead
    #[arg(
        value_name = "B",
        required_unless_present = "reference",
        conflicts_with = "reference"
    )]
    pub b: Option<PathBuf>,

    /// Compare A with what the reference solver writes for A's spot
    ///
    /// The reference is the program that PK_REFERENCE_SOLVER names, as a JSON array of strings
    /// holding {spot} and {out}: see reference/README.md, "Reading a reference's answer".
    #[arg(long, help_heading = "Reference")]
    pub reference: bool,

    /// Keep the dump the reference writes in this file
    ///
    /// Its folder must exist, and the file must not, unless --force is given.
    #[arg(long, value_name = "FILE", help_heading = "Reference")]
    pub keep_reference: Option<PathBuf>,

    /// Overwrite the --keep-reference file if it exists
    #[arg(long, requires = "keep_reference", help_heading = "Reference")]
    pub force: bool,

    /// The spot file a dump is mapped onto [default: the spot of the other solution, when it
    /// is a solution file]
    #[arg(long, value_name = "FILE")]
    pub spot: Option<PathBuf>,
}

/// A solution as read from its file.
enum Loaded {
    File(Box<SavedSolution>),
    Dump(Box<SolutionDump>),
}

/// One side of the comparison: where it came from, what it is, and the spot it is on.
struct Side {
    /// `A` or `B`.
    name: char,
    path: PathBuf,
    read: Loaded,
}

impl Side {
    /// The spot of a solution file.
    fn file_spot(&self) -> Option<&Spot> {
        match &self.read {
            Loaded::File(saved) => Some(saved.solution().spot()),
            Loaded::Dump(_) => None,
        }
    }
}

pub fn run(args: &Args) -> Result<()> {
    // A flag's default counts as present to clap's `requires`, so this is checked here.
    if let (Some(keep), false) = (&args.keep_reference, args.reference) {
        bail!(
            "--keep-reference `{}` keeps the dump the reference writes; pass --reference too, \
             in place of B",
            keep.display()
        );
    }
    // The reference and where its dump goes are checked before anything is read.
    let reference = args.reference.then(reference_solver).transpose()?;
    if let Some(keep) = &args.keep_reference {
        check_destination(keep, args.force)?;
    }
    let a = read(&args.a, 'A')?;
    let b = args.b.as_ref().map(|path| read(path, 'B')).transpose()?;
    let given = args.spot.as_deref().map(load_spot).transpose()?;
    let files = [Some(&a), b.as_ref()]
        .iter()
        .flatten()
        .filter(|side| side.file_spot().is_some())
        .count();
    if let Some(path) = &args.spot
        && files == 2
    {
        bail!(
            "--spot `{}`: A and B are both solution files, and --spot names the spot a dump is \
             mapped onto; leave it out",
            path.display()
        );
    }

    // The spot a dump is mapped onto, and where it came from: --spot, or the spot of a file.
    let dump_spot: Option<(Spot, String)> = match (&given, &args.spot) {
        (Some(spot), Some(path)) => Some((spot.clone(), format!("--spot `{}`", path.display()))),
        _ => [Some(&a), b.as_ref()]
            .into_iter()
            .flatten()
            .find_map(|side| {
                let spot = side.file_spot()?;
                Some((spot.clone(), format!("{}'s, from its file", side.name)))
            }),
    };
    let against_reference = b.is_none();
    let spot_for = |side: &Side| -> Result<Spot> {
        if let Some(spot) = side.file_spot() {
            return Ok(spot.clone());
        }
        match &dump_spot {
            Some((spot, _)) => Ok(spot.clone()),
            None => bail!(
                "{}, `{}`, is a dump, which is compared on the spot it answers, and {}; give the \
                 spot file with --spot <FILE>",
                side.name,
                side.path.display(),
                if against_reference {
                    "a dump holds no spot file to give the reference"
                } else {
                    "neither A nor B is a solution file whose spot it could be mapped onto"
                }
            ),
        }
    };

    let a_spot = spot_for(&a)?;
    print_side(&a);
    let (b, ran_reference) = match (b, &reference) {
        (Some(b), _) => (b, false),
        (None, Some(reference)) => (
            run_reference(reference, &a_spot, args.keep_reference.as_deref())?,
            true,
        ),
        (None, None) => unreachable!("B or --reference is required"),
    };
    // The reference's dump answers A's spot.
    let b_spot = if ran_reference {
        a_spot.clone()
    } else {
        spot_for(&b)?
    };
    print_side(&b);
    let dumps: Vec<String> = [&a, &b]
        .iter()
        .filter(|side| side.file_spot().is_none())
        .map(|side| side.name.to_string())
        .collect();
    let source = if ran_reference {
        "A's, which the reference solved and B is mapped onto".to_string()
    } else if dumps.is_empty() {
        "each file's own, which are checked to be one game".to_string()
    } else {
        let spot_source = dump_spot.as_ref().map_or("", |(_, s)| s.as_str());
        format!(
            "{spot_source}, which {} {} mapped onto",
            list(&dumps),
            if dumps.len() == 1 { "is" } else { "are" }
        )
    };
    field(
        "Spot",
        format!(
            "{}, {} from the {}: {source}",
            a_spot.name,
            a_spot.board,
            a_spot.street()
        ),
    );

    let started = Instant::now();
    let a_mapped = map(&a, &a_spot)?;
    let b_mapped = map(&b, &b_spot)?;
    let (a_path, b_path) = (a.path.clone(), b.path.clone());
    let files = [a.file_spot().is_some(), b.file_spot().is_some()];
    // The files' solutions are held by the mapped ones now.
    drop((a, b));
    let differences = game_differences([a_mapped.game(), b_mapped.game()], [&a_spot, &b_spot]);
    if !differences.is_empty() {
        println!();
        println!("A and B are not solutions of one game:");
        for difference in &differences {
            println!("- {difference}");
        }
        bail!(
            "`{}` and `{}` are not solutions of the same game: {} listed above; compare two \
             solutions of one spot and tree",
            a_path.display(),
            b_path.display(),
            count(differences.len(), "difference", "differences")
        );
    }

    let tolerances = Tolerances::default();
    print_tolerances(&tolerances, a_spot.street());
    let comparison = compare_mapped(&a_mapped, &b_mapped, &tolerances);
    let took = started.elapsed();
    print_coverage(&comparison, files);
    println!();
    print!("{comparison}");
    println!();
    field("Compared", format!("in {}", duration(took)));
    if comparison.passes() {
        field(
            "Verdict",
            "A and B agree within the tolerances: every check passes",
        );
        return Ok(());
    }
    let failed = comparison.failures.len();
    field(
        "Verdict",
        format!(
            "A and B differ beyond the tolerances: {} fail ({})",
            count(failed, "check", "checks"),
            by_check(&comparison)
        ),
    );
    Err(anyhow!(
        "`{}` and `{}` differ beyond the tolerances: {} of the comparison above {}",
        a_path.display(),
        b_path.display(),
        count(failed, "check", "checks"),
        if failed == 1 { "fails" } else { "fail" }
    ))
}

/// `1 check` or `3 checks`.
fn count(n: usize, one: &str, many: &str) -> String {
    if n == 1 {
        format!("1 {one}")
    } else {
        format!("{} {many}", thousands(n as u64))
    }
}

/// The failures by check: `frequency 2, reported EV 10`.
fn by_check(comparison: &Comparison) -> String {
    let parts: Vec<String> = [
        Check::Exploitability,
        Check::Value,
        Check::Frequency,
        Check::Ev,
        Check::ReportedEv,
    ]
    .into_iter()
    .filter_map(|check| {
        let n = comparison.failures_of(check).count();
        (n > 0).then(|| format!("{check} {}", thousands(n as u64)))
    })
    .collect();
    parts.join(", ")
}

/// Reads the solution at `path`, a solution file or a dump, told apart by what it holds.
fn read(path: &Path, name: char) -> Result<Side> {
    let shown = path.display();
    let mut start = Vec::with_capacity(64);
    File::open(path)
        .and_then(|file| file.take(64).read_to_end(&mut start))
        .map_err(|error| anyhow!("could not read {name}, `{shown}`: {error}; check the path"))?;
    let read = if start.starts_with(b"PKSOL") {
        let saved = SavedSolution::load(path).map_err(|error| file_error(path, error))?;
        Loaded::File(Box::new(saved))
    } else if start
        .iter()
        .find(|byte| !byte.is_ascii_whitespace())
        .is_some_and(|&byte| byte == b'{')
    {
        let dump = SolutionDump::load(path).map_err(|error| {
            anyhow!(
                "`{shown}` is not a valid solution dump: {error}; give a dump in the format of \
                 ARCHITECTURE.md (Solution dump)"
            )
        })?;
        Loaded::Dump(Box::new(dump))
    } else {
        bail!(
            "`{shown}` is neither a solution file nor a solution dump: a solution file starts \
             with `PKSOL` and a dump is JSON; give a file that solve-postflop --out or inspect \
             --dump wrote, or another solver's dump"
        );
    };
    Ok(Side {
        name,
        path: path.to_path_buf(),
        read,
    })
}

/// A solution file's error, naming the file and saying what to do.
fn file_error(path: &Path, error: SolutionFileError) -> anyhow::Error {
    match error {
        SolutionFileError::Io { source, .. } => anyhow!(
            "could not read the solution file `{}`: {source}; check the path",
            path.display()
        ),
        error @ (SolutionFileError::TreeMismatch { .. } | SolutionFileError::Version { .. }) => {
            anyhow!("{error}")
        }
        error => anyhow!("{error}; the file is damaged: solve the spot again"),
    }
}

fn load_spot(path: &Path) -> Result<Spot> {
    Spot::load(path).map_err(|error| match error {
        SpotError::Io { source, .. } => anyhow!(
            "could not read the spot file `{}` that --spot names: {source}; check the path",
            path.display()
        ),
        other => anyhow!("the spot file `{}` is not valid: {other}", path.display()),
    })
}

/// The names of the first `count` streets of a spot that starts on `first`: `the flop`.
fn street_names(first: Street, count: usize) -> Vec<String> {
    Street::ALL
        .into_iter()
        .filter(|&street| street >= first)
        .take(count)
        .map(|street| format!("the {street}"))
        .collect()
}

/// `holds the flop and the turn, without the river`, for a solution of a spot that starts on
/// `first` holding its first `count` streets.
fn holds(first: Street, count: usize) -> String {
    let played = street_names(first, 3);
    let held = list(&street_names(first, count));
    if count < played.len() {
        format!("holds {held}, without {}", list(&played[count..]))
    } else {
        format!("holds {held}")
    }
}

/// The street a board of `cards` cards starts on.
fn street_of(cards: usize) -> Street {
    match cards {
        0..=3 => Street::Flop,
        4 => Street::Turn,
        _ => Street::River,
    }
}

/// The line that says what a side is: its file, its solver, iterations and exploitability, and
/// the streets it holds.
fn print_side(side: &Side) {
    let text = match &side.read {
        Loaded::File(saved) => {
            let header = saved.header();
            format!(
                "`{}`, a solution file written by {}: {} iterations, {} of the pot exploitable \
                 as stored; {}",
                side.path.display(),
                header.written_by(),
                thousands(header.iterations),
                percent(header.exploitability.percent_of_pot),
                holds(header.spot.street(), header.streets.len())
            )
        }
        Loaded::Dump(dump) => {
            let solver = &dump.solver;
            let iterations = solver.iterations.map_or_else(
                || "no iterations given".to_string(),
                |n| format!("{} iterations", thousands(n)),
            );
            let pot = f64::from(dump.spot.pot);
            let exploitable = solver.exploitability.map_or_else(
                || "no exploitability reported".to_string(),
                |chips| {
                    format!(
                        "{} of the pot exploitable as its solver reports",
                        percent(100.0 * chips / pot)
                    )
                },
            );
            let partial = if dump.complete {
                ""
            } else {
                ", a partial dump"
            };
            format!(
                "`{}`, a solution dump by {}: {iterations}, {exploitable}; {}{partial}",
                side.path.display(),
                solver.name,
                holds(street_of(dump.spot.board.len()), dump.streets())
            )
        }
    };
    field(&side.name.to_string(), text);
}

/// The side mapped onto its game: a file with the EVs it holds, a dump onto `spot`.
fn map(side: &Side, spot: &Spot) -> Result<MappedSolution> {
    match &side.read {
        Loaded::File(saved) => Ok(MappedSolution::from_saved(saved)),
        Loaded::Dump(dump) => MappedSolution::from_dump(dump, spot).map_err(|error| {
            let onto = format!("the spot {}", spot.name);
            match error {
                ImportError::Mismatch(mismatches) => {
                    println!();
                    println!("{} does not answer {onto}:", side.name);
                    for mismatch in &mismatches {
                        println!("- {mismatch}");
                    }
                    anyhow!(
                        "`{}` does not answer {onto}: {} listed above; compare it with a \
                         solution of the spot it answers, or give that spot with --spot",
                        side.path.display(),
                        count(mismatches.len(), "difference", "differences")
                    )
                }
                ImportError::Dump(error) => anyhow!(
                    "`{}` is not a valid solution dump: {error}",
                    side.path.display()
                ),
                ImportError::Spot(error) => anyhow!(
                    "the game of {onto}, which `{}` is mapped onto, could not be built: {error}",
                    side.path.display()
                ),
            }
        }),
    }
}

/// The reference solver that [`SOLVER_ENV`] names, or an error that says how to name one.
fn reference_solver() -> Result<ExternalSolver> {
    match ExternalSolver::from_env() {
        Ok(Some(reference)) => Ok(reference),
        Ok(None) => bail!(
            "--reference: no reference solver is set; set {SOLVER_ENV} to its command as a JSON \
             array of strings holding {{spot}} and {{out}}, such as \
             [\"C:/tools/postflop-ref.exe\", \"{{spot}}\", \"{{out}}\"], as \
             reference/README.md (Reading a reference's answer) describes"
        ),
        Err(error) => bail!(
            "--reference: {SOLVER_ENV} does not hold a usable command: {error}; see \
             reference/README.md (Reading a reference's answer)"
        ),
    }
}

/// Runs `reference` on `spot` and reads the dump it writes, to `keep` if given and to a folder
/// of its own otherwise, which is removed afterwards. Gives the dump as side B.
fn run_reference(reference: &ExternalSolver, spot: &Spot, keep: Option<&Path>) -> Result<Side> {
    let folder = std::env::temp_dir().join(format!("pk-compare-{}", std::process::id()));
    fs::create_dir_all(&folder).map_err(|error| {
        anyhow!(
            "--reference: could not create the folder `{}` for the reference's files: {error}",
            folder.display()
        )
    })?;
    let spot_path = folder.join("spot.toml");
    let out = keep.map_or_else(|| folder.join("reference.json"), Path::to_path_buf);
    let result = (|| {
        fs::write(&spot_path, spot.to_toml()).map_err(|error| {
            anyhow!(
                "--reference: could not write A's spot to `{}`: {error}",
                spot_path.display()
            )
        })?;
        field(
            "Reference",
            format!(
                "running `{}` on A's spot, which writes its dump to `{}`",
                reference.program.display(),
                out.display()
            ),
        );
        let started = Instant::now();
        let dump = reference.run(&spot_path, &out).map_err(|error| {
            // The program's own output, which the error ends with, is kept on one line.
            let text = error.to_string();
            let lines: Vec<&str> = text
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .collect();
            anyhow!("--reference: {}", lines.join(" | "))
        })?;
        let size = fs::metadata(&out).map(|m| m.len()).unwrap_or(0);
        field(
            "Reference",
            format!(
                "answered in {}: {} of JSON{}",
                duration(started.elapsed()),
                bytes(size),
                if keep.is_some() {
                    format!(", kept in `{}`", out.display())
                } else {
                    String::new()
                }
            ),
        );
        Ok(Side {
            name: 'B',
            path: out.clone(),
            read: Loaded::Dump(Box::new(dump)),
        })
    })();
    let _ = fs::remove_dir_all(&folder);
    result
}

/// What differs between the games of A and B, the spots of `spots`: nothing when they are one
/// game, the same tree, hands, pot and rake, as [`compare_mapped`] needs.
fn game_differences(games: [&PostflopGame; 2], spots: [&Spot; 2]) -> Vec<String> {
    let [a, b] = games;
    let same = a.tree().nodes() == b.tree().nodes()
        && a.board_hands().board() == b.board_hands().board()
        && [Player::Oop, Player::Ip]
            .iter()
            .all(|&p| a.hands(p) == b.hands(p))
        && a.pot() == b.pot()
        && a.rake() == b.rake();
    if same {
        return Vec::new();
    }
    let [sa, sb] = spots;
    let mut differences = Vec::new();
    if sa.board != sb.board {
        differences.push(format!(
            "the board is {} in A and {} in B",
            sa.board, sb.board
        ));
    }
    if a.pot() != b.pot() {
        differences.push(format!(
            "the pot is {} in A and {} in B",
            crate::format::bb(a.pot()),
            crate::format::bb(b.pot())
        ));
    }
    if sa.effective_stack != sb.effective_stack {
        differences.push(format!(
            "the stack is {} in A and {} in B",
            crate::format::bb(sa.effective_stack),
            crate::format::bb(sb.effective_stack)
        ));
    }
    if a.rake() != b.rake() {
        let rake = |spot: &Spot| {
            if spot.rake_percent == 0.0 {
                "none".to_string()
            } else {
                format!(
                    "{}% capped at {}",
                    spot.rake_percent,
                    crate::format::bb(spot.rake_cap)
                )
            }
        };
        differences.push(format!(
            "the rake is {} in A and {} in B",
            rake(sa),
            rake(sb)
        ));
    }
    for player in [Player::Oop, Player::Ip] {
        if a.hands(player) != b.hands(player) {
            differences.push(hands_difference(player, a, b));
        }
    }
    let (ta, tb) = (&sa.tree, &sb.tree);
    if (ta.max_raises, ta.allin_threshold) != (tb.max_raises, tb.allin_threshold) {
        differences.push(format!(
            "the tree's rules are {} in A and {} in B",
            tree_rules(ta.max_raises, ta.allin_threshold),
            tree_rules(tb.max_raises, tb.allin_threshold)
        ));
    }
    for street in Street::ALL {
        let shown = |sizing: Option<&pk_game::StreetSizing>, max_raises: u32| {
            sizing.map_or_else(
                || "nothing".to_string(),
                |sizing| street_sizing(sizing, max_raises),
            )
        };
        let (za, zb) = (ta.street(street), tb.street(street));
        if za != zb {
            differences.push(format!(
                "on the {street}, the tree {} in A and {} in B",
                shown(za, ta.max_raises),
                shown(zb, tb.max_raises)
            ));
        }
    }
    if a.tree().nodes() != b.tree().nodes() {
        differences.push(format!(
            "the betting trees differ: A's has {} nodes and B's {}",
            thousands(a.tree().nodes().len() as u64),
            thousands(b.tree().nodes().len() as u64)
        ));
    }
    differences
}

/// How one player's hands differ between the games `a` and `b`.
fn hands_difference(player: Player, a: &PostflopGame, b: &PostflopGame) -> String {
    let weights = |game: &PostflopGame| -> Vec<(Combo, f64)> {
        let hands = game.hands(player);
        hands
            .combos()
            .iter()
            .copied()
            .zip(hands.weights().iter().copied())
            .collect()
    };
    let (wa, wb) = (weights(a), weights(b));
    let weight_in = |list: &[(Combo, f64)], combo: Combo| {
        list.iter().find(|(c, _)| *c == combo).map(|&(_, w)| w)
    };
    let mut differ: Vec<String> = Vec::new();
    for &(combo, w) in &wa {
        match weight_in(&wb, combo) {
            None => differ.push(format!("{combo} (in A only)")),
            Some(other) if other != w => differ.push(format!("{combo} ({w} in A, {other} in B)")),
            Some(_) => {}
        }
    }
    for &(combo, _) in &wb {
        if weight_in(&wa, combo).is_none() {
            differ.push(format!("{combo} (in B only)"));
        }
    }
    let shown: Vec<String> = differ.iter().take(4).cloned().collect();
    format!(
        "{player}'s range differs: A has {} hands on the board and B {}, and {} of them are in \
         one only or weigh differently, such as {}",
        wa.len(),
        wb.len(),
        differ.len(),
        list(&shown)
    )
}

/// The tolerances, a line each.
fn print_tolerances(t: &Tolerances, first: Street) {
    field(
        "Tolerances",
        "the defaults, which are the gates' (DECISIONS D20, D21):",
    );
    field(
        "  exploit",
        format!(
            "each strategy at most {}% of the pot exploitable",
            t.exploitability
        ),
    );
    field(
        "  values",
        format!("each player's two values within {}% of the pot", t.value),
    );
    let below = match first {
        Street::Flop => format!(
            ", {} below one deal and {} below two",
            points(t.frequency_below_deals[0]),
            points(t.frequency_below_deals[1])
        ),
        Street::Turn => format!(", {} below the deal", points(t.frequency_below_deals[0])),
        Street::River => String::new(),
    };
    field(
        "  frequency",
        format!(
            "within {} points on the {first}{below}, at the nodes reached more than {}% of the \
             time given their cards",
            points(t.frequency),
            100.0 * t.min_reach
        ),
    );
    field(
        "  EVs",
        format!(
            "each class's two EVs at the root within {}% of the pot; the EVs and values a \
             solution reports within {}% of the pot of those its strategy gives",
            t.ev, t.reported_ev
        ),
    );
}

/// A frequency limit in points: `2`, `10`.
fn points(probability: f64) -> String {
    let points = (100.0 * probability * 100.0).round() / 100.0;
    format!("{points}")
}

/// What the comparison can and cannot check, for each side that does not hold every street,
/// and for the streets one side leaves out.
fn print_coverage(comparison: &Comparison, files: [bool; 2]) {
    let name = |side: usize| if side == 0 { 'A' } else { 'B' };
    let mut lines: Vec<String> = Vec::new();
    for unchecked in &comparison.unchecked {
        match *unchecked {
            Unchecked::Exploitability { side, reported } => {
                let s = &comparison.solutions[side];
                let figure = reported.map(|chips| percent(comparison.percent(chips)));
                let held = list(&street_names(comparison.first_street, s.streets));
                let line = match (files[side], figure) {
                    (true, Some(figure)) => format!(
                        "{} holds {held} only, so its strategy cannot be measured here: its \
                         exploitability is the {figure} of the pot its file records, measured \
                         on its whole strategy, river included, when the file was written, and \
                         the verdict holds that figure to the limit",
                        name(side)
                    ),
                    (false, Some(figure)) => format!(
                        "{} holds {held} only, so its strategy cannot be measured here: its \
                         exploitability is the {figure} of the pot its solver reports, which \
                         the verdict holds to the limit without checking it",
                        name(side)
                    ),
                    (_, None) => format!(
                        "{} holds {held} only and reports no exploitability, so its \
                         exploitability is not checked at all",
                        name(side)
                    ),
                };
                lines.push(line);
            }
            Unchecked::Values { side, reported } => {
                let line = match (reported, files[side]) {
                    (true, true) => format!(
                        "{}'s values are those its file records, measured when it was written, \
                         and are compared as they are",
                        name(side)
                    ),
                    (true, false) => format!(
                        "{}'s values are compared as its solver reports them, without checking \
                         them against its strategy",
                        name(side)
                    ),
                    (false, _) => format!(
                        "{} reports no values, so the values are not compared",
                        name(side)
                    ),
                };
                lines.push(line);
            }
        }
    }
    let total = Street::ALL
        .iter()
        .filter(|&&street| street >= comparison.first_street)
        .count();
    let compared = comparison.streets.len();
    if compared < total {
        let left: Vec<String> = Street::ALL
            .iter()
            .filter(|&&street| street >= comparison.first_street)
            .skip(compared)
            .map(|street| format!("the {street}"))
            .collect();
        lines.push(format!(
            "{} {} compared by neither frequencies nor EVs, since one of the two does not \
             hold {}",
            list(&left),
            if left.len() == 1 { "is" } else { "are" },
            if left.len() == 1 { "it" } else { "them" }
        ));
        for (side, s) in comparison.solutions.iter().enumerate() {
            if !s.complete && s.has_evs {
                lines.push(format!(
                    "{}'s EVs are compared with the other's for each class at the root, and \
                     with what its own strategy gives only at the nodes whose whole subtree it \
                     holds",
                    name(side)
                ));
            }
        }
    }
    if lines.is_empty() {
        field(
            "Checked",
            "everything: both hold every street, and both are measured here by our code",
        );
        return;
    }
    field("Checked", "not everything:");
    for line in lines {
        println!("- {line}");
    }
}

/// Checks that the reference's dump can be kept at `path`: its folder exists, and the file
/// does not, unless `force` allows overwriting it.
fn check_destination(path: &Path, force: bool) -> Result<()> {
    let shown = path.display();
    let folder = match path.parent() {
        Some(folder) if !folder.as_os_str().is_empty() => folder,
        _ => Path::new("."),
    };
    if !folder.is_dir() {
        bail!(
            "--keep-reference `{shown}`: the folder `{}` does not exist; create it, or give a \
             path in a folder that does",
            folder.display()
        );
    }
    match fs::metadata(path) {
        Ok(metadata) if metadata.is_dir() => {
            bail!("--keep-reference `{shown}` is a folder; give the path of a file to write")
        }
        Ok(_) if !force => bail!(
            "--keep-reference `{shown}` already exists; pass --force to overwrite it, or give \
             another path"
        ),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => bail!("--keep-reference `{shown}`: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limits_read_as_points_and_counts_as_words() {
        assert_eq!(points(0.02), "2");
        assert_eq!(points(0.10), "10");
        assert_eq!(points(0.25), "25");
        assert_eq!(count(1, "check", "checks"), "1 check");
        assert_eq!(count(1200, "check", "checks"), "1,200 checks");
    }
}
