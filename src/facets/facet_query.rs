//! Facet SQL builder — `build_facet_sql(col, &Schema) -> String` (`dev/PLAN.md` §6.5).
//!
//! Pressing the facet chord on the focused grid column fires a single cheap aggregate against the
//! **already-loaded** table `t` (DuckDB distinct/group-by at ~10-15 ms on 5M rows — the reason
//! DuckDB was chosen over Polars). This module emits the SQL; the worker runs it on the same
//! channel/engine as the main query (§6.5 — no second connection), and [`facet_state`] parses the
//! resulting [`Table`](crate::engine::Table) back into a [`FacetResult`](super::facet_state::FacetResult).
//!
//! **Type-aware shape** (so the parser can interpret the result by the column's type):
//!  - **Numeric / temporal / bool / other** → one **summary** row of four aggregate columns:
//!    `mn` (MIN), `mx` (MAX), `distinct_count` (COUNT DISTINCT), `null_count`
//!    (COUNT(*) FILTER (WHERE col IS NULL)).
//!  - **Text** → a **top-K** result: one `(value, n)` row per most-frequent value (the GROUP BY
//!    histogram), with the column-wide `distinct_count` / `null_count` carried on every row as
//!    correlated scalar sub-selects so the single returned `Table` holds everything the popup needs.
//!
//! Pure `&str` → `String`: it runs nothing. The exact emitted SQL — including the quoting of a
//! column literally named `order` or one containing a `"` — is golden-tested without DuckDB
//! ([`facet_query_tests`]). Identifiers are quoted through the shared [`crate::sql_ident::quote_ident`]
//! so quoting can never drift from the other emitters.

use crate::schema::{ColumnType, Schema};
use crate::sql_ident::quote_ident;

/// Default top-K cap for the string-histogram facet (the most-frequent values shown). Small: the
/// popup shows a handful of bars, and a wider list would not fit the popup or read at a glance.
pub const DEFAULT_TOP_K: usize = 10;

/// The four summary-aggregate column aliases, in the order [`build_facet_sql`] emits them for a
/// numeric/temporal/bool/other column. `facet_state` keys off these positions.
pub const SUMMARY_COLUMNS: [&str; 4] = ["mn", "mx", "distinct_count", "null_count"];

/// Build the facet aggregate SQL for `col`, shaped by the column's [`ColumnType`] in `schema`.
///
/// Uses [`DEFAULT_TOP_K`] for the text histogram limit. A column not present in `schema` (which
/// the App never passes — the chord targets a known grid column) is treated as text, the most
/// general shape.
pub fn build_facet_sql(col: &str, schema: &Schema) -> String {
    let ty = schema.column_type_ci(col).cloned();
    build_facet_sql_with_k(col, ty.as_ref(), DEFAULT_TOP_K)
}

/// Build the facet SQL for `col` of the given (optional) type, with an explicit top-K cap. The
/// type-table-driven core, separated so the per-type goldens pin the exact emitted string and the
/// `k` is visible to tests.
pub fn build_facet_sql_with_k(col: &str, ty: Option<&ColumnType>, k: usize) -> String {
    if is_histogram_type(ty) {
        build_histogram_sql(col, k)
    } else {
        build_summary_sql(col)
    }
}

/// Whether a column's facet is the **string histogram** (top-K `GROUP BY`) shape vs the numeric
/// **summary** (MIN/MAX) shape. Text and `Other` (structured/unknown) get the histogram; an
/// unknown (missing) type defaults to the histogram (the most general shape). Numerics, temporals,
/// and bools get the summary — MIN/MAX is meaningful for them and a per-value histogram is not.
fn is_histogram_type(ty: Option<&ColumnType>) -> bool {
    match ty {
        Some(ColumnType::Text) | Some(ColumnType::Other(_)) | None => true,
        Some(_) => false,
    }
}

/// The numeric/temporal/bool **summary**: one row, four aggregate columns aliased per
/// [`SUMMARY_COLUMNS`]. The null count uses the SQL-standard `COUNT(*) FILTER (WHERE col IS NULL)`
/// (a NULL is excluded from `MIN`/`MAX`/`COUNT(DISTINCT)`, so it is counted separately).
fn build_summary_sql(col: &str) -> String {
    let q = quote_ident(col);
    format!(
        "SELECT min({q}) AS mn, max({q}) AS mx, count(DISTINCT {q}) AS distinct_count, \
         count(*) FILTER (WHERE {q} IS NULL) AS null_count FROM t"
    )
}

/// The text **top-K histogram**: one `(value, n)` row per most-frequent non-null value, plus the
/// column-wide `distinct_count` / `null_count` on every row.
///
/// The counts come from a one-row `stats` CTE that the top-K `bars` is `LEFT JOIN`ed onto, so the
/// counts **always materialize even when there are zero bars** — an entirely-NULL column yields one
/// `(NULL, NULL, distinct_count, null_count)` row carrying the true null count, instead of the
/// empty result the old per-row correlated sub-select produced (which lost the null count). The
/// parser ([`facet_state`](super::facet_state)) skips the sentinel NULL-value row, so no spurious
/// bar appears.
///
/// `ORDER BY n DESC, value ASC` is a **stable, deterministic** order (the determinism rule for
/// anything user-visible) — the secondary `value ASC` tie-breaks equal counts so the snapshot never
/// flips. `WHERE col IS NOT NULL` drops the null bucket from the bars (the null count is the
/// separate `null_count` column). `GROUP BY 1` groups by the (quoted) value positionally so the
/// quoting is written once.
fn build_histogram_sql(col: &str, k: usize) -> String {
    let q = quote_ident(col);
    format!(
        "WITH bars AS (\
         SELECT {q} AS value, count(*) AS n FROM t WHERE {q} IS NOT NULL \
         GROUP BY 1 ORDER BY n DESC, value ASC LIMIT {k}), \
         stats AS (\
         SELECT count(DISTINCT {q}) AS distinct_count, \
         count(*) FILTER (WHERE {q} IS NULL) AS null_count FROM t) \
         SELECT bars.value AS value, bars.n AS n, \
         stats.distinct_count AS distinct_count, stats.null_count AS null_count \
         FROM stats LEFT JOIN bars ON true ORDER BY n DESC, value ASC"
    )
}

#[cfg(test)]
#[path = "facet_query_tests.rs"]
mod facet_query_tests;
