//! `solver serve`: a local HTTP server over a folder of solution files, for the viewer's page,
//! which can also start a solve into that folder from range files and tree files. It listens on
//! the loopback address only, so that nothing but this machine reaches it, and answers one
//! request at a time, while a solve runs on a thread of its own. What it answers is
//! [`crate::server`]'s; this module listens, hands each request over and prints a line for it.

use std::io::Read;
use std::path::PathBuf;
use std::time::Instant;

use anyhow::{Result, anyhow, bail};
use clap::Args as ClapArgs;

use crate::commands::solve_postflop::DEFAULT_LOG;
use crate::format::{bytes, duration, field};
use crate::limits::{parse_gigabytes, parse_nodes};
use crate::server::State;
use crate::server::solving::{DEFAULT_RANGES, DEFAULT_TREES, Inputs};

/// The address the server listens on: the loopback address, which only this machine reaches.
pub const HOST: &str = "127.0.0.1";

/// The largest body a request may carry: a solve asked for is a few hundred bytes.
pub const MAX_BODY: u64 = 64 * 1024;

/// The options of `solver serve`.
#[derive(Debug, ClapArgs)]
pub struct Args {
    /// The folder of solution files to serve, as solve-postflop --out writes them; a solve
    /// from the page writes its file here
    #[arg(value_name = "FOLDER")]
    pub folder: PathBuf,

    /// The port to listen on, on 127.0.0.1; 0 takes any free port, which is printed
    #[arg(long, value_name = "N", default_value = "8080")]
    pub port: u16,

    /// Hold loaded solutions and a running solve of at most this many gigabytes (10^9 bytes)
    /// by their estimates, dropping the least recently used solution first
    #[arg(
        long,
        value_name = "GB",
        default_value = "10",
        value_parser = parse_gigabytes
    )]
    pub max_memory: u64,

    /// The folder of range files a solve from the page can use, laid out
    /// <config>/<formation>.toml; each one's config is read from configs/ under the current
    /// directory, as solve-postflop --ranges reads it
    #[arg(long, value_name = "DIR", default_value = DEFAULT_RANGES, help_heading = "Solving")]
    pub ranges: PathBuf,

    /// The folder of tree files a solve from the page can use
    #[arg(long, value_name = "DIR", default_value = DEFAULT_TREES, help_heading = "Solving")]
    pub trees: PathBuf,

    /// Refuse a solve from the page whose betting tree or public tree has more nodes than this
    #[arg(
        long,
        value_name = "N",
        default_value = "1000000",
        value_parser = parse_nodes,
        help_heading = "Solving"
    )]
    pub max_nodes: u64,

    /// The resource log, to which a record of each solve from the page is appended
    #[arg(long, value_name = "FILE", default_value = DEFAULT_LOG, help_heading = "Solving")]
    pub log: PathBuf,

    /// Append no record to the resource log
    #[arg(long, conflicts_with = "log", help_heading = "Solving")]
    pub no_log: bool,
}

pub fn run(args: &Args) -> Result<()> {
    let folder = &args.folder;
    if !folder.is_dir() {
        if folder.exists() {
            bail!(
                "`{}` is not a folder; give the folder that holds the solution files",
                folder.display()
            );
        }
        bail!(
            "the folder `{}` does not exist; give the folder that holds the solution files",
            folder.display()
        );
    }
    let server = tiny_http::Server::http((HOST, args.port)).map_err(|error| {
        anyhow!(
            "could not listen on {HOST}:{}: {error}; give another --port, or --port 0 for any \
             free one",
            args.port
        )
    })?;
    let port = server
        .server_addr()
        .to_ip()
        .map_or(args.port, |address| address.port());
    let inputs = Inputs {
        ranges: args.ranges.clone(),
        trees: args.trees.clone(),
        max_nodes: args.max_nodes,
        log: (!args.no_log).then(|| args.log.clone()),
    };
    let state = State::with_inputs(folder.clone(), args.max_memory, inputs);
    field(
        "Serving",
        format!(
            "the solution files of `{}` at http://{HOST}:{port}/",
            folder.display()
        ),
    );
    field(
        "Memory",
        format!(
            "up to {} of loaded solutions and a running solve, by estimate, the least recently \
             used solution dropped first",
            bytes(args.max_memory)
        ),
    );
    field(
        "Solving",
        format!(
            "from the range files of `{}` and the tree files of `{}`",
            args.ranges.display(),
            args.trees.display()
        ),
    );
    field("Stop", "with Ctrl-C");
    for mut request in server.incoming_requests() {
        let started = Instant::now();
        let method = request.method().as_str().to_string();
        let url = request.url().to_string();
        let mut body = Vec::new();
        let read = request
            .as_reader()
            .take(MAX_BODY + 1)
            .read_to_end(&mut body);
        let (status, content_type, body, notes) = match read {
            Err(error) => error_reply(
                400,
                &format!("the request's body could not be read: {error}"),
            ),
            Ok(_) if body.len() as u64 > MAX_BODY => error_reply(
                413,
                &format!("the request's body is over {} bytes", MAX_BODY),
            ),
            Ok(_) => {
                let reply = state.answer(&method, &url, &body);
                let json = reply.is_json();
                (
                    reply.status,
                    if json { None } else { Some(reply.content_type) },
                    reply.body,
                    reply.notes,
                )
            }
        };
        for note in &notes {
            println!("  {note}");
        }
        let mut response = tiny_http::Response::from_data(body).with_status_code(status);
        response = match content_type {
            Some(content_type) => response.with_header(header("Content-Type", content_type)),
            None => response
                .with_header(header("Content-Type", "application/json; charset=utf-8"))
                .with_header(header("Cache-Control", "no-store")),
        };
        // A client that went away before its reply is nothing to stop for.
        let sent = request.respond(response);
        let failed = match sent {
            Ok(()) => String::new(),
            Err(error) => format!(", not sent: {error}"),
        };
        // The page polls a running solve about once a second; those lines would bury the rest.
        if method == "GET" && url == "/api/solve" && status == 200 && failed.is_empty() {
            continue;
        }
        println!(
            "{method} {url} {status} {}{failed}",
            duration(started.elapsed())
        );
    }
    Ok(())
}

/// An error as the server replies with it before the request reaches [`State::answer`]: its
/// status, no media type of its own (JSON), its body and no notes.
fn error_reply(status: u16, message: &str) -> (u16, Option<&'static str>, Vec<u8>, Vec<String>) {
    let body =
        serde_json::to_vec(&serde_json::json!({ "error": message })).expect("an error is JSON");
    (status, None, body, Vec::new())
}

fn header(name: &str, value: &str) -> tiny_http::Header {
    tiny_http::Header::from_bytes(name.as_bytes(), value.as_bytes())
        .expect("the header's name and value are ASCII")
}
