//! Worker subsystem — the dedicated background thread that owns the [`QueryEngine`] and the
//! channel types it speaks (`dev/PLAN.md` §2.4 / §3.1, §0/D4).
//!
//! The worker is the only thread that ever issues queries on the engine (DuckDB's `Connection`
//! is `Send` but `!Sync`, so it must never cross threads); the dispatcher holds a separate
//! `Send + Sync` [`InterruptHandle`](crate::engine::InterruptHandle) clone and interrupts the
//! in-flight query from its own thread. See [`dispatcher`](crate::query::dispatcher).
//!
//! Conventions (jiq-inherited): no `mod.rs`; submodules declared here; tests in separate
//! `{name}_tests.rs` wired via `#[path]` inside each submodule file.

pub mod thread;
pub mod types;

pub use thread::spawn_worker;
pub use types::{ProcessedResult, QueryRequest, QueryResponse};
