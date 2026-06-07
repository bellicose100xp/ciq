//! Query subsystem — debounce, preprocessing, error enhancement, stale-discard state, and
//! (added next) the worker thread + channel types.
//!
//! Reused/adapted from jiq's `src/query/` (`dev/PLAN.md` §3.2). Per ciq conventions: no
//! `mod.rs`; submodules declared here; tests in `{name}_tests.rs` wired via `#[path]`.

pub mod debouncer;
pub mod dispatcher;
pub mod error_enhance;
pub mod preprocess;
pub mod query_state;
pub mod worker;
