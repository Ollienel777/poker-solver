//! Runs the `solver` binary on small inputs, as a user would, and checks what it prints, where
//! it prints it, and its exit code.
//!
//! Each run is made from the repository's root, where `configs` is, so that a range file's
//! config is found as it is by default, except the runs that check what happens when it is not.
//! No run writes to the repository's resource log: each passes `--no-log`, or `--log` with a
//! file in a folder of its own under the system's temporary folder.

use std::fs;
use std::path::Path;

use pk_game::Street;
use pk_postflop::memory::format_bytes;
use pk_postflop::{
    SaveOptions, SavedSolution, SolveOptions, Spot, estimate_memory, estimate_save_memory,
};

mod common;

use common::*;

const MONOTONE_FLOP: &str = "reference/spots/flop/monotone_pot.toml";

// ---------------------------------------------------------------------------------------------
// Solving

#[test]
fn a_river_spot_file_solves_through_the_command_line() {
    let run = solver(&["solve-postflop", "--spot", TOY, "--no-log", "--no-out"]).succeeded();
    run.prints(&[
        "Spot         toy_polarized",
        "From         the spot file `reference/spots/river/toy_polarized.toml`",
        "Board        Ks9h6d3c2s, starting on the river",
        "Pot          10bb, with 20bb behind each player",
        "Rake         none",
        "Tree         the spot file's [tree] table",
        "  river      bets 50% of the pot",
        "  rules      no raises",
        "Hands        OOP 22, IP 12, of the combos the board leaves",
        "Nodes        9 in the public tree, from 9 in the betting tree",
        "Solving to 0.3% of the pot, for at most 10,000 iterations, on 1 thread, with f32 storage",
        " iterations   exploitable     elapsed   peak memory",
    ]);
    assert!(
        !run.stdout.contains("Players"),
        "a spot file names no positions"
    );
    assert!(
        run.line("Memory       about ")
            .contains("the limit is 10.00 GB")
    );
    assert!(
        run.line("Stopped      at the target, after ")
            .contains(" iterations, in ")
    );
    let exploitable = run.line("Exploitable  ");
    assert!(exploitable.ends_with("per hand dealt"), "{exploitable}");
    // The toy game's values are 40/11 and 70/11 big blinds at its equilibrium.
    let values = run.line("Values       ");
    assert!(values.starts_with("Values       OOP 3.6"), "{values}");
    assert!(values.contains(", IP 6.3"), "{values}");
    assert!(
        run.line("Peak memory  ")
            .contains("resident, against an estimate of")
    );
    assert!(!run.stdout.contains("Log"), "--no-log appends nothing");

    // A line per measurement: the iterations, the exploitability, the time and the memory.
    let header = run
        .stdout
        .lines()
        .position(|line| line.starts_with(" iterations"))
        .unwrap();
    let progress: Vec<&str> = run
        .stdout
        .lines()
        .skip(header + 1)
        .take_while(|line| !line.is_empty())
        .collect();
    assert!(!progress.is_empty());
    for line in &progress {
        let columns: Vec<&str> = line.split_whitespace().collect();
        assert_eq!(columns.len(), 5, "{line}");
        assert!(columns[0].parse::<u64>().is_ok(), "{line}");
        assert!(
            columns[1].ends_with('%') && columns[2].ends_with('s'),
            "{line}"
        );
        assert!(columns[4].ends_with('B'), "{line}");
    }
}

#[test]
fn a_range_file_with_a_river_board_and_a_tree_file_solves_and_logs_its_resources() {
    let folder = scratch("log");
    let log = folder.join("runs").join("log.jsonl");
    let run = solver(&[
        "solve-postflop",
        "--ranges",
        RANGES,
        "--board",
        "Ks7d2c9h3s",
        "--tree",
        RIVER_TREE,
        "--threads",
        "2",
        "--log",
        arg(&log),
        "--no-out",
    ])
    .succeeded();
    run.prints(&[
        "Spot         BTN_vs_BB_SRP_Ks9h7d3s2c",
        "From         the range file `reference/ranges/a/BTN_vs_BB_SRP.toml`, under config A \
         from `configs/a.toml`",
        "Board        Ks9h7d3s2c, starting on the river",
        "Players      BB out of position, BTN in position",
        "Pot          5.5bb, with 97.5bb behind each player",
        "Rake         5% of the pot, capped at 6bb",
        "Tree         the tree file `crates/cli/tests/data/river_tree.toml`",
        "  river      bets 50% and 100% of the pot",
        "  rules      no raises",
    ]);
    assert!(
        run.line("Hands        ")
            .starts_with("Hands        OOP (BB) ")
    );
    assert!(
        run.line("Solving to ").contains("on 1 thread"),
        "a river spot has no chance node"
    );
    assert!(
        run.line("Values       ")
            .starts_with("Values       OOP (BB) ")
    );
    run.prints(&[&format!(
        "Log          appended the solve's record to `{}`",
        log.display()
    )]);
    let text = fs::read_to_string(&log).unwrap();
    assert_eq!(text.lines().count(), 1);
    assert!(
        text.contains(r#""spot":"BTN_vs_BB_SRP_Ks9h7d3s2c""#),
        "{text}"
    );
    assert!(text.contains(r#""stop":"target""#), "{text}");
    // A second solve appends a second line.
    solver(&[
        "solve-postflop",
        "--spot",
        TOY,
        "--log",
        arg(&log),
        "--no-out",
    ])
    .succeeded();
    assert_eq!(fs::read_to_string(&log).unwrap().lines().count(), 2);
    fs::remove_dir_all(&folder).unwrap();
}

#[test]
fn a_solve_stopped_by_the_iteration_cap_says_so() {
    let run = solver(&[
        "solve-postflop",
        "--spot",
        TOY,
        "--max-iters",
        "3",
        "--target",
        "0.001",
        "--no-log",
        "--no-out",
    ])
    .succeeded();
    run.line("Stopped      at the cap of 3 iterations, in ");
    assert!(
        run.line("Exploitable  ")
            .ends_with("above the target of 0.001%")
    );
}

// ---------------------------------------------------------------------------------------------
// Saving

#[test]
fn a_solve_writes_a_solution_file_that_loads_back() {
    let folder = scratch("save");
    let path = folder.join("toy.pksol");
    let run = solver(&[
        "solve-postflop",
        "--spot",
        TOY,
        "--no-log",
        "--out",
        arg(&path),
    ])
    .succeeded();
    run.prints(&[&format!(
        "Saving       `{}`, holding the river",
        path.display()
    )]);

    // The file is the library's solution file, of this spot and this solve.
    let saved = SavedSolution::load(&path).unwrap();
    let header = saved.header();
    assert_eq!(header.spot, Spot::load(root().join(TOY)).unwrap());
    assert_eq!(header.streets, [Street::River]);
    assert!(saved.solution().is_complete());
    assert!(header.iterations < 1000);
    assert!(
        run.line("Stopped      ")
            .contains(&format!(", after {} iterations, in ", header.iterations))
    );
    assert!(run.line("Exploitable  ").starts_with(&format!(
        "Exploitable  {:.4}% of the pot",
        header.solve_exploitability.percent_of_pot
    )));
    // The line that says it was saved gives the file's size, and the exploitability of the
    // strategy as the file holds it.
    let line = run.line("Saved        ");
    let size = fs::metadata(&path).unwrap().len();
    assert!(
        line.starts_with(&format!("Saved        {} in ", format_bytes(size))),
        "{line}"
    );
    assert!(
        line.ends_with(&format!(
            "; as stored, in 16 bits a probability, exploitable for {:.4}% of the pot",
            header.exploitability.percent_of_pot
        )),
        "{line}"
    );
    // Nothing else is left in the folder.
    assert_eq!(fs::read_dir(&folder).unwrap().count(), 1);
    fs::remove_dir_all(&folder).unwrap();
}

#[test]
fn a_solution_file_can_leave_out_the_river() {
    let folder = scratch("last-street");
    let (partial, whole) = (folder.join("turn.pksol"), folder.join("whole.pksol"));
    let run = solver(&[
        "solve-postflop",
        "--spot",
        TURN,
        "--no-log",
        "--out",
        arg(&partial),
        "--last-street",
        "turn",
    ])
    .succeeded();
    run.prints(&[&format!(
        "Saving       `{}`, holding the turn, without the river",
        partial.display()
    )]);
    let saved = SavedSolution::load(&partial).unwrap();
    assert_eq!(saved.header().streets, [Street::Turn]);
    assert_eq!(saved.last_street(), Street::Turn);
    assert!(!saved.solution().is_complete());

    // The river is the default, and nearly all of the file.
    let run = solver(&[
        "solve-postflop",
        "--spot",
        TURN,
        "--no-log",
        "--out",
        arg(&whole),
    ])
    .succeeded();
    run.prints(&[&format!(
        "Saving       `{}`, holding the turn and the river",
        whole.display()
    )]);
    let saved = SavedSolution::load(&whole).unwrap();
    assert_eq!(saved.header().streets, [Street::Turn, Street::River]);
    let size = |path: &Path| fs::metadata(path).unwrap().len();
    assert!(size(&whole) > 5 * size(&partial));

    // A street before the board's is refused before anything is solved.
    let refused = folder.join("refused.pksol");
    let run = solver(&[
        "solve-postflop",
        "--spot",
        TURN,
        "--no-log",
        "--out",
        arg(&refused),
        "--last-street",
        "flop",
    ]);
    let message = run.failed(1);
    assert!(
        message.starts_with("--last-street `flop`: the board "),
        "{message}"
    );
    assert!(
        message.ends_with(
            " starts on the turn, and a solution file holds the spot's first street at least; \
             give `turn` or `river`"
        ),
        "{message}"
    );
    assert_eq!(run.stdout, "", "nothing is solved");
    assert!(!refused.exists());
    fs::remove_dir_all(&folder).unwrap();
}

// ---------------------------------------------------------------------------------------------
// Estimating

#[test]
fn estimate_prints_the_memory_item_by_item_and_that_a_solve_would_run() {
    let run = solver(&[
        "estimate",
        "--ranges",
        RANGES,
        "--board",
        "Ks7d2c",
        "--tree",
        "configs/trees/one_bet.toml",
        "--threads",
        "8",
    ])
    .succeeded();
    run.prints(&[
        "Spot         BTN_vs_BB_SRP_Ks7d2c",
        "  flop       bets 50% of the pot",
        "  turn       bets 50% of the pot",
        "  river      bets 50% of the pot",
        "Memory a solve would need, with f32 storage on 8 threads, estimated before anything is \
         allocated:",
        "Memory saving its solution would need, in a file holding the flop, the turn and the \
         river:",
        "A solve would run: the solve's estimate and its save's are within the limit of 10.00 GB.",
    ]);
    assert!(
        run.line("Nodes        ")
            .starts_with("Nodes        191,844 in the public tree")
    );
    for item in [
        "  regrets and strategy sums, f32",
        "  average strategy, 1 held at once",
        "  thread pool, 8 threads",
        "  total",
        "  quantized strategy",
        "  raw EV sections",
    ] {
        run.line(item);
    }
    assert!(
        !run.stdout.contains("Solving"),
        "an estimate solves nothing"
    );

    // What the file is to hold is the save's to estimate, and a solve that saves nothing has
    // no save.
    let base = [
        "estimate",
        "--ranges",
        RANGES,
        "--board",
        "Ks7d2c",
        "--tree",
        "configs/trees/one_bet.toml",
    ];
    let mut to_the_turn = base.to_vec();
    to_the_turn.extend(["--last-street", "turn"]);
    let run = solver(&to_the_turn).succeeded();
    run.prints(&[
        "Memory saving its solution would need, in a file holding the flop and the turn, without \
         the river:",
    ]);
    let mut no_out = base.to_vec();
    no_out.push("--no-out");
    let run = solver(&no_out).succeeded();
    run.prints(&["A solve would run: the estimate is within the limit of 10.00 GB."]);
    assert!(!run.stdout.contains("Memory saving its solution"));
}

#[test]
fn estimate_says_why_a_solve_would_be_refused_and_what_to_change() {
    // Config A's own tree on a flop is over the node limit: the estimate is made as if the
    // limit were lifted, and the refusal points to a smaller tree.
    let run = solver(&["estimate", "--ranges", RANGES, "--board", "Ks7d2c"]).succeeded();
    assert!(
        run.line("Tree         ")
            .contains("config A's [postflop] table")
    );
    assert!(
        run.line("Nodes        ")
            .starts_with("Nodes        9,789,026 in the public tree")
    );
    assert!(
        run.line("Memory a solve would need, ")
            .contains("as if the node limit were lifted")
    );
    // Over both limits: each is listed, and the advice is given once.
    run.prints(&[
        "A solve would be refused:",
        "- the public tree, with a child for every card that can fall, would have 9,789,026 \
         nodes, more than the limit of 1,000,000",
    ]);
    for reason in [
        "- the solve would need about ",
        "- saving the solution would need about ",
    ] {
        let memory = run.line(reason);
        assert!(
            memory.ends_with("more than the limit of 10.00 GB"),
            "{memory}"
        );
    }
    let advice = run.line("To solve it, ");
    assert!(advice.starts_with(
        "To solve it, pass --tree with a tree file of fewer bet and raise sizes than config A's \
         [postflop] table, such as configs/trees/one_bet.toml"
    ));
    assert!(advice.ends_with("raise --max-nodes and --max-memory, if the machine has the memory."));

    // A memory limit below the estimate.
    let run = solver(&[
        "estimate",
        "--spot",
        TOY,
        "--max-memory",
        "0.000001",
        "--no-out",
    ])
    .succeeded();
    let verdict = run.line("A solve would be refused: ");
    assert!(
        verdict.ends_with("more than the limit of 1.0 kB."),
        "{verdict}"
    );
    run.prints(&[
        "To solve it, give the spot file's [tree] table fewer bet and raise sizes, or a \
                  lower max_raises; or raise --max-memory, if the machine has the memory.",
    ]);
}

#[test]
fn every_test_spot_loads_and_estimates_through_the_command_line() {
    let mut spots = 0;
    for street in ["river", "turn", "flop"] {
        let folder = root().join("reference/spots").join(street);
        for entry in fs::read_dir(&folder).unwrap() {
            let path = entry.unwrap().path();
            let relative = format!(
                "reference/spots/{street}/{}",
                path.file_name().unwrap().to_str().unwrap()
            );
            let run = solver(&["estimate", "--spot", &relative]).succeeded();
            run.prints(&[&format!("From         the spot file `{relative}`")]);
            assert!(
                run.line("Board        ")
                    .ends_with(&format!("starting on the {street}"))
            );
            run.line("A solve would run: ");
            spots += 1;
        }
    }
    assert_eq!(spots, 16);
}

/// Every test spot solves through the command line to its target.
#[test]
#[ignore = "slow"]
fn every_test_spot_solves_through_the_command_line() {
    for street in ["river", "turn", "flop"] {
        let folder = root().join("reference/spots").join(street);
        for entry in fs::read_dir(&folder).unwrap() {
            let path = entry.unwrap().path();
            let run = solver(&[
                "solve-postflop",
                "--spot",
                arg(&path),
                "--no-log",
                "--no-out",
            ])
            .succeeded();
            run.line("Stopped      at the target");
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Errors

#[test]
fn a_mistyped_path_names_the_file_and_says_to_check_the_path() {
    for (args, file) in [
        (
            vec!["--spot", "reference/spots/river/toy_polarised.toml"],
            "the spot file `reference/spots/river/toy_polarised.toml`",
        ),
        (
            vec![
                "--ranges",
                "reference/ranges/a/BTN_vs_BB_SPR.toml",
                "--board",
                "Ks7d2c",
            ],
            "the range file `reference/ranges/a/BTN_vs_BB_SPR.toml`",
        ),
        (
            vec![
                "--ranges",
                RANGES,
                "--board",
                "Ks7d2c",
                "--tree",
                "trees/one_bet.toml",
            ],
            "the tree file `trees/one_bet.toml`",
        ),
        (
            vec![
                "--ranges",
                RANGES,
                "--board",
                "Ks7d2c",
                "--config",
                "config/a.toml",
            ],
            "the config `config/a.toml` that --config names",
        ),
    ] {
        for command in ["solve-postflop", "estimate"] {
            let mut line = vec![command];
            line.extend(&args);
            if command == "solve-postflop" {
                line.push("--no-out");
            }
            let run = solver(&line);
            let message = run.failed(1);
            assert!(
                message.starts_with(&format!("could not read {file}: ")),
                "{message}"
            );
            assert!(message.ends_with("; check the path"), "{message}");
            assert_eq!(
                run.stdout, "",
                "nothing is printed before the inputs are read"
            );
        }
    }
}

#[test]
fn a_board_with_a_repeated_card_or_the_wrong_number_of_cards_is_refused() {
    for (board, error) in [
        (
            "Ks7dKs",
            "--board `Ks7dKs` holds Ks twice; give each card once, as in Ks7d2c",
        ),
        (
            "Ks7d",
            "--board `Ks7d` has 2 cards; a board has 3 (a flop), 4 (a turn) or 5 (a river), as \
             in Ks7d2c",
        ),
        (
            "Ks7d2c9h3s4h",
            "--board `Ks7d2c9h3s4h` has 6 cards; a board has 3 (a flop), 4 (a turn) or 5 (a \
             river), as in Ks7d2c",
        ),
        (
            "Ks7d2x",
            "--board `Ks7d2x`: `2x` is not a card; write each card as a rank (23456789TJQKA) and \
             a suit (cdhs), as in Ks7d2c",
        ),
    ] {
        let run = solver(&[
            "solve-postflop",
            "--ranges",
            RANGES,
            "--board",
            board,
            "--no-out",
        ]);
        assert_eq!(run.failed(1), error);
    }
}

#[test]
fn a_board_that_leaves_a_range_no_hand_is_refused() {
    let run = solver(&[
        "solve-postflop",
        "--ranges",
        NARROW_RANGES,
        "--board",
        "AsAhAd",
        "--no-out",
    ]);
    assert_eq!(
        run.failed(1),
        "the board AsAhAd leaves BB no hand: every combo of its range (`oop.range` in \
         `crates/cli/tests/data/narrow_ranges.toml`) holds a card of the board; check --board, \
         or give a range that holds a hand this board leaves possible"
    );
}

#[test]
fn a_tree_over_the_node_limit_is_refused_with_what_to_do() {
    let run = solver(&[
        "solve-postflop",
        "--ranges",
        RANGES,
        "--board",
        "Ks7d2c",
        "--no-log",
        "--no-out",
    ]);
    assert_eq!(
        run.failed(1),
        "the public tree, with a child for every card that can fall, would have 9,789,026 \
         nodes, more than the limit of 1,000,000, so the solve is refused; pass --tree with a \
         tree file of fewer bet and raise sizes than config A's [postflop] table, such as \
         configs/trees/one_bet.toml (one half-pot bet a street, no raises); or raise \
         --max-nodes, if the machine has the memory that `solver estimate` gives"
    );
    // The spot is printed before the refusal, and nothing is solved.
    run.line("Spot         BTN_vs_BB_SRP_Ks7d2c");
    assert!(!run.stdout.contains("Solving"));

    // From a tree file or a spot file, the advice names that file.
    let run = solver(&[
        "solve-postflop",
        "--spot",
        "reference/spots/flop/monotone_pot.toml",
        "--max-nodes",
        "1000",
        "--no-out",
    ]);
    let message = run.failed(1);
    assert!(message.contains("give the spot file's [tree] table fewer bet and raise sizes"));
}

#[test]
fn a_memory_estimate_over_the_limit_is_refused_with_what_to_do() {
    let run = solver(&[
        "solve-postflop",
        "--spot",
        TOY,
        "--max-memory",
        "0.000001",
        "--no-log",
        "--no-out",
    ]);
    let message = run.failed(1);
    assert!(
        message.starts_with("the solve would need about "),
        "{message}"
    );
    assert!(
        message.contains("by its estimate, more than the limit of 1.0 kB, so the solve is refused"),
        "{message}"
    );
    assert!(message.ends_with("raise --max-memory, if the machine has the memory"));
    assert!(!run.stdout.contains("Solving"));
}

#[test]
fn a_save_over_the_memory_limit_is_refused_before_the_solve() {
    // A flop whose solve merges runouts, so that it holds less than the save of its solution,
    // which is on the full tree; and a limit between the two estimates.
    let spot = Spot::load(root().join(MONOTONE_FLOP)).unwrap();
    let options = SolveOptions::default();
    let solve = estimate_memory(&spot, &options).unwrap().total;
    let save = estimate_save_memory(&spot, &options, &SaveOptions::default())
        .unwrap()
        .total;
    assert!(solve < save, "{solve} against {save}");
    let limit = format!("{:.6}", (solve + save) as f64 / 2e9);
    let limit_bytes = format_bytes((limit.parse::<f64>().unwrap() * 1e9) as u64);

    let folder = scratch("save-memory");
    let path = folder.join("monotone.pksol");
    let run = solver(&[
        "solve-postflop",
        "--spot",
        MONOTONE_FLOP,
        "--max-memory",
        &limit,
        "--no-log",
        "--out",
        arg(&path),
    ]);
    let message = run.failed(1);
    assert!(
        message.starts_with(&format!(
            "saving the solution would need about {} of memory by its estimate, more than the \
             limit of {limit_bytes}, so the solve is refused; ",
            format_bytes(save)
        )),
        "{message}"
    );
    assert!(message.ends_with("raise --max-memory, if the machine has the memory"));
    // Both estimates are printed before the refusal, and nothing is solved or written.
    run.prints(&[&format!(
        "Memory       about {} to solve, the most while measuring exploitability, and {} to save, \
         by the estimates; the limit is {limit_bytes}",
        format_bytes(solve),
        format_bytes(save)
    )]);
    assert!(!run.stdout.contains("Solving"));
    assert_eq!(fs::read_dir(&folder).unwrap().count(), 0);

    // `estimate` says the same, and that the solve alone would run.
    let run = solver(&["estimate", "--spot", MONOTONE_FLOP, "--max-memory", &limit]).succeeded();
    run.line("A solve would be refused: saving the solution would need about ");
    let run = solver(&[
        "estimate",
        "--spot",
        MONOTONE_FLOP,
        "--max-memory",
        &limit,
        "--no-out",
    ])
    .succeeded();
    run.prints(&[&format!(
        "A solve would run: the estimate is within the limit of {limit_bytes}."
    )]);
    fs::remove_dir_all(&folder).unwrap();
}

#[test]
fn the_output_is_checked_before_the_solve_starts() {
    let folder = scratch("out");
    let missing = folder.join("no-such-folder").join("toy.pksol");
    let run = solver(&[
        "solve-postflop",
        "--spot",
        TOY,
        "--no-log",
        "--out",
        arg(&missing),
    ]);
    let message = run.failed(1);
    assert!(
        message.contains("no-such-folder` does not exist; create it"),
        "{message}"
    );
    assert_eq!(run.stdout, "", "nothing is solved");

    let earlier = folder.join("toy.pksol");
    fs::write(&earlier, "an earlier solve").unwrap();
    let run = solver(&[
        "solve-postflop",
        "--spot",
        TOY,
        "--no-log",
        "--out",
        arg(&earlier),
    ]);
    assert_eq!(
        run.failed(1),
        format!(
            "--out `{}` already exists; pass --force to overwrite it, or give another path",
            earlier.display()
        )
    );
    assert_eq!(run.stdout, "");

    // With --force the solve runs, and its solution replaces the earlier file.
    let run = solver(&[
        "solve-postflop",
        "--spot",
        TOY,
        "--no-log",
        "--out",
        arg(&earlier),
        "--force",
    ])
    .succeeded();
    run.line("Stopped      at the target");
    run.line("Saved        ");
    assert!(fs::read(&earlier).unwrap().starts_with(b"PKSOL"));
    fs::remove_dir_all(&folder).unwrap();
}

#[test]
fn the_config_is_read_from_configs_under_the_current_directory_or_from_config() {
    // From the crate's folder, which holds no `configs`.
    let crate_folder = Path::new(env!("CARGO_MANIFEST_DIR"));
    let ranges = "../../reference/ranges/a/BTN_vs_BB_SRP.toml";
    let tree = "tests/data/river_tree.toml";
    let base = [
        "estimate",
        "--ranges",
        ranges,
        "--board",
        "Ks7d2c9h3s",
        "--tree",
        tree,
    ];
    let run = solver_in(crate_folder, &base);
    let message = run.failed(1);
    assert!(message.starts_with(&format!(
        "the range file `{ranges}` names config `a`, which is read from `configs/a.toml` under \
         the current directory, but "
    )));
    let looked = crate_folder.join("configs").join("a.toml");
    assert!(message.contains(&format!("but {} could not be read", looked.display())));
    assert!(message.ends_with(
        "run solver from the folder that holds `configs`, or give the config with --config \
         <file>"
    ));

    let mut with_config = base.to_vec();
    with_config.extend(["--config", "../../configs/a.toml"]);
    let run = solver_in(crate_folder, &with_config).succeeded();
    run.line("A solve would run: ");

    // A config the range file was not made for.
    let run = solver(&[
        "estimate",
        "--ranges",
        RANGES,
        "--board",
        "Ks7d2c9h3s",
        "--config",
        "configs/c.toml",
    ]);
    let message = run.failed(1);
    assert!(message.starts_with(&format!(
        "the range file `{RANGES}` does not fit config C from `configs/c.toml`: `config` is `a`"
    )));
    assert!(message.ends_with("give the config it was made for with --config"));
}

#[test]
fn a_command_line_that_does_not_parse_is_an_error_like_any_other() {
    let run = solver(&["solve-postflop", "--spot", TOY, "--ranges", RANGES]);
    let message = run.failed(2);
    assert!(
        message.starts_with("the argument '--spot <FILE>' cannot be used with"),
        "{message}"
    );
    assert!(
        message.contains("Usage: solver solve-postflop"),
        "{message}"
    );

    let run = solver(&["solve-postflop", "--board", "Ks7d2c"]);
    run.failed(2);
    let run = solver(&["solve-postflop", "--spot", TOY, "--threads", "0"]);
    let message = run.failed(2);
    assert!(
        message
            .starts_with("invalid value '0' for '--threads <N>': a solve runs on 1 thread or more")
    );
    let run = solver(&["solve-postflop", "--spot", TOY, "--target", "low"]);
    let message = run.failed(2);
    assert!(message.contains("give a percent of the pot from 0 up, such as 0.3"));
    let run = solver(&[
        "solve-postflop",
        "--spot",
        TOY,
        "--no-log",
        "--log",
        "x.jsonl",
    ]);
    run.failed(2);

    // A solve says where its solution goes, or that it goes nowhere, and not both.
    let run = solver(&["solve-postflop", "--spot", TOY]);
    let message = run.failed(2);
    assert!(
        message.starts_with("the following required arguments were not provided:"),
        "{message}"
    );
    assert!(message.contains("--out <FILE>"), "{message}");
    let run = solver(&[
        "solve-postflop",
        "--spot",
        TOY,
        "--no-out",
        "--out",
        "x.pksol",
    ]);
    run.failed(2);
    // The options about the file are not given with --no-out.
    for option in [&["--force"][..], &["--last-street", "turn"]] {
        let mut line = vec!["solve-postflop", "--spot", TOY, "--no-log", "--no-out"];
        line.extend(option);
        let message = solver(&line).failed(2);
        assert!(message.contains("cannot be used with"), "{message}");
    }
    let run = solver(&[
        "solve-postflop",
        "--spot",
        TOY,
        "--out",
        "x.pksol",
        "--last-street",
        "preflop",
    ]);
    let message = run.failed(2);
    assert!(
        message.starts_with(
            "invalid value 'preflop' for '--last-street <STREET>': give a street: flop, turn or \
             river"
        ),
        "{message}"
    );

    // Help and the version are not errors.
    let run = solver(&["--version"]).succeeded();
    assert_eq!(
        run.stdout,
        format!("solver {}\n", env!("CARGO_PKG_VERSION"))
    );
    let run = solver(&["estimate", "--help"]).succeeded();
    assert!(
        run.stdout
            .contains("Usage: solver estimate [OPTIONS] <--ranges <FILE>|--spot <FILE>>")
    );
}
