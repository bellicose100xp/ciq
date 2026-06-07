//! Engine result types: `QueryOutcome`, the columnar `Table`/`Column`/`Cell`, and the
//! `InterruptHandle`.
//!
//! Canonical per `dev/PLAN.md` §0/D1:
//! - `query()` returns `QueryOutcome { Rows(Table) | Error{message, sql} | Cancelled }` —
//!   a cancelled query and a SQL error are *normal* outcomes of live-typing, not exceptional
//!   `Result` errors. This makes the worker→`QueryResponse` mapping a total, compiler-checked
//!   match.
//! - `Table` is **columnar** (`Vec<Column>`), because every consumer (grid widths/alignment,
//!   typed autocomplete, facets) is column-oriented. A cheap row-view (`Table::row`) serves
//!   the grid's by-row iteration without transposing.
//! - `InterruptHandle` is a thin newtype over `Arc<duckdb::InterruptHandle>` (verified
//!   `Send + Sync`, reusable-after-interrupt — see `dev/ASSUMPTIONS.md` A1). The dispatcher
//!   holds a clone and calls `.interrupt()` from its thread (§0/D4). There is **no**
//!   `Connection::interrupt()` method.

use std::sync::Arc;

use crate::schema::{ColumnType, Schema};

/// The result of a single `QueryEngine::query` / `distinct` call.
///
/// Three arms, mapping one-to-one onto the three UI states: show the grid, show an inline
/// error, or show nothing (the query was superseded and cancelled).
#[derive(Debug, Clone)]
pub enum QueryOutcome {
    /// The query succeeded; carries the columnar result.
    Rows(Table),
    /// The query failed (e.g. invalid SQL). `message` is the engine's (later enhanced)
    /// message; `sql` is the exact text that produced it, for display/debugging.
    Error { message: String, sql: String },
    /// The query was interrupted because a newer request superseded it (out-of-band cancel,
    /// §0/D4). The App discards it by `request_id`.
    Cancelled,
}

impl QueryOutcome {
    /// The contained table, if this is a successful `Rows` outcome.
    pub fn rows(&self) -> Option<&Table> {
        match self {
            QueryOutcome::Rows(t) => Some(t),
            _ => None,
        }
    }

    pub fn is_rows(&self) -> bool {
        matches!(self, QueryOutcome::Rows(_))
    }
    pub fn is_error(&self) -> bool {
        matches!(self, QueryOutcome::Error { .. })
    }
    pub fn is_cancelled(&self) -> bool {
        matches!(self, QueryOutcome::Cancelled)
    }
}

/// A single cell value. `Null` is distinct from an empty text cell — the grid renders them
/// differently and the empty-vs-NULL ingest semantics are a tracked decision (PLAN.md Q12).
#[derive(Debug, Clone, PartialEq)]
pub enum Cell {
    Null,
    Int(i64),
    Float(f64),
    Bool(bool),
    /// Text, and the rendered form of dates/timestamps/decimals/other types (the engine
    /// formats them to strings at fetch time; ciq does not re-parse them).
    Text(String),
}

impl Cell {
    /// Whether this cell is SQL `NULL` (vs any present value, including empty text).
    pub fn is_null(&self) -> bool {
        matches!(self, Cell::Null)
    }

    /// A display string for the cell. `Null` renders as the empty string here; the grid
    /// layer substitutes a themed null glyph so NULL stays visually distinct from `Text("")`.
    pub fn display(&self) -> String {
        match self {
            Cell::Null => String::new(),
            Cell::Int(i) => i.to_string(),
            Cell::Float(f) => f.to_string(),
            Cell::Bool(b) => b.to_string(),
            Cell::Text(s) => s.clone(),
        }
    }
}

/// One column of a result: its name, type, and the cells (one per row, in row order).
#[derive(Debug, Clone)]
pub struct Column {
    pub name: String,
    pub ty: ColumnType,
    pub cells: Vec<Cell>,
}

impl Column {
    pub fn new(name: impl Into<String>, ty: ColumnType, cells: Vec<Cell>) -> Self {
        Self {
            name: name.into(),
            ty,
            cells,
        }
    }
}

/// A columnar result table: `Vec<Column>`, all columns the same length (`row_count`).
///
/// Columnar because every consumer is column-oriented; `row(i)` gives a cheap by-row view
/// (a `Vec<&Cell>` borrowing the columns) for the grid's row iteration.
#[derive(Debug, Clone, Default)]
pub struct Table {
    columns: Vec<Column>,
    row_count: usize,
}

impl Table {
    /// Build a table from columns. All columns must have the same number of cells.
    ///
    /// # Panics
    /// Debug-asserts that every column has `row_count` cells. (Engine impls construct this
    /// from a uniform result set, so a ragged table is a bug, not user input.)
    pub fn new(columns: Vec<Column>) -> Self {
        let row_count = columns.first().map(|c| c.cells.len()).unwrap_or(0);
        debug_assert!(
            columns.iter().all(|c| c.cells.len() == row_count),
            "Table columns must be equal length"
        );
        Self { columns, row_count }
    }

    pub fn columns(&self) -> &[Column] {
        &self.columns
    }

    pub fn row_count(&self) -> usize {
        self.row_count
    }

    pub fn col_count(&self) -> usize {
        self.columns.len()
    }

    pub fn is_empty(&self) -> bool {
        self.row_count == 0
    }

    /// A cheap by-row view: the `i`-th cell of every column, borrowed. Returns `None` if
    /// `i` is out of range.
    pub fn row(&self, i: usize) -> Option<Vec<&Cell>> {
        if i >= self.row_count {
            return None;
        }
        Some(self.columns.iter().map(|c| &c.cells[i]).collect())
    }

    /// The schema of this result (column names + types), derived from the columns.
    pub fn schema(&self) -> Schema {
        use crate::schema::ColumnMeta;
        Schema::new(
            self.columns
                .iter()
                .map(|c| ColumnMeta::new(c.name.clone(), c.ty.clone()))
                .collect(),
        )
    }
}

/// A cheap, cloneable, `Send + Sync` handle for interrupting an in-flight query from a
/// thread other than the worker (the dispatcher, §0/D4).
///
/// Thin newtype over `Arc<duckdb::InterruptHandle>` (the real return type of
/// `Connection::interrupt_handle()`, verified `Send + Sync` and reusable-after-interrupt in
/// `dev/ASSUMPTIONS.md` A1). The `FakeEngine` builds one from a no-op stand-in for tests.
#[derive(Clone)]
pub struct InterruptHandle {
    inner: Arc<dyn Interruptible>,
}

/// Internal: the thing an `InterruptHandle` can interrupt. Implemented by the real DuckDB
/// handle wrapper and by the fake engine's test stand-in.
pub(crate) trait Interruptible: Send + Sync {
    fn interrupt(&self);
}

impl InterruptHandle {
    pub(crate) fn new(inner: Arc<dyn Interruptible>) -> Self {
        Self { inner }
    }

    /// A handle whose `interrupt()` does nothing. The shell's placeholder before the engine
    /// finishes loading (the real handle is installed via
    /// [`Dispatcher::set_interrupt`](crate::query::dispatcher::Dispatcher::set_interrupt) on
    /// load completion); also handy in tests that don't exercise cancellation.
    pub fn noop() -> Self {
        struct Noop;
        impl Interruptible for Noop {
            fn interrupt(&self) {}
        }
        Self::new(Arc::new(Noop))
    }

    /// Interrupt the query currently running on the associated connection. Safe to call from
    /// any thread. Not request-scoped — it cancels *whatever* query is running, so the
    /// dispatcher only calls it while a specific request is known in-flight (§0/D4).
    pub fn interrupt(&self) {
        self.inner.interrupt();
    }
}

impl std::fmt::Debug for InterruptHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("InterruptHandle")
    }
}
