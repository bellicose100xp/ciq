//! Column palette — the SELECT-pane column picker (`dev/PLAN.md` §6.2 update; user-locked redesign
//! 2026-06-09).
//!
//! `Ctrl+P` from the SELECT pane opens a popup listing every schema column with a checkbox.
//! Toggling a column **immediately rewrites the SELECT-pane text** (every checkbox change is live),
//! so the grid filters as the user clicks. The popup is purely a SELECT-pane affordance — it does
//! not open from any other pane and there is no top-level Ctrl+P binding.
//!
//! Module split:
//!  - [`palette_state`] — pure `{all_columns, checked, cursor}` over a [`std::collections::BTreeSet`]
//!    of schema indices, plus `parse_select_list` / `write_to_select` so the popup mirrors the
//!    SELECT pane's text (open) and rewrites it (every toggle).
//!  - [`palette_render`] — the popup blit; checkbox + name + type badge per row, with a distinct
//!    magenta accent so the popup reads as different from the cyan-default popups.
//!
//! There is no longer a `query_emit` module — the popup writes SELECT-pane fragments, not full
//! `SELECT … FROM t LIMIT …` SQL strings; the composer (`crate::app::query_form::composer`) builds
//! the full SQL on debounce.

pub mod palette_render;
pub mod palette_state;

pub use palette_state::{ColumnRef, PaletteState};
