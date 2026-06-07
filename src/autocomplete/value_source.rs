//! Value-completion source — the distinct-value SQL builder and the [`ValueCache`] (`dev/PLAN.md`
//! §5.5, `dev/DECISIONS.md` S3).
//!
//! When the cursor enters a value position (`WHERE col = '`, `col IN ('`, `col LIKE '`), ciq offers
//! the column's **distinct actual values** — the direct analog of jiq sampling string values from
//! live JSON (`value_collector::collect_distinct_strings`), made cheap by a DuckDB `GROUP BY`
//! (~10 ms on 5M rows per the spike).
//!
//! Two pieces, both engine-free here:
//!  - [`build_distinct_sql`] — a **pure string builder** that emits the bounded, frequency-ordered
//!    `GROUP BY` query, with correct identifier quoting. It does not run anything; the worker runs
//!    the returned SQL (P3.7). Pure-string so the exact emitted SQL — including the quoting of a
//!    column literally named `order` or one containing a `"` — is unit-asserted without DuckDB.
//!  - [`ValueCache`] — the port of jiq's `ValueMemo` *contract*: a plain owned map from column name
//!    to its distinct values, holding **no `Connection` and no engine handle**. The candidate
//!    generator (P3.5) takes `&ValueCache` as immutable data, so every unit/property test seeds it
//!    by hand and never spins up an engine. The engine call that *fills* it goes through the worker
//!    channel (P3.7); autocomplete never opens its own connection (§5.5).

use std::collections::BTreeMap;

/// Re-export the shared identifier escaper from its neutral top-level home ([`crate::sql_ident`]).
/// Kept re-exported here so existing `value_source::quote_ident` callers compile unchanged; the
/// single implementation now lives in `sql_ident.rs` so `ingest`/`palette`/`facets` share it
/// without importing `autocomplete` (the §0/D2 anti-coupling rule).
pub use crate::sql_ident::quote_ident;

/// Per-column cap on distinct values fetched/cached — the **per-column** cap, mirroring jiq's
/// `MAX_VALUES_PER_PATH` (`value_collector.rs:14`), **not** the global `MAX_GLOBAL_STRING_VALUES`
/// (S3 / §5.5 cap reconciliation). Bounds the value-suggestion list and the cache size.
pub const MAX_VALUES_PER_PATH: usize = 10_000;

/// Build the distinct-value query for `col`, capped at `cap` rows, frequency-ordered.
///
/// Emits exactly:
/// `SELECT "<col>", count(*) AS n FROM t WHERE "<col>" IS NOT NULL GROUP BY 1 ORDER BY n DESC, 1 ASC LIMIT <cap>`
///
/// `GROUP BY 1` groups by the quoted column (positional, so the quoting is written once); the
/// `IS NOT NULL` filter drops the null bucket (a NULL is not a completable value); `ORDER BY n DESC,
/// 1 ASC` surfaces the most common values first, with the secondary value tie-break (`1 ASC`, the
/// quoted column positionally) breaking equal counts. The tie-break is what makes the order — and,
/// under `LIMIT`, *which* tied values survive the cap — **stable/deterministic** (the determinism
/// rule for anything user-visible): without it, DuckDB's order within an `n`-tie is unspecified and
/// the suggestion list could vary run to run. This mirrors the facet histogram's `n DESC, value
/// ASC`. The column identifier is quote-escaped via [`quote_ident`].
pub fn build_distinct_sql(col: &str, cap: usize) -> String {
    let q = quote_ident(col);
    format!(
        "SELECT {q}, count(*) AS n FROM t WHERE {q} IS NOT NULL GROUP BY 1 ORDER BY n DESC, 1 ASC LIMIT {cap}"
    )
}

/// Build the distinct-value query using the default per-column cap ([`MAX_VALUES_PER_PATH`]).
pub fn build_distinct_sql_default(col: &str) -> String {
    build_distinct_sql(col, MAX_VALUES_PER_PATH)
}

/// A cache of distinct column values for value-completion — the owned-data port of jiq's
/// `ValueMemo`. Maps a column name to the values fetched for it (already frequency-ordered by the
/// distinct query). Holds **no engine handle**: it is filled by the worker (P3.7) and read as plain
/// data by the candidate generator (P3.5).
///
/// Keyed by a `BTreeMap` so iteration order is deterministic (the determinism rule), which keeps
/// any cache-wide snapshot/debug output stable.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ValueCache {
    by_column: BTreeMap<String, Vec<String>>,
}

impl ValueCache {
    /// An empty cache.
    pub fn new() -> Self {
        Self::default()
    }

    /// Seed (or replace) the cached values for `col`. The list is stored verbatim, in the order
    /// given (the distinct query already frequency-orders it). Tests use this to hand-build a cache
    /// with no engine.
    pub fn insert(&mut self, col: impl Into<String>, values: Vec<String>) {
        self.by_column.insert(col.into(), values);
    }

    /// The cached distinct values for `col`, or `None` on a miss (not yet fetched).
    pub fn get(&self, col: &str) -> Option<&[String]> {
        self.by_column.get(col).map(Vec::as_slice)
    }

    /// Whether `col` has cached values (a hit), so the caller can decide to fetch on a miss.
    pub fn contains(&self, col: &str) -> bool {
        self.by_column.contains_key(col)
    }

    /// Number of columns with cached values.
    pub fn len(&self) -> usize {
        self.by_column.len()
    }

    /// Whether the cache holds nothing.
    pub fn is_empty(&self) -> bool {
        self.by_column.is_empty()
    }
}

#[cfg(test)]
#[path = "value_source_tests.rs"]
mod value_source_tests;
