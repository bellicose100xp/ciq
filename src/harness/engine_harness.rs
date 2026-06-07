//! `EngineHarness` — load a fixture CSV once into a real `DuckdbEngine`, fire arbitrary SQL,
//! assert on the returned `QueryOutcome`. Deterministic; no terminal.
//!
//! This is the engine-level half of the headless harness (`dev/PLAN.md` §4.2). It exists so
//! engine semantics (type sniffing, result shape, error/cancel mapping) can be exercised in a
//! few lines without each test re-doing tempfile + load boilerplate.

use std::io::Write;
use std::path::Path;

use tempfile::NamedTempFile;

use crate::engine::{CsvOpts, DuckdbEngine, QueryEngine, QueryOutcome};
use crate::error::EngineError;
use crate::schema::Schema;

/// A loaded engine over a fixture CSV, ready to query.
///
/// Holds the `NamedTempFile` alive for the harness's lifetime when constructed from a string,
/// so the on-disk fixture isn't deleted out from under DuckDB.
pub struct EngineHarness {
    engine: DuckdbEngine,
    // Kept alive so the temp CSV file persists while the engine references its path.
    _fixture: Option<NamedTempFile>,
}

impl EngineHarness {
    /// Load an existing CSV file at `path` once. Errors if it can't be parsed.
    pub fn open(path: &Path) -> Result<Self, EngineError> {
        let engine = DuckdbEngine::open(path, &CsvOpts::default())?;
        Ok(Self {
            engine,
            _fixture: None,
        })
    }

    /// Write `csv` to a temp `.csv`, load it once, and keep the temp file alive. The common
    /// path for tests that want an inline fixture.
    pub fn from_csv(csv: &str) -> Result<Self, EngineError> {
        let mut f = NamedTempFile::with_suffix(".csv").map_err(|e| EngineError::Io {
            path: "<tempfile>".into(),
            source: e,
        })?;
        f.write_all(csv.as_bytes()).map_err(|e| EngineError::Io {
            path: f.path().display().to_string(),
            source: e,
        })?;
        f.flush().map_err(|e| EngineError::Io {
            path: f.path().display().to_string(),
            source: e,
        })?;
        let engine = DuckdbEngine::open(f.path(), &CsvOpts::default())?;
        Ok(Self {
            engine,
            _fixture: Some(f),
        })
    }

    /// Run a query and return its outcome.
    pub fn query(&self, sql: &str) -> QueryOutcome {
        self.engine.query(sql)
    }

    /// Distinct values of a column (value-autocomplete path).
    pub fn distinct(&self, col: &str, limit: usize) -> QueryOutcome {
        self.engine.distinct(col, limit)
    }

    /// The schema captured at load.
    pub fn schema(&self) -> &Schema {
        self.engine.schema()
    }

    /// Borrow the underlying engine (e.g. to grab an `InterruptHandle` for a cancel test).
    pub fn engine(&self) -> &DuckdbEngine {
        &self.engine
    }
}
