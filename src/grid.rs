//! Grid subsystem — the aligned tabular results renderer (`dev/PLAN.md` §6.4).
//!
//! The one genuinely new renderer vs. jiq (which pretty-prints JSON). It splits, like every
//! ciq render surface, into a **pure layout core** and a **thin blit**:
//! - [`col_width`] — per-column width math + cell text rendering (ellipsis, null glyph). Pure.
//! - [`grid_layout`] — `layout_grid(table, &GridView) -> GridFrame`: alignment, gutters,
//!   column-granular horizontal scroll, the 1-line-per-row body. Pure (no `Frame`/clock/color).
//!
//! The thin blit (`grid_render`, P2.7) paints a `GridFrame` to a ratatui `Frame` (sticky header
//! outside the scrolled body); it is the only `Frame`-touching code and is `TestBackend`-
//! snapshot-tested.
//!
//! Conventions (jiq-inherited): no `mod.rs`; submodules declared here; tests in separate
//! `{name}_tests.rs` wired via `#[path]` inside each submodule file.

pub mod col_width;
pub mod grid_layout;

pub use grid_layout::{Align, GridFrame, GridLayout, GridView, layout_grid};
