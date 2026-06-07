//! Worker channel types: `QueryRequest`, `QueryResponse`, and `ProcessedResult`.
//!
//! The worker contract is reused from jiq (`dev/PLAN.md` §3.2) — a `QueryRequest` goes out, a
//! `QueryResponse::{ProcessedSuccess, Error, Cancelled}` comes back, each carrying a
//! `request_id` for stale-discard. ciq diverges on two settled points:
//!
//! - **No `cancel_token` on `QueryRequest`** (§0/D4). Cancellation is out-of-band: the
//!   dispatcher holds an [`InterruptHandle`](crate::engine::InterruptHandle) clone and calls
//!   `.interrupt()` from its own thread when a newer `request_id` supersedes the in-flight
//!   query. The worker only ever blocks in `engine.query()` and returns `Cancelled`.
//! - **`ProcessedResult` carries tabular data, not JSON** (§0/S6): the columnar [`Table`] of
//!   rows, the result [`Schema`], and the redacted-from-snapshots `execution_time_ms`. jiq's
//!   JSON-only `parsed` field is dropped, and no pre-laid-out grid is carried: the App re-lays
//!   out from `rows` against the *real* terminal viewport on every frame (resize reflow without
//!   re-querying), so a worker-side grid against a fixed viewport would be thrown away. Anything
//!   the grid would derive (row count, widths) comes from `rows`, so storing it would be
//!   redundant denormalized state.

use crate::engine::Table;
use crate::schema::Schema;

/// A request to run one SQL query, stamped with a monotonic `request_id` for stale-discard.
///
/// No `cancel_token`: cancellation is out-of-band (§0/D4) — the dispatcher interrupts the
/// in-flight query directly through its [`InterruptHandle`](crate::engine::InterruptHandle)
/// clone, so the worker never needs to watch a token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryRequest {
    /// The SQL to run (already preprocessed/LIMIT-wrapped by the dispatcher).
    pub query: String,
    /// Monotonic id; the dispatcher discards any response whose id isn't the latest issued.
    pub request_id: u64,
}

impl QueryRequest {
    pub fn new(query: impl Into<String>, request_id: u64) -> Self {
        Self {
            query: query.into(),
            request_id,
        }
    }
}

/// A fully processed successful query result: the data behind the grid.
///
/// Built on the worker thread (off the UI thread) from a `QueryOutcome::Rows`. Carries exactly
/// the fields with a real ciq consumer (§0/S6): the [`Table`] of `rows` (the App lays the grid
/// out from it against the real viewport on every frame, and selects copy targets, without
/// re-querying), the result [`Schema`], and `execution_time_ms`.
#[derive(Debug, Clone)]
pub struct ProcessedResult {
    /// The columnar result table. The App lays out the grid from it against the real viewport
    /// (so a resize reflows without re-querying) and selects copy targets (a cell/row/column).
    pub rows: Table,
    /// The result schema (column names + types), derived from the result columns.
    pub schema: Schema,
    /// Wall-clock execution time of the query, in milliseconds. **Redacted from snapshots**
    /// (the determinism rule) so timing never flips a golden.
    pub execution_time_ms: u64,
}

impl ProcessedResult {
    pub fn new(rows: Table, schema: Schema, execution_time_ms: u64) -> Self {
        Self {
            rows,
            schema,
            execution_time_ms,
        }
    }
}

/// The response from the worker for one [`QueryRequest`], correlated by `request_id`.
///
/// Three arms map one-to-one onto `QueryOutcome` (`Rows`→`ProcessedSuccess`, `Error`→`Error`,
/// `Cancelled`→`Cancelled`), so the worker's match is total and compiler-checked. Every response
/// carries the real `request_id` of the query it answers — including the per-request panic catch,
/// which surfaces as `Error` under that query's id (and is stale-discarded like any other
/// response if a newer query has superseded it).
#[derive(Debug, Clone)]
pub enum QueryResponse {
    /// The query succeeded; carries the processed result and its `request_id`.
    ProcessedSuccess {
        result: ProcessedResult,
        request_id: u64,
    },
    /// The query failed (invalid SQL, or the engine panicked while running it — caught
    /// per-request and reported under that query's `request_id`).
    Error { message: String, request_id: u64 },
    /// The query was interrupted (superseded). The App discards it by `request_id`.
    Cancelled { request_id: u64 },
}

impl QueryResponse {
    /// The `request_id` this response is correlated with (for stale-discard).
    pub fn request_id(&self) -> u64 {
        match self {
            QueryResponse::ProcessedSuccess { request_id, .. } => *request_id,
            QueryResponse::Error { request_id, .. } => *request_id,
            QueryResponse::Cancelled { request_id } => *request_id,
        }
    }
}

#[cfg(test)]
#[path = "types_tests.rs"]
mod types_tests;
