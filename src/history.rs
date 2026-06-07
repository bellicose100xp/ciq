//! Query history — in-session ring + on-disk persistence + recall popup (`dev/PLAN.md` §7.6, P5.2).
//!
//! Ported from jiq's `history/` (JSON-filter entries -> SQL query strings), split into the three
//! files ciq's compute/paint/seam convention prescribes:
//!  - [`history_state`] — the **pure** ring state machine: add (dedupe to newest-first), recall,
//!    navigate (popup cursor + inline cycling), fuzzy filter. No terminal, no filesystem; tested
//!    in-memory.
//!  - [`storage`] — the **only I/O**: newline-delimited load/save, every fn taking an explicit path
//!    so tests run against a tempdir, never `$HOME`.
//!  - [`history_events`] — the **pure** key -> [`HistoryAction`](history_events::HistoryAction)
//!    mapping for the open popup (the App applies it, mirroring the palette/facet routing).
//!  - [`history_render`] — the **thin blit**: a bordered popup reusing the palette/autocomplete
//!    chrome, `TestBackend`-snapshot-tested; colors via `theme::history`.
//!
//! Wiring (in `App`): a history chord opens the popup; selecting an entry drops its SQL into the
//! query bar, which then flows through the **same** preprocess-validate + debounce + dispatch path
//! a typed query uses — recalled SQL is never special-cased past the read-only single-statement
//! guard.
//!
//! Per ciq conventions: no `mod.rs`; the submodules are declared from this sibling file.

pub mod history_events;
pub mod history_render;
pub mod history_state;
pub mod storage;

pub use history_events::{HistoryAction, map_key};
pub use history_render::render_history;
pub use history_state::{HistoryState, MAX_VISIBLE_HISTORY};
