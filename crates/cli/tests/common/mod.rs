//! What the command line's tests share: running the `solver` binary from a folder, what a run
//! printed and how it ended, the test inputs, and folders of their own to write in.

// Each test crate uses some of these.
#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

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
