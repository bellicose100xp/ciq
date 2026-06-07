//! ciq binary entry point.
//!
//! Parses CLI args, stands up debug logging, resolves the effective CSV ingest options
//! (CLI > config > sniffed), and — when given a file — launches the interactive TUI session via
//! the crossterm event loop (`ciq::app::event_loop::run`). Kept thin: the testable core lives in
//! the library (`ciq::*`), reachable without launching a terminal. Only the event loop touches a
//! real terminal (the §4.7 human surface).

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::Parser;

use ciq::engine::{CsvOpts, DuckdbEngine, QueryEngine, QueryOutcome};
use ciq::ingest::{load_csv_config_str, merge, parse_types_spec, sniff_bytes};
use ciq::output::{OutputFormat, render_output};
use ciq::query::preprocess::prepare_interactive;

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

    /// Field delimiter (e.g. ';' or a tab). Overrides sniffing.
    #[arg(long, value_name = "CHAR")]
    delim: Option<char>,

    /// Quote character. Overrides sniffing.
    #[arg(long, value_name = "CHAR")]
    quote: Option<char>,

    /// Escape character inside quoted fields.
    #[arg(long, value_name = "CHAR")]
    escape: Option<char>,

    /// Treat the first row as a header (the inverse of --no-header).
    #[arg(long, conflicts_with = "no_header")]
    header: bool,

    /// Treat the first row as data, not a header.
    #[arg(long)]
    no_header: bool,

    /// String that ingests as SQL NULL (e.g. "NA").
    #[arg(long, value_name = "STR")]
    null_string: Option<String>,

    /// Rows the type sniffer samples (-1 scans the whole file). Was --sniff-rows.
    #[arg(long, value_name = "N", alias = "sniff-rows")]
    sample_size: Option<i64>,

    /// Per-column type overrides, e.g. --types 'zip=VARCHAR,amount=DECIMAL(12,2)'.
    #[arg(long, value_name = "SPEC")]
    types: Option<String>,

    /// Ingest every column as VARCHAR (no type sniffing).
    #[arg(long)]
    all_varchar: bool,

    /// Explicit date parse format, e.g. --date-format '%d/%m/%Y'.
    #[arg(long, value_name = "FMT")]
    date_format: Option<String>,

    /// Run a query non-interactively and write the result to stdout in this format, then exit
    /// (no TUI). One of csv, tsv, json, markdown.
    #[arg(long, value_name = "FORMAT")]
    output: Option<String>,

    /// The SQL to run on the `--output` path. Defaults to `SELECT * FROM t` (the whole table).
    #[arg(long, short = 'q', value_name = "SQL")]
    query: Option<String>,
}

impl Cli {
    /// Build the *CLI* layer of [`CsvOpts`] from the parsed flags. An unset flag stays `None` so
    /// the config / sniffed layers decide it.
    fn to_csv_opts(&self) -> Result<CsvOpts, String> {
        let header = if self.header {
            Some(true)
        } else if self.no_header {
            Some(false)
        } else {
            None
        };
        let types = match &self.types {
            Some(spec) => Some(parse_types_spec(spec)?),
            None => None,
        };
        Ok(CsvOpts {
            delimiter: self.delim,
            quote: self.quote,
            escape: self.escape,
            header,
            null_string: self.null_string.clone(),
            sample_size: self.sample_size,
            types,
            all_varchar: if self.all_varchar { Some(true) } else { None },
            date_format: self.date_format.clone(),
        })
    }
}

/// Resolve the effective ingest options for `path`: merge CLI > config > sniffed.
///
/// The config layer reads the minimal `[csv]` section from `~/.config/ciq/config.toml` if it
/// exists (the full schema is Phase 5 / Q5). The sniffed layer reads the file's leading bytes.
/// Lives in `main.rs` (the CLI shim) because it touches the filesystem; the pieces it composes
/// (`merge`, `sniff_bytes`, `load_csv_config_str`, `Cli::to_csv_opts`) are each pure/headless.
fn resolve_opts(cli: &Cli, path: &Path) -> Result<CsvOpts, String> {
    let cli_opts = cli.to_csv_opts()?;

    let config_opts = read_config_opts();

    let sniffed_opts = match std::fs::read(path) {
        Ok(bytes) => {
            // Sniff only the leading bytes; DuckDB's own sniffer does the thorough pass at load.
            let head = &bytes[..bytes.len().min(64 * 1024)];
            sniff_bytes(head).to_opts()
        }
        // Unreadable here just means no sniff layer; the load below surfaces the real error.
        Err(_) => CsvOpts::default(),
    };

    Ok(merge(&config_opts, &cli_opts, &sniffed_opts))
}

/// Read the `[csv]` config layer from `~/.config/ciq/config.toml`, or the default if absent /
/// unreadable. (Minimal `[csv]`-only loader; the full config schema is Phase 5 / Q5.)
fn read_config_opts() -> CsvOpts {
    let Some(home) = std::env::var_os("HOME") else {
        return CsvOpts::default();
    };
    let path = Path::new(&home)
        .join(".config")
        .join("ciq")
        .join("config.toml");
    match std::fs::read_to_string(&path) {
        Ok(text) => load_csv_config_str(&text).to_opts(),
        Err(_) => CsvOpts::default(),
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    // Stand up debug logging first so everything after it can be instrumented. No-op (and
    // no file) unless --debug / CIQ_DEBUG=1.
    ciq::logging::init_logger(cli.debug);
    log::debug!("=== ciq debug session started ===");

    match &cli.file {
        Some(path) => {
            let opts = match resolve_opts(&cli, path) {
                Ok(o) => o,
                Err(e) => {
                    eprintln!("ciq: {e}");
                    return ExitCode::FAILURE;
                }
            };
            // `--output` => non-interactive: run one query, print the result, exit. No terminal.
            if let Some(fmt) = &cli.output {
                return run_output(path, &opts, fmt, cli.query.as_deref());
            }
            match ciq::app::event_loop::run(path.clone(), opts) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("ciq: {e}");
                    ExitCode::FAILURE
                }
            }
        }
        None => {
            // stdin ingest lands in a later phase; for now require a file.
            eprintln!("ciq: no file given (stdin ingest lands in a later phase)");
            ExitCode::FAILURE
        }
    }
}

/// The headless `--output` path: load `path` once, run `query` (default `SELECT * FROM t`),
/// serialize the result in `fmt`, and write it to stdout. Pure I/O at the edges; the formatting
/// (`render_output`) and the safety check (`prepare_interactive`) are headless-tested. This is
/// the integration seam ciq exposes for scripting and for the `--output` end-to-end test.
fn run_output(path: &Path, opts: &CsvOpts, fmt: &str, query: Option<&str>) -> ExitCode {
    let Some(format) = OutputFormat::parse(fmt) else {
        eprintln!("ciq: unknown --output format '{fmt}' (expected csv, tsv, json, or markdown)");
        return ExitCode::FAILURE;
    };

    let raw = query.unwrap_or("SELECT * FROM t");
    // Reuse the interactive grammar guard (single read-only SELECT — never mutates `t`) but with
    // an effectively-unbounded row cap: the Output Result path bypasses the viewport LIMIT (§2.3).
    // The cap is `i64::MAX` (a valid DuckDB BIGINT — `usize::MAX` overflows its INT64 LIMIT), so
    // it never truncates a real result while keeping the read-only/single-statement guard.
    const OUTPUT_LIMIT: usize = i64::MAX as usize;
    let sql = match prepare_interactive(raw, OUTPUT_LIMIT) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("ciq: {}", e.message());
            return ExitCode::FAILURE;
        }
    };

    let engine = match DuckdbEngine::open(path, opts) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("ciq: {e}");
            return ExitCode::FAILURE;
        }
    };

    match engine.query(&sql) {
        QueryOutcome::Rows(table) => {
            let schema = table.schema();
            print!("{}", render_output(&table, &schema, format));
            ExitCode::SUCCESS
        }
        QueryOutcome::Error { message, .. } => {
            eprintln!("ciq: {message}");
            ExitCode::FAILURE
        }
        // A non-interactive single query is never superseded, so it cannot be cancelled.
        QueryOutcome::Cancelled => {
            eprintln!("ciq: query cancelled");
            ExitCode::FAILURE
        }
    }
}
