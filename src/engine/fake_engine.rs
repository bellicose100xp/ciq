//! `FakeEngine` — a deterministic, in-memory `QueryEngine` with **no DuckDB dependency**.
//!
//! This is the seam that makes the worker, dispatcher, debouncer, app-events, and autocomplete
//! layers headless-testable (North Star 2): those tests construct a `FakeEngine` with canned
//! responses and assert behavior with zero DuckDB/terminal linkage and fully deterministic
//! output.
//!
//! It does **not** parse or execute SQL. Instead it returns pre-seeded [`QueryOutcome`]s:
//! a default outcome for any unrecognized query, plus optional exact-match overrides keyed by
//! query string. It also exposes **counting hooks** (`load_count`, `query_count`) so tests can assert
//! invariants like "the CSV is parsed exactly once per session" and "N debounced keystrokes
//! produce exactly one query".
//!
//! Cancellation is modeled too: an [`InterruptHandle`] from a `FakeEngine` flips an internal
//! flag, and a query issued while "interrupted" returns [`QueryOutcome::Cancelled`] once, so
//! the out-of-band cancel wiring can be exercised without a real blocking call.

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};

use crate::engine::types::{Interruptible, QueryOutcome, Table};
use crate::engine::{CsvOpts, InterruptHandle, QueryEngine};
use crate::error::EngineError;
use crate::schema::Schema;

/// A deterministic gate that makes `FakeEngine::query` **block** until released — the seam the
/// out-of-band cancel test (P2.5) needs to hold a query "in-flight" while it dispatches a newer
/// one. It is channel-based (no sleeps): the query signals it has entered, then blocks on a
/// release channel; the interrupt path (via [`InterruptHandle`]) flips the interrupted flag and
/// releases the gate, so the blocked query unblocks and observes the flag.
struct Gate {
    /// The query sends `()` here the moment it enters `query()` and is about to block — the
    /// test waits on the paired receiver so the dispatch ordering is deterministic (no race).
    entered_tx: Sender<()>,
    /// The query blocks `recv()`-ing here; both an explicit release and an interrupt send `()`.
    release_rx: Mutex<Receiver<()>>,
}

/// The sender half the [`InterruptHandle`] / explicit release use to wake a blocked gated query.
#[derive(Clone)]
struct GateRelease {
    release_tx: Sender<()>,
}

/// A deterministic in-memory engine for tests.
pub struct FakeEngine {
    schema: Schema,
    default_outcome: QueryOutcome,
    overrides: HashMap<String, QueryOutcome>,
    load_count: Arc<AtomicUsize>,
    query_count: Arc<AtomicUsize>,
    distinct_count: Arc<AtomicUsize>,
    interrupted: Arc<AtomicBool>,
    /// When set, `query()` blocks on this gate until released/interrupted (see [`Gate`]).
    gate: Option<Arc<Gate>>,
    /// Release handle for the gate, shared with the [`InterruptHandle`] so an interrupt wakes a
    /// blocked query.
    gate_release: Option<GateRelease>,
}

impl FakeEngine {
    /// A fake engine with the given schema; unrecognized queries return an empty table.
    pub fn new(schema: Schema) -> Self {
        Self {
            schema,
            default_outcome: QueryOutcome::Rows(Table::default()),
            overrides: HashMap::new(),
            load_count: Arc::new(AtomicUsize::new(0)),
            query_count: Arc::new(AtomicUsize::new(0)),
            distinct_count: Arc::new(AtomicUsize::new(0)),
            interrupted: Arc::new(AtomicBool::new(false)),
            gate: None,
            gate_release: None,
        }
    }

    /// Set the outcome returned for any query without an exact-match override.
    pub fn with_default(mut self, outcome: QueryOutcome) -> Self {
        self.default_outcome = outcome;
        self
    }

    /// Register an exact-match response for a specific query string.
    pub fn with_response(mut self, sql: impl Into<String>, outcome: QueryOutcome) -> Self {
        self.overrides.insert(sql.into(), outcome);
        self
    }

    /// Make `query()` **block** until released or interrupted (the P2.5 cancel-dispatch seam).
    ///
    /// Returns a [`GateControl`] the test drives: `wait_entered()` blocks until a query has
    /// entered and is about to block (so the test can dispatch the *next* request without a
    /// race), and `release()` unblocks a gated query that should complete normally. An
    /// interrupt (via the engine's [`InterruptHandle`]) also unblocks the gate and makes the
    /// query return [`QueryOutcome::Cancelled`].
    pub fn with_gate(mut self) -> (Self, GateControl) {
        let (entered_tx, entered_rx) = channel();
        let (release_tx, release_rx) = channel();
        self.gate = Some(Arc::new(Gate {
            entered_tx,
            release_rx: Mutex::new(release_rx),
        }));
        self.gate_release = Some(GateRelease {
            release_tx: release_tx.clone(),
        });
        (
            self,
            GateControl {
                entered_rx,
                release_tx,
            },
        )
    }

    /// How many times `load` has been called (parse-once invariant guard).
    pub fn load_count(&self) -> usize {
        self.load_count.load(Ordering::SeqCst)
    }

    /// How many times `query` has been called (debounce-coalescing guard).
    pub fn query_count(&self) -> usize {
        self.query_count.load(Ordering::SeqCst)
    }

    /// How many times `distinct` has been called.
    pub fn distinct_count(&self) -> usize {
        self.distinct_count.load(Ordering::SeqCst)
    }

    /// Consume the "interrupted" flag if set: returns true once after an interrupt, then resets.
    fn take_interrupted(&self) -> bool {
        self.interrupted.swap(false, Ordering::SeqCst)
    }
}

impl QueryEngine for FakeEngine {
    fn load(&mut self, _path: &Path, _opts: &CsvOpts) -> Result<Schema, EngineError> {
        self.load_count.fetch_add(1, Ordering::SeqCst);
        Ok(self.schema.clone())
    }

    fn query(&self, sql: &str) -> QueryOutcome {
        self.query_count.fetch_add(1, Ordering::SeqCst);
        if let Some(gate) = &self.gate {
            // Signal "entered, about to block" then block until released or interrupted. Models
            // a real engine blocked inside DuckDB while the dispatcher decides to cancel it.
            let _ = gate.entered_tx.send(());
            let _ = gate.release_rx.lock().unwrap().recv();
        }
        if self.take_interrupted() {
            return QueryOutcome::Cancelled;
        }
        self.overrides
            .get(sql)
            .cloned()
            .unwrap_or_else(|| self.default_outcome.clone())
    }

    fn distinct(&self, _col: &str, _limit: usize) -> QueryOutcome {
        self.distinct_count.fetch_add(1, Ordering::SeqCst);
        if self.take_interrupted() {
            return QueryOutcome::Cancelled;
        }
        self.default_outcome.clone()
    }

    fn schema(&self) -> &Schema {
        &self.schema
    }

    fn interrupt_handle(&self) -> InterruptHandle {
        InterruptHandle::new(Arc::new(FakeInterrupt {
            flag: self.interrupted.clone(),
            gate_release: self.gate_release.clone(),
        }))
    }
}

/// Interrupt stand-in: sets the shared flag the engine reads on its next query, and — if the
/// engine is gated — wakes the blocked query so it can observe the flag and return `Cancelled`.
struct FakeInterrupt {
    flag: Arc<AtomicBool>,
    gate_release: Option<GateRelease>,
}

impl Interruptible for FakeInterrupt {
    fn interrupt(&self) {
        self.flag.store(true, Ordering::SeqCst);
        if let Some(release) = &self.gate_release {
            let _ = release.release_tx.send(());
        }
    }
}

/// Test-side control for a gated [`FakeEngine`] (`FakeEngine::with_gate`).
pub struct GateControl {
    entered_rx: Receiver<()>,
    release_tx: Sender<()>,
}

impl GateControl {
    /// Block until a query has entered `query()` and is about to block. Deterministic
    /// rendezvous — lets the test dispatch the next request only once the prior one is truly
    /// in-flight (no sleep, no race).
    pub fn wait_entered(&self) {
        let _ = self.entered_rx.recv();
    }

    /// Unblock a gated query so it completes normally (returns its canned outcome).
    pub fn release(&self) {
        let _ = self.release_tx.send(());
    }
}
