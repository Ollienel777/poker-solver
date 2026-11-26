//! Runs `solver compare` on solution files and dumps that `solver solve-postflop` and
//! `solver inspect --dump` write, as a user would, and checks its verdict and exit code against
//! the library's own comparison of the same pair, whose text it prints.
//!
//! M5's gate ends with a real spot solved and compared with the reference solver: the slow
//! `the_gate_flop_compared_with_the_reference` does that when `PK_REFERENCE_SOLVER` names one.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use pk_postflop::external::SOLVER_ENV;
use pk_postflop::{
    Comparison, MappedSolution, SavedSolution, SolutionDump, Spot, Tolerances, compare_mapped,
};

mod common;

use common::*;

const ONE_BET: &str = "configs/trees/one_bet.toml";
/// BTN's open and BB's call of it, recorded by hand from a reference (#49).
const GATE_RANGES: &str = "reference/ranges/a/BTN_vs_BB_SRP_recorded.toml";

/// Solves a spot through the command line and saves it to `out`, with `options` added.
fn solve_to(spot: &[&str], out: &Path, options: &[&str]) {
    let mut args = vec!["solve-postflop"];
    args.extend(spot);
    args.extend(["--no-log", "--out", arg(out)]);
    args.extend(options);
    solver(&args).succeeded();
}

/// `solver compare` with `args`, from the repository's root, with `PK_REFERENCE_SOLVER` set to
/// `reference` or, if `None`, unset.
fn compare_with(args: &[&str], reference: Option<&str>) -> Run {
    let mut command = Command::new(env!("CARGO_BIN_EXE_solver"));
    command.arg("compare").args(args).current_dir(root());
    match reference {
        Some(template) => command.env(SOLVER_ENV, template),
        None => command.env_remove(SOLVER_ENV),
    };
    let output = command.output().expect("the solver binary runs");
    let run = Run {
        code: output.status.code().expect("the solver exits with a code"),
        stdout: String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        stderr: String::from_utf8(output.stderr).expect("stderr is UTF-8"),
    };
    println!(
        "solver compare {}\n{}{}",
        args.join(" "),
        run.stdout,
        run.stderr
    );
    run
}

fn compare(args: &[&str]) -> Run {
    compare_with(args, None)
}

/// A solution file as the command maps it: with the EVs it holds.
fn mapped(path: &Path) -> MappedSolution {
    MappedSolution::from_saved(&SavedSolution::load(path).unwrap())
}

/// The library's comparison of `a` and `b` under the default tolerances.
fn library(a: &MappedSolution, b: &MappedSolution) -> Comparison {
    compare_mapped(a, b, &Tolerances::default())
}

/// Checks that `run` printed `comparison` in full and ended as its verdict says: with 0 and
/// "every check passes", or with 1, one error that counts the failures, and the verdict line.
fn check_verdict(run: &Run, comparison: &Comparison, a: &Path, b: &Path) {
    assert!(
        run.stdout.contains(&comparison.to_string()),
        "the library's comparison is printed in full:\n{comparison}"
    );
    if comparison.passes() {
        assert_eq!(run.code, 0, "{}", run.stderr);
        assert_eq!(run.stderr, "");
        run.prints(&["Verdict      A and B agree within the tolerances: every check passes"]);
    } else {
        let failed = comparison.failures.len();
        // A count is written with a comma between each three digits.
        let digits = failed.to_string();
        let mut written = String::new();
        for (i, digit) in digits.chars().enumerate() {
            if i > 0 && (digits.len() - i).is_multiple_of(3) {
                written.push(',');
            }
            written.push(digit);
        }
        let checks = if failed == 1 {
            "1 check of the comparison above fails".to_string()
        } else {
            format!("{written} checks of the comparison above fail")
        };
        assert_eq!(
            run.failed(1),
            format!(
                "`{}` and `{}` differ beyond the tolerances: {checks}",
                a.display(),
                b.display()
            )
        );
        let verdict = run.line("Verdict      ");
        assert!(
            verdict.starts_with("Verdict      A and B differ beyond the tolerances: "),
            "{verdict}"
        );
    }
}

/// `turn_small` solved and saved whole, and again saved without its river, in `folder`.
fn turn_files(folder: &Path) -> (PathBuf, PathBuf) {
    let (whole, partial) = (folder.join("whole.pksol"), folder.join("turn.pksol"));
    solve_to(&["--spot", TURN], &whole, &[]);
    solve_to(&["--spot", TURN], &partial, &["--last-street", "turn"]);
    (whole, partial)
}

/// The dump `inspect --dump` writes of `file`, to `out`.
fn dump_to(file: &Path, out: &Path) {
    solver(&["inspect", arg(file), "--dump", arg(out)]).succeeded();
}

// ---------------------------------------------------------------------------------------------
// Passing and failing

#[test]
fn a_file_compares_with_itself_and_with_its_own_dump() {
    let folder = scratch("compare-self");
    let (whole, _) = turn_files(&folder);
    let run = compare(&[arg(&whole), arg(&whole)]);
    check_verdict(
        &run,
        &library(&mapped(&whole), &mapped(&whole)),
        &whole,
        &whole,
    );
    run.prints(&[
        &format!(
            "A            `{}`, a solution file written by pk-postflop {}: {} iterations, \
             {:.4}% of the pot exploitable as stored; holds the turn and the river",
            whole.display(),
            env!("CARGO_PKG_VERSION"),
            SavedSolution::load(&whole).unwrap().header().iterations,
            SavedSolution::load(&whole)
                .unwrap()
                .header()
                .exploitability
                .percent_of_pot
        ),
        "Spot         turn_small, Kh9c7d2s from the turn: each file's own, which are checked to \
         be one game",
        "Tolerances   the defaults, which are the gates' (DECISIONS D20, D21):",
        "  exploit    each strategy at most 0.3% of the pot exploitable",
        "  values     each player's two values within 0.2% of the pot",
        "  frequency  within 2 points on the turn, 10 below the deal, at the nodes reached more \
         than 1% of the time given their cards",
        "Checked      everything: both hold every street, and both are measured here by our code",
    ]);

    // Its dump, as B and as A: mapped onto the file's spot, and the same in every number.
    let dump = folder.join("whole.json");
    dump_to(&whole, &dump);
    let spot = Spot::load(root().join(TURN)).unwrap();
    let from_dump = MappedSolution::from_dump(&SolutionDump::load(&dump).unwrap(), &spot).unwrap();
    let run = compare(&[arg(&whole), arg(&dump)]);
    check_verdict(&run, &library(&mapped(&whole), &from_dump), &whole, &dump);
    assert!(
        run.line("B            ")
            .contains("a solution dump by pk-postflop: ")
    );
    run.prints(&[
        "Spot         turn_small, Kh9c7d2s from the turn: A's, from its file, which B is \
         mapped onto",
    ]);
    let run = compare(&[arg(&dump), arg(&whole)]);
    check_verdict(&run, &library(&from_dump, &mapped(&whole)), &dump, &whole);
    run.prints(&[
        "Spot         turn_small, Kh9c7d2s from the turn: B's, from its file, which A is \
         mapped onto",
    ]);
    fs::remove_dir_all(&folder).unwrap();
}

#[test]
fn a_file_and_the_spot_solved_to_another_target_compare_as_the_library_says() {
    let folder = scratch("compare-targets");
    let mut verdicts = Vec::new();
    for (name, spot) in [
        ("turn", vec!["--spot", TURN]),
        (
            "river",
            vec![
                "--ranges",
                RANGES,
                "--board",
                "Ks7d2c9h3s",
                "--tree",
                RIVER_TREE,
            ],
        ),
    ] {
        let file = |target: &str| folder.join(format!("{name}-{target}.pksol"));
        for target in ["0.3", "0.03", "0.01"] {
            solve_to(&spot, &file(target), &["--target", target]);
        }
        for (a, b) in [("0.3", "0.03"), ("0.03", "0.01")] {
            let (a, b) = (file(a), file(b));
            let comparison = library(&mapped(&a), &mapped(&b));
            let run = compare(&[arg(&a), arg(&b)]);
            check_verdict(&run, &comparison, &a, &b);
            println!(
                "{name}: {} against {}: {}",
                a.display(),
                b.display(),
                if comparison.passes() {
                    "passes"
                } else {
                    "fails"
                }
            );
            verdicts.push(comparison.passes());
        }
    }
    // Measured: at 0.3% against 0.03% both spots fail on frequencies, and at 0.03% against
    // 0.01% both pass. Whatever the solver does later, both outcomes are to be exercised.
    assert!(
        verdicts.contains(&true) && verdicts.contains(&false),
        "{verdicts:?}"
    );

    // A solve stopped after two iterations fails, on exploitability among others.
    let loose = folder.join("turn-0.3.pksol");
    let early = folder.join("turn-early.pksol");
    solve_to(&["--spot", TURN], &early, &["--max-iters", "2"]);
    let comparison = library(&mapped(&loose), &mapped(&early));
    assert!(!comparison.passes());
    let run = compare(&[arg(&loose), arg(&early)]);
    check_verdict(&run, &comparison, &loose, &early);
    assert!(
        run.line("Verdict      ").contains("exploitability 1"),
        "{}",
        run.line("Verdict")
    );
    fs::remove_dir_all(&folder).unwrap();
}

/// A file without its river, compared with the whole file of the same solve on the streets it
/// holds, EVs included, and the output says what could not be checked.
#[test]
fn a_file_without_its_river_compares_with_the_whole_file_on_the_streets_it_holds() {
    let folder = scratch("compare-partial");
    let (whole, partial) = turn_files(&folder);
    let (a, b) = (mapped(&partial), mapped(&whole));
    let comparison = library(&a, &b);
    assert!(comparison.passes());
    // The partial file reports the EVs it holds, where `from_solution` gives it none, so its
    // classes' EVs are compared with the whole file's at the root.
    let saved = SavedSolution::load(&partial).unwrap();
    assert!(
        MappedSolution::from_solution(saved.solution())
            .reported_evs()
            .is_none()
    );
    assert!(comparison.solutions[0].has_evs);
    assert!(comparison.solutions[0].measured.is_none());
    let root = &comparison.classes;
    assert!(!root.is_empty());
    for class in root {
        assert!(
            class.ev[0].is_some() && class.ev[0] == class.ev[1],
            "{class:?}"
        );
    }
    let evs = a.reported_evs().unwrap().get(0).unwrap();
    let file = saved.ev(0).unwrap();
    for (ev, stored) in evs.iter().zip(file) {
        assert_eq!(*ev, (!stored.is_nan()).then_some(f64::from(*stored)));
    }

    let run = compare(&[arg(&partial), arg(&whole)]);
    check_verdict(&run, &comparison, &partial, &whole);
    let header = saved.header();
    run.prints(&[
        "Checked      not everything:",
        &format!(
            "- A holds the turn only, so its strategy cannot be measured here: its \
             exploitability is the {:.4}% of the pot its file records, measured on its whole \
             strategy, river included, when the file was written, and the verdict holds that \
             figure to the limit",
            header.exploitability.percent_of_pot
        ),
        "- A's values are those its file records, measured when it was written, and are \
         compared as they are",
        "- the river is compared by neither frequencies nor EVs, since one of the two does not \
         hold it",
        "- A's EVs are compared with the other's for each class at the root, and with what its \
         own strategy gives only at the nodes whose whole subtree it holds",
    ]);
    assert!(
        run.line("A            ")
            .ends_with("holds the turn, without the river")
    );
    fs::remove_dir_all(&folder).unwrap();
}

// ---------------------------------------------------------------------------------------------
// Errors

#[test]
fn two_files_of_different_spots_are_an_error_that_says_what_differs() {
    let folder = scratch("compare-spots");
    let turn = folder.join("turn.pksol");
    let toy = folder.join("toy.pksol");
    solve_to(&["--spot", TURN], &turn, &[]);
    solve_to(&["--spot", TOY], &toy, &[]);
    let run = compare(&[arg(&turn), arg(&toy)]);
    let message = run.failed(1);
    assert!(
        message.starts_with(&format!(
            "`{}` and `{}` are not solutions of the same game: ",
            turn.display(),
            toy.display()
        )),
        "{message}"
    );
    assert!(
        message.ends_with(" differences listed above; compare two solutions of one spot and tree"),
        "{message}"
    );
    run.prints(&[
        "A and B are not solutions of one game:",
        "- the board is Kh9c7d2s in A and Ks9h6d3c2s in B",
        "- the stack is 15bb in A and 20bb in B",
        "- the rake is 5% capped at 1bb in A and none in B",
    ]);
    assert!(!run.stdout.contains("Comparison of"), "nothing is compared");

    // The same board and ranges under another tree: the sizing that differs is named.
    let mut spot = Spot::load(root().join(TURN)).unwrap();
    spot.tree.turn.as_mut().unwrap().bets = vec![0.75];
    let other = folder.join("other_tree.toml");
    fs::write(&other, spot.to_toml()).unwrap();
    let wider = folder.join("wider.pksol");
    solve_to(&["--spot", arg(&other)], &wider, &[]);
    let run = compare(&[arg(&turn), arg(&wider)]);
    run.failed(1);
    run.prints(&[
        "- on the turn, the tree bets 50% of the pot; raises to 3x the bet; all-in in A and \
         bets 75% of the pot; raises to 3x the bet; all-in in B",
    ]);
    assert!(
        run.line("- the betting trees differ: ")
            .contains("A's has 64 nodes")
    );
    fs::remove_dir_all(&folder).unwrap();
}

#[test]
fn a_dump_is_mapped_onto_a_spot_and_says_so_when_there_is_none() {
    let folder = scratch("compare-dumps");
    let (whole, partial) = turn_files(&folder);
    let (whole_dump, partial_dump) = (folder.join("whole.json"), folder.join("turn.json"));
    dump_to(&whole, &whole_dump);
    dump_to(&partial, &partial_dump);

    // Two dumps have no spot to be mapped onto unless --spot gives one.
    let run = compare(&[arg(&partial_dump), arg(&whole_dump)]);
    assert_eq!(
        run.failed(1),
        format!(
            "A, `{}`, is a dump, which is compared on the spot it answers, and neither A nor B \
             is a solution file whose spot it could be mapped onto; give the spot file with \
             --spot <FILE>",
            partial_dump.display()
        )
    );
    assert_eq!(run.stdout, "", "nothing is printed before the error");
    let spot = Spot::load(root().join(TURN)).unwrap();
    let map =
        |path: &Path| MappedSolution::from_dump(&SolutionDump::load(path).unwrap(), &spot).unwrap();
    let run = compare(&[arg(&partial_dump), arg(&whole_dump), "--spot", TURN]);
    check_verdict(
        &run,
        &library(&map(&partial_dump), &map(&whole_dump)),
        &partial_dump,
        &whole_dump,
    );
    run.prints(&[&format!(
        "Spot         turn_small, Kh9c7d2s from the turn: --spot `{TURN}`, which A and B are \
         mapped onto"
    )]);
    assert!(
        run.line("A            ")
            .ends_with("holds the turn, without the river, a partial dump")
    );

    // A dump of another spot does not answer the file's: what differs is listed.
    let toy = folder.join("toy.pksol");
    solve_to(&["--spot", TOY], &toy, &[]);
    let run = compare(&[arg(&toy), arg(&whole_dump)]);
    assert_eq!(
        run.failed(1),
        format!(
            "`{}` does not answer the spot toy_polarized: 3 differences listed above; compare \
             it with a solution of the spot it answers, or give that spot with --spot",
            whole_dump.display()
        )
    );
    run.prints(&[
        "B does not answer the spot toy_polarized:",
        "- `spot.board` is Kh9c7d2s in the dump, and Ks9h6d3c2s in our spot",
    ]);

    // --spot is for a dump; two files need none.
    let message = compare(&[arg(&whole), arg(&partial), "--spot", TURN]).failed(1);
    assert!(
        message.starts_with(&format!("--spot `{TURN}`: A and B are both solution files")),
        "{message}"
    );
    fs::remove_dir_all(&folder).unwrap();
}

#[test]
fn a_file_that_is_not_a_solution_and_a_missing_reference_are_named() {
    let folder = scratch("compare-files");
    let text = folder.join("notes.txt");
    fs::write(&text, "not a solution\n").unwrap();
    let (whole, _) = turn_files(&folder);
    assert_eq!(
        compare(&[arg(&whole), arg(&text)]).failed(1),
        format!(
            "`{}` is neither a solution file nor a solution dump: a solution file starts with \
             `PKSOL` and a dump is JSON; give a file that solve-postflop --out or inspect --dump \
             wrote, or another solver's dump",
            text.display()
        )
    );
    let message = compare(&["no/such/file.pksol", arg(&whole)]).failed(1);
    assert!(
        message.starts_with("could not read A, `no/such/file.pksol`: "),
        "{message}"
    );
    assert!(message.ends_with("; check the path"), "{message}");
    let broken = folder.join("broken.json");
    fs::write(&broken, "{\"format\": \"pk-solution-dump\"}").unwrap();
    let message = compare(&[arg(&whole), arg(&broken)]).failed(1);
    assert!(
        message.starts_with(&format!(
            "`{}` is not a valid solution dump: ",
            broken.display()
        )),
        "{message}"
    );

    // The reference must be named, before anything is read.
    let run = compare(&["no/such/file.pksol", "--reference"]);
    let message = run.failed(1);
    assert!(
        message.starts_with(
            "--reference: no reference solver is set; set PK_REFERENCE_SOLVER to its command as \
             a JSON array of strings"
        ),
        "{message}"
    );
    assert!(
        message.ends_with("as reference/README.md (Reading a reference's answer) describes"),
        "{message}"
    );
    let run = compare_with(
        &[arg(&whole), "--reference"],
        Some(r#"["no-such-reference-solver.exe", "{spot}", "{out}"]"#),
    );
    let message = run.failed(1);
    assert!(
        message.starts_with("--reference: could not start no-such-reference-solver.exe: "),
        "{message}"
    );
    let run = compare_with(&[arg(&whole), "--reference"], Some("not json"));
    assert!(
        run.failed(1)
            .starts_with("--reference: PK_REFERENCE_SOLVER does not hold a usable command: ")
    );
    // A command line with neither B nor --reference, or with both, does not parse.
    compare(&[arg(&whole)]).failed(2);
    compare(&[arg(&whole), arg(&whole), "--reference"]).failed(2);
    let message = compare(&[arg(&whole), arg(&whole), "--keep-reference", "x.json"]).failed(1);
    assert_eq!(
        message,
        "--keep-reference `x.json` keeps the dump the reference writes; pass --reference too, \
         in place of B"
    );
    fs::remove_dir_all(&folder).unwrap();
}

// ---------------------------------------------------------------------------------------------
// The gate

/// M5's gate: the ranges recorded by hand for BTN against BB, on the flop Ks7d2c under
/// `configs/trees/one_bet.toml`, solved by `solve-postflop` to 0.03% of the pot, saved, and
/// compared with what the reference solver writes for its spot. The status must be the
/// library's verdict on the same pair, whatever that verdict is: a failure is a finding.
/// Without `PK_REFERENCE_SOLVER` it does nothing. Run in release with `--nocapture` to see the
/// comparison.
#[test]
#[ignore = "slow"]
fn the_gate_flop_compared_with_the_reference() {
    let Ok(template) = std::env::var(SOLVER_ENV) else {
        println!("no reference solver: `{SOLVER_ENV}` is not set, so nothing to compare");
        return;
    };
    let folder = scratch("compare-gate");
    let (file, kept) = (folder.join("gate.pksol"), folder.join("reference.json"));
    let started = Instant::now();
    solve_to(
        &[
            "--ranges",
            GATE_RANGES,
            "--board",
            "Ks7d2c",
            "--tree",
            ONE_BET,
        ],
        &file,
        &["--target", "0.03"],
    );
    println!("our solve and save took {:.1?}", started.elapsed());
    let started = Instant::now();
    let run = compare_with(
        &[arg(&file), "--reference", "--keep-reference", arg(&kept)],
        Some(&template),
    );
    println!(
        "the comparison, the reference's solve included, took {:.1?}",
        started.elapsed()
    );
    let saved = SavedSolution::load(&file).unwrap();
    let spot = saved.solution().spot().clone();
    let theirs = MappedSolution::from_dump(&SolutionDump::load(&kept).unwrap(), &spot).unwrap();
    let comparison = library(&MappedSolution::from_saved(&saved), &theirs);
    check_verdict(&run, &comparison, &file, &kept);
    println!(
        "the gate's comparison {}",
        if comparison.passes() {
            "passes".to_string()
        } else {
            format!("fails {} checks", comparison.failures.len())
        }
    );
    fs::remove_dir_all(&folder).unwrap();
}
