//! ciq binary entry point.
//!
//! P1.1 scaffold: parses CLI args and exits. Ingest, the App, and the TUI loop are
//! wired in later phases (P2). Kept thin — the testable core lives in the library
//! (`ciq::*`), reachable without launching a terminal.

use std::path::PathBuf;

use clap::Parser;

/// CSV Interactive Query — type DuckDB SQL, watch an aligned grid update live.
#[derive(Debug, Parser)]
#[command(name = "ciq", version, about)]
struct Cli {
    /// CSV file to open. If omitted, ciq reads from stdin (wired in a later phase).
    #[arg(value_name = "FILE")]
    file: Option<PathBuf>,
}

fn main() {
    let cli = Cli::parse();

    // P1.1: no interactive surface yet. Acknowledge the arg and exit cleanly so
    // `ciq --version` / `ciq --help` work and the binary smoke-tests pass.
    match cli.file {
        Some(path) => {
            eprintln!(
                "ciq: scaffold build — would open {} (ingest + TUI land in Phase 2)",
                path.display()
            );
        }
        None => {
            eprintln!("ciq: scaffold build — no file given (stdin ingest lands in Phase 2)");
        }
    }
}
