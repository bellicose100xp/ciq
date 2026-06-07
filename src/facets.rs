//! Instant facets / column stats (`dev/PLAN.md` §6.5, P4.6).
//!
//! Pressing the facet chord on the focused grid column fires one cheap aggregate against the
//! already-loaded table `t` (DuckDB distinct/group-by at ~10-15 ms on 5M rows — the reason DuckDB
//! was chosen over Polars) and shows min / max / distinct-count / null-count, plus a small top-K
//! value histogram for low-cardinality text columns.
//!
//! Three files, the compute/paint split every ciq feature uses:
//!  - [`facet_query`] — pure `build_facet_sql(col, &Schema) -> String`: the type-aware aggregate
//!    SQL (numeric summary vs string histogram). Runs nothing; golden-tested.
//!  - [`facet_state`] — the [`FacetState`](facet_state::FacetState) machine: the focused column +
//!    the parsed [`FacetResult`](facet_state::FacetResult), parsed from the worker's response
//!    `Table`. Pure transitions.
//!  - [`facet_render`] — pure `format_facets(&FacetResult, width) -> Vec<Line>` (stat lines +
//!    histogram bar-width math) plus the thin `render_facet` popup blit.
//!
//! The facet query rides the **same** worker channel + `request_id` staleness as the main query
//! (§6.5 — no second connection); the App routes its response to [`FacetState`] (not the grid) by a
//! [`RequestKind::Facet`](crate::query::worker::types::RequestKind) tag, exactly like P3.7's value
//! fetches.
//!
//! Per ciq conventions: no `mod.rs`; the three submodules are declared from this sibling file.

pub mod facet_query;
pub mod facet_render;
pub mod facet_state;

pub use facet_query::build_facet_sql;
pub use facet_render::format_facets;
pub use facet_state::{FacetBar, FacetResult, FacetState};

#[cfg(test)]
#[path = "facets/facet_engine_tests.rs"]
mod facet_engine_tests;
