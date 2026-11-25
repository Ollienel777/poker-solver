//! Where `solve-postflop` writes its solution: the checks on the destination and on the streets
//! to hold, made before a solve starts so that a solve of many minutes is not lost to a mistyped
//! folder or written over an earlier result, and [`save`], the one function every solution is
//! written through.
//!
//! The solution file is `pk_postflop::solution_file`'s, described in ARCHITECTURE.md (Solution
//! file).

use std::fs;
use std::io::ErrorKind;
use std::path::Path;

use anyhow::{Result, anyhow, bail};
use pk_game::Street;
use pk_postflop::{SaveOptions, SaveReport, Solution, Spot};

use crate::format::list;

/// A street as `--last-street` names it: `flop`, `turn` or `river`.
pub fn parse_street(text: &str) -> Result<Street, String> {
    let name = text.trim().to_ascii_lowercase();
    Street::ALL
        .into_iter()
        .find(|street| street.name() == name)
        .ok_or_else(|| "give a street: flop, turn or river".to_string())
}

/// Checks, before a solve, that the solution can be written to `path`: its folder exists, and
/// the file does not, unless `force` allows overwriting it.
pub fn check_destination(path: &Path, force: bool) -> Result<()> {
    let shown = path.display();
    let folder = match path.parent() {
        Some(folder) if !folder.as_os_str().is_empty() => folder,
        _ => Path::new("."),
    };
    if !folder.is_dir() {
        if folder.exists() {
            bail!(
                "--out `{shown}`: `{}` is not a folder; give a path in a folder",
                folder.display()
            );
        }
        bail!(
            "--out `{shown}`: the folder `{}` does not exist; create it, or give a path in a \
             folder that does",
            folder.display()
        );
    }
    match fs::metadata(path) {
        Ok(metadata) if metadata.is_dir() => {
            bail!("--out `{shown}` is a folder; give the path of a file to write")
        }
        Ok(_) if !force => bail!(
            "--out `{shown}` already exists; pass --force to overwrite it, or give another path"
        ),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => bail!("--out `{shown}`: {error}"),
    }
}

/// Checks, before a solve, that a solution file of `spot` can hold the streets up to
/// `last_street`, and gives the options that save it so: a file holds the spot's first street
/// at least.
pub fn options(spot: &Spot, last_street: Street) -> Result<SaveOptions> {
    let first = spot.street();
    if last_street < first {
        let streets: Vec<String> = Street::ALL
            .into_iter()
            .filter(|&street| street >= first)
            .map(|street| format!("`{street}`"))
            .collect();
        bail!(
            "--last-street `{last_street}`: the board {} starts on the {first}, and a solution \
             file holds the spot's first street at least; give {}",
            spot.board,
            streets.join(" or ")
        );
    }
    Ok(SaveOptions { last_street })
}

/// The streets a solution file of `spot` holds under `options`, and those it leaves out: `the
/// flop and the turn, without the river`.
pub fn streets(spot: &Spot, options: &SaveOptions) -> String {
    let first = spot.street();
    let named = |keep: &dyn Fn(Street) -> bool| -> Vec<String> {
        Street::ALL
            .into_iter()
            .filter(|&street| street >= first && keep(street))
            .map(|street| format!("the {street}"))
            .collect()
    };
    let held = list(&named(&|street| street <= options.last_street));
    let left_out = named(&|street| street > options.last_street);
    if left_out.is_empty() {
        held
    } else {
        format!("{held}, without {}", list(&left_out))
    }
}

/// Writes `solution` to `path` as a solution file, and gives what was written.
pub fn save(solution: &Solution, path: &Path, options: &SaveOptions) -> Result<SaveReport> {
    pk_postflop::save(solution, path, options)
        .map_err(|error| anyhow!("the solution was not saved: {error}"))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    /// A fresh, empty folder under the system's temporary folder.
    fn folder(name: &str) -> PathBuf {
        let folder =
            std::env::temp_dir().join(format!("pk-cli-save-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&folder);
        fs::create_dir_all(&folder).unwrap();
        folder
    }

    fn message(result: Result<()>) -> String {
        let message = result.unwrap_err().to_string();
        println!("{message}");
        message
    }

    #[test]
    fn a_new_file_in_a_folder_that_exists_can_be_written() {
        let folder = folder("new");
        check_destination(&folder.join("spot.pksol"), false).unwrap();
        check_destination(&folder.join("spot.pksol"), true).unwrap();
        // A bare file name is in the current folder, which exists.
        check_destination(Path::new("no-such-solution.pksol"), false).unwrap();
        fs::remove_dir_all(&folder).unwrap();
    }

    #[test]
    fn a_folder_that_does_not_exist_is_refused() {
        let folder = folder("missing");
        let path = folder.join("no").join("such").join("spot.pksol");
        let text = message(check_destination(&path, true));
        assert!(text.contains("does not exist; create it"), "{text}");
        assert!(text.contains(&folder.join("no").join("such").display().to_string()));
        fs::remove_dir_all(&folder).unwrap();
    }

    #[test]
    fn a_file_that_exists_is_overwritten_only_with_force() {
        let folder = folder("exists");
        let path = folder.join("spot.pksol");
        fs::write(&path, b"an earlier solve").unwrap();
        let text = message(check_destination(&path, false));
        assert!(text.contains("already exists; pass --force"), "{text}");
        check_destination(&path, true).unwrap();
        // A file in the way of the folder, and a folder in place of the file.
        let text = message(check_destination(&path.join("spot.pksol"), true));
        assert!(text.contains("is not a folder"), "{text}");
        let text = message(check_destination(&folder, true));
        assert!(
            text.contains("is a folder; give the path of a file"),
            "{text}"
        );
        fs::remove_dir_all(&folder).unwrap();
    }
}
