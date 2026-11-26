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
    /// `solver serve <folder> --port 0`, with `options`, run from the repository's root as
    /// `solve-postflop` is, once it says where it listens.
    fn start(folder: &Path, options: &[&str]) -> Server {
        let mut child = Command::new(env!("CARGO_BIN_EXE_solver"))
            .args(["serve", arg(folder), "--port", "0"])
            .args(options)
            .current_dir(root())
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
        self.request("GET", target, "")
    }

    /// The status, media type and body of a request with `method` for `target` with `body`.
    fn request(&self, method: &str, target: &str, body: &str) -> (u16, String, String) {
        let mut stream = TcpStream::connect(("127.0.0.1", self.port)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(120)))
            .unwrap();
        write!(
            stream,
            "{method} {target} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\n\
             Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
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

    /// The status and JSON of a POST of `body` to `target`.
    fn post(&self, target: &str, body: &Value) -> (u16, Value) {
        let (status, content_type, text) = self.request("POST", target, &body.to_string());
        assert_eq!(content_type, "application/json; charset=utf-8", "{text}");
        (status, serde_json::from_str(&text).unwrap())
    }

    /// The solve's status, once its state is one of `states`.
    fn wait_for_state(&self, states: &[&str]) -> Value {
        let started = Instant::now();
        loop {
            let status = self.json("/api/solve");
            if states.contains(&status["state"].as_str().unwrap()) {
                return status;
            }
            assert!(
                started.elapsed() < Duration::from_secs(300),
                "the solve is still {status}"
            );
            thread::sleep(Duration::from_millis(20));
        }
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

// ---------------------------------------------------------------------------------------------
// Solving from the page

/// A folder of inputs for solves from the page, under `parent`: `ranges/a/` with the gate's
/// range file and one that is not a range file, and `trees/` with the river tree of these tests
/// and the one-bet tree. Gives the options that serve them.
fn inputs_in(parent: &Path) -> Vec<String> {
    let ranges = parent.join("ranges");
    fs::create_dir_all(ranges.join("a")).unwrap();
    fs::copy(
        root().join(RANGES),
        ranges.join("a").join("BTN_vs_BB_SRP.toml"),
    )
    .unwrap();
    fs::write(ranges.join("a").join("broken.toml"), "format = 1\n").unwrap();
    let trees = parent.join("trees");
    fs::create_dir_all(&trees).unwrap();
    fs::copy(root().join(RIVER_TREE), trees.join("river_tree.toml")).unwrap();
    fs::copy(
        root().join("configs/trees/one_bet.toml"),
        trees.join("one_bet.toml"),
    )
    .unwrap();
    vec![
        "--ranges".to_string(),
        arg(&ranges).to_string(),
        "--trees".to_string(),
        arg(&trees).to_string(),
    ]
}

/// A solve of a river board under the river tree, which takes well under a second.
fn river_solve() -> Value {
    serde_json::json!({
        "ranges": "a/BTN_vs_BB_SRP.toml",
        "board": "Ks7d2c9h3s",
        "tree": "river_tree.toml",
    })
}

/// A solve of a turn board to a target of 0, which runs until its iteration cap, far off, or
/// until it is cancelled.
fn endless_solve() -> Value {
    serde_json::json!({
        "ranges": "a/BTN_vs_BB_SRP.toml",
        "board": "Ks7d2c9h",
        "tree": "one_bet.toml",
        "target": 0,
        "max_iterations": 100_000_000u64,
        "out": "endless",
    })
}

/// The inputs listed, a solve estimated without starting, then started, followed from running
/// to done, its file listed and its root answered as `inspect --json` gives it, its record in
/// the log, and the same name refused unless it is to be overwritten.
#[test]
fn a_solve_from_the_page_is_estimated_run_and_opened() {
    let parent = scratch("serve-solve");
    let folder = parent.join("solutions");
    fs::create_dir_all(&folder).unwrap();
    let log = parent.join("log.jsonl");
    let mut options = inputs_in(&parent);
    options.extend(["--log".to_string(), arg(&log).to_string()]);
    let options: Vec<&str> = options.iter().map(String::as_str).collect();
    let server = Server::start(&folder, &options);

    // The inputs.
    let inputs = server.json("/api/inputs");
    let ranges = inputs["ranges"].as_array().unwrap();
    assert_eq!(ranges.len(), 2, "{inputs}");
    assert_eq!(ranges[0]["name"], "a/BTN_vs_BB_SRP.toml");
    assert_eq!(ranges[0]["positions"], serde_json::json!(["BB", "BTN"]));
    assert_eq!(
        (ranges[0]["pot"].as_u64(), ranges[0]["stack"].as_u64()),
        (Some(550), Some(9750))
    );
    assert_eq!(ranges[0]["formation"], "BTN_vs_BB_SRP");
    assert!(
        ranges[0]["config_tree"]
            .as_str()
            .unwrap()
            .starts_with("flop: bets 33% and 75%")
    );
    assert!(ranges[0]["error"].is_null());
    assert_eq!(ranges[1]["name"], "a/broken.toml");
    assert!(ranges[1]["error"].is_string());
    let trees: Vec<&str> = inputs["trees"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tree| tree["name"].as_str().unwrap())
        .collect();
    assert_eq!(trees, ["one_bet.toml", "river_tree.toml"]);
    assert_eq!(
        inputs["trees"][1]["summary"],
        "river: bets 50% and 100% of the pot / no raises"
    );
    assert_eq!(inputs["max_memory"], 10_000_000_000u64);

    // The estimate alone starts nothing.
    let (status, estimate) = server.post("/api/estimate", &river_solve());
    assert_eq!(status, 200, "{estimate}");
    assert!(estimate["refusal"].is_null(), "{estimate}");
    assert_eq!(estimate["out"], "BTN_vs_BB_SRP_Ks9h7d3s2c.pksol");
    assert_eq!(estimate["exists"], false);
    assert_eq!(estimate["tree"], "river_tree.toml");
    assert_eq!(estimate["positions"], serde_json::json!(["BB", "BTN"]));
    assert_eq!(estimate["streets"], serde_json::json!(["river"]));
    assert_eq!(server.json("/api/solve")["state"], "idle");
    assert_eq!(server.json("/api/files")["files"], serde_json::json!([]));

    // Started, it runs on its own thread and ends done, its file written.
    let (status, started) = server.post("/api/solve", &river_solve());
    assert_eq!(status, 202, "{started}");
    assert!(
        ["running", "saving", "done"].contains(&started["state"].as_str().unwrap()),
        "{started}"
    );
    let done = server.wait_for_state(&["done", "failed", "cancelled"]);
    assert_eq!(done["state"], "done", "{done}");
    let file = folder.join("BTN_vs_BB_SRP_Ks9h7d3s2c.pksol");
    assert_eq!(done["result"]["file"], "BTN_vs_BB_SRP_Ks9h7d3s2c.pksol");
    let header = read_header(&file).unwrap();
    assert_eq!(done["result"]["iterations"], header.iterations);
    assert_eq!(
        done["result"]["exploitability"]["percent_of_pot"].as_f64(),
        Some(header.exploitability.percent_of_pot)
    );
    assert!(header.exploitability.percent_of_pot <= 0.3);
    let measurements = done["solve"]["measurements"].as_array().unwrap();
    assert_eq!(
        measurements.last().unwrap()["iterations"],
        header.iterations,
        "{done}"
    );
    assert_eq!(done["solve"]["iterations"], header.iterations);
    // The estimate was the library's, for the spot the file holds.
    let solve_options = SolveOptions {
        max_iterations: 10_000,
        ..SolveOptions::default()
    };
    let library = pk_postflop::estimate_memory(&header.spot, &solve_options).unwrap();
    assert_eq!(estimate["memory"]["solve"], library.total);
    assert_eq!(estimate["hands"], serde_json::json!(library.hands));
    assert_eq!(estimate["nodes"]["public"], header.nodes);

    // The file is listed, and its root is what inspect gives, at the library's numbers.
    let files = server.json("/api/files");
    assert_eq!(files["files"][0]["name"], "BTN_vs_BB_SRP_Ks9h7d3s2c.pksol");
    let root_node = server.json("/api/node?file=BTN_vs_BB_SRP_Ks9h7d3s2c.pksol");
    assert_eq!(root_node, inspect_json(&file, &[]));
    let saved = SavedSolution::load(&file).unwrap();
    let evs = saved.ev(0).unwrap();
    let ev = |hand: usize| (!evs[hand].is_nan()).then_some(f64::from(evs[hand]));
    check_json_decision(&root_node["node"], saved.solution(), 0, &ev, 1.0);
    // The solve's record is in the log.
    assert_eq!(fs::read_to_string(&log).unwrap().lines().count(), 1);

    // The same name again is refused, unless it is to be overwritten.
    let (status, refused) = server.post("/api/solve", &river_solve());
    assert_eq!(status, 400);
    assert_eq!(
        refused["error"],
        "out `BTN_vs_BB_SRP_Ks9h7d3s2c.pksol` already exists in the folder; ask to overwrite it, \
         or give another name"
    );
    assert_eq!(
        server.post("/api/estimate", &river_solve()).1["exists"],
        true
    );
    let mut again = river_solve();
    again["force"] = serde_json::json!(true);
    again["last_street"] = serde_json::json!("river");
    assert_eq!(server.post("/api/solve", &again).0, 202);
    assert_eq!(server.wait_for_state(&["done", "failed"])["state"], "done");
    assert_eq!(fs::read_to_string(&log).unwrap().lines().count(), 2);
    drop(server);
    fs::remove_dir_all(&parent).unwrap();
}

/// One solve at a time: a second start while one runs is refused with 409, a cancel stops it
/// at its next measurement and saves nothing, and the next solve starts.
#[test]
fn a_solve_runs_one_at_a_time_and_can_be_cancelled() {
    let parent = scratch("serve-cancel");
    let folder = parent.join("solutions");
    fs::create_dir_all(&folder).unwrap();
    let mut options = inputs_in(&parent);
    options.push("--no-log".to_string());
    let options: Vec<&str> = options.iter().map(String::as_str).collect();
    let server = Server::start(&folder, &options);
    assert_eq!(
        server.post("/api/solve/cancel", &Value::Null),
        (409, serde_json::json!({"error": "no solve is running"}))
    );

    let (status, started) = server.post("/api/solve", &endless_solve());
    assert_eq!(status, 202, "{started}");
    assert_eq!(started["state"], "running");
    // It cannot end before it is cancelled: its target is 0 and its cap far off.
    let (status, refused) = server.post("/api/solve", &river_solve());
    assert_eq!(status, 409);
    assert_eq!(
        refused["error"],
        "a solve of BTN_vs_BB_SRP_Ks9h7d2c into `endless.pksol` is running already, and the \
         server runs one at a time; wait for it to end, or cancel it"
    );
    // The server goes on answering while it runs, and its memory is counted.
    assert_eq!(server.json("/api/files")["files"], serde_json::json!([]));
    let estimate = server.post("/api/estimate", &river_solve()).1;
    assert_eq!(estimate["memory"]["solving"], started["solve"]["memory"]);
    assert!(started["solve"]["memory"].as_u64().unwrap() > 0);

    let (status, cancelling) = server.post("/api/solve/cancel", &Value::Null);
    assert_eq!(status, 200, "{cancelling}");
    assert_eq!(cancelling["cancelling"], true);
    let ended = server.wait_for_state(&["cancelled", "done", "failed"]);
    assert_eq!(ended["state"], "cancelled", "{ended}");
    assert!(!folder.join("endless.pksol").exists());
    assert!(!folder.join("endless.pksol.part").exists());
    server.wait_for("  solve cancelled");

    // The next solve starts, and ends.
    assert_eq!(server.post("/api/solve", &river_solve()).0, 202);
    assert_eq!(server.wait_for_state(&["done", "failed"])["state"], "done");
    drop(server);
    fs::remove_dir_all(&parent).unwrap();
}

/// A spot over a limit is refused with the estimate, and the words of `solve-postflop`, and
/// nothing starts; a bad input or a name that leaves its folder is refused with 400 or 404; a
/// solve that fails ends failed, and the server goes on.
#[test]
fn a_solve_from_the_page_is_refused_or_fails_and_the_server_goes_on() {
    let parent = scratch("serve-solve-refusals");
    let folder = parent.join("solutions");
    fs::create_dir_all(&folder).unwrap();
    let mut options = inputs_in(&parent);
    options.push("--no-log".to_string());
    let base: Vec<&str> = options.iter().map(String::as_str).collect();

    // Over the node limit, and over a tiny memory limit.
    // A turn's public tree is many times its betting tree, so a node limit between the two
    // is over the public tree's.
    let turn = serde_json::json!({
        "ranges": "a/BTN_vs_BB_SRP.toml",
        "board": "Ks7d2c9h",
        "tree": "one_bet.toml",
    });
    for (extra, request, kind) in [
        (["--max-nodes", "100"], &turn, "public_tree"),
        (["--max-memory", "0.001"], &river_solve(), "memory"),
    ] {
        let mut all = base.clone();
        all.extend(extra);
        let server = Server::start(&folder, &all);
        let (status, estimate) = server.post("/api/estimate", request);
        assert_eq!(status, 200);
        let refusal = &estimate["refusal"];
        assert_eq!(refusal["over"][0]["kind"], kind, "{estimate}");
        let message = refusal["message"].as_str().unwrap();
        assert!(message.contains(", so the solve is refused; "), "{message}");
        assert!(
            refusal["advice"]
                .as_str()
                .unwrap()
                .contains("give the tree file `"),
            "{refusal}"
        );
        let (status, refused) = server.post("/api/solve", request);
        assert_eq!(status, 422);
        assert_eq!(refused["error"].as_str(), Some(message));
        assert_eq!(refused["estimate"]["refusal"], *refusal);
        assert_eq!(server.json("/api/solve")["state"], "idle");
        assert_eq!(server.json("/api/files")["files"], serde_json::json!([]));
        drop(server);
    }

    let server = Server::start(&folder, &base);
    let with = |key: &str, value: Value| {
        let mut request = river_solve();
        request[key] = value;
        request
    };
    let error = |request: &Value| {
        let (status, body) = server.post("/api/estimate", request);
        let message = body["error"].as_str().unwrap_or_else(|| panic!("{body}"));
        assert_eq!(body.as_object().unwrap().len(), 1, "{body}");
        // The start refuses the same, with the same words.
        assert_eq!(server.post("/api/solve", request), (status, body.clone()));
        (status, message.to_string())
    };
    let (status, message) = error(&with("board", "Ks7d".into()));
    assert_eq!(status, 400);
    assert!(
        message.starts_with("board `Ks7d` has 2 cards;"),
        "{message}"
    );
    let (status, message) = error(&with("board", "KsKs7d".into()));
    assert_eq!(status, 400);
    assert!(message.contains("holds Ks twice"), "{message}");
    let (status, message) = error(&with("board", "Ks7d2x".into()));
    assert_eq!(status, 400);
    assert!(message.contains("`2x` is not a card"), "{message}");
    let (status, message) = error(&with("ranges", "a/nope.toml".into()));
    assert_eq!(status, 404);
    assert!(
        message.starts_with("ranges `a/nope.toml`: there is no range file"),
        "{message}"
    );
    for name in [
        "../a/BTN_vs_BB_SRP.toml",
        "a/../a/BTN_vs_BB_SRP.toml",
        "a\\BTN_vs_BB_SRP.toml",
        "/etc/passwd",
        "a/b/c.toml",
        "BTN_vs_BB_SRP.toml",
    ] {
        let (status, message) = error(&with("ranges", name.into()));
        assert_eq!(status, 400, "{name}: {message}");
        assert!(
            message.starts_with(&format!("ranges `{name}`: give a range file")),
            "{message}"
        );
    }
    let (status, message) = error(&with("ranges", "a/broken.toml".into()));
    assert_eq!(status, 400);
    assert!(message.contains("is not valid"), "{message}");
    for (name, wanted) in [
        ("../trees/one_bet.toml", 400),
        ("C:one_bet.toml", 400),
        ("nope.toml", 404),
    ] {
        assert_eq!(error(&with("tree", name.into())).0, wanted, "{name}");
    }
    for out in ["../x", "sub/x", "x.toml", "C:x"] {
        assert_eq!(error(&with("out", out.into())).0, 400, "{out}");
    }
    let (status, message) = error(&with("target", (-1).into()));
    assert_eq!(
        (status, message.as_str()),
        (
            400,
            "target -1: give a percent of the pot from 0 up, such as 0.3"
        )
    );
    let (status, message) = error(&with("last_street", "flop".into()));
    assert_eq!(status, 400);
    assert!(
        message.starts_with("last street `flop`: the board "),
        "{message}"
    );
    assert_eq!(error(&with("last_street", "preflop".into())).0, 400);
    assert_eq!(error(&with("nodes", 3.into())).0, 400);
    assert_eq!(error(&serde_json::json!({"board": "Ks7d2c"})).0, 400);
    let (status, _, body) = server.request("POST", "/api/solve", "not json");
    assert_eq!(status, 400, "{body}");
    assert_eq!(server.json("/api/solve")["state"], "idle");

    // A solve whose file cannot be written ends failed, saving nothing, and the server goes on.
    fs::create_dir_all(folder.join("fail.pksol.part")).unwrap();
    let (status, _) = server.post("/api/solve", &with("out", "fail".into()));
    assert_eq!(status, 202);
    let failed = server.wait_for_state(&["failed", "done"]);
    assert_eq!(failed["state"], "failed", "{failed}");
    assert!(
        failed["message"]
            .as_str()
            .unwrap()
            .starts_with("the solution was not saved: "),
        "{failed}"
    );
    assert!(!folder.join("fail.pksol").exists());
    assert_eq!(server.json("/api/files")["files"], serde_json::json!([]));
    assert_eq!(server.post("/api/solve", &river_solve()).0, 202);
    assert_eq!(server.wait_for_state(&["done", "failed"])["state"], "done");
    drop(server);
    fs::remove_dir_all(&parent).unwrap();
}

// ---------------------------------------------------------------------------------------------
// The gate

/// The viewer's gate (#95): the ranges recorded by hand for BTN against BB, on the flop Ks7d2c
/// under `configs/trees/one_bet.toml`, solved from the page's own endpoint with the
/// repository's range and tree folders, then asked for as the page asks, at decisions on every
/// street, at a deal and at an end. Each answer is what `inspect --json` prints for the node,
/// which the M5 gate's test checks cell by cell against the library, and the root is checked
/// against the library here as well. Run in release with `--nocapture`.
#[test]
#[ignore = "slow"]
fn the_gate_flop_solved_from_the_page_and_served_at_every_street() {
    let parent = scratch("serve-gate");
    let folder = parent.join("solutions");
    fs::create_dir_all(&folder).unwrap();
    let server = Server::start(&folder, &["--no-log"]);
    let spot = serde_json::json!({
        "ranges": "a/BTN_vs_BB_SRP_recorded.toml",
        "board": "Ks7d2c",
        "tree": "one_bet.toml",
        "out": "gate",
    });

    let (status, estimate) = server.post("/api/estimate", &spot);
    assert_eq!(status, 200, "{estimate}");
    assert!(estimate["refusal"].is_null(), "{estimate}");
    assert_eq!(estimate["nodes"]["public"], 191_844);

    let started = Instant::now();
    let (status, answer) = server.post("/api/solve", &spot);
    assert_eq!(status, 202, "{answer}");
    let done = server.wait_for_state(&["done", "failed", "cancelled"]);
    assert_eq!(done["state"], "done", "{done}");
    println!(
        "solved from the page and saved in {:.1?}: {} iterations, {}% of the pot",
        started.elapsed(),
        done["result"]["iterations"],
        done["result"]["exploitability"]["percent_of_pot"]
    );
    let file = folder.join("gate.pksol");
    assert!(read_header(&file).unwrap().exploitability.percent_of_pot <= 0.3);
    assert_eq!(server.json("/api/files")["files"][0]["name"], "gate.pksol");

    // What the page asks for, a click at a time, down one line of play to the river.
    let nodes = [
        ("", "decision", "flop"),
        ("x", "decision", "flop"),
        ("x x", "deal", "turn"),
        ("x b50 f", "end", "flop"),
        ("x b50 c", "deal", "turn"),
        ("x b50 c 8h", "decision", "turn"),
        ("x b50 c 8h x", "decision", "turn"),
        ("x b50 c 8h x b50 c", "deal", "river"),
        ("x b50 c 8h x b50 c 2s", "decision", "river"),
        ("x b50 c 8h x b50 c 2s x", "decision", "river"),
        ("x b50 c 8h x b50 c 2s x b50 c", "end", "river"),
    ];
    for (path, kind, street) in nodes {
        let started = Instant::now();
        let target = format!(
            "/api/node?file=gate.pksol&path={}&hand=AKs",
            path.replace(' ', "+")
        );
        let served = server.json(&target);
        let took = started.elapsed();
        assert_eq!(served["node"]["kind"], kind, "{path}");
        assert_eq!(served["node"]["street"], street, "{path}");
        assert_eq!(
            served,
            inspect_json(&file, &["--node", path, "--hand", "AKs"]),
            "{path}"
        );
        if kind == "decision" {
            assert_eq!(served["node"]["classes"].as_array().unwrap().len(), 169);
        }
        println!("`{path}`: a {kind} on the {street}, answered in {took:.1?}");
    }

    let saved = SavedSolution::load(&file).unwrap();
    let evs = saved.ev(0).unwrap();
    let ev = |hand: usize| (!evs[hand].is_nan()).then_some(f64::from(evs[hand]));
    let root_node = server.json("/api/node?file=gate.pksol");
    check_json_decision(&root_node["node"], saved.solution(), 0, &ev, 1.0);
    drop(server);
    fs::remove_dir_all(&parent).unwrap();
}
