//! Tests for `build_facet_sql` — the type-aware facet SQL builder (P4.6, §6.5).
//!
//! Goldens assert the **exact** emitted string per [`ColumnType`] WITHOUT executing it (the engine
//! wiring is exercised separately, in the App-level wiring test). The two shapes — numeric
//! **summary** (MIN/MAX) vs text **histogram** (top-K GROUP BY) — and the identifier quoting are the
//! load-bearing surface: a wrong shape or a mis-quoted column silently corrupts every facet.

use super::*;
use crate::schema::{ColumnMeta, ColumnType, Schema};

fn schema() -> Schema {
    Schema::new(vec![
        ColumnMeta::new("id", ColumnType::Int),
        ColumnMeta::new("amount", ColumnType::Float),
        ColumnMeta::new("active", ColumnType::Bool),
        ColumnMeta::new("created_at", ColumnType::Date),
        ColumnMeta::new("seen_at", ColumnType::Timestamp),
        ColumnMeta::new("status", ColumnType::Text),
        ColumnMeta::new("payload", ColumnType::Other("STRUCT".into())),
        ColumnMeta::new("order", ColumnType::Int),
        ColumnMeta::new("Total ($)", ColumnType::Float),
    ])
}

// --- numeric / temporal / bool: the summary shape (MIN/MAX/distinct/nulls) ---

#[test]
fn int_column_emits_summary() {
    assert_eq!(
        build_facet_sql("id", &schema()),
        r#"SELECT min("id") AS mn, max("id") AS mx, count(DISTINCT "id") AS distinct_count, count(*) FILTER (WHERE "id" IS NULL) AS null_count FROM t"#
    );
}

#[test]
fn float_column_emits_summary() {
    assert_eq!(
        build_facet_sql("amount", &schema()),
        r#"SELECT min("amount") AS mn, max("amount") AS mx, count(DISTINCT "amount") AS distinct_count, count(*) FILTER (WHERE "amount" IS NULL) AS null_count FROM t"#
    );
}

#[test]
fn bool_column_emits_summary() {
    assert_eq!(
        build_facet_sql("active", &schema()),
        r#"SELECT min("active") AS mn, max("active") AS mx, count(DISTINCT "active") AS distinct_count, count(*) FILTER (WHERE "active" IS NULL) AS null_count FROM t"#
    );
}

#[test]
fn date_column_emits_summary() {
    assert_eq!(
        build_facet_sql("created_at", &schema()),
        r#"SELECT min("created_at") AS mn, max("created_at") AS mx, count(DISTINCT "created_at") AS distinct_count, count(*) FILTER (WHERE "created_at" IS NULL) AS null_count FROM t"#
    );
}

#[test]
fn timestamp_column_emits_summary() {
    assert_eq!(
        build_facet_sql("seen_at", &schema()),
        r#"SELECT min("seen_at") AS mn, max("seen_at") AS mx, count(DISTINCT "seen_at") AS distinct_count, count(*) FILTER (WHERE "seen_at" IS NULL) AS null_count FROM t"#
    );
}

// --- text / other: the histogram shape (top-K GROUP BY + column-wide distinct/null) ---

#[test]
fn text_column_emits_histogram() {
    assert_eq!(
        build_facet_sql("status", &schema()),
        r#"SELECT "status" AS value, count(*) AS n, (SELECT count(DISTINCT "status") FROM t) AS distinct_count, (SELECT count(*) FILTER (WHERE "status" IS NULL) FROM t) AS null_count FROM t WHERE "status" IS NOT NULL GROUP BY 1 ORDER BY n DESC, value ASC LIMIT 10"#
    );
}

#[test]
fn other_type_column_emits_histogram() {
    // A structured/unknown type gets the most general (histogram) shape — MIN/MAX is not meaningful.
    assert_eq!(
        build_facet_sql("payload", &schema()),
        r#"SELECT "payload" AS value, count(*) AS n, (SELECT count(DISTINCT "payload") FROM t) AS distinct_count, (SELECT count(*) FILTER (WHERE "payload" IS NULL) FROM t) AS null_count FROM t WHERE "payload" IS NOT NULL GROUP BY 1 ORDER BY n DESC, value ASC LIMIT 10"#
    );
}

#[test]
fn unknown_column_defaults_to_histogram() {
    // A column not in the schema (the App never passes one — defensive) gets the general shape.
    assert_eq!(
        build_facet_sql("nonexistent", &schema()),
        r#"SELECT "nonexistent" AS value, count(*) AS n, (SELECT count(DISTINCT "nonexistent") FROM t) AS distinct_count, (SELECT count(*) FILTER (WHERE "nonexistent" IS NULL) FROM t) AS null_count FROM t WHERE "nonexistent" IS NOT NULL GROUP BY 1 ORDER BY n DESC, value ASC LIMIT 10"#
    );
}

// --- identifier quoting (both shapes share the shared sql_ident escaper) ---

#[test]
fn keyword_column_is_quoted_in_summary() {
    // `order` is a reserved word; it is double-quoted so it can't break or smuggle into the query.
    let sql = build_facet_sql("order", &schema());
    assert!(sql.contains(r#"min("order")"#), "got: {sql}");
    assert!(
        sql.contains(r#"FILTER (WHERE "order" IS NULL)"#),
        "got: {sql}"
    );
}

#[test]
fn special_char_column_is_quoted_in_summary() {
    // `Total ($)` is a Float column → summary shape; the space + `()` force double-quoting.
    let sql = build_facet_sql("Total ($)", &schema());
    assert!(sql.contains(r#"min("Total ($)")"#), "got: {sql}");
}

#[test]
fn embedded_quote_is_doubled() {
    let schema = Schema::new(vec![ColumnMeta::new(r#"we"ird"#, ColumnType::Text)]);
    let sql = build_facet_sql(r#"we"ird"#, &schema);
    assert!(sql.contains(r#""we""ird" AS value"#), "got: {sql}");
}

// --- case-insensitive type resolution + explicit k ---

#[test]
fn column_type_resolved_case_insensitively() {
    // DuckDB resolves unquoted identifiers case-insensitively; the type lookup follows. `STATUS`
    // resolves to the `status` text column, so it gets the histogram shape — but the identifier is
    // quoted with the *typed* casing (the user's text), which is how the chord passes it.
    let sql = build_facet_sql("STATUS", &schema());
    assert!(sql.contains("GROUP BY 1"), "text => histogram, got: {sql}");
}

#[test]
fn explicit_k_changes_limit() {
    let sql = build_facet_sql_with_k("status", Some(&ColumnType::Text), 3);
    assert!(sql.ends_with("LIMIT 3"), "got: {sql}");
}

#[test]
fn summary_columns_constant_matches_emitted_aliases() {
    let sql = build_facet_sql("id", &schema());
    for alias in SUMMARY_COLUMNS {
        assert!(
            sql.contains(&format!("AS {alias}")),
            "missing {alias}: {sql}"
        );
    }
}
