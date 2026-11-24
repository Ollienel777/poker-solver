use std::path::PathBuf;

use pk_core::{Combo, HandClass, NUM_CLASSES, NUM_COMBOS};
use proptest::prelude::*;

use super::*;
use crate::position::Position::*;
use crate::rules::round_chips;

// ---------------------------------------------------------------------------------------------
// Helpers

/// A valid file, in the layout the writer produces. The error tests change one thing in it.
const BASE: &str = r#"format = 1
config = "a"
formation = "BTN_vs_BB_SRP"
line = "UTG f, UTG1 f, LJ f, HJ f, CO f, BTN r2.5, SB f, BB c"
pot_bb = 5.5
effective_stack_bb = 97.5
source = "test file"

[oop]
position = "BB"
range = "22-JJ,A2s-AJs,QQ:0.5"

[ip]
position = "BTN"
range = "22+,A2s+,A2o-A6o:0.5"
"#;

fn repo_path(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
}

fn config(name: &str) -> GameConfig {
    let path = repo_path(&format!("configs/{name}.toml"));
    GameConfig::load(&path).unwrap_or_else(|err| panic!("{} should load: {err}", path.display()))
}

fn read(text: &str) -> RangeFile {
    RangeFile::from_toml(text).unwrap_or_else(|err| panic!("{text}\nshould load: {err}"))
}

fn range(text: &str) -> Range {
    text.parse()
        .unwrap_or_else(|err| panic!("{text:?} should parse: {err}"))
}

fn player(position: Position, text: &str) -> PlayerRange {
    PlayerRange {
        position,
        range: range(text),
    }
}

fn base_file() -> RangeFile {
    read(BASE)
}

/// `BASE` with the first `from` replaced by `to`.
fn base_with(from: &str, to: &str) -> String {
    assert!(BASE.contains(from), "test setup: {from:?} is not in BASE");
    BASE.replacen(from, to, 1)
}

/// The field and reason of the error that loading `text` gives.
fn rejected(text: &str) -> (String, String) {
    match RangeFile::from_toml(text) {
        Err(RangeFileError::Invalid { field, reason }) => {
            println!("`{field}` {reason}");
            (field, reason)
        }
        other => panic!("expected an invalid-file error, got {other:?} for\n{text}"),
    }
}

/// The field that `BASE` is rejected for once `from` is replaced by `to`.
fn rejected_field(from: &str, to: &str) -> String {
    rejected(&base_with(from, to)).0
}

/// The field and reason of the error that checking `file` against `config` gives.
fn check_rejected(file: &RangeFile, config: &GameConfig) -> (String, String) {
    match file.check(config) {
        Err(RangeFileError::Invalid { field, reason }) => {
            println!("`{field}` {reason}");
            (field, reason)
        }
        other => panic!("expected an invalid-file error, got {other:?} for {file:?}"),
    }
}

/// Writes the file, reads it back and writes it again, and checks that nothing changed.
fn assert_round_trip(file: &RangeFile) {
    let text = file.to_toml();
    let reread = read(&text);
    assert_eq!(&reread, file, "{text}");
    assert_eq!(reread.to_toml(), text);
}

// ---------------------------------------------------------------------------------------------
// Reading and writing

#[test]
fn the_base_file_reads_as_written_and_writes_back_unchanged() {
    let file = base_file();
    assert_eq!(file.config, "a");
    assert_eq!(file.formation, "BTN_vs_BB_SRP");
    assert_eq!(
        file.line,
        "UTG f, UTG1 f, LJ f, HJ f, CO f, BTN r2.5, SB f, BB c"
    );
    assert_eq!(file.pot, 550);
    assert_eq!(file.effective_stack, 9750);
    assert_eq!(file.source, "test file");
    assert_eq!(file.oop, player(Bb, "22-JJ,A2s-AJs,QQ:0.5"));
    assert_eq!(file.ip, player(Btn, "22+,A2s+,A2o-A6o:0.5"));
    assert_eq!(file.player(Player::Oop), &file.oop);
    assert_eq!(file.player(Player::Ip), &file.ip);

    assert_eq!(file.to_toml(), BASE);
    assert_eq!(file.to_string(), BASE);
}

#[test]
fn a_hand_typed_file_reads_the_same_and_writes_in_the_canonical_layout() {
    // Keys out of order, `[ip]` first, comments, lowercase positions, a literal string and
    // ranges typed in no particular order.
    let typed = r#"
# Typed from a range view.
format = 1
source = 'test file'
formation = "BTN_vs_BB_SRP"
config = "a"
line = "UTG f, UTG1 f, LJ f, HJ f, CO f, BTN r2.5, SB f, BB c"
effective_stack_bb = 97.5   # after the 2.5bb open and call
pot_bb = 5.5

[ip]
range = "A2o-A6o:0.5, A2s+, 22+"
position = "btn"

[oop]
position = "bb"
range = "QQ:0.5, 22-TT, JJ, A2s, A3s, A4s, A5s, A6s-AJs"
"#;
    let file = read(typed);
    assert_eq!(file, base_file());
    assert_eq!(file.to_toml(), BASE);

    // An amount may be typed as an integer. It is written back as a decimal.
    let whole = read(&base_with("pot_bb = 5.5", "pot_bb = 6"));
    assert_eq!(whole.pot, 600);
    assert_eq!(whole.to_toml(), base_with("pot_bb = 5.5", "pot_bb = 6.0"));
}

#[test]
fn writing_then_reading_gives_an_equal_file() {
    let base = base_file();
    let files = [
        base.clone(),
        // Weights at several levels, and combos whose class is split.
        RangeFile {
            oop: player(Sb, "AA:0.125,KK:0.3333,AKs,AsKs:0.75,AhKd:0.01,72o:1e-3"),
            ip: player(Bb, "22+:0.5,A2s+:0.9,KQo,KhQh:0.2"),
            ..base.clone()
        },
        // One combo per side, and amounts that need both decimal places.
        RangeFile {
            pot: 1,
            effective_stack: 7,
            oop: player(Utg, "AsAh"),
            ip: player(Co, "2c2d:0.5"),
            ..base.clone()
        },
        RangeFile {
            pot: 1225,
            effective_stack: MAX_CHIPS,
            oop: player(Utg2, "AA"),
            ip: player(Btn, &Range::full().to_string()),
            ..base.clone()
        },
        // Text that needs escaping.
        RangeFile {
            config: "my \"config\"".into(),
            formation: "back\\slash".into(),
            line: "two\nlines\tand a tab\u{7f}\u{1}".into(),
            source: "ünïcödé, 'quotes', and a # that is not a comment".into(),
            ..base.clone()
        },
    ];
    for file in &files {
        assert_round_trip(file);
    }
}

#[test]
fn amounts_are_written_as_exact_decimals() {
    for (chips, text) in [
        (1, "0.01"),
        (7, "0.07"),
        (50, "0.5"),
        (550, "5.5"),
        (1225, "12.25"),
        (9000, "90.0"),
        (10_000, "100.0"),
        (MAX_CHIPS, "1000000.0"),
    ] {
        assert_eq!(decimal_bb(chips), text);
        let file = RangeFile {
            pot: chips,
            ..base_file()
        };
        assert!(file.to_toml().contains(&format!("\npot_bb = {text}\n")));
        assert_eq!(read(&file.to_toml()).pot, chips);
    }
}

fn any_weight() -> impl Strategy<Value = f32> {
    prop_oneof![
        4 => Just(0.0f32),
        2 => Just(1.0f32),
        1 => Just(0.5f32),
        1 => 0.0f32..=1.0f32,
    ]
}

/// A range with weights by class, a few combos set on their own, and at least one hand.
fn any_range() -> impl Strategy<Value = Range> {
    let classes = prop::collection::vec(any_weight(), NUM_CLASSES);
    let combos = prop::collection::vec((0..NUM_COMBOS, any_weight()), 0..6);
    (classes, combos, 0..NUM_COMBOS).prop_map(|(classes, combos, fallback)| {
        let mut range = Range::empty();
        for (index, weight) in classes.into_iter().enumerate() {
            range.set_class(HandClass::from_index(index), weight);
        }
        for (index, weight) in combos {
            range.set(Combo::from_index(index), weight);
        }
        if range.is_empty() {
            range.set(Combo::from_index(fallback), 1.0);
        }
        range
    })
}

/// Any text at all, after a letter so that it is never blank.
fn any_text() -> impl Strategy<Value = String> {
    any::<String>().prop_map(|text| format!("x{text}"))
}

fn any_file() -> impl Strategy<Value = RangeFile> {
    let texts = (any_text(), any_text(), any_text(), any_text());
    let amounts = (1..=MAX_CHIPS, 1..=MAX_CHIPS);
    let seats = (0..9usize, 1..9usize);
    (texts, amounts, seats, any_range(), any_range()).prop_map(
        |((config, formation, line, source), (pot, effective_stack), (oop, offset), a, b)| {
            let table = Position::table(9).unwrap();
            RangeFile {
                config,
                formation,
                line,
                pot,
                effective_stack,
                source,
                oop: PlayerRange {
                    position: table[oop],
                    range: a,
                },
                ip: PlayerRange {
                    position: table[(oop + offset) % 9],
                    range: b,
                },
            }
        },
    )
}

proptest! {
    #[test]
    fn any_valid_file_round_trips(file in any_file()) {
        let text = file.to_toml();
        let reread = RangeFile::from_toml(&text);
        prop_assert!(reread.is_ok(), "{:?}\n{}", reread, text);
        let reread = reread.unwrap();
        prop_assert_eq!(&reread, &file);
        prop_assert_eq!(reread.to_toml(), text);
    }
}

// ---------------------------------------------------------------------------------------------
// Load errors

#[test]
fn the_format_must_be_1() {
    let (field, reason) = rejected(&base_with("format = 1", "format = 2"));
    assert_eq!(field, "format");
    assert!(reason.contains("format 1 only"), "{reason}");
    assert_eq!(rejected_field("format = 1", "format = \"1\""), "format");
    assert_eq!(rejected_field("format = 1", "format = 1.0"), "format");
    assert_eq!(rejected_field("format = 1\n", ""), "format");
    // The version comes first, so a file from a later format is reported as one, not by the
    // keys that format adds.
    assert_eq!(
        rejected_field("format = 1", "format = 2\nboard = \"Ks7d2c\""),
        "format"
    );
}

#[test]
fn every_field_is_required() {
    for (line, field) in [
        ("config = \"a\"\n", "config"),
        ("formation = \"BTN_vs_BB_SRP\"\n", "formation"),
        (
            "line = \"UTG f, UTG1 f, LJ f, HJ f, CO f, BTN r2.5, SB f, BB c\"\n",
            "line",
        ),
        ("pot_bb = 5.5\n", "pot_bb"),
        ("effective_stack_bb = 97.5\n", "effective_stack_bb"),
        ("source = \"test file\"\n", "source"),
        ("position = \"BB\"\n", "oop.position"),
        ("range = \"22-JJ,A2s-AJs,QQ:0.5\"\n", "oop.range"),
        ("position = \"BTN\"\n", "ip.position"),
        ("range = \"22+,A2s+,A2o-A6o:0.5\"\n", "ip.range"),
    ] {
        let (rejected_as, reason) = rejected(&base_with(line, ""));
        assert_eq!(rejected_as, field);
        assert_eq!(reason, "is missing");
    }

    let no_oop = base_with(
        "[oop]\nposition = \"BB\"\nrange = \"22-JJ,A2s-AJs,QQ:0.5\"\n",
        "",
    );
    assert_eq!(rejected(&no_oop).0, "oop");
    let no_ip = &BASE[..BASE.find("\n[ip]").unwrap()];
    assert_eq!(rejected(no_ip).0, "ip");
}

#[test]
fn unknown_keys_are_rejected() {
    let (field, reason) = rejected(&base_with("pot_bb", "board = \"Ks7d2c\"\npot_bb"));
    assert_eq!(field, "board");
    assert!(reason.contains("`effective_stack_bb`"), "{reason}");
    assert_eq!(
        rejected_field("range = \"22-JJ", "weights = \"1\"\nrange = \"22-JJ"),
        "oop.weights"
    );
    assert_eq!(
        rejected_field("range = \"22+", "rake = 0.05\nrange = \"22+"),
        "ip.rake"
    );
    // A misspelled key is reported as itself, not as the key it was meant to be.
    assert_eq!(
        rejected_field("effective_stack_bb", "efective_stack_bb"),
        "efective_stack_bb"
    );
    // Keys are case-sensitive.
    assert_eq!(rejected_field("[oop]", "[OOP]"), "OOP");
    assert_eq!(rejected_field("pot_bb", "Pot_bb"), "Pot_bb");
}

#[test]
fn positions_must_be_known_and_different() {
    let (field, reason) = rejected(&base_with("position = \"BB\"", "position = \"MP\""));
    assert_eq!(field, "oop.position");
    assert!(
        reason.contains("`MP`") && reason.contains("UTG1"),
        "{reason}"
    );
    for bad in ["\"\"", "\"BU\"", "3"] {
        assert_eq!(
            rejected_field("position = \"BTN\"", &format!("position = {bad}")),
            "ip.position"
        );
    }

    let (field, reason) = rejected(&base_with("position = \"BTN\"", "position = \"bb\""));
    assert_eq!(field, "ip.position");
    assert!(reason.contains("same seat"), "{reason}");
}

#[test]
fn ranges_must_parse_with_weights_from_0_to_1() {
    // The range parser's own message follows the field.
    for (bad, message) in [
        (
            "AA:1.5",
            "`1.5` is not a weight: expected a number from 0 to 1",
        ),
        ("AA:-0.5", "`-0.5` is not a weight"),
        ("KK,AA:2", "`2` is not a weight"),
        ("AKx", "`AKx` is not a hand"),
        ("A2s-K5s", "`A2s-K5s` is not a span of classes"),
        ("AA,,KK", "the range has an empty item"),
    ] {
        let (field, reason) = rejected(&base_with("22-JJ,A2s-AJs,QQ:0.5", bad));
        assert_eq!(field, "oop.range");
        assert_eq!(
            reason,
            format!("is not a valid range: {}", parse_error(bad))
        );
        assert!(reason.contains(message), "{reason}");
    }
    assert_eq!(
        rejected_field("22+,A2s+,A2o-A6o:0.5", "22+:1.01"),
        "ip.range"
    );
    assert_eq!(
        rejected_field("range = \"22+,A2s+,A2o-A6o:0.5\"", "range = [\"22+\"]"),
        "ip.range"
    );
}

fn parse_error(text: &str) -> String {
    text.parse::<Range>().unwrap_err().to_string()
}

#[test]
fn a_range_must_hold_a_hand() {
    for empty in ["", "  ", "AA:0", "AA,AA:0"] {
        let (field, reason) = rejected(&base_with("22-JJ,A2s-AJs,QQ:0.5", empty));
        assert_eq!(field, "oop.range");
        assert!(reason.contains("no hands"), "{reason}");
    }
}

#[test]
fn amounts_must_be_positive_whole_chips() {
    for bad in [
        "5.505",
        "0",
        "0.0",
        "-0.0",
        "1e-9",
        "-5.5",
        "nan",
        "inf",
        "1000000.01",
        "\"5.5\"",
        "true",
    ] {
        assert_eq!(
            rejected_field("pot_bb = 5.5", &format!("pot_bb = {bad}")),
            "pot_bb",
            "{bad}"
        );
    }
    for bad in ["97.501", "0", "-1", "1e7"] {
        let line = format!("effective_stack_bb = {bad}");
        assert_eq!(
            rejected_field("effective_stack_bb = 97.5", &line),
            "effective_stack_bb",
            "{bad}"
        );
    }
    let (_, reason) = rejected(&base_with("pot_bb = 5.5", "pot_bb = 5.505"));
    assert!(reason.contains("not a whole number of chips"), "{reason}");

    // The smallest and largest amounts load.
    assert_eq!(read(&base_with("pot_bb = 5.5", "pot_bb = 0.01")).pot, 1);
    let largest = base_with("pot_bb = 5.5", "pot_bb = 1000000");
    assert_eq!(read(&largest).pot, MAX_CHIPS);
}

#[test]
fn text_fields_must_be_strings_and_not_blank() {
    assert_eq!(rejected_field("config = \"a\"", "config = 1"), "config");
    assert_eq!(rejected_field("config = \"a\"", "config = \"\""), "config");
    assert_eq!(
        rejected_field("formation = \"BTN_vs_BB_SRP\"", "formation = true"),
        "formation"
    );
    let line = BASE
        .lines()
        .find(|line| line.starts_with("line = "))
        .unwrap();
    assert_eq!(rejected_field(line, "line = \"  \""), "line");
    let (field, reason) = rejected(&base_with("source = \"test file\"", "source = \"\""));
    assert_eq!((field.as_str(), reason.as_str()), ("source", "is blank"));
    assert_eq!(
        rejected_field("source = \"test file\"", "source = \"\t \""),
        "source"
    );
}

#[test]
fn oop_and_ip_must_be_tables() {
    let flat = BASE[..BASE.find("\n[oop]").unwrap()].to_string() + "oop = \"BB\"\nip = \"BTN\"\n";
    let (field, reason) = rejected(&flat);
    assert_eq!(field, "oop");
    assert!(reason.contains("must be a table"), "{reason}");
}

#[test]
fn malformed_toml_is_a_toml_error() {
    let repeated_table = base_with("[ip]", "[oop]");
    for text in [
        "format = ",
        "format = 1\nformat = 1",
        "[oop\nposition = \"BB\"",
        &repeated_table,
    ] {
        assert!(
            matches!(RangeFile::from_toml(text), Err(RangeFileError::Toml(_))),
            "{text}"
        );
    }
}

#[test]
fn a_missing_file_is_an_io_error() {
    let error = RangeFile::load("reference/ranges/does-not-exist.toml").unwrap_err();
    assert!(matches!(error, RangeFileError::Io { .. }));
    assert!(error.to_string().contains("does-not-exist.toml"));
}

#[test]
fn errors_name_the_field_in_their_message() {
    let text = base_with("A2o-A6o:0.5", "A2o-A6o:1.5");
    assert_eq!(
        RangeFile::from_toml(&text).unwrap_err().to_string(),
        "`ip.range` is not a valid range: `1.5` is not a weight: expected a number from 0 to 1"
    );
}

// ---------------------------------------------------------------------------------------------
// Checking against a config

#[test]
fn the_base_file_passes_its_config() {
    base_file().check(&config("a")).unwrap();
    // The config is named in any case.
    let upper = RangeFile {
        config: "A".into(),
        ..base_file()
    };
    upper.check(&config("a")).unwrap();
}

#[test]
fn the_config_must_be_the_one_the_file_names() {
    let (field, reason) = check_rejected(&base_file(), &config("b"));
    assert_eq!(field, "config");
    assert!(reason.contains("named B"), "{reason}");
}

#[test]
fn positions_must_exist_at_the_table() {
    let file = RangeFile {
        oop: player(Utg2, "AA"),
        ..base_file()
    };
    let (field, reason) = check_rejected(&file, &config("a"));
    assert_eq!(field, "oop.position");
    assert!(reason.contains("config A does not seat"), "{reason}");

    let file = RangeFile {
        ip: player(Utg2, "AA"),
        ..base_file()
    };
    assert_eq!(check_rejected(&file, &config("a")).0, "ip.position");
}

#[test]
fn swapped_sides_are_caught() {
    let swapped = BASE
        .replacen("position = \"BB\"", "position = \"TEMP\"", 1)
        .replacen("position = \"BTN\"", "position = \"BB\"", 1)
        .replacen("position = \"TEMP\"", "position = \"BTN\"", 1);
    let file = read(&swapped);
    assert_eq!((file.oop.position, file.ip.position), (Btn, Bb));
    let (field, reason) = check_rejected(&file, &config("a"));
    assert_eq!(field, "oop.position");
    assert!(reason.contains("swapped"), "{reason}");

    // After the flop the SB acts first and the BTN last, whatever the preflop order: the
    // straddler closes the action before the flop in config C, and is still out of position
    // to every seat from UTG1 on after it.
    for (oop, ip) in [
        (Sb, Bb),
        (Bb, Utg),
        (Utg, Utg1),
        (Utg, Btn),
        (Sb, Btn),
        (Co, Btn),
    ] {
        for name in ["a", "c"] {
            let game = config(name);
            // Each player has put in 10bb, and the pot holds every blind and ante as well.
            let file = RangeFile {
                config: name.into(),
                effective_stack: game.live_stack() - 1000,
                pot: game.dead_money() + 2000 + 500,
                oop: player(oop, "AA"),
                ip: player(ip, "KK"),
                ..base_file()
            };
            file.check(&game)
                .unwrap_or_else(|err| panic!("{oop} vs {ip} under {name}: {err}"));
            let swapped = RangeFile {
                oop: file.ip.clone(),
                ip: file.oop.clone(),
                ..file
            };
            assert_eq!(check_rejected(&swapped, &game).0, "oop.position");
        }
    }
}

#[test]
fn the_effective_stack_must_leave_room_for_the_blind() {
    let a = config("a");
    // At most 100 - 1 = 99bb is left once a player has called the big blind.
    let limped = RangeFile {
        effective_stack: 9900,
        pot: 250,
        ..base_file()
    };
    limped.check(&a).unwrap();
    for effective_stack in [9901, 10_000, 20_000] {
        let too_deep = RangeFile {
            effective_stack,
            ..limped.clone()
        };
        assert_eq!(check_rejected(&too_deep, &a).0, "effective_stack_bb");
    }

    // Under config C a player who sees the flop has called at least the 2bb straddle from a
    // 99.5bb live stack.
    let file = RangeFile {
        config: "c".into(),
        effective_stack: 9751,
        pot: 2000,
        ..base_file()
    };
    let (field, reason) = check_rejected(&file, &config("c"));
    assert_eq!(field, "effective_stack_bb");
    assert!(reason.contains("at most 97.5bb"), "{reason}");
}

#[test]
fn the_pot_must_fit_the_stack_and_the_forced_bets() {
    let a = config("a");
    // With 2.5bb put in by each player, the pot holds their 5bb and the SB's 0.5bb at least, and
    // at most 2.5bb from each of the eight seats.
    for (pot, passes) in [(549, false), (550, true), (2000, true), (2001, false)] {
        let file = RangeFile { pot, ..base_file() };
        if passes {
            file.check(&a).unwrap();
        } else {
            assert_eq!(check_rejected(&file, &a).0, "pot_bb", "{pot}");
        }
    }

    // Config A's amounts for this spot are wrong under config C, where 4bb of antes, the SB and
    // the straddle are in the pot as well.
    let file = RangeFile {
        config: "c".into(),
        ..base_file()
    };
    let (field, reason) = check_rejected(&file, &config("c"));
    assert_eq!(field, "pot_bb");
    assert!(reason.contains("from 10.5bb"), "{reason}");
}

#[test]
fn a_file_built_in_code_gets_the_same_checks() {
    let a = config("a");
    let base = base_file();
    let cases = [
        (
            RangeFile {
                source: " ".into(),
                ..base.clone()
            },
            "source",
        ),
        (
            RangeFile {
                pot: 0,
                ..base.clone()
            },
            "pot_bb",
        ),
        (
            RangeFile {
                effective_stack: MAX_CHIPS + 1,
                ..base.clone()
            },
            "effective_stack_bb",
        ),
        (
            RangeFile {
                oop: PlayerRange {
                    position: Bb,
                    range: Range::empty(),
                },
                ..base.clone()
            },
            "oop.range",
        ),
        (
            RangeFile {
                ip: player(Bb, "AA"),
                ..base.clone()
            },
            "ip.position",
        ),
    ];
    for (file, field) in cases {
        assert_eq!(check_rejected(&file, &a).0, field);
    }
}

// ---------------------------------------------------------------------------------------------
// The files in the repository

/// Every file under `reference/ranges/`, which is laid out as `<config>/<formation>.toml`.
fn reference_files() -> Vec<PathBuf> {
    let root = repo_path("reference/ranges");
    let mut files = Vec::new();
    for entry in fs::read_dir(&root).unwrap() {
        let folder = entry.unwrap().path();
        assert!(
            folder.is_dir(),
            "{} is not a config folder",
            folder.display()
        );
        for entry in fs::read_dir(&folder).unwrap() {
            let path = entry.unwrap().path();
            assert_eq!(
                path.extension().and_then(|extension| extension.to_str()),
                Some("toml"),
                "{} is not a range file",
                path.display()
            );
            files.push(path);
        }
    }
    files.sort();
    files
}

#[test]
fn every_reference_file_loads_checks_and_rewrites_stably() {
    let files = reference_files();
    assert!(files.len() >= 4, "found only {files:?}");
    for path in files {
        let shown = path.display();
        let file = RangeFile::load(&path).unwrap_or_else(|err| panic!("{shown}: {err}"));

        let folder = path
            .parent()
            .unwrap()
            .file_name()
            .unwrap()
            .to_str()
            .unwrap();
        let stem = path.file_stem().unwrap().to_str().unwrap();
        assert_eq!(
            file.config, folder,
            "{shown}: `config` must match the folder"
        );
        assert_eq!(
            file.formation, stem,
            "{shown}: `formation` must match the file name"
        );

        file.check(&config(&file.config))
            .unwrap_or_else(|err| panic!("{shown}: {err}"));

        // Writing what was read, then reading and writing that, gives the same text.
        let written = file.to_toml();
        assert_eq!(read(&written), file, "{shown}");
        assert_eq!(read(&written).to_toml(), written, "{shown}");
    }
}

#[test]
fn the_reference_examples_add_up() {
    // Each example's preflop line, as the live chips every seat put in, blinds included. The
    // sizes come from the config: an open of `open` times the effective blind, and a 3-bet out
    // of position of `threebet_oop` times the open. Antes are added from the config.
    let a = config("a");
    let a_open = round_chips(a.preflop.open * f64::from(a.effective_blind()));
    let a_threebet = round_chips(a.preflop.threebet_oop * f64::from(a_open));
    let c = config("c");
    let c_open = round_chips(c.preflop.open * f64::from(c.effective_blind()));
    assert_eq!((a_open, a_threebet, c_open), (250, 1000, 500));

    let examples = [
        // BTN opens to 2.5bb, SB folds, BB calls.
        (
            "a/BTN_vs_BB_SRP",
            &a,
            vec![(Sb, 50), (Bb, a_open), (Btn, a_open)],
        ),
        // BTN opens to 2.5bb, SB folds, BB 3-bets to 10bb, BTN calls.
        (
            "a/BB_vs_BTN_3BP",
            &a,
            vec![(Sb, 50), (Bb, a_threebet), (Btn, a_threebet)],
        ),
        // BTN opens to 5bb over the 2bb straddle, SB folds, BB calls, UTG folds.
        (
            "c/BTN_vs_BB_SRP",
            &c,
            vec![(Sb, 50), (Bb, c_open), (Utg, 200), (Btn, c_open)],
        ),
        // BTN opens to 5bb, both blinds fold, UTG calls from the straddle.
        (
            "c/BTN_vs_UTG_SRP",
            &c,
            vec![(Sb, 50), (Bb, 100), (Utg, c_open), (Btn, c_open)],
        ),
    ];
    for (name, config, put_in) in examples {
        let file = RangeFile::load(repo_path(&format!("reference/ranges/{name}.toml"))).unwrap();
        let chips = |position: Position| {
            put_in
                .iter()
                .find(|&&(p, _)| p == position)
                .map_or(0, |&(_, chips)| chips)
        };
        for &position in config.positions() {
            assert!(
                chips(position) >= config.blind(position),
                "{name}: {position}"
            );
        }
        let (oop, ip) = (chips(file.oop.position), chips(file.ip.position));
        assert_eq!(oop, ip, "{name}: both players put in the same");

        let pot = config.dead_money() + put_in.iter().map(|&(_, chips)| chips).sum::<Chips>();
        assert_eq!(file.pot, pot, "{name}");
        assert_eq!(file.effective_stack, config.live_stack() - oop, "{name}");
        assert_eq!(file.source, "illustrative, not from a reference", "{name}");
    }
}

#[test]
fn the_architecture_example_loads_and_checks() {
    let doc = fs::read_to_string(repo_path("docs/ARCHITECTURE.md"))
        .unwrap()
        .replace("\r\n", "\n");
    let section = &doc[doc.find("### Range file").expect("the section exists")..];
    let start = section
        .find("```toml\n")
        .expect("the section has an example")
        + "```toml\n".len();
    let example = &section[start..start + section[start..].find("```").unwrap()];
    let file = read(example);
    file.check(&config(&file.config)).unwrap();
    assert_round_trip(&file);
}
