//! CSV ingest — dialect detection, override options, and the `read_csv(...)` SQL builder.
//!
//! ciq parses the CSV exactly once at load (the parse-once north star, §6.6). What DuckDB
//! parses it *as* — delimiter, quote, header, per-column types — is governed by three layers,
//! merged with a fixed precedence:
//!
//! ```text
//!   CLI flags  >  [csv] config section  >  sniffed defaults  >  full auto-detect
//! ```
//!
//! Each layer is a [`CsvOpts`] (every field `Option`, `None` = "defer to the layer below").
//! [`csv_opts::merge`] folds the three into one effective `CsvOpts`; [`csv_opts::to_read_csv_sql`]
//! turns that into the exact `read_csv('<path>', ...)` (or `read_csv_auto`) call the engine runs.
//!
//! Everything here is **pure and headless**: the sniffer is a function over fixture bytes, the
//! merge is a function over three structs, and the SQL builder is a `String`-returning function
//! that runs nothing — so an agent verifies the engine invocation byte-for-byte without spawning
//! DuckDB (North Star 2).
//!
//! The three genuinely-open ingest decisions are resolved here and recorded in
//! `dev/DECISIONS.md`:
//!  - **Q3** column-name policy: keep raw header names; auto-double-quote on emit (via the shared
//!    [`crate::sql_ident`]); lean on DuckDB for duplicate/empty header dedup.
//!  - **Q7** ragged-row policy: lean on DuckDB's detector — under default auto-detect a ragged
//!    file degrades to a single text column (no error), and under an explicit delimiter a short
//!    row is a clean `EngineError::Load`. Both paths are fail-safe: never a panic, never silent
//!    corruption.
//!  - **Q12** empty-vs-NULL: follow DuckDB's default — *every* empty field (unquoted `,,` OR
//!    quoted `,"",`) ingests as SQL NULL; `null_string` is the user lever to keep empties as `''`.

pub mod csv_config;
pub mod csv_opts;
pub mod sniff;

pub use csv_config::{CsvConfig, load_csv_config_str};
pub use csv_opts::{ColumnTypeOverride, CsvOpts, merge, parse_types_spec, to_read_csv_sql};
pub use sniff::{SniffResult, sniff, sniff_bytes};

// Q3/Q7/Q12 resolution: end-to-end fixture tests that pin the *engine's* observed ingest
// semantics (column-name dedup, ragged-row error, empty-vs-NULL) against committed fixtures in
// `tests/fixtures/`. These run a real `DuckdbEngine` (the only place ingest is allowed to touch
// the engine — and only in tests), so they live under `#[cfg(test)]`.
#[cfg(test)]
#[path = "ingest/ingest_semantics_tests.rs"]
mod ingest_semantics_tests;
