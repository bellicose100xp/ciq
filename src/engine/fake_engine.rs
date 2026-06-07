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
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use crate::engine::types::{Interruptible, QueryOutcome, Table};
use crate::engine::{CsvOpts, InterruptHandle, QueryEngine};
use crate::error::EngineError;
use crate::schema::Schema;

/// A deterministic in-memory engine for tests.
pub struct FakeEngine {
    schema: Schema,
    default_outcome: QueryOutcome,
    overrides: HashMap<String, QueryOutcome>,
    load_count: Arc<AtomicUsize>,
    query_count: Arc<AtomicUsize>,
    distinct_count: Arc<AtomicUsize>,
    interrupted: Arc<AtomicBool>,
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
        }))
    }
}

/// Interrupt stand-in: sets the shared flag the engine reads on its next query.
struct FakeInterrupt {
    flag: Arc<AtomicBool>,
}

impl Interruptible for FakeInterrupt {
    fn interrupt(&self) {
        self.flag.store(true, Ordering::SeqCst);
    }
}
