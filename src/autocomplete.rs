//! Schema-aware autocomplete (`dev/PLAN.md` §5).
//!
//! ciq's autocomplete is structurally simpler than jiq's because the CSV schema is **declared**
//! (a fixed `Vec<ColumnMeta>` computed once at load) rather than inferred from a moving JSON
//! target. There is no "is the cache ahead of or behind the cursor?" branching (jiq's
//! `is_cursor_at_logical_end` / `is_in_non_executing_context`): the clause-context detector needs
//! only the token under the cursor plus a backward scan to the governing clause keyword.
//!
//! The pipeline is two pure stages over the shared `crate::sql_lexer` token stream, then a pure
//! candidate generator (§5.3):
//!
//! ```text
//! query+cursor -> sql_lexer::tokenize -> clause_context::detect_context -> CursorContext
//!                                     -> candidates::get_suggestions(&Schema, &ValueCache) -> Vec<Suggestion>
//! ```
//!
//! Engine boundary (§0 / §5.5): every stage here is a pure function of plain data. The **only**
//! part that touches the engine is *filling* the [`value_source::ValueCache`] via a `distinct`
//! query through the worker channel — and even then the candidate generator takes the cache as an
//! immutable argument, so unit/property tests pass a hand-built cache and never spin up DuckDB.

pub mod clause_context;
