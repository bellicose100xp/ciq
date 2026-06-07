//! The query worker thread (`dev/PLAN.md` §2.4 / §3.1, §0/D1+D4).
//!
//! `spawn_worker` starts a dedicated background thread that **owns** the [`QueryEngine`] and
//! loops on a blocking `recv()` over the request channel. For each [`QueryRequest`] it runs
//! `engine.query(sql)` and maps the [`QueryOutcome`] one-to-one onto a [`QueryResponse`] sent
//! back on the response channel. The App thread never blocks on the engine.
//!
//! Why the worker owns the engine: DuckDB's `Connection` is `Send` but `!Sync`, so it must
//! never be shared across threads. The dispatcher cancels out-of-band through a separate
//! `Send + Sync` [`InterruptHandle`](crate::engine::InterruptHandle) clone (§0/D4) — the worker
//! never watches a cancel token, it only blocks in `query()` and returns `Cancelled` when
//! interrupted.
//!
//! Panic isolation (adapted from jiq): each request runs inside `panic::catch_unwind`, so a
//! panic inside the engine (a malformed-SQL edge case, a DuckDB bug) becomes a
//! `QueryResponse::Error { request_id }` for *that* request and the loop keeps serving the next
//! one — one bad query never tears the worker down or corrupts the TUI. A **quiet panic hook**
//! (log-only, no stderr) is installed for the worker's lifetime so the default hook's stderr
//! spew — which would corrupt a raw-mode terminal — is suppressed. The hook deliberately does
//! **not** send a response: that is the per-request catch's job, and a hook that also sent would
//! double-report (the hook fires for *caught* panics too). An outer `catch_unwind` around the
//! whole loop is the last-resort guard for a panic in the loop scaffolding itself (which is
//! logged; the loop has already ended at that point).
//!
//! Note: `set_hook`/`take_hook` are process-global. The worker installs the quiet hook while it
//! runs and restores the previous hook when the request channel closes. ciq's tests run
//! single-threaded (`--test-threads=1`, a load-bearing convention), so this global is never
//! contended by a concurrent test.

use std::panic::{self, AssertUnwindSafe};
use std::sync::mpsc::{Receiver, Sender};
use std::thread::JoinHandle;

use super::types::{ProcessedResult, QueryRequest, QueryResponse};
use crate::engine::{QueryEngine, QueryOutcome};

/// Spawn the query worker thread.
///
/// Takes ownership of `engine` (the worker is the sole query issuer), the request `Receiver`,
/// and the response `Sender`. Returns the thread's [`JoinHandle`] so tests can join after
/// dropping the request sender (which ends the loop). The loop runs until the request channel
/// closes.
pub fn spawn_worker(
    engine: Box<dyn QueryEngine>,
    request_rx: Receiver<QueryRequest>,
    response_tx: Sender<QueryResponse>,
) -> JoinHandle<()> {
    std::thread::spawn(move || {
        // Quiet panic hook: log only, no stderr (which would corrupt the raw-mode terminal) and
        // no response (the per-request catch owns response-sending, so a hook that also sent
        // would double-report).
        let prev_hook = panic::take_hook();
        panic::set_hook(Box::new(|info| {
            log::error!(
                "query worker panic: {} at {:?}",
                panic_message(info.payload()),
                info.location()
            );
        }));

        let result = panic::catch_unwind(AssertUnwindSafe(|| {
            worker_loop(engine.as_ref(), &request_rx, &response_tx);
        }));

        panic::set_hook(prev_hook);

        if let Err(payload) = result {
            log::error!("query worker thread panicked: {}", panic_message(&payload));
        }
    })
}

/// The blocking `recv()` loop — processes requests until the channel closes.
fn worker_loop(
    engine: &dyn QueryEngine,
    request_rx: &Receiver<QueryRequest>,
    response_tx: &Sender<QueryResponse>,
) {
    while let Ok(request) = request_rx.recv() {
        let request_id = request.request_id;
        // Catch a per-request panic so one bad query reports an error for its id and the loop
        // keeps serving the next request, rather than tearing down the worker.
        let response = panic::catch_unwind(AssertUnwindSafe(|| handle_request(engine, request)))
            .unwrap_or_else(|payload| QueryResponse::Error {
                message: format!("query panicked: {}", panic_message(&payload)),
                request_id,
            });
        if response_tx.send(response).is_err() {
            break; // App dropped the receiver; nothing left to serve.
        }
    }
}

/// Run one request and map its [`QueryOutcome`] to a [`QueryResponse`].
fn handle_request(engine: &dyn QueryEngine, request: QueryRequest) -> QueryResponse {
    let QueryRequest { query, request_id } = request;
    let (outcome, elapsed_ms) = timed(|| engine.query(&query));
    match outcome {
        QueryOutcome::Rows(table) => {
            let schema = table.schema();
            QueryResponse::ProcessedSuccess {
                result: ProcessedResult::new(table, schema, elapsed_ms),
                request_id,
            }
        }
        QueryOutcome::Error { message, .. } => QueryResponse::Error {
            message,
            request_id,
        },
        QueryOutcome::Cancelled => QueryResponse::Cancelled { request_id },
    }
}

/// Run `f`, returning its result and the wall-clock milliseconds it took.
///
/// The single wall-clock read in this module. Its `u64` output only ever fills
/// `ProcessedResult.execution_time_ms`, which is **redacted from snapshots** and never feeds
/// logic — so determinism is preserved despite the clock read. Confined here behind the
/// documented `disallowed_methods` seam allowance (same rationale as `logging.rs`).
#[allow(clippy::disallowed_methods)]
fn timed<T>(f: impl FnOnce() -> T) -> (T, u64) {
    let start = std::time::Instant::now();
    let out = f();
    (out, start.elapsed().as_millis() as u64)
}

/// Best-effort string from a panic payload (`&str` or `String`, else a placeholder).
fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "unknown panic".to_string()
    }
}

#[cfg(test)]
#[path = "thread_tests.rs"]
mod thread_tests;
