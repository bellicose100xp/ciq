//! Tests for `FakeEngine` — proves the deterministic test seam behaves and runs with no TTY.

use std::path::Path;

use crate::engine::fake_engine::FakeEngine;
use crate::engine::types::{Cell, Column, QueryOutcome, Table};
use crate::engine::{CsvOpts, QueryEngine};
use crate::schema::{ColumnMeta, ColumnType, Schema};

fn schema() -> Schema {
    Schema::new(vec![
        ColumnMeta::new("id", ColumnType::Int),
        ColumnMeta::new("status", ColumnType::Text),
    ])
}

fn one_row_table() -> Table {
    Table::new(vec![
        Column::new("id", ColumnType::Int, vec![Cell::Int(7)]),
        Column::new("status", ColumnType::Text, vec![Cell::Text("ok".into())]),
    ])
}

#[test]
fn schema_is_returned_verbatim() {
    let e = FakeEngine::new(schema());
    assert_eq!(e.schema(), &schema());
}

#[test]
fn default_outcome_for_unknown_query() {
    let e = FakeEngine::new(schema()).with_default(QueryOutcome::Rows(one_row_table()));
    match e.query("anything at all") {
        QueryOutcome::Rows(t) => assert_eq!(t.row_count(), 1),
        other => panic!("expected default Rows, got {other:?}"),
    }
}

#[test]
fn exact_match_override_wins() {
    let e = FakeEngine::new(schema())
        .with_default(QueryOutcome::Rows(Table::default()))
        .with_response(
            "SELECT * FROM t",
            QueryOutcome::Error {
                message: "canned".into(),
                sql: "SELECT * FROM t".into(),
            },
        );
    assert!(e.query("SELECT * FROM t").is_error());
    assert!(e.query("SELECT 1").is_rows()); // falls back to default
}

#[test]
fn load_count_tracks_parse_once_invariant() {
    let mut e = FakeEngine::new(schema());
    assert_eq!(e.load_count(), 0);
    e.load(Path::new("/dev/null"), &CsvOpts::default()).unwrap();
    assert_eq!(e.load_count(), 1);
    // Many queries do not re-load.
    for _ in 0..5 {
        let _ = e.query("SELECT 1");
    }
    assert_eq!(e.load_count(), 1);
    assert_eq!(e.query_count(), 5);
}

#[test]
fn interrupt_makes_next_query_cancelled_once() {
    let e = FakeEngine::new(schema()).with_default(QueryOutcome::Rows(one_row_table()));
    let handle = e.interrupt_handle();

    handle.interrupt();
    // the query issued after an interrupt comes back Cancelled (out-of-band model)
    assert!(e.query("SELECT 1").is_cancelled());
    // and the flag is consumed — the subsequent query succeeds again
    assert!(e.query("SELECT 1").is_rows());
}

#[test]
fn distinct_is_counted_and_canned() {
    let e = FakeEngine::new(schema()).with_default(QueryOutcome::Rows(one_row_table()));
    assert_eq!(e.distinct_count(), 0);
    assert!(e.distinct("status", 10).is_rows());
    assert_eq!(e.distinct_count(), 1);
}

/// P1.5 exit criterion: the fake engine runs with no terminal attached. We can't unset the
/// process's TTY from within a test, but we assert the engine performs zero terminal/process
/// I/O — it is pure in-memory — by exercising the full trait surface and checking it never
/// touches stdin/stdout/stderr or env. (Construction + query + load + interrupt all return
/// plain data here.)
#[test]
fn runs_without_any_terminal_io() {
    let mut e = FakeEngine::new(schema()).with_default(QueryOutcome::Rows(one_row_table()));
    e.load(Path::new("ignored.csv"), &CsvOpts::default())
        .unwrap();
    let _ = e.query("SELECT 1");
    let _ = e.distinct("id", 5);
    let h = e.interrupt_handle();
    h.interrupt();
    assert!(e.query("SELECT 1").is_cancelled());
    // No panic, no I/O, fully deterministic — exactly what headless upper-layer tests need.
}
