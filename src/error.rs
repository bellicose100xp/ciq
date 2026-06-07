//! ciq error types.
//!
//! Scope note (PLAN.md §0/D1): the per-keystroke query path does **not** use these
//! errors — a SQL error or a cancellation is a *normal* `QueryOutcome` arm, not an
//! exceptional `Result`. `EngineError` is reserved for genuinely exceptional failures
//! (CSV load: file unreadable, malformed beyond recovery, OOM), where `load()` returns
//! `Result<Schema, EngineError>`.

use thiserror::Error;

/// Errors from the engine's exceptional paths (load / ingest), not the query hot path.
#[derive(Debug, Error)]
pub enum EngineError {
    /// The input CSV could not be read or parsed into a table at all.
    #[error("failed to load CSV from {path}: {source}")]
    Load {
        path: String,
        #[source]
        source: duckdb::Error,
    },

    /// A DuckDB operation outside the query hot path failed (e.g. `SET threads`,
    /// schema introspection at load time).
    #[error("duckdb error: {0}")]
    Duckdb(#[from] duckdb::Error),

    /// The input source (file / stdin) could not be read.
    #[error("i/o error reading {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
}

/// Top-level ciq error (CLI / app wiring). Subsystems add variants as phases land.
#[derive(Debug, Error)]
pub enum CiqError {
    #[error(transparent)]
    Engine(#[from] EngineError),
}

/// Convenience alias for ciq's top-level fallible operations.
pub type Result<T> = std::result::Result<T, CiqError>;
