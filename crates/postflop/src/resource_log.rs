//! A record of what each solve took, for choosing machine sizes from measurements: the spot and
//! the size of its trees, the memory estimate, the peak resident memory of the process, the
//! wall time, the iterations and the exploitability reached.
//!
//! The library builds a [`SolveRecord`] and appends it, as one line of JSON, to a file the
//! caller names ([`append_record`]); it never writes a file on its own. The example program
//! `solve_spot` appends one per solve to `runs/log.jsonl`, and the `solver` command line will
//! do the same.
//!
//! [`peak_resident_bytes`] asks the operating system for the most memory the process has held
//! resident: `VmHWM` from `/proc/self/status` on Linux, and `PeakWorkingSetSize` from
//! `GetProcessMemoryInfo` on Windows. It covers the whole process, so it is a solve's peak only
//! in a process that makes one solve, as the example does.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use pk_cfr::PublicGame;
use pk_game::Player;
use serde::Serialize;

use crate::memory::MemoryEstimate;
use crate::solve::{Solution, SolveOptions, StopReason};
use crate::spot::Spot;

/// One solve's resources, as a line of the resource log.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SolveRecord {
    /// When the record was made, in seconds since the Unix epoch.
    pub unix_time: u64,
    /// The spot's name and board.
    pub spot: String,
    pub board: String,
    /// Each player's hands on the spot's board, OOP first.
    pub hands: [usize; 2],
    pub betting_nodes: usize,
    pub public_nodes: usize,
    /// The estimate the solve was checked against, item by item.
    pub estimate: MemoryEstimate,
    /// The most memory the process has held resident, in bytes, if the operating system says.
    pub peak_resident_bytes: Option<u64>,
    /// The solve's wall time, in seconds.
    pub wall_seconds: f64,
    pub iterations: u64,
    /// The rule that stopped the solve: `target`, `iteration_cap` or `time_cap`.
    pub stop: &'static str,
    /// The final exploitability, as a percent of the pot and in chips per hand dealt.
    pub exploitability_percent: f64,
    pub exploitability_chips: f64,
    /// The target, in percent of the pot, and the caps the solve ran under.
    pub target: Option<f64>,
    pub max_iterations: u64,
    pub max_seconds: Option<f64>,
    /// Whether the library was built with debug assertions, which make every solve check its
    /// solution and so take longer and more memory.
    pub debug_assertions: bool,
    /// The operating system and architecture, as Rust names them.
    pub os: &'static str,
    pub arch: &'static str,
}

impl SolveRecord {
    /// The record of `solution`, a solve of `spot` under `options`, with the estimate it was
    /// checked against and the process's peak resident memory, from [`peak_resident_bytes`].
    pub fn new(
        spot: &Spot,
        options: &SolveOptions,
        solution: &Solution,
        estimate: &MemoryEstimate,
        peak_resident_bytes: Option<u64>,
    ) -> SolveRecord {
        let unix_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |time| time.as_secs());
        let stop = match solution.stop() {
            StopReason::Target => "target",
            StopReason::IterationCap => "iteration_cap",
            StopReason::TimeCap => "time_cap",
        };
        SolveRecord {
            unix_time,
            spot: spot.name.clone(),
            board: spot.board.to_string(),
            hands: [Player::Oop, Player::Ip].map(|player| solution.hands(player).len()),
            betting_nodes: solution.tree().nodes().len(),
            public_nodes: solution.game().num_nodes(),
            estimate: estimate.clone(),
            peak_resident_bytes,
            wall_seconds: solution.elapsed().as_secs_f64(),
            iterations: solution.iterations(),
            stop,
            exploitability_percent: solution.exploitability().percent_of_pot,
            exploitability_chips: solution.exploitability().chips,
            target: options.target,
            max_iterations: options.max_iterations,
            max_seconds: options.max_time.map(|time| time.as_secs_f64()),
            debug_assertions: cfg!(debug_assertions),
            os: std::env::consts::OS,
            arch: std::env::consts::ARCH,
        }
    }

    /// The record as one line of JSON, without the line break.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("a record has only finite numbers and plain fields")
    }
}

/// Appends `record` to the file at `path` as one line of JSON, creating the file and its
/// directory if they do not exist.
pub fn append_record(path: impl AsRef<Path>, record: &SolveRecord) -> io::Result<()> {
    let path = path.as_ref();
    if let Some(directory) = path.parent()
        && !directory.as_os_str().is_empty()
    {
        fs::create_dir_all(directory)?;
    }
    let mut line = record.to_json();
    line.push('\n');
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    file.write_all(line.as_bytes())
}

/// The most memory this process has held resident so far, in bytes, or `None` if the operating
/// system does not say: see the [module documentation](self).
pub fn peak_resident_bytes() -> Option<u64> {
    peak::resident()
}

#[cfg(target_os = "linux")]
mod peak {
    /// `VmHWM`, the peak resident set, from `/proc/self/status`, where it is given in kB.
    pub fn resident() -> Option<u64> {
        let status = std::fs::read_to_string("/proc/self/status").ok()?;
        let line = status.lines().find(|line| line.starts_with("VmHWM:"))?;
        let kilobytes: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
        Some(kilobytes * 1024)
    }
}

#[cfg(windows)]
mod peak {
    use windows_sys::Win32::System::ProcessStatus::{
        K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
    };
    use windows_sys::Win32::System::Threading::GetCurrentProcess;

    /// `PeakWorkingSetSize` from `GetProcessMemoryInfo`.
    pub fn resident() -> Option<u64> {
        let size = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
        let mut counters = PROCESS_MEMORY_COUNTERS {
            cb: size,
            ..PROCESS_MEMORY_COUNTERS::default()
        };
        // SAFETY: `GetCurrentProcess` returns a pseudo-handle, which needs no closing, and
        // `counters` is a writable struct of the size passed.
        let ok = unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &mut counters, size) };
        (ok != 0).then_some(counters.PeakWorkingSetSize as u64)
    }
}

#[cfg(not(any(target_os = "linux", windows)))]
mod peak {
    pub fn resident() -> Option<u64> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::estimate_memory;
    use crate::solve::solve;

    fn spot() -> Spot {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../reference/spots/river/toy_polarized.toml"
        );
        Spot::load(path).unwrap()
    }

    #[test]
    fn the_peak_resident_memory_is_known_on_linux_and_windows_and_covers_what_is_held() {
        let before = peak_resident_bytes();
        if cfg!(any(target_os = "linux", windows)) {
            let before = before.expect("the operating system says");
            // 64 MiB written, so resident, raises the peak by about that much.
            let block = vec![1u8; 64 << 20];
            let after = peak_resident_bytes().unwrap();
            assert!(after >= before, "{before} then {after}");
            assert!(after >= (64 << 20), "{after}");
            assert_eq!(block.iter().map(|&b| u64::from(b)).sum::<u64>(), 64 << 20);
        } else {
            assert_eq!(before, None);
        }
    }

    #[test]
    fn a_record_is_one_line_of_json_appended_to_the_log() {
        let spot = spot();
        let options = SolveOptions::default();
        let estimate = estimate_memory(&spot, &options).unwrap();
        let solution = solve(&spot, &options, |_| {}).unwrap();
        let record = SolveRecord::new(&spot, &options, &solution, &estimate, Some(12345));
        let json = record.to_json();
        assert!(!json.contains('\n'));
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["spot"], "toy_polarized");
        assert_eq!(value["hands"], serde_json::json!([22, 12]));
        assert_eq!(value["public_nodes"], 9);
        assert_eq!(value["peak_resident_bytes"], 12345);
        assert_eq!(value["stop"], "target");
        assert_eq!(value["estimate"]["total"], estimate.total);
        assert!(value["exploitability_percent"].as_f64().unwrap() <= 0.3);

        let directory = std::env::temp_dir().join(format!(
            "pk-postflop-log-{}-{}",
            std::process::id(),
            record.unix_time
        ));
        let path = directory.join("runs").join("log.jsonl");
        append_record(&path, &record).unwrap();
        append_record(&path, &record).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        fs::remove_dir_all(&directory).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines, [json.as_str(), json.as_str()]);
    }
}
