//! Tests for the value-completion source (`dev/PLAN.md` §5.5, `dev/DECISIONS.md` S3).
//!
//! `build_distinct_sql` is asserted **without executing** — the emitted SQL string, including
//! identifier quoting, is the contract. (The one integration test that runs it against a real
//! fixture engine belongs to P3.7 wiring, not here.) `ValueCache` is exercised purely by hand-seed
//! / lookup / miss — it holds no engine handle.

use super::*;

// ── build_distinct_sql ──────────────────────────────────────────────────────────────────────

#[test]
fn distinct_sql_for_plain_column() {
    assert_eq!(
        build_distinct_sql("status", 10),
        "SELECT \"status\", count(*) AS n FROM t WHERE \"status\" IS NOT NULL \
         GROUP BY 1 ORDER BY n DESC LIMIT 10"
    );
}

#[test]
fn distinct_sql_quotes_a_reserved_word_column() {
    // A column named after a reserved word must be double-quoted so it isn't parsed as the keyword.
    assert_eq!(
        build_distinct_sql("order", 5),
        "SELECT \"order\", count(*) AS n FROM t WHERE \"order\" IS NOT NULL \
         GROUP BY 1 ORDER BY n DESC LIMIT 5"
    );
}

#[test]
fn distinct_sql_escapes_embedded_double_quote() {
    // An embedded `"` is escaped by doubling (`""`), so a malicious/odd header can't break out of
    // the identifier or smuggle SQL.
    assert_eq!(
        build_distinct_sql("we\"ird", 3),
        "SELECT \"we\"\"ird\", count(*) AS n FROM t WHERE \"we\"\"ird\" IS NOT NULL \
         GROUP BY 1 ORDER BY n DESC LIMIT 3"
    );
}

#[test]
fn distinct_sql_default_cap_uses_max_values_per_path() {
    let sql = build_distinct_sql_default("city");
    assert!(sql.ends_with(&format!("LIMIT {MAX_VALUES_PER_PATH}")));
    assert_eq!(sql, build_distinct_sql("city", MAX_VALUES_PER_PATH));
}

#[test]
fn max_values_per_path_matches_jiq_per_path_cap() {
    // S3: the per-column cap mirrors jiq's MAX_VALUES_PER_PATH (10_000), not the global cap.
    assert_eq!(MAX_VALUES_PER_PATH, 10_000);
}

#[test]
fn quote_ident_doubles_embedded_quotes_only() {
    assert_eq!(quote_ident("plain"), "\"plain\"");
    assert_eq!(quote_ident("a\"b"), "\"a\"\"b\"");
    assert_eq!(quote_ident("a\"\"b"), "\"a\"\"\"\"b\"");
    assert_eq!(quote_ident(""), "\"\"");
}

// ── ValueCache ──────────────────────────────────────────────────────────────────────────────

#[test]
fn cache_starts_empty() {
    let c = ValueCache::new();
    assert!(c.is_empty());
    assert_eq!(c.len(), 0);
    assert_eq!(c.get("status"), None);
    assert!(!c.contains("status"));
}

#[test]
fn cache_seed_lookup_and_miss() {
    let mut c = ValueCache::new();
    c.insert("status", vec!["active".into(), "inactive".into()]);
    assert!(c.contains("status"));
    assert_eq!(
        c.get("status"),
        Some(["active".to_string(), "inactive".to_string()].as_slice())
    );
    // A different column is a miss.
    assert_eq!(c.get("city"), None);
    assert!(!c.contains("city"));
    assert_eq!(c.len(), 1);
    assert!(!c.is_empty());
}

#[test]
fn cache_insert_replaces_existing() {
    let mut c = ValueCache::new();
    c.insert("status", vec!["old".into()]);
    c.insert("status", vec!["new".into(), "newer".into()]);
    assert_eq!(
        c.get("status"),
        Some(["new".to_string(), "newer".to_string()].as_slice())
    );
    assert_eq!(c.len(), 1);
}

#[test]
fn cache_preserves_insertion_order_of_values() {
    // The distinct query frequency-orders the list; the cache must store it verbatim, not re-sort.
    let mut c = ValueCache::new();
    c.insert("k", vec!["z".into(), "a".into(), "m".into()]);
    assert_eq!(
        c.get("k"),
        Some(["z".to_string(), "a".to_string(), "m".to_string()].as_slice())
    );
}

#[test]
fn cache_is_plain_data_clone_and_eq() {
    // No engine handle: it is Clone + PartialEq plain data, so tests can compare snapshots.
    let mut a = ValueCache::new();
    a.insert("c", vec!["1".into()]);
    let b = a.clone();
    assert_eq!(a, b);
}
