//! Tests for `FacetState` — the focused column, pending/ready transitions, and the pure parse of a
//! facet-query response `Table` into a [`FacetResult`] (P4.6, §6.5).

use super::*;
use crate::engine::types::{Cell, Column, Table};
use crate::schema::ColumnType;

/// A summary-shape result table: one row of `mn, mx, distinct_count, null_count`.
fn summary_table(mn: Cell, mx: Cell, distinct: i64, nulls: i64) -> Table {
    Table::new(vec![
        Column::new("mn", ColumnType::Int, vec![mn]),
        Column::new("mx", ColumnType::Int, vec![mx]),
        Column::new("distinct_count", ColumnType::Int, vec![Cell::Int(distinct)]),
        Column::new("null_count", ColumnType::Int, vec![Cell::Int(nulls)]),
    ])
}

/// A histogram-shape result table: `(value, n, distinct_count, null_count)` rows, the last two
/// repeating the column-wide counts on every row.
fn histogram_table(rows: &[(&str, i64)], distinct: i64, nulls: i64) -> Table {
    let values = rows.iter().map(|(v, _)| Cell::Text((*v).into())).collect();
    let counts = rows.iter().map(|(_, n)| Cell::Int(*n)).collect();
    let distincts = rows.iter().map(|_| Cell::Int(distinct)).collect();
    let null_counts = rows.iter().map(|_| Cell::Int(nulls)).collect();
    Table::new(vec![
        Column::new("value", ColumnType::Text, values),
        Column::new("n", ColumnType::Int, counts),
        Column::new("distinct_count", ColumnType::Int, distincts),
        Column::new("null_count", ColumnType::Int, null_counts),
    ])
}

// --- pending / ready ---

#[test]
fn pending_state_has_no_result() {
    let s = FacetState::pending("id", ColumnType::Int);
    assert_eq!(s.column(), "id");
    assert_eq!(s.column_type(), &ColumnType::Int);
    assert!(!s.is_ready());
    assert!(s.result().is_none());
}

// --- summary parse (numeric/temporal/bool) ---

#[test]
fn applies_summary_result_for_numeric_column() {
    let mut s = FacetState::pending("id", ColumnType::Int);
    s.apply_result(&summary_table(Cell::Int(1), Cell::Int(99), 42, 3));
    assert!(s.is_ready());
    assert_eq!(
        s.result().unwrap(),
        &FacetResult::Summary {
            min: Some("1".into()),
            max: Some("99".into()),
            distinct: 42,
            nulls: 3,
        }
    );
}

#[test]
fn all_null_column_yields_none_min_max() {
    // An entirely-NULL column makes MIN/MAX return NULL — parsed as `None`.
    let mut s = FacetState::pending("amount", ColumnType::Float);
    s.apply_result(&summary_table(Cell::Null, Cell::Null, 0, 7));
    assert_eq!(
        s.result().unwrap(),
        &FacetResult::Summary {
            min: None,
            max: None,
            distinct: 0,
            nulls: 7,
        }
    );
}

#[test]
fn summary_accessors_expose_distinct_and_nulls() {
    let mut s = FacetState::pending("id", ColumnType::Int);
    s.apply_result(&summary_table(Cell::Int(0), Cell::Int(5), 6, 2));
    let r = s.result().unwrap();
    assert_eq!(r.distinct(), 6);
    assert_eq!(r.nulls(), 2);
    assert_eq!(r.max_count(), 0, "a summary has no histogram scale");
}

// --- histogram parse (text/other) ---

#[test]
fn applies_histogram_result_for_text_column() {
    let mut s = FacetState::pending("status", ColumnType::Text);
    s.apply_result(&histogram_table(
        &[("active", 50), ("archived", 30), ("pending", 20)],
        3,
        4,
    ));
    assert_eq!(
        s.result().unwrap(),
        &FacetResult::Histogram {
            bars: vec![
                FacetBar::new("active", 50),
                FacetBar::new("archived", 30),
                FacetBar::new("pending", 20),
            ],
            distinct: 3,
            nulls: 4,
        }
    );
}

#[test]
fn histogram_max_count_is_the_largest_bar() {
    let mut s = FacetState::pending("status", ColumnType::Text);
    s.apply_result(&histogram_table(&[("a", 50), ("b", 30)], 2, 0));
    assert_eq!(s.result().unwrap().max_count(), 50);
}

#[test]
fn other_type_parses_as_histogram() {
    let mut s = FacetState::pending("payload", ColumnType::Other("STRUCT".into()));
    s.apply_result(&histogram_table(&[("{a: 1}", 5)], 1, 0));
    assert!(matches!(s.result().unwrap(), FacetResult::Histogram { .. }));
}

#[test]
fn all_null_histogram_reports_real_null_count_via_sentinel_row() {
    // An entirely-NULL text column: the `stats LEFT JOIN bars` shape returns ONE sentinel row with
    // a NULL value/n but the true distinct/null counts. The parser must skip the NULL-value
    // sentinel from the bars (no spurious bar) while still reading the real null count — the bug
    // where an all-NULL column reported nulls=0.
    let mut s = FacetState::pending("status", ColumnType::Text);
    s.apply_result(&Table::new(vec![
        Column::new("value", ColumnType::Text, vec![Cell::Null]),
        Column::new("n", ColumnType::Int, vec![Cell::Null]),
        Column::new("distinct_count", ColumnType::Int, vec![Cell::Int(0)]),
        Column::new("null_count", ColumnType::Int, vec![Cell::Int(5)]),
    ]));
    assert_eq!(
        s.result().unwrap(),
        &FacetResult::Histogram {
            bars: vec![],
            distinct: 0,
            nulls: 5,
        }
    );
    assert_eq!(s.result().unwrap().max_count(), 0);
}

#[test]
fn empty_histogram_table_yields_no_bars() {
    // A genuinely empty result table (no rows at all — an engine/shape surprise) parses to an
    // empty histogram with zero counts, never a panic.
    let mut s = FacetState::pending("status", ColumnType::Text);
    s.apply_result(&Table::new(vec![
        Column::new("value", ColumnType::Text, vec![]),
        Column::new("n", ColumnType::Int, vec![]),
        Column::new("distinct_count", ColumnType::Int, vec![]),
        Column::new("null_count", ColumnType::Int, vec![]),
    ]));
    assert_eq!(
        s.result().unwrap(),
        &FacetResult::Histogram {
            bars: vec![],
            distinct: 0,
            nulls: 0,
        }
    );
    assert_eq!(s.result().unwrap().max_count(), 0);
}

// --- robustness: a shape surprise never panics ---

#[test]
fn short_table_yields_zeroed_result_without_panic() {
    let mut s = FacetState::pending("id", ColumnType::Int);
    s.apply_result(&Table::default()); // no columns
    assert_eq!(
        s.result().unwrap(),
        &FacetResult::Summary {
            min: None,
            max: None,
            distinct: 0,
            nulls: 0,
        }
    );
}

#[test]
fn count_text_cells_are_coerced() {
    // DuckDB counts can arrive as Int; a Text fallback parses, a bad one is 0 (never panics).
    let mut s = FacetState::pending("status", ColumnType::Text);
    let table = Table::new(vec![
        Column::new("value", ColumnType::Text, vec![Cell::Text("x".into())]),
        Column::new("n", ColumnType::Text, vec![Cell::Text("7".into())]),
        Column::new(
            "distinct_count",
            ColumnType::Text,
            vec![Cell::Text("1".into())],
        ),
        Column::new(
            "null_count",
            ColumnType::Text,
            vec![Cell::Text("nope".into())],
        ),
    ]);
    s.apply_result(&table);
    let FacetResult::Histogram {
        bars,
        distinct,
        nulls,
    } = s.result().unwrap()
    else {
        panic!("expected histogram");
    };
    assert_eq!(bars[0].count, 7);
    assert_eq!(*distinct, 1);
    assert_eq!(*nulls, 0, "unparseable count coerces to 0");
}
