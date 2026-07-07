//! Row search (`Ctrl+F`) — filter the result grid to rows where ANY column matches the typed
//! needle, with the matched text highlighted in place.
//!
//! Ported from jiq's `src/search/` in shape (bar + active/confirmed states + live match
//! highlighting), but rotated onto ciq's tabular reality: jiq *scrolls* to matches inside a fixed
//! text document; ciq *filters* the displayed rows (any-column substring match) because a table's
//! natural search verb is "show me the rows that contain this". The pure core (matcher + state +
//! row filter) is engine-free and on the hard coverage floor; the bar blit lives in
//! [`search_render`] (a `TestBackend` seam like the other `*_render` files).

pub mod matcher;
pub mod search_render;
pub mod search_state;

pub use search_state::SearchState;
