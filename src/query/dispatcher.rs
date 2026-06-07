//! Out-of-band query dispatcher (`dev/PLAN.md` §3.1, §0/D4).
//!
//! The dispatcher is the App-thread half of the cancellation model. It owns:
//! - the `Sender<QueryRequest>` to the worker,
//! - a clone of the engine's `Send + Sync` [`InterruptHandle`], and
//! - a [`QueryState`] tracking the latest issued `request_id` and whether a query is in-flight.
//!
//! On [`dispatch`](Dispatcher::dispatch): if a prior request is believed in-flight, it calls
//! `.interrupt()` on the handle **from this (dispatcher) thread** *before* sending the new
//! request — this is the whole point of D4: DuckDB's `Connection` is `Send + !Sync`, so the
//! worker (which owns it and is blocked inside `query()`) cannot interrupt itself; only another
//! thread holding the `Send + Sync` handle can. The worker then returns `Cancelled` for the
//! superseded id, which the App drains (it's stale per [`is_stale`]) before the new result
//! surfaces.
//!
//! `interrupt()` is **not** request-scoped — it cancels whatever query is running — so the
//! dispatcher only fires it while it believes a specific request is in-flight (the
//! [`QueryState::in_flight`] gate), and the worker drains the `Cancelled` before dequeuing the
//! next request (§0/D4 invariant).

use std::sync::mpsc::{SendError, Sender};

use crate::engine::InterruptHandle;
use crate::query::query_state::{QueryState, is_stale};
use crate::query::worker::types::QueryRequest;

/// The App-side dispatcher: supersede-and-interrupt on each new query, stale-discard on each
/// response.
pub struct Dispatcher {
    request_tx: Sender<QueryRequest>,
    interrupt: InterruptHandle,
    state: QueryState,
}

impl Dispatcher {
    /// Build a dispatcher over the worker's request channel and the engine's interrupt handle.
    pub fn new(request_tx: Sender<QueryRequest>, interrupt: InterruptHandle) -> Self {
        Self {
            request_tx,
            interrupt,
            state: QueryState::new(),
        }
    }

    /// Swap in the real engine interrupt handle once the engine finishes loading.
    ///
    /// The shell builds the `App` (and thus the dispatcher) **before** the off-thread CSV load
    /// completes, so it starts with a no-op placeholder handle; the loader hands back the real
    /// `Arc<duckdb::InterruptHandle>` on completion and the event loop installs it here. Safe to
    /// do at any point: no query can be in-flight to interrupt until after load, since
    /// `dispatch()` only fires `interrupt()` while [`in_flight`](Self::in_flight) is true.
    pub fn set_interrupt(&mut self, interrupt: InterruptHandle) {
        self.interrupt = interrupt;
    }

    /// Dispatch a new query. If a prior request is in-flight, interrupt it first (from this
    /// thread), then issue a fresh monotonic `request_id` and send the request. Returns the new
    /// id.
    ///
    /// Errors only if the worker's receiver has been dropped (the worker is gone).
    pub fn dispatch(&mut self, query: impl Into<String>) -> Result<u64, SendError<QueryRequest>> {
        if self.state.in_flight() {
            // Supersede the in-flight query: cancel whatever is running, from the dispatcher
            // thread (D4). Safe because we only do this while a request is known in-flight.
            self.interrupt.interrupt();
        }
        let request_id = self.state.issue();
        self.request_tx.send(QueryRequest::new(query, request_id))?;
        Ok(request_id)
    }

    /// Whether an arriving response id should be surfaced (it is the latest issued) or dropped
    /// as stale. Accepting clears the in-flight flag. Thin pass-through to [`QueryState::accept`]
    /// so the App has a single place to ask "is this response current?".
    pub fn accept(&mut self, response_id: u64) -> bool {
        self.state.accept(response_id)
    }

    /// Whether `response_id` is stale relative to the latest issued id (without mutating state).
    pub fn is_stale(&self, response_id: u64) -> bool {
        is_stale(response_id, self.state.latest_id())
    }

    /// The most recently issued request id.
    pub fn latest_id(&self) -> u64 {
        self.state.latest_id()
    }

    /// Whether a query is currently believed in-flight.
    pub fn in_flight(&self) -> bool {
        self.state.in_flight()
    }
}

#[cfg(test)]
#[path = "dispatcher_tests.rs"]
mod dispatcher_tests;
