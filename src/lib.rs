//! ciq — CSV Interactive Query.
//!
//! An interactive terminal UI that gives CSV files what `jiq` gives JSON: type a
//! DuckDB-SQL query and watch an aligned result grid update live as you type, against
//! an in-memory columnar table parsed once at startup.
//!
//! # Architecture (see `dev/PLAN.md`, esp. §0 canonical decisions)
//!
//! The crate is built around two north stars:
//! 1. **Most performant in-memory CSV CLI** — parse once, re-query a resident DuckDB
//!    table per debounced keystroke (1–20 ms), never re-parse.
//! 2. **AI-testable by construction** — the vast majority of code is pure/headless/
//!    deterministic; only a small, enumerated TUI shell needs human validation.
//!
//! Everything is `pub` and re-exported here so tests can construct internals directly
//! (the testing seam — a load-bearing convention, not incidental).
//!
//! Modules are added as build phases land (see `dev/TASKS.md`); Phase 1 stands up
//! `error` first, then `schema` and `engine`.

pub mod app;
pub mod autocomplete;
pub mod clipboard;
pub mod engine;
pub mod error;
pub mod facets;
pub mod grid;
pub mod ingest;
pub mod logging;
pub mod output;
pub mod palette;
pub mod query;
pub mod schema;
pub mod schema_bar;
pub mod sql_ident;
pub mod sql_lexer;
pub mod theme;

// The headless harness uses dev-only deps (`tempfile`) and is currently consumed only by
// in-crate tests, so it compiles under `cfg(test)` only. When agent-facing E2E in a later
// phase needs it from `tests/`, promote it behind a `testutil` feature instead.
#[cfg(test)]
pub mod harness;
