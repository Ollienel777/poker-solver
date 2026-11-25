//! Solution files (`pk_postflop::solution_file`): what a file holds after a round trip, a file
//! without its river, every way a file can fail to be read, and the river spots that stand for
//! a river left out of a file.

use std::fs;
use std::io::Write;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};

use pk_cfr::{PublicGame, PublicNode, PublicProfile};
use pk_game::{Node, Player, Street};
use pk_postflop::solution_file::{
    FORMAT_VERSION, MAX_HEADER_BYTES, dequantize, quantize, quantize_profile, tree_hash,
};
use pk_postflop::{
    MappedSolution, Payoff, PostflopGame, RiverSpot, SaveOptions, SavedSolution, Solution,
    SolutionFileError, SolveOptions, Spot, Tolerances, check_solution, compare_mapped,
    exploitability, node_values, read_header, save, solve,
};

fn data(name: &str) -> Spot {
    Spot::load(format!(
        "{}/tests/data/{name}.toml",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

fn reference(path: &str) -> Spot {
    Spot::load(format!(
        "{}/../../reference/spots/{path}.toml",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

/// An empty directory of the test's own.
fn dir(test: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("solution_file")
        .join(test);
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn solved(spot: &Spot, options: &SolveOptions) -> Solution {
    let solution = solve(spot, options, |_| {}).unwrap();
    assert!(check_solution(&solution).is_valid());
    solution
}

fn player_nodes(game: &PostflopGame) -> impl Iterator<Item = u32> + '_ {
    (0..game.num_nodes() as u32).filter(|&node| game.actor(node).is_some())
}

fn bits(strategy: Option<&[f32]>) -> Option<Vec<u32>> {
    strategy.map(|probabilities| probabilities.iter().map(|p| p.to_bits()).collect())
}

/// `<path>.part`.
fn part(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(".part");
    PathBuf::from(name)
}

/// Saves, loads, and checks everything a complete file must give back.
fn check_round_trip(name: &str, spot: &Spot) {
    let solution = solved(spot, &SolveOptions::default());
    let dir = dir(name);
    let path = dir.join(format!("{name}.pksol"));
    let report = save(&solution, &path, &SaveOptions::default()).unwrap();
    assert!(!part(&path).exists(), "{name}: the part file is left");
    let bytes = fs::read(&path).unwrap();
    assert_eq!(report.bytes, bytes.len() as u64, "{name}");
    let saved = SavedSolution::load(&path).unwrap();
    let loaded = saved.solution();
    let game = solution.game();

    // The strategy is exactly the quantized one, and each probability within a step.
    let quantized = quantize_profile(game, solution.strategy()).unwrap();
    let mut rows = 0;
    for node in player_nodes(game) {
        assert_eq!(
            bits(loaded.strategy().get(node as usize)),
            bits(quantized.get(node as usize)),
            "{name}, node {node}"
        );
        let actor = game.actor(node).unwrap();
        for hand in 0..game.hands(actor).len() {
            let original = solution.hand_strategy(node, hand).unwrap();
            let stored = loaded.hand_strategy(node, hand).unwrap();
            for (&p, &q) in original.iter().zip(&stored) {
                assert!(
                    (f64::from(p) - f64::from(q)).abs() <= 1.0 / 65_535.0,
                    "{name}, node {node}, hand {hand}: {p} stored as {q}"
                );
            }
            // The stored row is the decoding of codes that sum to 65,535, and its own codes.
            let codes = quantize(&stored).unwrap();
            assert_eq!(codes.iter().map(|&c| u32::from(c)).sum::<u32>(), 65_535);
            let decoded: Vec<f32> = codes.iter().map(|&c| dequantize(c)).collect();
            assert_eq!(bits(Some(&decoded)), bits(Some(&stored)));
            rows += 1;
        }
    }
    assert!(rows > 0);
    assert!(
        loaded
            .strategy()
            .nodes()
            .all(|node| game.actor(node as u32).is_some())
    );

    // The header.
    let header = saved.header();
    assert_eq!(header.version, FORMAT_VERSION);
    assert_eq!(header.solver, "pk-postflop");
    assert_eq!(header.solver_version, env!("CARGO_PKG_VERSION"));
    assert_eq!(&header.spot, spot);
    assert_eq!(header.tree_hash, tree_hash(game));
    assert_eq!(header.nodes as usize, game.num_nodes());
    assert_eq!(header.player_nodes as usize, player_nodes(game).count());
    assert_eq!(
        header.hands,
        [Player::Oop, Player::Ip].map(|p| game.hands(p).len() as u32)
    );
    let streets: Vec<Street> = Street::ALL
        .into_iter()
        .filter(|&s| s >= spot.street())
        .collect();
    assert_eq!(header.streets, streets);
    assert_eq!(header.sections.len(), 2 * streets.len());
    assert_eq!(header.sections, report.sections);
    assert_eq!(header.stop, solution.stop());
    assert_eq!(header.iterations, solution.iterations());
    assert_eq!(&header.solve_exploitability, solution.exploitability());
    assert_eq!(header.exploitability, exploitability(game, &quantized));
    assert_eq!(header, &read_header(&path).unwrap());

    // The EVs are the quantized strategy's, cast to f32.
    let values = node_values(game, &quantized);
    let mut defined = 0;
    for node in player_nodes(game) {
        let evs = saved.ev(node).unwrap();
        let expected = values.get(node).unwrap();
        assert_eq!(evs.len(), expected.len());
        for (ev, value) in evs.iter().zip(expected) {
            match value {
                Some(value) => {
                    assert_eq!(ev.to_bits(), (*value as f32).to_bits(), "{name}, {node}");
                    defined += 1;
                }
                None => assert!(ev.is_nan(), "{name}, node {node}: {ev}"),
            }
        }
    }
    assert!(defined > 0);
    assert_eq!(
        saved.ev(0).map(<[f32]>::len),
        Some(game.hands(Player::Oop).len())
    );
    assert!(saved.ev(game.num_nodes() as u32).is_none());

    // Measured again, the loaded strategy's exploitability is the header's, bit for bit.
    assert_eq!(
        exploitability(loaded.game(), loaded.strategy()),
        header.exploitability
    );
    assert_eq!(loaded.exploitability(), &header.exploitability);
    assert_eq!(loaded.solve_exploitability(), solution.exploitability());
    assert_eq!(loaded.values(), header.exploitability.values);
    assert!(loaded.is_complete());
    assert_eq!(saved.last_street(), Street::River);
    assert_eq!(loaded.spot(), spot);
    // The game is the one the solve built, its tree down to the node limit it was built under.
    assert!(loaded.tree() == solution.tree(), "{name}");
    assert_eq!(loaded.hands(Player::Oop), solution.hands(Player::Oop));
    assert_eq!(loaded.hands(Player::Ip), solution.hands(Player::Ip));
    // A file holds nothing from the machine, the solve's time included.
    assert_eq!(loaded.elapsed(), std::time::Duration::ZERO);
    assert!(
        check_solution(loaded).is_valid(),
        "{}",
        check_solution(loaded)
    );

    // Saved again, the loaded solution gives the same bytes.
    let again = dir.join("again.pksol");
    save(loaded, &again, &SaveOptions::default()).unwrap();
    assert!(
        fs::read(&again).unwrap() == bytes,
        "{name}: saving again changed the bytes"
    );

    // The comparison takes the loaded solution as it takes a solve, and finds the two alike.
    let ours = MappedSolution::from_solution(&solution);
    let theirs = MappedSolution::from_solution(loaded);
    assert!(theirs.is_complete());
    assert_eq!(ours.streets(), theirs.streets());
    let comparison = compare_mapped(&ours, &theirs, &Tolerances::default());
    assert!(comparison.passes(), "{name}:\n{comparison}");

    let before = solution.exploitability().percent_of_pot;
    let after = header.exploitability.percent_of_pot;
    println!(
        "{name}: {} bytes, exploitability {before:.6}% of the pot solved, {after:.6}% stored",
        bytes.len()
    );
    // Quantizing moves the exploitability by far less than the target.
    assert!(
        (after - before).abs() < 0.01,
        "{name}: {before} against {after}"
    );
}

#[test]
fn a_river_spot_saved_and_loaded_gives_back_the_quantized_strategy_exactly() {
    check_round_trip("dry_ip_polarized", &reference("river/dry_ip_polarized"));
}

#[test]
fn a_turn_spot_saved_and_loaded_gives_back_the_quantized_strategy_exactly() {
    check_round_trip("turn_small", &data("turn_small"));
}

#[test]
fn a_flop_spot_saved_and_loaded_gives_back_the_quantized_strategy_exactly() {
    check_round_trip("flop_tiny", &data("flop_tiny"));
}

#[test]
fn the_bytes_are_the_same_on_any_thread_count() {
    let spot = data("turn_small");
    let dir = dir("threads");
    let files: Vec<Vec<u8>> = [1, 4]
        .into_iter()
        .map(|threads| {
            let options = SolveOptions {
                threads,
                ..SolveOptions::default()
            };
            let path = dir.join(format!("{threads}.pksol"));
            save(&solved(&spot, &options), &path, &SaveOptions::default()).unwrap();
            fs::read(&path).unwrap()
        })
        .collect();
    assert!(files[0] == files[1]);
}

/// The first decision of a river after the line of `path`, which ends at a river card.
fn node(game: &PostflopGame, path: &str) -> u32 {
    game.find(path)
        .unwrap_or_else(|error| panic!("`{path}`: {error}"))
}

#[test]
fn a_file_without_the_river_holds_the_first_streets_and_says_so() {
    for (name, spot, turn_path, river_path) in [
        ("turn_small", data("turn_small"), "x", "x x 3c"),
        ("flop_tiny", data("flop_tiny"), "x x 8h", "x x 8h x x 3c"),
    ] {
        let solution = solved(&spot, &SolveOptions::default());
        let dir = dir(&format!("partial_{name}"));
        let (whole, partial) = (dir.join("whole.pksol"), dir.join("partial.pksol"));
        let options = SaveOptions {
            last_street: Street::Turn,
        };
        let whole_report = save(&solution, &whole, &SaveOptions::default()).unwrap();
        let report = save(&solution, &partial, &options).unwrap();
        assert!(report.bytes < whole_report.bytes, "{name}");
        let header = read_header(&partial).unwrap();
        let expected: Vec<Street> = Street::ALL
            .into_iter()
            .filter(|&s| s >= spot.street() && s <= Street::Turn)
            .collect();
        assert_eq!(header.streets, expected, "{name}");
        assert_eq!(header.last_street(), Street::Turn);
        assert!(!header.is_complete());
        let names: Vec<&str> = header.sections.iter().map(|s| s.name.as_str()).collect();
        assert!(names.iter().all(|n| !n.ends_with("river")), "{names:?}");
        // The exploitability is the whole strategy's, measured before the river was left out.
        let complete = read_header(&whole).unwrap();
        assert_eq!(header.exploitability, complete.exploitability);
        assert_eq!(header.solve_exploitability, complete.solve_exploitability);

        let saved = SavedSolution::load(&partial).unwrap();
        let full = SavedSolution::load(&whole).unwrap();
        let loaded = saved.solution();
        let game = loaded.game();
        assert_eq!(saved.last_street(), Street::Turn);
        assert_eq!(loaded.last_street(), Street::Turn);
        assert!(!loaded.is_complete());
        // The turn as the complete file holds it, and nothing on the river.
        for node in player_nodes(game) {
            if game.street(node) <= Street::Turn {
                assert!(loaded.holds(node));
                assert_eq!(
                    bits(loaded.strategy().get(node as usize)),
                    bits(full.solution().strategy().get(node as usize))
                );
                let evs = |s: &SavedSolution| bits(s.ev(node));
                assert_eq!(evs(&saved), evs(&full), "{name}, node {node}");
            } else {
                assert!(!loaded.holds(node));
                assert!(loaded.strategy().get(node as usize).is_none());
                assert!(loaded.hand_strategy(node, 0).is_none());
                assert!(loaded.range_view_frequencies(node).is_none());
                assert!(loaded.range_view_classes(node).is_none());
                assert!(loaded.action_frequencies(node).is_none());
                assert!(loaded.hand_weights(node).is_none());
                assert!(loaded.range_view_weights(node).is_none());
                assert!(saved.ev(node).is_none());
            }
        }
        // A turn node reads as before.
        let turn = node(game, turn_path);
        assert_eq!(
            loaded.range_view_frequencies(turn),
            full.solution().range_view_frequencies(turn)
        );
        // The reach of the first river node is known, and below it is not.
        let river = node(game, river_path);
        assert_eq!(game.street(river), Street::River);
        assert!(!loaded.holds(river));
        assert_eq!(loaded.reach(river), full.solution().reach(river));
        assert_eq!(
            loaded.reach_probability(river).to_bits(),
            full.solution().reach_probability(river).to_bits()
        );
        let below = game.children(river)[0];
        let error = catch_unwind(AssertUnwindSafe(|| loaded.reach(below))).unwrap_err();
        let message = error.downcast_ref::<String>().unwrap();
        assert!(message.contains("does not hold"), "{message}");

        // Loading the complete file up to the turn gives the same, with the complete header.
        let first = SavedSolution::load_streets(&whole, Street::Turn).unwrap();
        assert_eq!(first.last_street(), Street::Turn);
        // Asked for less than the spot's first street, it loads that street.
        let least = SavedSolution::load_streets(&whole, Street::Flop).unwrap();
        assert_eq!(least.last_street(), spot.street());
        assert!(least.solution().holds(0));
        assert!(first.header().is_complete());
        for node in 0..game.num_nodes() {
            assert_eq!(
                bits(first.solution().strategy().get(node)),
                bits(loaded.strategy().get(node))
            );
            assert_eq!(bits(first.ev(node as u32)), bits(saved.ev(node as u32)));
        }

        // Nothing measures a partial strategy: it cannot be saved, its check reports the
        // nodes it does not hold, and its comparison is of the streets it holds.
        let again = dir.join("again.pksol");
        let error = save(loaded, &again, &options).unwrap_err();
        assert!(
            matches!(
                error,
                SolutionFileError::Incomplete {
                    last_street: Street::Turn
                }
            ),
            "{error}"
        );
        assert!(!again.exists() && !part(&again).exists());
        assert!(!check_solution(loaded).is_valid());
        let mapped = MappedSolution::from_solution(loaded);
        assert!(!mapped.is_complete());
        assert!(mapped.exploitability().is_none());
        assert_eq!(mapped.streets(), expected.len());
        assert!(mapped.holds(turn) && !mapped.holds(river));
        let whole_mapped = MappedSolution::from_solution(full.solution());
        let comparison = compare_mapped(&whole_mapped, &mapped, &Tolerances::default());
        assert!(comparison.passes(), "{name}:\n{comparison}");
        // The complete solution saved without its river gives the partial file's bytes.
        save(full.solution(), &again, &options).unwrap();
        assert!(fs::read(&again).unwrap() == fs::read(&partial).unwrap());
    }
}

#[test]
fn a_river_spot_keeps_its_one_street_and_a_flop_file_can_keep_the_flop_alone() {
    let spot = reference("river/dry_ip_polarized");
    let solution = solved(&spot, &SolveOptions::default());
    let dir = dir("first_street");
    // A river spot holds the river whatever the options, which may not ask for less.
    let error = save(
        &solution,
        dir.join("x.pksol"),
        &SaveOptions {
            last_street: Street::Turn,
        },
    )
    .unwrap_err();
    assert!(
        matches!(
            error,
            SolutionFileError::Options {
                last_street: Street::Turn,
                first: Street::River
            }
        ),
        "{error}"
    );
    assert!(error.to_string().contains("starts on the river"), "{error}");
    // A flop spot can keep its flop alone.
    let solution = solved(&data("flop_tiny"), &SolveOptions::default());
    let path = dir.join("flop.pksol");
    save(
        &solution,
        &path,
        &SaveOptions {
            last_street: Street::Flop,
        },
    )
    .unwrap();
    let saved = SavedSolution::load(&path).unwrap();
    assert_eq!(saved.header().streets, [Street::Flop]);
    assert_eq!(saved.last_street(), Street::Flop);
    assert!(saved.solution().holds(0));
    let turn = node(saved.solution().game(), "x x 8h");
    assert!(!saved.solution().holds(turn));
}

// ---------------------------------------------------------------------------------------------
// Files that cannot be read

/// The parts of a file: its header's JSON and its sections' bytes.
struct Parts {
    header: serde_json::Value,
    sections: Vec<Vec<u8>>,
}

fn parts(bytes: &[u8]) -> Parts {
    let length = u32::from_le_bytes(bytes[7..11].try_into().unwrap()) as usize;
    let header: serde_json::Value = serde_json::from_slice(&bytes[11..11 + length]).unwrap();
    let mut at = 11 + length;
    let sections = header["sections"]
        .as_array()
        .unwrap()
        .iter()
        .map(|section| {
            let length = section["length"].as_u64().unwrap() as usize;
            at += length;
            bytes[at - length..at].to_vec()
        })
        .collect();
    Parts { header, sections }
}

/// A file of `parts`, with the header's lengths set to the sections'.
fn file(parts: &Parts) -> Vec<u8> {
    let mut header = parts.header.clone();
    for (section, bytes) in header["sections"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .zip(&parts.sections)
    {
        section["length"] = bytes.len().into();
    }
    let text = serde_json::to_string(&header).unwrap();
    let mut out = b"PKSOL".to_vec();
    out.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
    out.extend_from_slice(&(text.len() as u32).to_le_bytes());
    out.extend_from_slice(text.as_bytes());
    for section in &parts.sections {
        out.extend_from_slice(section);
    }
    out
}

fn compress(raw: &[u8]) -> Vec<u8> {
    let mut encoder = zstd::stream::write::Encoder::new(Vec::new(), 3).unwrap();
    encoder.include_checksum(true).unwrap();
    encoder.include_contentsize(true).unwrap();
    encoder
        .set_pledged_src_size(Some(raw.len() as u64))
        .unwrap();
    encoder.write_all(raw).unwrap();
    encoder.finish().unwrap()
}

/// Loads `bytes` written to a file, and gives the error, with the error of reading the header
/// alone when that fails too.
fn refused(dir: &Path, bytes: &[u8]) -> SolutionFileError {
    let path = dir.join("broken.pksol");
    fs::write(&path, bytes).unwrap();
    match catch_unwind(|| SavedSolution::load(&path)) {
        Ok(Ok(_)) => panic!("a broken file was loaded"),
        Ok(Err(error)) => error,
        Err(_) => panic!("loading a broken file panicked"),
    }
}

#[test]
fn every_way_a_file_can_be_broken_gives_an_error_that_says_what_is_wrong() {
    let spot = data("turn_small");
    let solution = solved(&spot, &SolveOptions::default());
    let dir = dir("broken");
    let path = dir.join("good.pksol");
    save(&solution, &path, &SaveOptions::default()).unwrap();
    let good = fs::read(&path).unwrap();
    let message = |error: &SolutionFileError| error.to_string();

    // Not a solution file.
    for bytes in [
        &b""[..],
        b"PKS",
        b"{\"format\": \"pk-solution-dump\"}",
        b"PKSOX\x01\x00",
    ] {
        let error = refused(&dir, bytes);
        assert!(
            matches!(error, SolutionFileError::NotASolutionFile { .. }),
            "{error}"
        );
        assert!(
            message(&error).contains("is not a solution file"),
            "{error}"
        );
    }

    // Another format version, named with the one supported.
    let mut bytes = good.clone();
    bytes[5..7].copy_from_slice(&2u16.to_le_bytes());
    let error = refused(&dir, &bytes);
    assert!(
        matches!(
            error,
            SolutionFileError::Version {
                found: 2,
                supported: 1,
                ..
            }
        ),
        "{error}"
    );
    assert!(
        message(&error).contains("format version 2; this solver reads version 1 only"),
        "{error}"
    );

    // Cut short anywhere: in the preamble, in the header, in each section.
    let original = parts(&good);
    let header_end = good.len() - original.sections.iter().map(Vec::len).sum::<usize>();
    let mut cuts = vec![6, 9, 20, header_end - 1];
    let mut end = header_end;
    for section in &original.sections {
        cuts.push(end + section.len() / 2);
        end += section.len();
        cuts.push(end - 1);
    }
    for cut in cuts {
        let error = refused(&dir, &good[..cut]);
        assert!(
            matches!(error, SolutionFileError::Truncated { .. }),
            "cut at {cut}: {error}"
        );
        assert!(message(&error).contains("is cut short"), "{error}");
    }
    let error = refused(&dir, &good[..header_end + 3]);
    assert!(
        message(&error).contains("in section `strategy.turn`, 3 bytes into"),
        "{error}"
    );
    // And one that goes on.
    let mut bytes = good.clone();
    bytes.extend_from_slice(b"more");
    let error = refused(&dir, &bytes);
    assert!(
        matches!(error, SolutionFileError::TrailingBytes { extra: 4, .. }),
        "{error}"
    );

    // A header too long, not JSON, or without a key, each named.
    let mut bytes = good.clone();
    bytes[7..11].copy_from_slice(&(MAX_HEADER_BYTES + 1).to_le_bytes());
    let error = refused(&dir, &bytes);
    assert!(message(&error).contains("more than the 1048576"), "{error}");
    let mut bytes = good.clone();
    bytes[11] = b'[';
    let error = refused(&dir, &bytes);
    assert!(message(&error).contains("is not valid JSON"), "{error}");
    let mut header = original.header.clone();
    header["solve"]
        .as_object_mut()
        .unwrap()
        .remove("iterations");
    let error = refused(
        &dir,
        &file(&Parts {
            header,
            sections: original.sections.clone(),
        }),
    );
    assert!(
        message(&error).contains("the header's `solve.iterations` is missing"),
        "{error}"
    );

    // A spot that is not a spot.
    let mut header = original.header.clone();
    header["spot"] = "format = 1\nname = \"x\"\n".into();
    let error = refused(
        &dir,
        &file(&Parts {
            header,
            sections: original.sections.clone(),
        }),
    );
    assert!(matches!(error, SolutionFileError::Spot { .. }), "{error}");

    // A spot edited so that its tree is another: a river bet of half the pot, not 0.75.
    let mut header = original.header.clone();
    let text = header["spot"].as_str().unwrap().to_string();
    let edited = text.replace("river = { bets = [0.75]", "river = { bets = [0.5]");
    assert_ne!(edited, text);
    header["spot"] = edited.into();
    let error = refused(
        &dir,
        &file(&Parts {
            header,
            sections: original.sections.clone(),
        }),
    );
    assert!(
        matches!(error, SolutionFileError::TreeMismatch { .. }),
        "{error}"
    );
    let text = message(&error);
    let version = env!("CARGO_PKG_VERSION");
    assert!(
        text.contains(&format!(
            "was written by pk-postflop {version} for a tree that pk-postflop {version} builds \
             differently"
        )),
        "{text}"
    );
    // A spot edited so that its tree is larger than the file's says so too. (One edited so
    // that it builds the same tree, such as a raise cap the stacks never reach, is read as
    // the file it is.)
    let mut header = original.header.clone();
    let edited =
        text_of(&original.header).replace("turn = { bets = [0.5]", "turn = { bets = [0.25, 0.5]");
    assert_ne!(edited, text_of(&original.header));
    header["spot"] = edited.into();
    let error = refused(
        &dir,
        &file(&Parts {
            header,
            sections: original.sections.clone(),
        }),
    );
    assert!(
        matches!(error, SolutionFileError::TreeMismatch { .. }),
        "{error}"
    );
    assert!(message(&error).contains("nodes here"), "{error}");
    // A tree hash edited.
    let mut header = original.header.clone();
    header["tree"]["hash"] = "0123456789abcdef".into();
    let error = refused(
        &dir,
        &file(&Parts {
            header,
            sections: original.sections.clone(),
        }),
    );
    assert!(
        matches!(error, SolutionFileError::TreeMismatch { .. }),
        "{error}"
    );

    // A raw length that does not fit the tree.
    let mut header = original.header.clone();
    header["sections"][1]["raw_length"] = 8.into();
    let error = refused(
        &dir,
        &file(&Parts {
            header,
            sections: original.sections.clone(),
        }),
    );
    assert!(
        message(&error).contains("section `ev.turn` holds 8 bytes by the header"),
        "{error}"
    );

    // Every byte of every section changed in turn: each is refused, naming its section, unless
    // the frame decodes to the same bytes, which its checksum vouches for.
    let reference = SavedSolution::load(&path).unwrap();
    // The frame's header and first block's header, its checksum, and 40 bytes spread between:
    // each load builds the game, so every byte of a 40 kB file would take minutes.
    let mut accepted = 0;
    for (index, section) in original.sections.iter().enumerate() {
        let name = original.header["sections"][index]["name"].as_str().unwrap();
        let mut positions: Vec<usize> = (0..16).chain(section.len() - 8..section.len()).collect();
        positions.extend((0..40).map(|i| 16 + i * (section.len() - 24) / 40));
        for at in positions {
            let mut sections = original.sections.clone();
            sections[index][at] ^= 0x41;
            let broken = dir.join("broken.pksol");
            fs::write(
                &broken,
                file(&Parts {
                    header: original.header.clone(),
                    sections,
                }),
            )
            .unwrap();
            match catch_unwind(|| SavedSolution::load(&broken)) {
                Err(_) => panic!("byte {at} of `{name}`: loading panicked"),
                Ok(Err(SolutionFileError::Section { name: n, .. })) if n == name => {}
                Ok(Err(error)) => panic!("byte {at} of `{name}`: {error}"),
                Ok(Ok(loaded)) => {
                    let strategy = |s: &SavedSolution| {
                        let game = s.solution().game();
                        player_nodes(game)
                            .map(|node| {
                                let strategy = bits(s.solution().strategy().get(node as usize));
                                (strategy, bits(s.ev(node)))
                            })
                            .collect::<Vec<_>>()
                    };
                    assert!(
                        strategy(&loaded) == strategy(&reference),
                        "byte {at} of `{name}`"
                    );
                    accepted += 1;
                }
            }
        }
    }
    println!("{accepted} changed bytes decoded to the same sections");

    // A section of two frames, of a frame without its checksum, and of codes that do not sum
    // to 65,535, though each a valid frame.
    let raw = zstd::bulk::decompress(&original.sections[0], 1 << 24).unwrap();
    let mut sections = original.sections.clone();
    sections[0] = [compress(&raw[..2]), compress(&raw[2..])].concat();
    let error = refused(
        &dir,
        &file(&Parts {
            header: original.header.clone(),
            sections,
        }),
    );
    assert!(message(&error).contains("followed by"), "{error}");
    let mut sections = original.sections.clone();
    sections[0] = zstd::bulk::compress(&raw, 3).unwrap();
    let error = refused(
        &dir,
        &file(&Parts {
            header: original.header.clone(),
            sections,
        }),
    );
    assert!(message(&error).contains("without its checksum"), "{error}");
    let mut changed = raw.clone();
    changed[0] = changed[0].wrapping_add(1);
    let mut sections = original.sections.clone();
    sections[0] = compress(&changed);
    let error = refused(
        &dir,
        &file(&Parts {
            header: original.header.clone(),
            sections,
        }),
    );
    assert!(message(&error).contains("sum to"), "{error}");
    // And an EV that is infinite.
    let raw = zstd::bulk::decompress(&original.sections[1], 1 << 24).unwrap();
    let mut changed = raw.clone();
    changed[..4].copy_from_slice(&f32::INFINITY.to_le_bytes());
    let mut sections = original.sections.clone();
    sections[1] = compress(&changed);
    let error = refused(
        &dir,
        &file(&Parts {
            header: original.header.clone(),
            sections,
        }),
    );
    assert!(message(&error).contains("is infinite"), "{error}");

    // The good file, taken apart and put back together by these helpers, is still good.
    let path = dir.join("rebuilt.pksol");
    fs::write(&path, file(&original)).unwrap();
    SavedSolution::load(&path).unwrap();
}

fn text_of(header: &serde_json::Value) -> String {
    header["spot"].as_str().unwrap().to_string()
}

// ---------------------------------------------------------------------------------------------
// Rivers solved again

/// Every first decision of a river in `game`: the children of the chance nodes that deal it
/// which are player nodes.
fn river_starts(game: &PostflopGame) -> Vec<u32> {
    (0..game.num_nodes() as u32)
        .filter(|&node| {
            matches!(game.node(node as usize), PublicNode::Chance { .. })
                && game.street(node) == Street::River
        })
        .flat_map(|chance| game.children(chance).to_vec())
        .filter(|&child| game.actor(child).is_some())
        .collect()
}

/// Checks that the river spot's tree is the full tree's subtree at its node, action for
/// action, with the payoffs moved by what was put in before, and that its ranges are the range
/// weights times the reaches.
fn check_river_tree(saved: &SavedSolution, river: &RiverSpot) {
    let full = saved.solution().game();
    let node = river.node;
    let game = PostflopGame::from_spot(&river.spot).unwrap();
    assert_eq!(game.board(0), full.board(node));
    assert_eq!(river.put_in[0], river.put_in[1]);
    let put_in = f64::from(river.put_in[0]);
    for local in 0..game.num_nodes() as u32 {
        let at = node + local;
        assert_eq!(game.actor(local), full.actor(at), "node {at}");
        assert_eq!(game.actions(local), full.actions(at), "node {at}");
        let children: Vec<u32> = full.children(at).iter().map(|c| c - node).collect();
        assert_eq!(game.children(local), children.as_slice(), "node {at}");
        assert_eq!(game.board(local), full.board(at));
        if let (Some(mine), Some(theirs)) = (game.payoff(local), full.payoff(at)) {
            match (mine, theirs) {
                (Payoff::Fold(mine), Payoff::Fold(theirs)) => {
                    for player in 0..2 {
                        assert!((mine[player] - put_in - theirs[player]).abs() < 1e-9);
                    }
                }
                (Payoff::Showdown(mine), Payoff::Showdown(theirs)) => {
                    assert!((mine.win - put_in - theirs.win).abs() < 1e-9);
                    assert!((mine.tie - put_in - theirs.tie).abs() < 1e-9);
                    assert!((mine.lose - put_in - theirs.lose).abs() < 1e-9);
                }
                other => panic!("node {at}: {other:?}"),
            }
        }
        assert_eq!(game.payoff(local).is_some(), full.payoff(at).is_some());
    }
    // The full subtree ends where the river spot's tree does.
    let end = node + game.num_nodes() as u32;
    assert!(
        end as usize == full.num_nodes() || !full.path(end).iter().any(|&(at, _)| at == node),
        "the subtree at {node} has more nodes than the river spot's tree"
    );
    // What was put in before is what the check-down below the node shows.
    let check = full.children(full.children(node)[0])[0];
    let Node::Terminal { contributions, .. } = full.tree().node(full.tree_node(check)) else {
        panic!("checking down the river ends the hand");
    };
    assert_eq!(*contributions, river.put_in);
    // The ranges: the range weight times the own reach, in f32, of the hands that reach it.
    let reach = saved.solution().reach(node);
    for player in [Player::Oop, Player::Ip] {
        let hands = full.hands(player);
        let range = river.spot.range(player);
        for (hand, &combo) in hands.combos().iter().enumerate() {
            let weight = (hands.weights()[hand] * reach[player.index()][hand]) as f32;
            assert_eq!(range.weight(combo), weight, "{player}, {combo}");
        }
        assert_eq!(game.hands(player).len(), range.num_combos());
    }
}

#[test]
fn a_river_spot_builds_the_full_trees_subtree_at_its_node() {
    // Under the uniform strategy every river is reached. The all-in threshold is restated for
    // the river's stack, so the trees agree under thresholds that convert bets as well.
    for threshold in [1.0, 0.6, 0.3] {
        let mut spot = data("turn_small");
        spot.tree.allin_threshold = threshold;
        let options = SolveOptions {
            target: None,
            max_iterations: 0,
            ..SolveOptions::default()
        };
        let solution = solved(&spot, &options);
        let dir = dir(&format!("river_tree_{threshold}"));
        let path = dir.join("x.pksol");
        save(
            &solution,
            &path,
            &SaveOptions {
                last_street: Street::Turn,
            },
        )
        .unwrap();
        let saved = SavedSolution::load(&path).unwrap();
        let starts = river_starts(saved.solution().game());
        // Fewer at a lower threshold, which turns more turn bets into all-in.
        assert!(starts.len() >= 48, "{}", starts.len());
        let mut converted = false;
        for &node in &starts {
            let river = saved.river_spot(node).unwrap();
            assert!(river.spot.validate().is_ok());
            check_river_tree(&saved, &river);
            converted |= river.spot.tree.allin_threshold < 1.0;
        }
        assert_eq!(converted, threshold < 1.0);
    }
    // On the flop, with chips put in on the flop or the turn, under the uniform strategy.
    let options = SolveOptions {
        target: None,
        max_iterations: 0,
        ..SolveOptions::default()
    };
    let solution = solved(&data("flop_small"), &options);
    let path = dir("river_tree_flop").join("x.pksol");
    save(
        &solution,
        &path,
        &SaveOptions {
            last_street: Street::Turn,
        },
    )
    .unwrap();
    let saved = SavedSolution::load(&path).unwrap();
    let game = saved.solution().game();
    let mut checked = 0;
    let mut put_in = std::collections::BTreeSet::new();
    for node in river_starts(game).into_iter().step_by(97) {
        match saved.river_spot(node) {
            Ok(river) => {
                check_river_tree(&saved, &river);
                put_in.insert(river.put_in[0]);
                checked += 1;
            }
            Err(SolutionFileError::NotRiverStart { reason, .. }) => {
                assert!(reason.contains("never reached"), "{reason}");
            }
            Err(error) => panic!("{error}"),
        }
    }
    assert!(checked > 50, "{checked}");
    // Checked through, or a half-pot bet called on the flop or on the turn.
    assert_eq!(put_in.into_iter().collect::<Vec<_>>(), [0, 500]);
}

#[test]
fn a_river_spot_solved_again_agrees_with_the_full_solve_within_their_exploitability() {
    // Rake-free, so that the game is constant-sum and each player's value under a strategy is
    // within what the strategies' exploitability allows of the game's value.
    let mut spot = data("turn_small");
    spot.rake_percent = 0.0;
    spot.rake_cap = 0;
    let options = SolveOptions {
        target: Some(0.05),
        ..SolveOptions::default()
    };
    let solution = solved(&spot, &options);
    let dir = dir("river_values");
    let (whole, partial) = (dir.join("whole.pksol"), dir.join("partial.pksol"));
    save(&solution, &whole, &SaveOptions::default()).unwrap();
    save(
        &solution,
        &partial,
        &SaveOptions {
            last_street: Street::Turn,
        },
    )
    .unwrap();
    let complete = SavedSolution::load(&whole).unwrap();
    let saved = SavedSolution::load(&partial).unwrap();
    let full = complete.solution().game();
    // Rivers after the turn checks through, a blank and an ace, and after a turn bet is called,
    // by OOP and by IP.
    let mut compared = 0;
    for path in ["x x 2c", "x x Ac", "b50 c 5s", "x b50 c Qh"] {
        let node = node(full, path);
        let river = saved.river_spot(node).unwrap();
        assert_eq!(river, complete.river_spot(node).unwrap());
        let game = PostflopGame::from_spot(&river.spot).unwrap();
        // The full solve's strategy on the river, on the river spot's hands.
        let stored = complete.solution().strategy();
        let mapped = PublicProfile::from_fn(&game, |local, hand| {
            let actor = game.actor(local as u32).unwrap();
            let combo = game.hands(actor).combos()[hand];
            let theirs = full.hands(actor).position(combo).unwrap();
            stored.hand(node as usize + local, theirs).unwrap()
        });
        // Its EVs at the node are the file's, counted from the river's start.
        let evs = node_values(&game, &mapped);
        let file = complete.ev(node).unwrap();
        let put_in = f64::from(river.put_in[0]);
        for (hand, &combo) in game.hands(Player::Oop).combos().iter().enumerate() {
            let theirs = full.hands(Player::Oop).position(combo).unwrap();
            match evs.get(0).unwrap()[hand] {
                Some(ev) => assert!(
                    (ev - put_in - f64::from(file[theirs])).abs() < 1e-3,
                    "node {node}, {combo}: {ev} − {put_in} against {}",
                    file[theirs]
                ),
                None => assert!(file[theirs].is_nan()),
            }
        }
        // Solved again, its values are within what the two exploitabilities allow.
        let measured = exploitability(&game, &mapped);
        let again = solve(
            &river.spot,
            &SolveOptions {
                target: Some(0.01),
                ..SolveOptions::default()
            },
            |_| {},
        )
        .unwrap();
        let resolved = again.exploitability();
        let [a0, a1] = measured.gains();
        let [b0, b1] = resolved.gains();
        let bound = (a1 + b0).max(a0 + b1) + 1e-9;
        let difference = (measured.values[0] - resolved.values[0]).abs();
        println!(
            "node {node} `{}`: values {:?} and {:?}, {difference:.4} apart, bound {bound:.4}",
            full.node_path(node),
            measured.values,
            resolved.values
        );
        assert!(difference <= bound, "{difference} against {bound}");
        assert!(
            (measured.values[1] - resolved.values[1]).abs() <= bound,
            "IP: {measured:?} against {resolved:?}"
        );
        compared += 1;
    }
    assert_eq!(compared, 4);
}

#[test]
fn a_river_spot_is_refused_where_no_river_starts_or_nothing_reaches() {
    let solution = solved(&data("turn_small"), &SolveOptions::default());
    let path = dir("river_refused").join("x.pksol");
    save(&solution, &path, &SaveOptions::default()).unwrap();
    let saved = SavedSolution::load(&path).unwrap();
    let game = saved.solution().game();
    let refuse = |node: u32| match saved.river_spot(node) {
        Err(SolutionFileError::NotRiverStart { reason, .. }) => reason,
        other => panic!("node {node}: {other:?}"),
    };
    // The root, a chance node, a river node below the first, and a node out of range.
    let river = node(game, "x x 3c");
    let chance = node(game, "x x");
    for node in [0, chance, game.children(river)[0]] {
        assert!(refuse(node).contains("is not the first decision of a river"));
    }
    assert!(refuse(game.num_nodes() as u32).contains("out of range"));
    // A river that one player never reaches under the solve's strategy.
    let unreached = river_starts(game).into_iter().find(|&node| {
        let reach = saved.solution().reach(node);
        reach.iter().any(|r| r.iter().all(|&x| x == 0.0))
    });
    if let Some(node) = unreached {
        assert!(refuse(node).contains("never reached"));
    }
    // On a river spot the root is its own river spot, with the same hands and weights.
    let spot = reference("river/four_flush");
    let solution = solved(&spot, &SolveOptions::default());
    let path = dir("river_root").join("x.pksol");
    save(&solution, &path, &SaveOptions::default()).unwrap();
    let saved = SavedSolution::load(&path).unwrap();
    let river = saved.river_spot(0).unwrap();
    assert_eq!(river.put_in, [0, 0]);
    let game = PostflopGame::from_spot(&river.spot).unwrap();
    for player in [Player::Oop, Player::Ip] {
        assert_eq!(game.hands(player), solution.hands(player));
    }
    assert_eq!(
        (river.spot.board, river.spot.pot, river.spot.effective_stack),
        (spot.board, spot.pot, spot.effective_stack)
    );
    assert_eq!(river.spot.tree, spot.tree);
    assert_eq!(tree_hash(&game), saved.header().tree_hash);
}
