//! ciq binary entry point.
//!
//! Parses CLI args, stands up debug logging, and — when given a file — launches the interactive
//! TUI session via the crossterm event loop (`ciq::app::event_loop::run`). Kept thin: the
//! testable core lives in the library (`ciq::*`), reachable without launching a terminal. Only
//! the event loop touches a real terminal (the §4.7 human surface).

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;

use ciq::engine::CsvOpts;

/// CSV Interactive Query — type DuckDB SQL, watch an aligned grid update live.
#[derive(Debug, Parser)]
#[command(name = "ciq", version, about)]
struct Cli {
    /// CSV file to open. If omitted, ciq reads from stdin (wired in a later phase).
    #[arg(value_name = "FILE")]
    file: Option<PathBuf>,

    /// Enable debug logging to /tmp/ciq/ciq-debug.log (file only; never the terminal).
    #[arg(long)]
    debug: bool,
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    // Stand up debug logging first so everything after it can be instrumented. No-op (and
    // no file) unless --debug / CIQ_DEBUG=1.
    ciq::logging::init_logger(cli.debug);
    log::debug!("=== ciq debug session started ===");

    match cli.file {
        Some(path) => match ciq::app::event_loop::run(path, CsvOpts::default()) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("ciq: {e}");
                ExitCode::FAILURE
            }
        },
        None => {
            // stdin ingest lands in a later phase; for now require a file.
            eprintln!("ciq: no file given (stdin ingest lands in a later phase)");
            ExitCode::FAILURE
        }
    }
}
