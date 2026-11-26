//! The quantizer, the tree hash and the header's reader. The round trips, the errors a file can
//! cause and the river spots are in `tests/solution_file.rs`.

use proptest::prelude::*;

use super::*;

/// A row of `k` probabilities that sum to 1 as closely as `f32` allows: random weights
/// normalised in `f64`, then rounded, with some exact zeros and ones among them.
fn row() -> impl Strategy<Value = Vec<f32>> {
    (1usize..=9)
        .prop_flat_map(|k| prop::collection::vec(prop_oneof![Just(0.0f64), 0.0f64..1.0], k))
        .prop_map(|weights| {
            let total: f64 = weights.iter().sum();
            if total == 0.0 {
                let mut row = vec![0.0; weights.len()];
                row[weights.len() - 1] = 1.0;
                return row;
            }
            weights.iter().map(|w| (w / total) as f32).collect()
        })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(20_000))]

    #[test]
    fn codes_sum_to_65535_and_stay_within_a_step(row in row()) {
        let codes = quantize(&row).expect("a distribution is quantized");
        prop_assert_eq!(codes.iter().map(|&c| u32::from(c)).sum::<u32>(), 65_535);
        for (&code, &p) in codes.iter().zip(&row) {
            // The code within one step of the product, and its probability within 1/65,535.
            prop_assert!((f64::from(code) - 65_535.0 * f64::from(p)).abs() < 1.0);
            let decoded = dequantize(code);
            prop_assert!(
                (f64::from(decoded) - f64::from(p)).abs() <= 1.0 / 65_535.0,
                "{p} became {code}, {decoded}"
            );
        }
    }

    #[test]
    fn quantizing_a_decoded_row_gives_back_its_codes(row in row()) {
        let codes = quantize(&row).unwrap();
        let decoded: Vec<f32> = codes.iter().map(|&c| dequantize(c)).collect();
        prop_assert_eq!(quantize(&decoded).unwrap(), codes);
    }

    #[test]
    fn any_codes_that_sum_to_65535_come_back(
        cuts in prop::collection::vec(0u32..=65_535, 0..8)
    ) {
        // Codes from cutting 0..=65,535 at random points: every row of codes a file can hold.
        let mut points = cuts;
        points.push(0);
        points.push(65_535);
        points.sort_unstable();
        let codes: Vec<u16> = points.windows(2).map(|w| (w[1] - w[0]) as u16).collect();
        let decoded: Vec<f32> = codes.iter().map(|&c| dequantize(c)).collect();
        prop_assert_eq!(quantize(&decoded).unwrap(), codes);
    }
}

#[test]
fn the_largest_remainders_get_the_missing_codes_and_ties_go_to_the_lower_action() {
    // Thirds: 21,845 each, exactly.
    let third = 1.0f32 / 3.0;
    assert_eq!(quantize(&[third; 3]).unwrap(), [21_845; 3]);
    // Halves: 32,767.5 each, one code missing, and the tie goes to the first.
    assert_eq!(quantize(&[0.5, 0.5]).unwrap(), [32_768, 32_767]);
    // A quarter each: 16,383.75 each, three codes missing, which go to the first three.
    assert_eq!(
        quantize(&[0.25; 4]).unwrap(),
        [16_384, 16_384, 16_384, 16_383]
    );
    // The largest remainder wins over the lower index. In f32, 0.1, 0.2 and 0.7 times 65,535
    // are 6,553.500098, 13,107.000195 and 45,874.499219: the floors fall one short, and the
    // code goes to the first, whose remainder is the largest.
    assert_eq!(quantize(&[0.1, 0.2, 0.7]).unwrap(), [6_554, 13_107, 45_874]);
    // With the order changed, it still goes to the largest remainder, now the last action.
    assert_eq!(quantize(&[0.7, 0.2, 0.1]).unwrap(), [45_874, 13_107, 6_554]);
    // Pure strategies are exact.
    assert_eq!(quantize(&[0.0, 1.0, 0.0]).unwrap(), [0, 65_535, 0]);
    assert_eq!(dequantize(65_535), 1.0);
    assert_eq!(dequantize(0), 0.0);
}

#[test]
fn a_row_that_is_not_a_distribution_is_refused() {
    for row in [
        vec![],
        vec![0.5, 0.4],
        vec![0.6, 0.6],
        vec![1.5, -0.5],
        vec![f32::NAN, 1.0],
        vec![f32::INFINITY],
        vec![0.5, 0.500_01],
    ] {
        assert_eq!(quantize(&row), None, "{row:?}");
    }
    // Within the tolerance of a solve's rounding, a row is quantized.
    assert!(quantize(&[0.5, 0.500_000_6]).is_some());
}

#[test]
fn fnv_1a_is_the_published_function() {
    // The test vectors of the FNV reference: the empty string and "a", "foobar".
    for (bytes, expected) in [
        (&b""[..], 0xcbf2_9ce4_8422_2325u64),
        (b"a", 0xaf63_dc4c_8601_ec8c),
        (b"foobar", 0x8594_4171_f739_67e8),
    ] {
        let mut hash = Fnv::new();
        hash.bytes(bytes);
        assert_eq!(hash.0, expected, "{bytes:?}");
    }
}

#[test]
fn a_sections_bytes_do_not_depend_on_how_its_raw_bytes_are_written() {
    // 3 MB, more than a frame's window at level 3, of codes that are partly runs and partly
    // noise, as a strategy's are, from a fixed xorshift.
    let mut state = 0x9e37_79b9_7f4a_7c15u64;
    let raw: Vec<u8> = (0..1_500_000)
        .flat_map(|i| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            let code = if i % 5 < 2 {
                65_535
            } else {
                (state % 65_536) as u16
            };
            code.to_le_bytes()
        })
        .collect();
    let written = |chunk: usize| {
        let mut encoder = section_encoder(raw.len() as u64).unwrap();
        for piece in raw.chunks(chunk) {
            encoder.write_all(piece).unwrap();
        }
        let bytes = encoder.finish().unwrap();
        // The room it was made with was never outgrown.
        assert_eq!(bytes.capacity() as u64, section_room(raw.len() as u64));
        bytes
    };
    let whole = written(raw.len());
    for chunk in [2, 7, 4_096, 100_003, 131_072, 1 << 21] {
        assert!(written(chunk) == whole, "written {chunk} bytes at a time");
    }
    // And it is one frame of the raw bytes.
    assert_eq!(zstd::bulk::decompress(&whole, raw.len()).unwrap(), raw);
}

fn spot(name: &str) -> Spot {
    Spot::load(format!(
        "{}/tests/data/{name}.toml",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

#[test]
fn the_tree_hash_sees_every_change_the_layout_depends_on() {
    let base = spot("turn_small");
    let hash = |spot: &Spot| tree_hash(&PostflopGame::from_spot(spot).unwrap());
    let original = hash(&base);
    // The same spot built again, and with another name and description, hash the same.
    assert_eq!(hash(&base), original);
    let mut renamed = base.clone();
    renamed.name = "another".into();
    renamed.description = "Another description.".into();
    assert_eq!(hash(&renamed), original);
    // Every change to the tree, the hands, the weights, the pot or the rake changes it.
    let mut changes: Vec<(&str, Spot)> = Vec::new();
    let mut spot = base.clone();
    spot.tree.river.as_mut().unwrap().bets = vec![0.5];
    changes.push(("a river bet size", spot));
    let mut spot = base.clone();
    spot.tree.max_raises = 0;
    changes.push(("the raise cap", spot));
    let mut spot = base.clone();
    spot.effective_stack += 100;
    changes.push(("the stack", spot));
    let mut spot = base.clone();
    spot.pot += 100;
    changes.push(("the pot", spot));
    let mut spot = base.clone();
    spot.rake_cap += 1;
    changes.push(("the rake cap", spot));
    let mut spot = base.clone();
    spot.rake_percent = 4.5;
    changes.push(("the rake percent", spot));
    let mut spot = base.clone();
    spot.oop_range.set("KsQs".parse().unwrap(), 0.75);
    changes.push(("a weight", spot));
    let mut spot = base.clone();
    spot.ip_range.set("AcAd".parse().unwrap(), 1.0);
    changes.push(("a hand", spot));
    let mut spot = base.clone();
    spot.board = "Kh9c7d3s".parse().unwrap();
    changes.push(("the board", spot));
    for (what, spot) in changes {
        assert_ne!(hash(&spot), original, "{what}");
    }
}

#[test]
fn the_header_reader_names_the_field_it_refuses() {
    let solver = r#""solver": {"name": "pk-postflop", "version": "0.1.0"}"#;
    let tree = r#""tree": {"hash": "00000000000000ff", "nodes": 10, "player_nodes": 4}"#;
    let hands = r#""hands": {"oop": 3, "ip": 2}"#;
    let e = r#"{"values": [1.0, 2.0], "best_responses": [1.5, 2.5], "chips": 0.5, "percent_of_pot": 0.1}"#;
    let solve = format!(r#""solve": {{"stop": "target", "iterations": 5, "exploitability": {e}}}"#);
    let sections = r#""sections": [{"name": "strategy.river", "length": 1, "raw_length": 2}, {"name": "ev.river", "length": 3, "raw_length": 4}]"#;
    let header = |streets: &str, extra: &str| {
        format!(
            r#"{{{solver}, "spot": "s", {tree}, {hands}, "streets": {streets}, {solve}, "exploitability": {e}, {sections}{extra}}}"#
        )
    };
    let fields = read::fields(&header(r#"["river"]"#, "")).ok().unwrap();
    assert_eq!(fields.tree_hash, 0xff);
    assert_eq!(fields.hands, [3, 2]);
    assert_eq!(fields.streets, [Street::River]);
    assert_eq!(fields.iterations, 5);
    assert_eq!(fields.exploitability.best_responses, [1.5, 2.5]);
    assert_eq!(fields.sections[1].raw_length, 4);
    let refused = |text: &str| match read::fields(text) {
        Ok(_) => panic!("accepted: {text}"),
        Err(invalid) => (invalid.field, invalid.reason),
    };
    let cases: Vec<(String, &str)> = vec![
        ("{".to_string(), ""),
        ("[]".to_string(), ""),
        (header(r#"["river"]"#, r#", "extra": 1"#), "extra"),
        (header(r#"["turn"]"#, ""), "sections[0].name"),
        (header(r#"["turn", "flop"]"#, ""), "streets[1]"),
        (header("[]", ""), "streets"),
        (header(r#"["preflop"]"#, ""), "streets[0]"),
        (
            header(r#"["river"]"#, "").replace(r#""nodes": 10"#, r#""nodes": -1"#),
            "tree.nodes",
        ),
        (
            header(r#"["river"]"#, "").replace("00000000000000ff", "ff"),
            "tree.hash",
        ),
        (
            header(r#"["river"]"#, "").replace(r#""ip": 2"#, r#""ip": "2""#),
            "hands.ip",
        ),
        (header(r#"["river"]"#, "").replace(r#""ip": 2"#, ""), ""),
        (
            header(r#"["river"]"#, "").replace(r#""stop": "target""#, r#""stop": "done""#),
            "solve.stop",
        ),
        (
            header(r#"["river"]"#, "").replacen("[1.0, 2.0]", "[1.0]", 1),
            "solve.exploitability.values",
        ),
        (
            header(r#"["river"]"#, "").replace(
                r#", {"name": "ev.river", "length": 3, "raw_length": 4}"#,
                "",
            ),
            "sections",
        ),
        (
            header(r#"["river"]"#, "").replace(r#""spot": "s", "#, r#""spot": "s", "spot": "t", "#),
            "",
        ),
    ];
    for (text, field) in cases {
        let (found, reason) = refused(&text);
        assert_eq!(found, field, "{text}: {reason}");
    }
    // A missing key is named.
    let (field, reason) = refused(&header(r#"["river"]"#, "").replace(&format!("{hands}, "), ""));
    assert_eq!((field.as_str(), reason.as_str()), ("hands", "is missing"));
}
