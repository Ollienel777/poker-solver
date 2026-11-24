//! Running an external solver as a separate program that writes a [solution dump](crate::dump).
//!
//! Another solver's code is never linked into this project or copied into it: it is only run,
//! as a program of its own, and its output read. This module knows nothing about any
//! particular solver. An [`ExternalSolver`] is a command template, a program and its
//! arguments, in which `{spot}` stands for the path of a spot file ([`crate::spot`]) and
//! `{out}` for the path the program must write its dump to. Each may be part of a longer
//! argument, as in `--out={out}`.
//!
//! [`ExternalSolver::run`] fills in the two paths, removes any file already at the output path,
//! starts the program, and waits for it up to the timeout, after which it is killed. A program
//! that exits with a failure, or succeeds without writing a dump, is an error that carries the
//! end of its standard error and standard output. Otherwise the dump is read with
//! [`SolutionDump::load`] and returned. Checking that it answers the spot is left to
//! [`MappedSolution::from_dump`](crate::MappedSolution::from_dump).
//!
//! # Configuration
//!
//! [`ExternalSolver::from_env`] reads the template from the environment variable
//! [`SOLVER_ENV`], `PK_REFERENCE_SOLVER`, as a JSON array of strings whose first is the
//! program: for example `["C:/tools/ref-wrapper.exe", "--spot", "{spot}", "--out", "{out}"]`.
//! When it is not set there is no reference solver, so that tests which need one can skip on
//! a machine without it.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use thiserror::Error;

use crate::dump::{DumpError, SolutionDump};

/// The environment variable that holds the reference solver's command template.
pub const SOLVER_ENV: &str = "PK_REFERENCE_SOLVER";

/// The placeholder for the spot file's path.
pub const SPOT_PLACEHOLDER: &str = "{spot}";

/// The placeholder for the path the dump is written to.
pub const OUT_PLACEHOLDER: &str = "{out}";

/// The most bytes of each output stream an error keeps: the end, where a failure is reported.
const KEPT: usize = 8 * 1024;

/// How long to wait for the output streams to close once the program has exited. A program
/// that leaves a child running with its streams open must not hang the caller.
const DRAIN: Duration = Duration::from_secs(5);

#[derive(Debug, Error)]
pub enum RunError {
    /// The command template is not usable.
    #[error("the reference solver's command {reason}")]
    Template { reason: String },
    /// The program could not be started.
    #[error("could not start {}: {source}", .program.display())]
    Spawn {
        program: PathBuf,
        source: std::io::Error,
    },
    /// A file at the output path could not be removed before the run.
    #[error("could not remove the earlier output {}: {source}", .path.display())]
    Clear {
        path: PathBuf,
        source: std::io::Error,
    },
    /// The program ran past the timeout and was killed.
    #[error(
        "{} did not finish within {timeout:?} and was stopped{}",
        .program.display(),
        tail(.stderr, .stdout)
    )]
    Timeout {
        program: PathBuf,
        timeout: Duration,
        stdout: String,
        stderr: String,
    },
    /// The program exited with a failure.
    #[error("{} failed ({status}){}", .program.display(), tail(.stderr, .stdout))]
    Failed {
        program: PathBuf,
        status: ExitStatus,
        stdout: String,
        stderr: String,
    },
    /// The program exited successfully but wrote nothing at the output path.
    #[error(
        "{} exited successfully but wrote no dump to {}{}",
        .program.display(),
        .path.display(),
        tail(.stderr, .stdout)
    )]
    NoOutput {
        program: PathBuf,
        path: PathBuf,
        stdout: String,
        stderr: String,
    },
    /// What the program wrote is not a valid dump.
    #[error("{} wrote an invalid dump to {}: {source}", .program.display(), .path.display())]
    Dump {
        program: PathBuf,
        path: PathBuf,
        source: DumpError,
    },
}

/// `: <stderr>`, or the standard output if the standard error is empty, for an error message.
fn tail(stderr: &str, stdout: &str) -> String {
    let (name, text) = if stderr.trim().is_empty() {
        ("standard output", stdout.trim())
    } else {
        ("standard error", stderr.trim())
    };
    if text.is_empty() {
        String::new()
    } else {
        format!("; its {name} ends:\n{text}")
    }
}

/// An external program that solves a spot file and writes a dump: see the
/// [module documentation](self).
#[derive(Debug, Clone, PartialEq)]
pub struct ExternalSolver {
    pub program: PathBuf,
    /// The arguments, holding `{spot}` and `{out}`.
    pub args: Vec<String>,
    /// How long a run may take before the program is killed. [`ExternalSolver::new`] sets
    /// [`ExternalSolver::DEFAULT_TIMEOUT`].
    pub timeout: Duration,
}

impl ExternalSolver {
    /// Ten minutes.
    pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(600);

    /// The solver run by `command`, a program and its arguments, with the default timeout.
    /// Fails if the command is empty, or if no argument holds `{spot}` or `{out}`.
    pub fn new(command: &[String]) -> Result<ExternalSolver, RunError> {
        let Some((program, args)) = command.split_first() else {
            return Err(RunError::Template {
                reason: "is empty; it needs at least the program".into(),
            });
        };
        if program.trim().is_empty() {
            return Err(RunError::Template {
                reason: "names no program: its first item is blank".into(),
            });
        }
        for placeholder in [SPOT_PLACEHOLDER, OUT_PLACEHOLDER] {
            if !args.iter().any(|arg| arg.contains(placeholder)) {
                let reason = format!(
                    "has no argument holding `{placeholder}`, so the program would not know \
                     {}",
                    if placeholder == SPOT_PLACEHOLDER {
                        "which spot to solve"
                    } else {
                        "where to write its dump"
                    }
                );
                return Err(RunError::Template { reason });
            }
        }
        Ok(ExternalSolver {
            program: PathBuf::from(program),
            args: args.to_vec(),
            timeout: Self::DEFAULT_TIMEOUT,
        })
    }

    /// The solver configured in [`SOLVER_ENV`], or `None` if the variable is not set or is
    /// empty. Fails if it is set to something other than a JSON array of strings that
    /// [`ExternalSolver::new`] accepts.
    pub fn from_env() -> Result<Option<ExternalSolver>, RunError> {
        match std::env::var(SOLVER_ENV) {
            Ok(text) if !text.trim().is_empty() => ExternalSolver::from_template(&text).map(Some),
            Ok(_) | Err(std::env::VarError::NotPresent) => Ok(None),
            Err(std::env::VarError::NotUnicode(_)) => Err(RunError::Template {
                reason: format!("in `{SOLVER_ENV}` is not valid Unicode"),
            }),
        }
    }

    /// The solver of a template written as a JSON array of strings, as [`SOLVER_ENV`] holds it.
    pub fn from_template(text: &str) -> Result<ExternalSolver, RunError> {
        let command: Vec<String> =
            serde_json::from_str(text).map_err(|err| RunError::Template {
                reason: format!(
                    "`{text}` is not a JSON array of strings, such as \
                     [\"wrapper.exe\", \"{SPOT_PLACEHOLDER}\", \"{OUT_PLACEHOLDER}\"]: {err}"
                ),
            })?;
        ExternalSolver::new(&command)
    }

    /// The arguments with the two paths filled in.
    pub fn arguments(&self, spot: &Path, out: &Path) -> Vec<String> {
        let (spot, out) = (spot.to_string_lossy(), out.to_string_lossy());
        self.args
            .iter()
            .map(|arg| {
                arg.replace(SPOT_PLACEHOLDER, &spot)
                    .replace(OUT_PLACEHOLDER, &out)
            })
            .collect()
    }

    /// Runs the program on the spot file `spot`, has it write its dump to `out`, and reads the
    /// dump: see the [module documentation](self).
    pub fn run(&self, spot: &Path, out: &Path) -> Result<SolutionDump, RunError> {
        match fs::remove_file(out) {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(source) => {
                return Err(RunError::Clear {
                    path: out.to_path_buf(),
                    source,
                });
            }
        }
        let mut child = Command::new(&self.program)
            .args(self.arguments(spot, out))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|source| RunError::Spawn {
                program: self.program.clone(),
                source,
            })?;
        let stdout = drain(child.stdout.take().expect("stdout is piped"));
        let stderr = drain(child.stderr.take().expect("stderr is piped"));

        let start = Instant::now();
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break Some(status),
                Ok(None) if start.elapsed() >= self.timeout => {
                    // The program may have exited just now; either way it is gone after this.
                    let _ = child.kill();
                    let _ = child.wait();
                    break None;
                }
                Ok(None) => thread::sleep(Duration::from_millis(10)),
                Err(source) => {
                    let _ = child.kill();
                    return Err(RunError::Spawn {
                        program: self.program.clone(),
                        source,
                    });
                }
            }
        };
        let (stdout, stderr) = (stdout.collect(), stderr.collect());
        let program = self.program.clone();
        let Some(status) = status else {
            return Err(RunError::Timeout {
                program,
                timeout: self.timeout,
                stdout,
                stderr,
            });
        };
        if !status.success() {
            return Err(RunError::Failed {
                program,
                status,
                stdout,
                stderr,
            });
        }
        if !out.exists() {
            return Err(RunError::NoOutput {
                program,
                path: out.to_path_buf(),
                stdout,
                stderr,
            });
        }
        SolutionDump::load(out).map_err(|source| RunError::Dump {
            program,
            path: out.to_path_buf(),
            source,
        })
    }
}

/// An output stream read on a thread of its own, so that a program writing much to one stream
/// does not block on it while the other is read.
struct Drain {
    done: mpsc::Receiver<Vec<u8>>,
}

fn drain(mut stream: impl Read + Send + 'static) -> Drain {
    let (send, done) = mpsc::channel();
    thread::spawn(move || {
        let mut kept = Vec::new();
        let mut buffer = [0; 4096];
        loop {
            match stream.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    kept.extend_from_slice(&buffer[..n]);
                    if kept.len() > 2 * KEPT {
                        kept.drain(..kept.len() - KEPT);
                    }
                }
            }
        }
        let start = kept.len().saturating_sub(KEPT);
        // The caller may have stopped waiting.
        let _ = send.send(kept[start..].to_vec());
    });
    Drain { done }
}

impl Drain {
    /// The end of what the stream carried, waiting a little for it to close.
    fn collect(self) -> String {
        match self.done.recv_timeout(DRAIN) {
            Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
            Err(_) => String::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn a_template_needs_a_program_and_both_placeholders() {
        let solver = ExternalSolver::new(&strings(&["wrap", "--out={out}", "{spot}"])).unwrap();
        assert_eq!(solver.program, PathBuf::from("wrap"));
        assert_eq!(solver.timeout, ExternalSolver::DEFAULT_TIMEOUT);
        assert_eq!(
            solver.arguments(Path::new("a b/s.toml"), Path::new("o.json")),
            ["--out=o.json", "a b/s.toml"]
        );
        for (command, expected) in [
            (vec![], "is empty"),
            (vec![" ", "{spot}", "{out}"], "names no program"),
            (vec!["wrap", "{out}"], "`{spot}`"),
            (vec!["wrap", "{spot}"], "`{out}`"),
        ] {
            let error = ExternalSolver::new(&strings(&command)).unwrap_err();
            assert!(error.to_string().contains(expected), "{command:?}: {error}");
        }
    }

    #[test]
    fn a_template_is_read_as_a_json_array_of_strings() {
        let solver =
            ExternalSolver::from_template(r#"["C:/Program Files/w.exe", "{spot}", "{out}"]"#)
                .unwrap();
        assert_eq!(solver.program, PathBuf::from("C:/Program Files/w.exe"));
        assert_eq!(solver.args, ["{spot}", "{out}"]);
        for text in ["w.exe {spot} {out}", r#"["w", 3]"#, "{}"] {
            let error = ExternalSolver::from_template(text).unwrap_err();
            assert!(
                error.to_string().contains("not a JSON array of strings"),
                "{text}: {error}"
            );
        }
    }

    #[test]
    fn a_program_that_cannot_start_is_reported() {
        let solver = ExternalSolver::new(&strings(&[
            "./no-such-program-for-the-runner-test",
            "{spot}",
            "{out}",
        ]))
        .unwrap();
        let out = std::env::temp_dir().join("pk-postflop-no-such-program.json");
        let error = solver.run(Path::new("spot.toml"), &out).unwrap_err();
        assert!(matches!(error, RunError::Spawn { .. }), "{error}");
        assert!(error.to_string().contains("could not start"), "{error}");
    }
}
