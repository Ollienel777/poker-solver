//! `solver serve`: a local HTTP server over a folder of solution files, for the viewer's page.
//! It listens on the loopback address only, so that nothing but this machine reaches it, and
//! answers one request at a time. What it answers is [`crate::server`]'s; this module listens,
//! hands each request over and prints a line for it.

use std::path::PathBuf;
use std::time::Instant;

use anyhow::{Result, anyhow, bail};
use clap::Args as ClapArgs;

use crate::format::{bytes, duration, field};
use crate::limits::parse_gigabytes;
use crate::server::State;

/// The address the server listens on: the loopback address, which only this machine reaches.
pub const HOST: &str = "127.0.0.1";

/// The options of `solver serve`.
#[derive(Debug, ClapArgs)]
pub struct Args {
    /// The folder of solution files to serve, as solve-postflop --out writes them
    #[arg(value_name = "FOLDER")]
    pub folder: PathBuf,

    /// The port to listen on, on 127.0.0.1; 0 takes any free port, which is printed
    #[arg(long, value_name = "N", default_value = "8080")]
    pub port: u16,

    /// Hold loaded solutions of at most this many gigabytes (10^9 bytes) by their estimates,
    /// dropping the least recently used first
    #[arg(
        long,
        value_name = "GB",
        default_value = "10",
        value_parser = parse_gigabytes
    )]
    pub max_memory: u64,
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
    let state = State::new(folder.clone(), args.max_memory);
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
            "up to {} of loaded solutions, by estimate, the least recently used dropped first",
            bytes(args.max_memory)
        ),
    );
    field("Stop", "with Ctrl-C");
    for request in server.incoming_requests() {
        let started = Instant::now();
        let method = request.method().as_str().to_string();
        let url = request.url().to_string();
        let reply = state.answer(&method, &url);
        let (status, json) = (reply.status, reply.is_json());
        for note in &reply.notes {
            println!("  {note}");
        }
        let mut response = tiny_http::Response::from_data(reply.body)
            .with_status_code(status)
            .with_header(header("Content-Type", reply.content_type));
        if json {
            response = response.with_header(header("Cache-Control", "no-store"));
        }
        // A client that went away before its reply is nothing to stop for.
        let sent = request.respond(response);
        let failed = match sent {
            Ok(()) => String::new(),
            Err(error) => format!(", not sent: {error}"),
        };
        println!(
            "{method} {url} {status} {}{failed}",
            duration(started.elapsed())
        );
    }
    Ok(())
}

fn header(name: &str, value: &str) -> tiny_http::Header {
    tiny_http::Header::from_bytes(name.as_bytes(), value.as_bytes())
        .expect("the header's name and value are ASCII")
}
