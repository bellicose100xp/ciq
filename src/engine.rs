//! Engine box — the swappable query engine behind a single trait.
//!
//! Canonical per `dev/PLAN.md` §0/D1. The rest of ciq talks to `dyn QueryEngine` and never
//! knows whether it's the real DuckDB engine or the in-memory `FakeEngine` used by tests —
//! this is the seam that keeps the worker/dispatcher/grid/autocomplete layers headless-
//! testable with zero DuckDB dependency, and that makes a future DataFusion swap a one-module
//! change.
//!
//! Conventions (jiq-inherited): no `mod.rs`; the trait lives here directly (avoiding a
//! same-name inner module); result types in `engine/types.rs`; impls in their own submodules
//! as they land (`duckdb_engine`, `fake_engine`).

pub mod duckdb_engine;
pub mod fake_engine;
pub mod types;

pub use duckdb_engine::DuckdbEngine;
pub use fake_engine::FakeEngine;
pub use types::{Cell, Column, InterruptHandle, QueryOutcome, Table};

use std::path::Path;

use crate::error::EngineError;
use crate::schema::Schema;

/// The query engine contract. One production impl (`DuckdbEngine`) and one test impl
/// (`FakeEngine`) satisfy it.
///
/// Design notes (§0/D1):
/// - **`query` returns `QueryOutcome`, not `Result`** — a SQL error or a cancellation is a
///   normal arm of the hot path, not an exceptional failure.
/// - **`query` takes no cancel argument** — cancellation is out-of-band (§0/D4): the worker
///   blocks here and cannot watch a token; the dispatcher interrupts via `interrupt_handle()`.
/// - **`load` is the only `Result<_, EngineError>` surface** — a genuine ingest failure
///   (unreadable/again-malformed file, OOM) *is* exceptional.
/// - **`&self` on `query`/`distinct`** — DuckDB's `Connection` uses interior mutability, so
///   the worker can issue queries through a shared reference while the dispatcher holds an
///   `InterruptHandle` clone.
pub trait QueryEngine: Send {
    /// Parse the CSV at `path` once into a resident in-memory table and return its schema.
    /// Called exactly once per session (the parse-once north star). `opts` carries CSV
    /// dialect/override settings (added in the ingest phase).
    fn load(&mut self, path: &Path, opts: &CsvOpts) -> Result<Schema, EngineError>;

    /// Run a read-only SQL query against the resident table. Returns rows, a SQL error, or
    /// `Cancelled` if interrupted. Never blocks the App thread (runs on the worker).
    fn query(&self, sql: &str) -> QueryOutcome;

    /// Distinct values of `col` (capped at `limit`) for value-autocomplete / facets. Returns
    /// a `QueryOutcome` so it flows through the same handling/cancellation path as `query`.
    fn distinct(&self, col: &str, limit: usize) -> QueryOutcome;

    /// The schema captured at load. Borrowed read-only by autocomplete/grid/etc.
    fn schema(&self) -> &Schema;

    /// A cheap, `Send + Sync`, cloneable handle the dispatcher holds to interrupt the
    /// in-flight query from its own thread (§0/D4).
    fn interrupt_handle(&self) -> InterruptHandle;
}

/// CSV ingest / dialect options (delimiter, quote, header, type overrides, …).
///
/// P1.3 placeholder: the full field set + CLI/config wiring lands in the ingest phase
/// (PLAN.md §6.6 / R5 — add `types`/`all_varchar`/`date_format`, unify `sniff_rows` with
/// `sample_size`). `Default` = "let DuckDB auto-detect everything", which is the common path.
#[derive(Debug, Clone, Default)]
pub struct CsvOpts {
    // Fields added in the ingest phase. Keep `Default` meaningful (full auto-detect).
}

#[cfg(test)]
#[path = "engine/types_tests.rs"]
mod types_tests;

#[cfg(test)]
#[path = "engine/duckdb_engine_tests.rs"]
mod duckdb_engine_tests;

#[cfg(test)]
#[path = "engine/fake_engine_tests.rs"]
mod fake_engine_tests;
