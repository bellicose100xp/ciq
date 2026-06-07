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
//! - **`ProcessedResult` carries tabular data, not JSON** (§0/S6): the pre-computed
//!   [`GridLayout`], the columnar [`Table`] of rows, the result [`Schema`], and the
//!   redacted-from-snapshots `execution_time_ms`. jiq's JSON-only `parsed` field is dropped,
//!   and its `line_count`/`max_width`/`line_widths`/`result_type` are not carried because the
//!   grid renderer derives them from the `GridLayout` (`body.len()`, `total_width`), so storing
//!   them would be redundant denormalized state.

use crate::engine::Table;
use crate::grid::GridLayout;
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

/// A fully processed successful query result: the laid-out grid plus the data behind it.
///
/// Built on the worker thread (off the UI thread) from a `QueryOutcome::Rows`. Carries exactly
/// the fields with a real ciq consumer (§0/S6): the pre-computed [`GridLayout`] the renderer
/// paints, the [`Table`] of `rows` (kept so re-layout on resize and copy-target selection work
/// without re-querying), the result [`Schema`], and `execution_time_ms`.
#[derive(Debug, Clone)]
pub struct ProcessedResult {
    /// The pre-computed aligned grid (header + body lines + per-column geometry) — what the
    /// blit (`grid_render`) paints. Computed on the worker thread.
    pub grid: GridLayout,
    /// The columnar result table. Retained so the App can re-layout the grid on a resize and
    /// select copy targets (a cell/row/column) without issuing another query.
    pub rows: Table,
    /// The result schema (column names + types), derived from the result columns.
    pub schema: Schema,
    /// Wall-clock execution time of the query, in milliseconds. **Redacted from snapshots**
    /// (the determinism rule) so timing never flips a golden.
    pub execution_time_ms: u64,
}

impl ProcessedResult {
    pub fn new(grid: GridLayout, rows: Table, schema: Schema, execution_time_ms: u64) -> Self {
        Self {
            grid,
            rows,
            schema,
            execution_time_ms,
        }
    }
}

/// The response from the worker for one [`QueryRequest`], correlated by `request_id`.
///
/// Three arms map one-to-one onto `QueryOutcome` (`Rows`→`ProcessedSuccess`, `Error`→`Error`,
/// `Cancelled`→`Cancelled`), so the worker's match is total and compiler-checked. A `request_id`
/// of `0` on `Error` marks a worker-level panic (no specific query), which the App applies
/// immediately rather than stale-discarding.
#[derive(Debug, Clone)]
pub enum QueryResponse {
    /// The query succeeded; carries the processed result and its `request_id`.
    ProcessedSuccess {
        result: ProcessedResult,
        request_id: u64,
    },
    /// The query failed (invalid SQL, or `request_id == 0` for a worker-level panic).
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
