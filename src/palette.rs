//! Column palette — the generated-state column picker (`dev/PLAN.md` §6.2, `dev/DECISIONS.md` D3).
//!
//! The headline CSV convenience: a chord (`Ctrl+P`) opens a fuzzy-filterable list of every column
//! with its sniffed type and a checkbox; the user multi-selects/reorders, and ciq **generates a
//! fresh canonical `SELECT`** from the palette's own structured state — they never hand-type
//! `SELECT a, b, c`.
//!
//! Per §0/D3 the palette **owns a ciq-generated query state** and emits SQL from that state; it
//! **never parses or splices** the user's hand-typed SQL. Whether the palette is "live" is decided
//! by byte-comparing the bar text against the last string [`query_emit::emit`] produced — no parser
//! anywhere (see [`palette_state::PaletteState::owns`]).
//!
//! Module split (jiq conventions; no `mod.rs`):
//!  - [`palette_state`] — the pure `{checked, predicates, needle, cursor}` state machine
//!    (toggle/reorder/filter/predicate transitions + ownership byte-compare).
//!  - [`query_emit`] — pure `emit(&PaletteState) -> String` (the canonical `SELECT`, both quoting
//!    surfaces, the `LIMIT min(k,N)` rule). Named `query_emit` (not `emit.rs`) to avoid colliding
//!    with `output/emit.rs` (§0/D3).
//!  - [`palette_render`] — the thin blit reusing the autocomplete popup chrome.

pub mod palette_render;
pub mod palette_state;
pub mod query_emit;

pub use palette_state::{ColumnRef, PaletteState, Predicate, PredicateOp};
