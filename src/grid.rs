//! Grid subsystem — the aligned tabular results renderer (`dev/PLAN.md` §6.4).
//!
//! The one genuinely new renderer vs. jiq (which pretty-prints JSON). It splits, like every
//! ciq render surface, into a **pure layout core** and a **thin blit**:
//! - [`col_width`] — per-column width math + cell text rendering (ellipsis, null glyph). Pure.
//! - [`grid_layout`] — `layout_grid(table, &GridView) -> GridFrame`: alignment, gutters,
//!   column-granular horizontal scroll, the 1-line-per-row body. Pure (no `Frame`/clock/color).
//! - [`grid_render`] — paints a `GridFrame` to a ratatui `Frame`: sticky header outside the
//!   scrolled body, body as a scrolled `Paragraph`. The only `Frame`-touching code here;
//!   `TestBackend`-snapshot-tested, colors from `theme::grid::*`.
//!
//! Conventions (jiq-inherited): no `mod.rs`; submodules declared here; tests in separate
//! `{name}_tests.rs` wired via `#[path]` inside each submodule file.

pub mod col_width;
pub mod grid_layout;
pub mod grid_render;

pub use grid_layout::{Align, BodyRow, GridFrame, GridLayout, GridView, layout_grid};
