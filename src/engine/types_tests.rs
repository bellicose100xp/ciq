//! Tests for `engine::types` — `QueryOutcome`, columnar `Table`, `Cell`, `InterruptHandle`.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::engine::types::{Cell, Column, InterruptHandle, Interruptible, QueryOutcome, Table};
use crate::schema::ColumnType;

fn table_2x3() -> Table {
    // 2 columns, 3 rows.
    Table::new(vec![
        Column::new(
            "id",
            ColumnType::Int,
            vec![Cell::Int(1), Cell::Int(2), Cell::Int(3)],
        ),
        Column::new(
            "name",
            ColumnType::Text,
            vec![
                Cell::Text("ada".into()),
                Cell::Null,
                Cell::Text("grace".into()),
            ],
        ),
    ])
}

#[test]
fn query_outcome_exhaustive_classification() {
    let rows = QueryOutcome::Rows(table_2x3());
    let err = QueryOutcome::Error {
        message: "boom".into(),
        sql: "SELECT bad".into(),
    };
    let cancelled = QueryOutcome::Cancelled;

    assert!(rows.is_rows() && !rows.is_error() && !rows.is_cancelled());
    assert!(err.is_error() && !err.is_rows() && !err.is_cancelled());
    assert!(cancelled.is_cancelled() && !cancelled.is_rows() && !cancelled.is_error());

    assert!(rows.rows().is_some());
    assert!(err.rows().is_none());
    assert!(cancelled.rows().is_none());

    // Exhaustive match — adding an arm without handling it must fail to compile.
    for o in [rows, err, cancelled] {
        match o {
            QueryOutcome::Rows(_) => {}
            QueryOutcome::Error { .. } => {}
            QueryOutcome::Cancelled => {}
        }
    }
}

#[test]
fn error_carries_message_and_sql() {
    let err = QueryOutcome::Error {
        message: "Parser Error: syntax error".into(),
        sql: "SELECT * FROM".into(),
    };
    match err {
        QueryOutcome::Error { message, sql } => {
            assert!(message.contains("syntax"));
            assert_eq!(sql, "SELECT * FROM");
        }
        _ => panic!("expected Error"),
    }
}

#[test]
fn table_is_columnar_with_dimensions() {
    let t = table_2x3();
    assert_eq!(t.col_count(), 2);
    assert_eq!(t.row_count(), 3);
    assert!(!t.is_empty());
    assert_eq!(t.columns()[0].name, "id");
    assert_eq!(t.columns()[1].ty, ColumnType::Text);
}

#[test]
fn table_row_view_borrows_across_columns() {
    let t = table_2x3();
    let row1 = t.row(1).expect("row 1 exists");
    assert_eq!(row1.len(), 2);
    assert_eq!(row1[0], &Cell::Int(2));
    assert_eq!(row1[1], &Cell::Null); // the null cell in column 2
    assert!(t.row(3).is_none()); // out of range
}

#[test]
fn empty_table() {
    let t = Table::default();
    assert!(t.is_empty());
    assert_eq!(t.row_count(), 0);
    assert_eq!(t.col_count(), 0);
    assert!(t.row(0).is_none());

    // A table of empty columns is also empty (0 rows).
    let t2 = Table::new(vec![Column::new("c", ColumnType::Int, vec![])]);
    assert!(t2.is_empty());
    assert_eq!(t2.col_count(), 1);
}

#[test]
fn table_derives_schema_from_columns() {
    let t = table_2x3();
    let s = t.schema();
    assert_eq!(s.len(), 2);
    assert_eq!(s.column_type("id"), Some(&ColumnType::Int));
    assert_eq!(s.column_type("name"), Some(&ColumnType::Text));
    let names: Vec<&str> = s.names().collect();
    assert_eq!(names, ["id", "name"]); // order preserved
}

#[test]
fn cell_null_distinct_from_empty_text() {
    // The Q12-relevant distinction: Null is not Text("").
    assert!(Cell::Null.is_null());
    assert!(!Cell::Text(String::new()).is_null());
    assert_ne!(Cell::Null, Cell::Text(String::new()));

    // display(): both render to "" here; the grid substitutes a null glyph for Null.
    assert_eq!(Cell::Null.display(), "");
    assert_eq!(Cell::Text(String::new()).display(), "");
    assert_eq!(Cell::Int(42).display(), "42");
    assert_eq!(Cell::Bool(true).display(), "true");
}

#[test]
fn interrupt_handle_is_clone_and_fires_underlying() {
    // A test Interruptible that counts interrupt() calls — proves the handle is cloneable
    // and that .interrupt() reaches the inner impl (mirrors how the dispatcher fires it).
    struct Counter(AtomicUsize);
    impl Interruptible for Counter {
        fn interrupt(&self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    let counter = Arc::new(Counter(AtomicUsize::new(0)));
    let handle = InterruptHandle::new(counter.clone());
    let clone = handle.clone();

    handle.interrupt();
    clone.interrupt();
    assert_eq!(counter.0.load(Ordering::SeqCst), 2);

    // Send + Sync: must be usable across threads (dispatcher holds it on another thread).
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<InterruptHandle>();

    // The Debug impl is opaque (it can't print the inner Arc<dyn>), so it just names the type.
    assert_eq!(format!("{:?}", InterruptHandle::noop()), "InterruptHandle");
}
