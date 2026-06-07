//! Tests for `schema::schema::{Schema, ColumnMeta}`.

use crate::schema::{ColumnMeta, ColumnType, Schema};

fn sample() -> Schema {
    Schema::new(vec![
        ColumnMeta::new("id", ColumnType::Int),
        ColumnMeta::new("created_at", ColumnType::Date),
        ColumnMeta::new("amount", ColumnType::Float),
        ColumnMeta::new("status", ColumnType::Text),
    ])
}

#[test]
fn empty_schema() {
    let s = Schema::default();
    assert!(s.is_empty());
    assert_eq!(s.len(), 0);
    assert_eq!(s.column("anything"), None);
    assert_eq!(s.names().count(), 0);
}

#[test]
fn len_and_order_preserved() {
    let s = sample();
    assert_eq!(s.len(), 4);
    assert!(!s.is_empty());
    let names: Vec<&str> = s.names().collect();
    // Order matches construction / CSV column order (drives SELECT * output order).
    assert_eq!(names, ["id", "created_at", "amount", "status"]);
}

#[test]
fn column_lookup_by_name() {
    let s = sample();
    assert_eq!(
        s.column("created_at"),
        Some(&ColumnMeta::new("created_at", ColumnType::Date))
    );
    assert_eq!(s.column("missing"), None);
}

#[test]
fn column_type_lookup() {
    let s = sample();
    assert_eq!(s.column_type("amount"), Some(&ColumnType::Float));
    assert_eq!(s.column_type("id"), Some(&ColumnType::Int));
    assert_eq!(s.column_type("missing"), None);
}

#[test]
fn column_ci_resolves_case_insensitively_to_canonical_spelling() {
    // DuckDB resolves unquoted identifiers case-insensitively; `column_ci` mirrors that and returns
    // the canonical header spelling so the value path keys off it regardless of typed casing.
    let s = sample();
    assert_eq!(s.column_ci("STATUS").unwrap().name, "status");
    assert_eq!(s.column_ci("Created_At").unwrap().name, "created_at");
    assert_eq!(s.column_type_ci("AMOUNT"), Some(&ColumnType::Float));
    // An exact match still works, and a truly-missing column is still None.
    assert_eq!(s.column_ci("status").unwrap().name, "status");
    assert_eq!(s.column_ci("missing"), None);
    assert_eq!(s.column_type_ci("missing"), None);
}

#[test]
fn column_ci_prefers_an_exact_match_over_a_case_fold() {
    // With two same-name-different-case headers, an exact match wins over the case-insensitive
    // fallback, so the resolved name is the one actually written.
    let s = Schema::new(vec![
        ColumnMeta::new("Status", ColumnType::Int),
        ColumnMeta::new("status", ColumnType::Text),
    ]);
    assert_eq!(s.column_ci("status").unwrap().ty, ColumnType::Text);
    assert_eq!(s.column_ci("Status").unwrap().ty, ColumnType::Int);
}

#[test]
fn duplicate_header_returns_first_match() {
    // CSV headers can duplicate; dedupe policy is a deferred ingest decision (PLAN Q3).
    // Until then, lookup returns the first occurrence deterministically.
    let s = Schema::new(vec![
        ColumnMeta::new("x", ColumnType::Int),
        ColumnMeta::new("x", ColumnType::Text),
    ]);
    assert_eq!(s.column("x").unwrap().ty, ColumnType::Int);
    assert_eq!(s.len(), 2);
}

#[test]
fn names_are_verbatim_not_quoted() {
    // Raw header text is stored verbatim; SQL quoting is applied at use-site, not here.
    let s = Schema::new(vec![ColumnMeta::new("Total ($)", ColumnType::Float)]);
    assert_eq!(s.column("Total ($)").unwrap().name, "Total ($)");
}

#[test]
fn columns_slice_is_table_order() {
    // The full ColumnMeta slice (consumed by grid layout / schema bar) is returned in
    // construction (CSV column) order.
    let s = sample();
    let cols = s.columns();
    assert_eq!(cols.len(), 4);
    assert_eq!(cols[0], ColumnMeta::new("id", ColumnType::Int));
    assert_eq!(cols[3], ColumnMeta::new("status", ColumnType::Text));
    assert!(Schema::default().columns().is_empty());
}
