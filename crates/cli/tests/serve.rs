//! Runs `solver serve` on a folder of solution files that `solver solve-postflop` writes, on a
//! free port of 127.0.0.1, and asks it what the viewer's page asks, with a plain HTTP request
//! over a `TcpStream`: the list of files, and nodes at each street of a turn spot, checked
//! against `inspect --json` and the library. Then every refusal: a file name that would leave
//! the folder, an unknown file or page, a bad path or hand, a solution over the memory limit,
//! after which the server still answers.

use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use pk_postflop::{SavedSolution, SolveOptions, node_values, read_header, solve};
use serde_json::Value;

mod common;

use common::*;

/// A server running in a child process, stopped when this is dropped.
struct Server {
    child: Child,
    port: u16,
    /// What it has printed, a line each.
    lines: Arc<Mutex<Vec<String>>>,
}

impl Server {
    /// `solver serve <folder> --port 0`, with `options`, once it says where it listens.
    fn start(folder: &Path, options: &[&str]) -> Server {
        let mut child = Command::new(env!("CARGO_BIN_EXE_solver"))
            .args(["serve", arg(folder), "--port", "0"])
            .args(options)
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("the solver binary runs");
        let mut stdout = BufReader::new(child.stdout.take().unwrap());
        let mut first = String::new();
        stdout.read_line(&mut first).unwrap();
        let prefix = "Serving      the solution files of `";
        assert!(first.starts_with(prefix), "{first}");
        let port = first
            .trim_end()
            .rsplit_once("http://127.0.0.1:")
            .and_then(|(_, rest)| rest.strip_suffix('/'))
            .and_then(|port| port.parse().ok())
            .unwrap_or_else(|| panic!("no port in {first}"));
        let lines = Arc::new(Mutex::new(vec![first.trim_end().to_string()]));
        let shared = Arc::clone(&lines);
        thread::spawn(move || {
            for line in stdout.lines().map_while(Result::ok) {
                println!("server: {line}");
                shared.lock().unwrap().push(line);
            }
        });
        Server { child, port, lines }
    }

    /// The status, media type and body of a GET of `target`.
    fn get(&self, target: &str) -> (u16, String, String) {
        let mut stream = TcpStream::connect(("127.0.0.1", self.port)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(120)))
            .unwrap();
        write!(
            stream,
            "GET {target} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"
        )
        .unwrap();
        let mut raw = Vec::new();
        stream.read_to_end(&mut raw).unwrap();
        let text = String::from_utf8(raw).expect("a reply is UTF-8");
        let (head, body) = text.split_once("\r\n\r\n").expect("a reply has a head");
        let mut head = head.lines();
        let status = head.next().unwrap().split(' ').nth(1).unwrap();
        let mut content_type = String::new();
        for line in head {
            let (name, value) = line.split_once(':').unwrap();
            let name = name.to_ascii_lowercase();
            assert_ne!(name, "transfer-encoding", "a reply has its length");
            if name == "content-type" {
                content_type = value.trim().to_string();
            }
        }
        (status.parse().unwrap(), content_type, body.to_string())
    }

    /// The JSON of a GET of `target`, which must succeed.
    fn json(&self, target: &str) -> Value {
        let (status, content_type, body) = self.get(target);
        assert_eq!(status, 200, "{target}: {body}");
        assert_eq!(content_type, "application/json; charset=utf-8");
        serde_json::from_str(&body).unwrap()
    }

    /// The status and the error of a GET of `target` that must fail.
    fn error(&self, target: &str) -> (u16, String) {
        let (status, content_type, body) = self.get(target);
        assert_ne!(status, 200, "{target}: {body}");
        assert_eq!(content_type, "application/json; charset=utf-8");
        let value: Value = serde_json::from_str(&body).unwrap();
        let object = value.as_object().unwrap();
        assert_eq!(object.len(), 1, "{body}");
        let message = object["error"].as_str().unwrap().to_string();
        assert_eq!(message.lines().count(), 1, "{message}");
        (status, message)
    }

    /// Waits until the server has printed a line that holds `text`.
    fn wait_for(&self, text: &str) {
        let started = Instant::now();
        while !self
            .lines
            .lock()
            .unwrap()
            .iter()
            .any(|line| line.contains(text))
        {
            assert!(
                started.elapsed() < Duration::from_secs(30),
                "no line holding {text:?} in {:#?}",
                self.lines.lock().unwrap()
            );
            thread::sleep(Duration::from_millis(10));
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Solves a spot through the command line and saves it to `out`, holding the streets up to
/// `last_street`.
fn solve_to(spot: &[&str], out: &Path, last_street: &str) {
    let mut args = vec!["solve-postflop"];
    args.extend(spot);
    args.extend(["--no-log", "--out", arg(out), "--last-street", last_street]);
    solver(&args).succeeded();
}

/// `inspect --json` of `file`, with `options`, read back.
fn inspect_json(file: &Path, options: &[&str]) -> Value {
    let mut args = vec!["inspect", arg(file), "--json"];
    args.extend(options);
    let run = solver(&args).succeeded();
    serde_json::from_str(&run.stdout).unwrap()
}

/// The turn spot solved and saved whole and without its river, a river spot, a file that is
/// not a solution file and one that is not a `.pksol`, a folder, and a solution file outside
/// the folder.
fn folder_of_files(name: &str) -> std::path::PathBuf {
    let parent = scratch(name);
    let folder = parent.join("solutions");
    fs::create_dir_all(folder.join("sub")).unwrap();
    solve_to(&["--spot", TURN], &folder.join("whole.pksol"), "river");
    solve_to(&["--spot", TURN], &folder.join("turn.pksol"), "turn");
    solve_to(
        &[
            "--ranges",
            RANGES,
            "--board",
            "Ks7d2c9h3s",
            "--tree",
            RIVER_TREE,
        ],
        &folder.join("river.pksol"),
        "river",
    );
    fs::write(folder.join("broken.pksol"), "format = 1\n").unwrap();
    fs::write(folder.join("notes.txt"), "not a solution\n").unwrap();
    fs::copy(folder.join("turn.pksol"), parent.join("outside.pksol")).unwrap();
    fs::copy(
        folder.join("turn.pksol"),
        folder.join("sub").join("inner.pksol"),
    )
    .unwrap();
    folder
}

/// The list, and nodes at each street of a turn spot, from the whole file and from the one
/// without its river, each as `inspect --json` gives it, and against the library.
#[test]
fn the_server_lists_the_files_and_answers_nodes_as_inspect_does() {
    let folder = folder_of_files("serve-nodes");
    let server = Server::start(&folder, &[]);

    // The list: the solution files by name, each from its header, and a file that is not one
    // with its error.
    let list = server.json("/api/files");
    let files = list["files"].as_array().unwrap();
    let names: Vec<&str> = files.iter().map(|f| f["name"].as_str().unwrap()).collect();
    assert_eq!(
        names,
        ["broken.pksol", "river.pksol", "turn.pksol", "whole.pksol"]
    );
    assert_eq!(
        files[0]["error"],
        "`broken.pksol` is not a solution file: it does not start with `PKSOL`; give a file \
         that solve-postflop --out wrote"
    );
    for (file, name) in files.iter().zip(&names).skip(1) {
        let path = folder.join(name);
        assert_eq!(file, &inspect_json(&path, &[])["file"], "{name}");
        let header = read_header(&path).unwrap();
        assert_eq!(file["iterations"], header.iterations);
        assert_eq!(file["bytes"], fs::metadata(&path).unwrap().len());
    }
    assert_eq!(files[2]["complete"], false);
    assert_eq!(files[3]["complete"], true);
    // Listing reads the headers alone.
    assert!(
        !server
            .lines
            .lock()
            .unwrap()
            .iter()
            .any(|line| line.contains("loaded `"))
    );

    // Nodes at each street of the whole file, each what inspect --json gives, the turn loaded
    // first and the river once a node on it is asked for.
    let whole = folder.join("whole.pksol");
    for (path, hand) in [
        ("", ""),
        ("b50", "AKs"),
        ("x x", ""),
        ("x x 3c", "AhTh"),
        ("x x 3c x x", ""),
        ("b50 f", ""),
    ] {
        let mut target = format!("/api/node?file=whole.pksol&path={}", path.replace(' ', "+"));
        let mut options = vec!["--node", path];
        if !hand.is_empty() {
            target.push_str(&format!("&hand={hand}"));
            options.extend(["--hand", hand]);
        }
        assert_eq!(
            server.json(&target),
            inspect_json(&whole, &options),
            "{target}"
        );
        if path.is_empty() {
            server.wait_for("loaded `whole.pksol` up to the turn, about ");
        }
    }
    server.wait_for("loaded `whole.pksol` up to the river, about ");
    // A path escaped as a browser escapes it, and in upper case.
    assert_eq!(
        server.json("/api/node?file=whole.pksol&path=X%20X%203C"),
        server.json("/api/node?file=whole.pksol&path=x+x+3c")
    );

    // The numbers against the library, on the turn and on the river.
    let saved = SavedSolution::load(&whole).unwrap();
    let solution = saved.solution();
    for path in ["", "x x 3c"] {
        let node = solution.game().find(path).unwrap();
        let evs = saved.ev(node).unwrap();
        let ev = |hand: usize| {
            let ev = evs[hand];
            (!ev.is_nan()).then_some(f64::from(ev))
        };
        let answer = server.json(&format!(
            "/api/node?file=whole.pksol&path={}&hand=AKs",
            path.replace(' ', "+")
        ));
        check_json_decision(&answer["node"], solution, node, &ev, 1.0);
    }

    // The river the other file leaves out is solved again, to the library's numbers.
    let partial = folder.join("turn.pksol");
    let mut answer = server.json("/api/node?file=turn.pksol&path=x+x+3c&hand=AhTh");
    let mut expected = inspect_json(&partial, &["--node", "x x 3c", "--hand", "AhTh"]);
    for value in [&mut answer, &mut expected] {
        let seconds = value["node"]["river"]["seconds"].take();
        assert!(seconds.as_f64().unwrap() >= 0.0);
    }
    assert_eq!(answer, expected);
    let saved = SavedSolution::load(&partial).unwrap();
    let start = saved.solution().game().find("x x 3c").unwrap();
    let river = saved.river_spot(start).unwrap();
    let options = SolveOptions {
        target: Some(0.01),
        max_iterations: 1_000,
        ..SolveOptions::default()
    };
    let solved = solve(&river.spot, &options, |_| {}).unwrap();
    let values = node_values(solved.game(), solved.strategy());
    let ev = |hand: usize| values.get(0).unwrap()[hand].map(|ev| ev - f64::from(river.put_in[0]));
    check_json_decision(
        &answer["node"],
        &solved,
        0,
        &ev,
        saved.solution().reach_probability(start),
    );

    // The page.
    let (status, content_type, body) = server.get("/");
    assert_eq!(status, 200);
    assert_eq!(content_type, "text/html; charset=utf-8");
    assert!(body.contains("/api/files"), "{body}");
    server.wait_for("GET / 200 ");
    drop(server);
    fs::remove_dir_all(folder.parent().unwrap()).unwrap();
}

/// Every refusal, each a JSON error of one line with its status, after which the server still
/// answers: a file name that would leave the folder, an unknown file or page, a file that is
/// not a solution file, a bad path, hand or key, and a solution over the memory limit.
#[test]
fn the_server_refuses_what_it_cannot_answer_and_goes_on() {
    let folder = folder_of_files("serve-refusals");
    let server = Server::start(&folder, &[]);

    // Names that would leave the folder, or name a file below it: 400, even for files that
    // exist there.
    for name in [
        "..%2Foutside.pksol",
        "..%5Coutside.pksol",
        "..",
        "sub%2Finner.pksol",
        "sub%5Cinner.pksol",
        "%2Fetc%2Fpasswd",
        "C%3A%5CWindows%5Cwin.ini",
        "C%3Aturn.pksol",
        "turn.pksol%3Astream",
    ] {
        let (status, message) = server.error(&format!("/api/node?file={name}"));
        assert_eq!(status, 400, "{name}: {message}");
        assert!(
            message.ends_with(
                "give the name of a solution file in the served folder, with no folder in it, \
                 as /api/files lists it"
            ),
            "{message}"
        );
    }
    // Files the folder does not hold as solution files: 404.
    for name in ["nope.pksol", "notes.txt", "sub"] {
        let (status, message) = server.error(&format!("/api/node?file={name}"));
        assert_eq!(status, 404, "{name}: {message}");
        assert!(
            message.starts_with(&format!(
                "file `{name}`: there is no solution file of that name in `"
            )),
            "{message}"
        );
    }
    let (status, message) = server.error("/api/nothing");
    assert_eq!(status, 404);
    assert!(
        message.starts_with("there is nothing at /api/nothing: "),
        "{message}"
    );
    // A file that is not a solution file, in inspect's words.
    let (status, message) = server.error("/api/node?file=broken.pksol");
    assert_eq!(status, 500);
    assert!(
        message.ends_with(
            "is not a solution file: it does not start with `PKSOL`; give a file that \
             solve-postflop --out wrote"
        ),
        "{message}"
    );

    // A bad path, hand or key: 400, in inspect's words.
    assert_eq!(
        server.error("/api/node?file=turn.pksol&path=x+b75"),
        (
            400,
            "path `x b75`: `b75` is not an action after `x`; the actions there are `x`, `b50` \
             and `a`"
                .to_string()
        )
    );
    assert_eq!(
        server.error("/api/node?file=turn.pksol&path=b50+f+x"),
        (
            400,
            "path `b50 f x`: `x` comes after the end of the hand, at `b50 f`, where IP folds"
                .to_string()
        )
    );
    let (status, message) = server.error("/api/node?file=turn.pksol&path=x+x+2s");
    assert_eq!(status, 400);
    assert!(
        message.starts_with("path `x x 2s`: `2s` cannot fall after `x x`: the 2s is on the board"),
        "{message}"
    );
    assert_eq!(
        server.error("/api/node?file=turn.pksol&hand=AK"),
        (
            400,
            "hand `AK`: give a hand class, such as AKs, AKo or 77, or a combo, such as AhKh"
                .to_string()
        )
    );
    assert_eq!(server.error("/api/node").0, 400);
    assert_eq!(
        server.error("/api/node?file=turn.pksol&node=x"),
        (
            400,
            "`node` is not a key here; the keys are `file`, `path`, `hand`".to_string()
        )
    );
    assert_eq!(server.error("/api/node?file=%ZZ").0, 400);
    assert_eq!(server.error("/api/files?file=turn.pksol").0, 400);

    // A request that is not a GET.
    let mut stream = TcpStream::connect(("127.0.0.1", server.port)).unwrap();
    write!(
        stream,
        "POST /api/files HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: 0\r\nConnection: \
         close\r\n\r\n"
    )
    .unwrap();
    let mut reply = String::new();
    stream.read_to_string(&mut reply).unwrap();
    assert!(reply.starts_with("HTTP/1.1 405 "), "{reply}");

    // After all of that, the server still answers.
    assert_eq!(
        server.json("/api/files")["files"].as_array().unwrap().len(),
        4
    );
    assert_eq!(
        server.json("/api/node?file=turn.pksol")["node"]["kind"],
        "decision"
    );
    drop(server);

    // A solution over the memory limit is refused, and the server goes on.
    let server = Server::start(&folder, &["--max-memory", "0.000001"]);
    let (status, message) = server.error("/api/node?file=turn.pksol");
    assert_eq!(status, 503);
    assert!(
        message.starts_with("file `turn.pksol`: its solution up to the turn needs about "),
        "{message}"
    );
    assert!(
        message.ends_with(
            " that --max-memory lets the server hold; restart it with a higher --max-memory, if \
             the machine has the memory"
        ),
        "{message}"
    );
    assert_eq!(
        server.json("/api/files")["files"].as_array().unwrap().len(),
        4
    );
    drop(server);
    fs::remove_dir_all(folder.parent().unwrap()).unwrap();
}

#[test]
fn a_folder_that_is_not_there_is_refused() {
    let folder = scratch("serve-missing");
    let missing = folder.join("no-such-folder");
    let message = solver(&["serve", arg(&missing)]).failed(1);
    assert_eq!(
        message,
        format!(
            "the folder `{}` does not exist; give the folder that holds the solution files",
            missing.display()
        )
    );
    let file = folder.join("file.pksol");
    fs::write(&file, "").unwrap();
    let message = solver(&["serve", arg(&file)]).failed(1);
    assert!(message.ends_with("is not a folder; give the folder that holds the solution files"));
    fs::remove_dir_all(&folder).unwrap();
}
