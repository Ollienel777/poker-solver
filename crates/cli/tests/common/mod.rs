//! What the command line's tests share: running the `solver` binary from a folder, what a run
//! printed and how it ended, the test inputs, folders of their own to write in, and the check of
//! a node's JSON, as `inspect --json` and `solver serve` give it, against the library.

// Each test crate uses some of these.
#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use pk_core::HandClass;
use pk_game::Player;
use pk_postflop::Solution;
use serde_json::Value;

pub const RANGES: &str = "reference/ranges/a/BTN_vs_BB_SRP.toml";
pub const RIVER_TREE: &str = "crates/cli/tests/data/river_tree.toml";
pub const NARROW_RANGES: &str = "crates/cli/tests/data/narrow_ranges.toml";
pub const TOY: &str = "reference/spots/river/toy_polarized.toml";
pub const TURN: &str = "crates/postflop/tests/data/turn_small.toml";

/// The repository's root.
pub fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// What a run printed and how it ended.
pub struct Run {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

impl Run {
    /// Panics with everything the run printed unless it succeeded.
    pub fn succeeded(self) -> Run {
        assert_eq!(
            self.code, 0,
            "stdout:\n{}\nstderr:\n{}",
            self.stdout, self.stderr
        );
        assert_eq!(
            self.stderr, "",
            "a run that succeeds prints nothing to stderr"
        );
        self
    }

    /// Panics unless the run failed with `code` and one error, `solver: <message>`; gives the
    /// message.
    pub fn failed(&self, code: i32) -> String {
        assert_eq!(
            self.code, code,
            "stdout:\n{}\nstderr:\n{}",
            self.stdout, self.stderr
        );
        let message = self
            .stderr
            .strip_prefix("solver: ")
            .unwrap_or_else(|| panic!("an error starts with `solver: `: {}", self.stderr));
        if code == 1 {
            assert_eq!(message.lines().count(), 1, "one line: {}", self.stderr);
        }
        message.trim_end().to_string()
    }

    /// Panics unless stdout holds every one of `lines`, each a whole line.
    pub fn prints(&self, lines: &[&str]) {
        for line in lines {
            assert!(
                self.stdout.lines().any(|printed| printed == *line),
                "no line {line:?} in\n{}",
                self.stdout
            );
        }
    }

    /// The first line of stdout that starts with `label`.
    pub fn line(&self, label: &str) -> &str {
        self.stdout
            .lines()
            .find(|line| line.starts_with(label))
            .unwrap_or_else(|| panic!("no line starting {label:?} in\n{}", self.stdout))
    }
}

pub fn solver_in(folder: &Path, args: &[&str]) -> Run {
    let output = Command::new(env!("CARGO_BIN_EXE_solver"))
        .args(args)
        .current_dir(folder)
        .output()
        .expect("the solver binary runs");
    let run = Run {
        code: output.status.code().expect("the solver exits with a code"),
        stdout: String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        stderr: String::from_utf8(output.stderr).expect("stderr is UTF-8"),
    };
    println!("solver {}\n{}{}", args.join(" "), run.stdout, run.stderr);
    run
}

/// Runs the solver from the repository's root.
pub fn solver(args: &[&str]) -> Run {
    solver_in(&root(), args)
}

/// A fresh, empty folder under the system's temporary folder.
pub fn scratch(name: &str) -> PathBuf {
    let folder = std::env::temp_dir().join(format!("pk-cli-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&folder);
    fs::create_dir_all(&folder).unwrap();
    folder
}

/// A path as an argument.
pub fn arg(path: &Path) -> &str {
    path.to_str().expect("a temporary path is UTF-8")
}

/// The numbers of an array of JSON numbers.
pub fn f64s(value: &Value) -> Vec<f64> {
    value
        .as_array()
        .unwrap_or_else(|| panic!("an array: {value}"))
        .iter()
        .map(|x| x.as_f64().expect("a number"))
        .collect()
}

/// Checks the JSON of a decision, `node` as `inspect --json` and `solver serve` give it, against
/// the library's numbers at the player node `at` of `solution`, exactly: how often play reaches
/// it, scaled by `reach_scale`, each range in combos, each action's two frequencies, every
/// class's share, frequencies, combos and EV, and the breakdown if there is one. The acting
/// player's EV with hand `h` is `ev(h)`, in chips from the start of the spot.
pub fn check_json_decision(
    node: &Value,
    solution: &Solution,
    at: u32,
    ev: &dyn Fn(usize) -> Option<f64>,
    reach_scale: f64,
) {
    let game = solution.game();
    let actor = game.actor(at).expect("a decision");
    assert_eq!(node["kind"], "decision");
    assert_eq!(node["actor"], ["oop", "ip"][actor.index()]);
    assert_eq!(
        node["reached"].as_f64(),
        Some(reach_scale * solution.reach_probability(at))
    );
    let reach = solution.reach(at);
    let combos: Vec<f64> = [Player::Oop, Player::Ip]
        .iter()
        .map(|&player| {
            let weights = solution.hands(player).weights();
            weights
                .iter()
                .zip(&reach[player.index()])
                .map(|(w, r)| w * r)
                .sum()
        })
        .collect();
    assert_eq!(f64s(&node["combos"]), combos);

    let taken = solution.action_frequencies(at).unwrap();
    let range = solution.range_view_frequencies(at).unwrap();
    let actions = node["actions"].as_array().unwrap();
    assert_eq!(actions.len(), game.actions(at).len());
    let path = node["path"].as_str().unwrap();
    for (index, (action, shown)) in game.actions(at).iter().zip(actions).enumerate() {
        let token = action.to_string();
        assert_eq!(shown["token"], token.as_str());
        let next = if path.is_empty() {
            token.clone()
        } else {
            format!("{path} {token}")
        };
        assert_eq!(shown["path"], next.as_str());
        assert_eq!(shown["frequency"].as_f64(), Some(taken[index]), "{token}");
        assert_eq!(
            shown["range_frequency"].as_f64(),
            Some(range[index]),
            "{token}"
        );
    }

    let weights = solution.hands(actor).weights();
    let weight = |hand: usize| weights[hand] * reach[actor.index()][hand];
    let classes = solution.range_view_classes(at).unwrap();
    let shown = node["classes"].as_array().unwrap();
    assert_eq!(shown.len(), 169);
    for (index, cell) in shown.iter().enumerate() {
        let class = HandClass::from_index(index);
        assert_eq!(cell["class"], class.to_string().as_str());
        let Some(view) = classes.iter().find(|view| view.class == class) else {
            assert_eq!(cell["in_range"], false, "{class}");
            assert!(cell["frequencies"].is_null(), "{class}");
            continue;
        };
        assert_eq!(cell["in_range"], true, "{class}");
        assert_eq!(cell["share"].as_f64(), Some(view.share), "{class}");
        assert_eq!(f64s(&cell["frequencies"]), view.frequencies, "{class}");
        let combos: f64 = view.hands.iter().map(|&h| weight(h)).sum();
        assert_eq!(cell["combos"].as_f64(), Some(combos), "{class}");
        let (mut total, mut sum) = (0.0, 0.0);
        for &hand in &view.hands {
            let w = weight(hand);
            if let (true, Some(ev)) = (w > 0.0, ev(hand)) {
                total += w;
                sum += w * ev;
            }
        }
        let expected = (total > 0.0).then(|| sum / total);
        assert_eq!(cell["ev"].as_f64(), expected, "{class}");
    }

    if node["hand"].is_null() {
        return;
    }
    let hands = solution.hands(actor);
    for row in node["hand"]["rows"].as_array().unwrap() {
        let combo = row["combo"].as_str().unwrap().parse().unwrap();
        let hand = hands
            .position(combo)
            .expect("a row is of a hand in the range");
        assert_eq!(row["weight"].as_f64(), Some(weight(hand)), "{combo}");
        let strategy: Vec<f64> = solution
            .hand_strategy(at, hand)
            .unwrap()
            .into_iter()
            .map(f64::from)
            .collect();
        assert_eq!(f64s(&row["strategy"]), strategy, "{combo}");
        assert_eq!(row["ev"].as_f64(), ev(hand), "{combo}");
    }
}
