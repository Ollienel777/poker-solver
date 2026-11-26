//! What `solver serve` answers: the routing of a request to its endpoint, and the state the
//! endpoints share, a folder of solution files and the solutions loaded from it. Everything the
//! server does is here, as a function from a request's method and URL to a [`Reply`];
//! `commands::serve` only listens, hands each request to [`State::answer`] and prints what
//! happened. ARCHITECTURE.md's `serve` section lists the endpoints.
//!
//! A node is answered from the file's solution, loaded up to the street the node needs and no
//! further, as `inspect` loads it, and kept loaded for the requests that follow ([`cache`]):
//! the loaded solutions are held to a memory budget, each counted at an estimate of its size,
//! and the least recently used is dropped first. A request for a node on a later street than
//! the one loaded loads the file again, deeper. A file written again since it was loaded is
//! loaded again.
//!
//! A solve started from the page ([`solving`]) runs on a thread of its own, one at a time, and
//! holds back its estimate of the same memory budget while it runs, the loaded solutions dropped
//! to make room for it.
//!
//! A request never takes the server down: an error is a reply, a JSON object with one line of
//! `error`, and a panic while answering is caught and replied to as an error, the loaded
//! solutions dropped in case it left them half-changed.

pub mod cache;
pub mod solving;

use std::fs;
use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::SystemTime;

use pk_game::Street;
use pk_postflop::solution_file::Header;
use pk_postflop::{PostflopGame, SavedSolution, SolveOptions, estimate_memory};
use serde::Serialize;

use crate::commands::inspect::{file_error, parse_hand};
use crate::format::bytes;
use crate::json::{FileJson, NodeAnswer};
use crate::view::{self, FileView};
use cache::Cache;
use solving::{EstimateJson, Inputs, Job, Run, SolveRequest};

/// The page's files, built into the binary: each one's path under `/`, its media type and its
/// bytes. `/` serves `index.html`. A file added to `crates/cli/assets/` is served once it has a
/// line here.
const ASSETS: &[Asset] = &[
    Asset {
        path: "index.html",
        content_type: "text/html; charset=utf-8",
        bytes: include_bytes!("../assets/index.html"),
    },
    Asset {
        path: "app.css",
        content_type: "text/css; charset=utf-8",
        bytes: include_bytes!("../assets/app.css"),
    },
    Asset {
        path: "app.js",
        content_type: "text/javascript; charset=utf-8",
        bytes: include_bytes!("../assets/app.js"),
    },
];

struct Asset {
    path: &'static str,
    content_type: &'static str,
    bytes: &'static [u8],
}

/// The extension of the files the server lists and opens.
pub const EXTENSION: &str = "pksol";

/// A reply to a request: its status, media type and body, and what the server has to say about
/// it, a line each, such as a solution loaded or dropped.
#[derive(Debug, Clone)]
pub struct Reply {
    pub status: u16,
    pub content_type: &'static str,
    pub body: Vec<u8>,
    pub notes: Vec<String>,
}

const JSON: &str = "application/json; charset=utf-8";

impl Reply {
    /// A reply of `value` as JSON, with status 200.
    fn json(value: &impl Serialize) -> Reply {
        Reply {
            status: 200,
            content_type: JSON,
            body: serde_json::to_vec(value).expect("a reply is JSON"),
            notes: Vec::new(),
        }
    }

    /// An error with `status`: `{"error": message}`.
    fn error(status: u16, message: impl Into<String>) -> Reply {
        #[derive(Serialize)]
        struct Error {
            error: String,
        }
        // One line, whatever the message it reuses.
        let message: String = message.into();
        let error = message.split_whitespace().collect::<Vec<_>>().join(" ");
        Reply {
            status,
            ..Reply::json(&Error { error })
        }
    }

    /// Whether the reply is JSON, as the endpoints under `/api/` give it.
    pub fn is_json(&self) -> bool {
        self.content_type == JSON
    }
}

/// An error reply on its way out of an endpoint.
type Answer<T> = Result<T, Reply>;

/// What the server shares between requests: the folder it serves, the solutions loaded from it,
/// where a solve's inputs come from, and the solve, which its own thread shares too.
pub struct State {
    folder: PathBuf,
    loaded: Arc<Mutex<Cache<Loaded>>>,
    inputs: Inputs,
    job: Arc<Mutex<Job>>,
}

/// A solution loaded from a file, with what it was loaded from.
struct Loaded {
    file: FileView,
    stamp: Stamp,
    saved: SavedSolution,
}

/// What tells a file written again from the one loaded: its length and when it was written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Stamp {
    bytes: u64,
    modified: Option<SystemTime>,
}

impl State {
    /// The state of a server of `folder`, holding loaded solutions of at most `budget` bytes by
    /// their estimates, with a solve's inputs from the default folders.
    #[cfg(test)]
    pub fn new(folder: PathBuf, budget: u64) -> State {
        State::with_inputs(folder, budget, Inputs::default())
    }

    /// The state of a server of `folder` under `budget`, with a solve's inputs from `inputs`.
    pub fn with_inputs(folder: PathBuf, budget: u64, inputs: Inputs) -> State {
        State {
            folder,
            loaded: Arc::new(Mutex::new(Cache::new(budget))),
            inputs,
            job: Arc::new(Mutex::new(Job::default())),
        }
    }

    /// The reply to a request with `method` for `url`, a path and a query, with `body`. A panic
    /// while answering is caught and replied to with status 500.
    pub fn answer(&self, method: &str, url: &str, body: &[u8]) -> Reply {
        match panic::catch_unwind(AssertUnwindSafe(|| self.route(method, url, body))) {
            Ok(reply) => reply,
            Err(payload) => {
                // The loaded solutions may be half-changed: drop them all.
                self.loaded().clear();
                let what = panic_message(payload.as_ref());
                Reply::error(
                    500,
                    format!(
                        "the server failed on this request ({what}); it goes on serving, with no \
                         solution loaded"
                    ),
                )
            }
        }
    }

    /// The loaded solutions. A request that panicked while holding them left them poisoned; they
    /// are emptied then, since the panic may have left them half-changed.
    fn loaded(&self) -> MutexGuard<'_, Cache<Loaded>> {
        lock_cache(&self.loaded)
    }

    /// Every endpoint, by method and path.
    fn route(&self, method: &str, url: &str, body: &[u8]) -> Reply {
        let (path, query) = url.split_once('?').unwrap_or((url, ""));
        let takes = match path {
            "/api/estimate" | "/api/solve/cancel" => "POST",
            "/api/solve" if method == "POST" => "POST",
            _ => "GET",
        };
        if method != takes {
            return Reply::error(
                405,
                format!(
                    "{method} {path}: {path} takes {takes}; the server answers GET, and POST for \
                     /api/estimate, /api/solve and /api/solve/cancel"
                ),
            );
        }
        let query = match Query::parse(query) {
            Ok(query) => query,
            Err(reply) => return reply,
        };
        let answer = match (method, path) {
            ("GET", "/api/files") => query.only(&[]).map(|()| self.files()),
            ("GET", "/api/node") => query
                .only(&["file", "path", "hand"])
                .and_then(|()| self.node(&query)),
            ("GET", "/api/inputs") => query
                .only(&[])
                .map(|()| solving::inputs(&self.inputs, self.loaded().budget())),
            ("POST", "/api/estimate") => query.only(&[]).and_then(|()| self.estimate(body)),
            ("POST", "/api/solve") => query.only(&[]).and_then(|()| self.start(body)),
            ("GET", "/api/solve") => query
                .only(&[])
                .map(|()| Reply::json(&solving::lock(&self.job).status())),
            ("POST", "/api/solve/cancel") => query.only(&[]).and_then(|()| self.cancel()),
            _ => match asset(path) {
                Some(asset) => Ok(Reply {
                    status: 200,
                    content_type: asset.content_type,
                    body: asset.bytes.to_vec(),
                    notes: Vec::new(),
                }),
                None => Err(Reply::error(
                    404,
                    format!(
                        "there is nothing at {path}: the page is at /, the solution files at \
                         /api/files, a node at /api/node?file=NAME&path=PATH, and a solve's \
                         inputs at /api/inputs"
                    ),
                )),
            },
        };
        answer.unwrap_or_else(|reply| reply)
    }

    /// `/api/files`: the solution files of the folder, by name, each described from its header.
    fn files(&self) -> Reply {
        #[derive(Serialize)]
        struct Files {
            folder: String,
            files: Vec<Listed>,
        }
        #[derive(Serialize)]
        #[serde(untagged)]
        enum Listed {
            File(Box<FileJson>),
            Unreadable {
                name: String,
                bytes: u64,
                error: String,
            },
        }
        let entries = match fs::read_dir(&self.folder) {
            Ok(entries) => entries,
            Err(error) => {
                return Reply::error(
                    500,
                    format!("could not list `{}`: {error}", self.folder.display()),
                );
            }
        };
        let mut names: Vec<String> = entries
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
            .filter_map(|entry| entry.file_name().into_string().ok())
            .filter(|name| has_extension(name))
            .collect();
        names.sort();
        let files = names
            .into_iter()
            .map(|name| {
                let path = self.folder.join(&name);
                match FileView::read(&path) {
                    Ok(file) => Listed::File(Box::new(FileJson::new(&file))),
                    Err(error) => Listed::Unreadable {
                        bytes: fs::metadata(&path).map_or(0, |m| m.len()),
                        error: format!("{:#}", file_error(Path::new(&name), error)),
                        name,
                    },
                }
            })
            .collect();
        Reply::json(&Files {
            folder: self.folder.display().to_string(),
            files,
        })
    }

    /// `/api/node?file=NAME&path=PATH&hand=HAND`: a node of a file, the root without `path`,
    /// with the combos of `hand` at a decision.
    fn node(&self, query: &Query) -> Answer<Reply> {
        let name = query.get("file").ok_or_else(|| {
            Reply::error(
                400,
                "give the solution file, as in /api/node?file=NAME.pksol, one that /api/files \
                 lists",
            )
        })?;
        let path_text = query.get("path").unwrap_or("");
        let hand = match query.get("hand") {
            None | Some("") => None,
            Some(text) => Some(
                parse_hand(text)
                    .map_err(|reason| Reply::error(400, format!("hand `{text}`: {reason}")))?,
            ),
        };
        let file = self.file_path(name)?;
        let (loaded, notes) = self.load(name, &file, path_text)?;
        let saved = &loaded.saved;
        // The messages name the file as the request does.
        let shown = view::find(saved, path_text)
            .and_then(|node| view::node_view(saved, Path::new(name), node, hand))
            .map_err(path_error);
        // What was loaded is said whether or not the node could be shown.
        let mut reply = match shown {
            Ok(shown) => Reply::json(&NodeAnswer::new(&loaded.file, &shown)),
            Err(reply) => reply,
        };
        reply.notes = notes;
        Ok(reply)
    }

    /// The path of the solution file `name` of the folder. A name that is not a plain file
    /// name, one with a folder in it, is refused with 400, and one that is not a solution file
    /// of the folder with 404.
    fn file_path(&self, name: &str) -> Answer<PathBuf> {
        if !plain(name) {
            return Err(Reply::error(
                400,
                format!(
                    "file `{name}`: give the name of a solution file in the served folder, with \
                     no folder in it, as /api/files lists it"
                ),
            ));
        }
        let path = self.folder.join(name);
        if !has_extension(name) || !path.is_file() {
            return Err(Reply::error(
                404,
                format!(
                    "file `{name}`: there is no solution file of that name in `{}`; /api/files \
                     lists them",
                    self.folder.display()
                ),
            ));
        }
        Ok(path)
    }

    /// The solution of the file `name` at `file`, loaded up to the street that the node `path`
    /// is on: the one held if it is loaded that far and the file has not been written again,
    /// or else loaded now, deeper, after making room for it. Gives what was loaded and dropped,
    /// as notes.
    fn load(&self, name: &str, file: &Path, path: &str) -> Answer<(Arc<Loaded>, Vec<String>)> {
        // The messages name the file as the request does.
        let unreadable =
            |error| Reply::error(500, format!("{:#}", file_error(Path::new(name), error)));
        let stamp = stamp(file).map_err(unreadable)?;
        let mut loaded = self.loaded();
        if let Some(held) = loaded.get(name) {
            let header = &held.file.header;
            let needed = view::street_of_path(header.spot.street(), path).min(header.last_street());
            if held.stamp == stamp && held.saved.last_street() >= needed {
                return Ok((held, Vec::new()));
            }
        }
        // The old solution goes before the new one is loaded, so that the two are never held
        // at once.
        loaded.remove(name);
        let info = FileView::read(file).map_err(unreadable)?;
        let header = &info.header;
        let last = view::street_of_path(header.spot.street(), path).min(header.last_street());
        let size = loaded_bytes(header, last).map_err(|error| {
            Reply::error(
                500,
                format!("file `{name}`: its spot cannot be built: {error}"),
            )
        })?;
        let mut notes: Vec<String> = Vec::new();
        let dropped = loaded.make_room(size).map_err(|too_large| {
            let message = if too_large.reserved > 0 && too_large.bytes <= too_large.budget {
                format!(
                    "file `{name}`: its solution up to the {last} needs about {} loaded, more \
                     than the {} that the running solve leaves of the {} that --max-memory lets \
                     the server hold; open it once the solve has ended",
                    bytes(too_large.bytes),
                    bytes(too_large.budget - too_large.reserved),
                    bytes(too_large.budget)
                )
            } else {
                format!(
                    "file `{name}`: its solution up to the {last} needs about {} loaded, over \
                     the {} that --max-memory lets the server hold; restart it with a higher \
                     --max-memory, if the machine has the memory",
                    bytes(too_large.bytes),
                    bytes(too_large.budget)
                )
            };
            Reply::error(503, message)
        })?;
        notes.extend(
            dropped
                .iter()
                .map(|old| format!("dropped `{old}`, the least recently used")),
        );
        let saved = SavedSolution::load_streets(file, last).map_err(unreadable)?;
        let (held, _) = loaded
            .insert(
                name,
                Loaded {
                    file: info,
                    stamp,
                    saved,
                },
                size,
            )
            .expect("room was made for it");
        notes.push(format!(
            "loaded `{name}` up to the {last}, about {} by estimate; {} of {} held",
            bytes(size),
            bytes(loaded.held()),
            bytes(loaded.budget())
        ));
        Ok((held, notes))
    }

    /// `POST /api/estimate`: the solve the body asks for, checked and estimated, as
    /// `solver estimate` gives it, whether or not it would be refused; nothing is started.
    fn estimate(&self, body: &[u8]) -> Answer<Reply> {
        let request = SolveRequest::parse(body)?;
        let budget = self.loaded().budget();
        let prepared = solving::prepare(&self.folder, &self.inputs, budget, request)?;
        let (held, reserved) = {
            let loaded = self.loaded();
            (loaded.held(), loaded.reserved())
        };
        Ok(Reply::json(&EstimateJson::new(&prepared, held, reserved)))
    }

    /// `POST /api/solve`: starts the solve the body asks for, after the checks and estimates of
    /// `/api/estimate`, on a thread of its own, and answers 202 with its status. Refuses with 409
    /// while another solve runs, with 400 a file that exists unless `force` is given, and with
    /// 422 a spot over a limit, with the estimate.
    fn start(&self, body: &[u8]) -> Answer<Reply> {
        let request = SolveRequest::parse(body)?;
        {
            let job = solving::lock(&self.job);
            if job.busy() {
                return Err(Reply::error(409, job.busy_message()));
            }
        }
        let budget = self.loaded().budget();
        let prepared = solving::prepare(&self.folder, &self.inputs, budget, request)?;
        if prepared.exists && !prepared.request.force {
            return Err(Reply::error(
                400,
                format!(
                    "out `{}` already exists in the folder; ask to overwrite it, or give another \
                     name",
                    prepared.out
                ),
            ));
        }
        let estimate = EstimateJson::new(&prepared, self.loaded().held(), 0);
        if let Some(refusal) = &estimate.refusal {
            #[derive(Serialize)]
            struct Refused<'a> {
                error: String,
                estimate: &'a EstimateJson,
            }
            let error = refusal
                .message
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            let mut reply = Reply::json(&Refused {
                error,
                estimate: &estimate,
            });
            reply.status = 422;
            return Err(reply);
        }
        let memory = prepared.memory();
        let mut notes: Vec<String> = Vec::new();
        let dropped = self.loaded().reserve(memory).map_err(|too_large| {
            Reply::error(
                503,
                format!(
                    "the solve needs about {} by its estimate, over the {} that --max-memory \
                     lets the server hold",
                    bytes(too_large.bytes),
                    bytes(too_large.budget)
                ),
            )
        })?;
        notes.extend(
            dropped
                .iter()
                .map(|old| format!("dropped `{old}` to make room for the solve")),
        );
        let run = Run::new(&prepared, &estimate);
        let begun = solving::lock(&self.job).begin(run);
        let cancel = match begun {
            Ok(cancel) => cancel,
            Err(message) => {
                self.loaded().release();
                return Err(Reply::error(409, message));
            }
        };
        notes.push(format!(
            "solving {} into `{}`, holding about {} of memory by estimate",
            prepared.input.spot.name,
            prepared.out,
            bytes(memory)
        ));
        let cache = Arc::clone(&self.loaded);
        solving::spawn(
            prepared,
            Arc::clone(&self.job),
            cancel,
            self.inputs.log.clone(),
            move || lock_cache(&cache).release(),
        );
        let mut reply = Reply::json(&solving::lock(&self.job).status());
        reply.status = 202;
        reply.notes = notes;
        Ok(reply)
    }

    /// `POST /api/solve/cancel`: asks the running solve to stop at its next measurement, saving
    /// nothing, and answers with its status. Refuses with 409 when no solve runs, or once it is
    /// being saved.
    fn cancel(&self) -> Answer<Reply> {
        let mut job = solving::lock(&self.job);
        job.cancel().map_err(|message| Reply::error(409, message))?;
        Ok(Reply::json(&job.status()))
    }
}

/// The loaded solutions. A request that panicked while holding them left them poisoned; they
/// are emptied then, since the panic may have left them half-changed.
fn lock_cache(cache: &Mutex<Cache<Loaded>>) -> MutexGuard<'_, Cache<Loaded>> {
    cache.lock().unwrap_or_else(|poisoned| {
        let mut cache = poisoned.into_inner();
        cache.clear();
        cache
    })
}

/// What a panic said, as far as its payload is text.
fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "a panic".to_string())
}

/// Whether `name` is a plain file name, with no folder in it: not empty, `.` or `..`, with no
/// `/`, `\`, `:`, `..`, NUL or control character, and not absolute (DECISIONS P85).
fn plain(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains(['/', '\\', ':', '\0'])
        && !name.contains("..")
        && !name.chars().any(char::is_control)
        && Path::new(name).is_relative()
}

/// The asset served at `path`.
fn asset(path: &str) -> Option<&'static Asset> {
    let name = path.strip_prefix('/')?;
    let name = if name.is_empty() { "index.html" } else { name };
    ASSETS.iter().find(|asset| asset.path == name)
}

fn has_extension(name: &str) -> bool {
    Path::new(name)
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case(EXTENSION))
}

fn stamp(file: &Path) -> Result<Stamp, pk_postflop::SolutionFileError> {
    let metadata = fs::metadata(file).map_err(|source| pk_postflop::SolutionFileError::Io {
        action: "read",
        path: file.to_path_buf(),
        source,
    })?;
    Ok(Stamp {
        bytes: metadata.len(),
        modified: metadata.modified().ok(),
    })
}

/// A path's error, from `view`, as the server gives it: the same words, the path named as a
/// path rather than as `--node`.
fn path_error(error: anyhow::Error) -> Reply {
    let message = format!("{error:#}");
    let message = match message.strip_prefix("--node ") {
        Some(rest) => format!("path {rest}"),
        None => message,
    };
    Reply::error(400, message)
}

/// The memory a solution loaded from a file with `header`, up to `last`, holds, by estimate:
/// its game, as [`estimate_memory`] counts the public tree, the boards' hands and the betting
/// tree; its strategy on the streets loaded, each probability an `f32` decoded from the file's
/// 16 bits; its EVs on those streets, `f32`s as in the file; and a place for each node in the
/// strategy and in the EVs.
pub fn loaded_bytes(header: &Header, last: Street) -> Result<u64, pk_postflop::SolveError> {
    let options = SolveOptions {
        max_nodes: PostflopGame::DEFAULT_MAX_NODES.max(u64::from(header.nodes)),
        threads: 1,
        ..SolveOptions::default()
    };
    let estimate = estimate_memory(&header.spot, &options)?;
    let game = estimate.public_tree + estimate.board_hands + estimate.betting_tree;
    let mut sections = 0;
    for street in header.streets.iter().filter(|&&street| street <= last) {
        for section in &header.sections {
            if section.name == format!("strategy.{street}") {
                sections += 2 * section.raw_length;
            } else if section.name == format!("ev.{street}") {
                sections += section.raw_length;
            }
        }
    }
    Ok(game + sections + PER_NODE_BYTES * u64::from(header.nodes))
}

/// A node's place in a loaded solution: its strategy's, an `Option` of a vector and its count of
/// hands, and its EVs', an `Option` of a boxed slice.
const PER_NODE_BYTES: u64 = 32 + 16;

/// A request's query, its keys and values decoded.
struct Query {
    pairs: Vec<(String, String)>,
}

impl Query {
    /// Reads `key=value&...`, each part percent-decoded, with `+` read as a space.
    fn parse(text: &str) -> Answer<Query> {
        let mut pairs = Vec::new();
        for part in text.split('&').filter(|part| !part.is_empty()) {
            let (key, value) = part.split_once('=').unwrap_or((part, ""));
            let decoded = |text: &str| {
                decode(text).ok_or_else(|| {
                    Reply::error(
                        400,
                        format!(
                            "the query `{text}` is not valid: escape a character as %XX, in \
                             UTF-8"
                        ),
                    )
                })
            };
            pairs.push((decoded(key)?, decoded(value)?));
        }
        Ok(Query { pairs })
    }

    /// The value of `key`; the last one, if it is given more than once.
    fn get(&self, key: &str) -> Option<&str> {
        self.pairs
            .iter()
            .rev()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    /// Refuses a key that is not one of `known`.
    fn only(&self, known: &[&str]) -> Answer<()> {
        match self
            .pairs
            .iter()
            .find(|(key, _)| !known.contains(&key.as_str()))
        {
            None => Ok(()),
            Some((key, _)) if known.is_empty() => Err(Reply::error(
                400,
                format!("`{key}` is not a key here, which takes none"),
            )),
            Some((key, _)) => Err(Reply::error(
                400,
                format!(
                    "`{key}` is not a key here; the keys are {}",
                    known
                        .iter()
                        .map(|k| format!("`{k}`"))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            )),
        }
    }
}

/// `text` percent-decoded, with `+` read as a space; `None` if an escape is not two hex digits
/// or the bytes are not UTF-8.
fn decode(text: &str) -> Option<String> {
    let mut out = Vec::with_capacity(text.len());
    let mut bytes = text.bytes();
    while let Some(byte) = bytes.next() {
        match byte {
            b'+' => out.push(b' '),
            b'%' => {
                let high = (bytes.next()? as char).to_digit(16)?;
                let low = (bytes.next()? as char).to_digit(16)?;
                out.push((16 * high + low) as u8);
            }
            byte => out.push(byte),
        }
    }
    String::from_utf8(out).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_query_is_percent_decoded() {
        assert_eq!(decode("x+b50%20c").as_deref(), Some("x b50 c"));
        assert_eq!(decode("%2e%2E%2F").as_deref(), Some("../"));
        assert_eq!(decode("Ah%E2%99%A5").as_deref(), Some("Ah\u{2665}"));
        for bad in ["%", "%2", "%zz", "%FF"] {
            assert_eq!(decode(bad), None, "{bad}");
        }
        let query = Query::parse("file=a.pksol&path=x+x&path=b50&hand=").unwrap();
        assert_eq!(query.get("file"), Some("a.pksol"));
        assert_eq!(query.get("path"), Some("b50"));
        assert_eq!(query.get("hand"), Some(""));
        assert_eq!(query.get("node"), None);
        assert!(query.only(&["file", "path", "hand"]).is_ok());
        let refused = query.only(&["file"]).unwrap_err();
        assert_eq!(refused.status, 400);
        assert_eq!(
            String::from_utf8(refused.body).unwrap(),
            r#"{"error":"`path` is not a key here; the keys are `file`"}"#
        );
    }

    #[test]
    fn a_file_name_must_name_a_file_in_the_folder() {
        let folder = std::env::temp_dir().join(format!("pk-cli-server-{}", std::process::id()));
        let _ = fs::remove_dir_all(&folder);
        fs::create_dir_all(folder.join("sub")).unwrap();
        fs::write(folder.join("a.pksol"), "").unwrap();
        fs::write(folder.join("sub").join("b.pksol"), "").unwrap();
        fs::write(folder.join("c.txt"), "").unwrap();
        let state = State::new(folder.clone(), 1);
        assert_eq!(state.file_path("a.pksol").unwrap(), folder.join("a.pksol"));
        for bad in [
            "",
            ".",
            "..",
            "../a.pksol",
            "..\\a.pksol",
            "sub/b.pksol",
            "sub\\b.pksol",
            "/etc/passwd",
            "C:\\Windows\\win.ini",
            "C:a.pksol",
            "a.pksol:stream",
            "a..pksol",
            "a\u{0}.pksol",
        ] {
            assert_eq!(state.file_path(bad).unwrap_err().status, 400, "{bad:?}");
        }
        for unknown in ["b.pksol", "c.txt", "sub", "d.pksol"] {
            assert_eq!(
                state.file_path(unknown).unwrap_err().status,
                404,
                "{unknown}"
            );
        }
        fs::remove_dir_all(&folder).unwrap();
    }

    #[test]
    fn a_route_that_is_not_an_endpoint_or_asset_is_not_found() {
        let state = State::new(std::env::temp_dir(), 1);
        let page = state.answer("GET", "/", b"");
        assert_eq!(
            (page.status, page.content_type),
            (200, ASSETS[0].content_type)
        );
        assert_eq!(page.body, ASSETS[0].bytes);
        assert_eq!(
            state.answer("GET", "/index.html", b"").body,
            ASSETS[0].bytes
        );
        assert_eq!(state.answer("GET", "/nothing", b"").status, 404);
        assert_eq!(state.answer("GET", "/../Cargo.toml", b"").status, 404);
        assert_eq!(state.answer("POST", "/api/files", b"").status, 405);
        assert_eq!(state.answer("GET", "/api/estimate", b"").status, 405);
        assert_eq!(state.answer("PUT", "/api/solve", b"").status, 405);
        assert_eq!(state.answer("GET", "/api/solve/cancel", b"").status, 405);
        assert_eq!(state.answer("POST", "/api/estimate", b"{").status, 400);
        assert_eq!(state.answer("POST", "/api/solve/cancel", b"").status, 409);
        assert_eq!(state.answer("GET", "/api/solve", b"").status, 200);
        assert_eq!(state.answer("GET", "/api/files?x=1", b"").status, 400);
        assert_eq!(state.answer("GET", "/api/node", b"").status, 400);
    }

    #[test]
    fn the_page_is_served_whole() {
        let state = State::new(std::env::temp_dir(), 1);
        for asset in ASSETS {
            let reply = state.answer("GET", &format!("/{}", asset.path), b"");
            assert_eq!(
                (reply.status, reply.content_type),
                (200, asset.content_type),
                "{}",
                asset.path
            );
            assert_eq!(reply.body, asset.bytes, "{}", asset.path);
        }
        // Every file the page asks for by `href` or `src` is one the server holds.
        let page = std::str::from_utf8(ASSETS[0].bytes).unwrap();
        let mut asked = 0;
        for attribute in ["href=\"", "src=\""] {
            for part in page.split(attribute).skip(1) {
                let name = &part[..part.find('"').unwrap()];
                assert!(
                    asset(&format!("/{name}")).is_some(),
                    "the page asks for `{name}`"
                );
                asked += 1;
            }
        }
        assert_eq!(asked, ASSETS.len() - 1);
    }
}
