//! Integration tests for `DuckdbEngine` — exercised headlessly over tiny tempfile CSVs.
//!
//! Covers the P1.4 exit criteria (`dev/TASKS.md`): parse-once + typed rows, `created_at ->
//! DATE` golden, malformed SQL -> `Error`, and the A1 regression guard (interrupt from
//! another thread -> `Cancelled`, then the SAME connection still returns correct rows).

use std::io::Write;
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use tempfile::NamedTempFile;

use crate::engine::duckdb_engine::DuckdbEngine;
use crate::engine::types::{Cell, QueryOutcome};
use crate::engine::{CsvOpts, QueryEngine};
use crate::schema::ColumnType;

/// Write `contents` to a temp .csv and return (the file handle to keep it alive, its path).
fn fixture(contents: &str) -> (NamedTempFile, PathBuf) {
    let mut f = NamedTempFile::with_suffix(".csv").expect("tempfile");
    f.write_all(contents.as_bytes()).expect("write fixture");
    f.flush().expect("flush");
    let path = f.path().to_path_buf();
    (f, path)
}

const SALES: &str = "id,status,amount,created_at\n\
1,shipped,12.50,2024-03-04\n\
2,pending,7.00,2024-03-05\n\
3,shipped,99.99,2024-03-06\n";

fn open_sales() -> (NamedTempFile, DuckdbEngine) {
    let (f, path) = fixture(SALES);
    let engine = DuckdbEngine::open(&path, &CsvOpts::default()).expect("load fixture");
    (f, engine)
}

#[test]
fn loads_and_sniffs_types_including_date() {
    let (_f, engine) = open_sales();
    let schema = engine.schema();
    assert_eq!(schema.len(), 4);
    // The headline DuckDB win: created_at sniffs to DATE, not text.
    assert_eq!(schema.column_type("created_at"), Some(&ColumnType::Date));
    assert_eq!(schema.column_type("id"), Some(&ColumnType::Int));
    assert_eq!(schema.column_type("amount"), Some(&ColumnType::Float));
    assert_eq!(schema.column_type("status"), Some(&ColumnType::Text));
}

#[test]
fn query_returns_typed_columnar_rows() {
    let (_f, engine) = open_sales();
    let outcome =
        engine.query("SELECT id, status, amount FROM t WHERE status = 'shipped' ORDER BY id");
    let table = match &outcome {
        QueryOutcome::Rows(t) => t,
        other => panic!("expected Rows, got {other:?}"),
    };
    assert_eq!(table.col_count(), 3);
    assert_eq!(table.row_count(), 2); // two shipped rows

    // columnar: column 0 is id (Int), column 2 is amount (Float)
    assert_eq!(table.columns()[0].ty, ColumnType::Int);
    assert_eq!(table.columns()[2].ty, ColumnType::Float);
    assert_eq!(table.columns()[0].cells[0], Cell::Int(1));
    assert_eq!(table.columns()[0].cells[1], Cell::Int(3));

    // row-view crosses columns
    let row0 = table.row(0).unwrap();
    assert_eq!(row0[0], &Cell::Int(1));
    assert_eq!(row0[1], &Cell::Text("shipped".into()));
}

#[test]
fn load_happens_and_count_is_correct() {
    let (_f, engine) = open_sales();
    let outcome = engine.query("SELECT count(*) AS n FROM t");
    match outcome {
        QueryOutcome::Rows(t) => {
            assert_eq!(t.row_count(), 1);
            assert_eq!(t.columns()[0].cells[0], Cell::Int(3));
        }
        other => panic!("expected Rows, got {other:?}"),
    }
}

#[test]
fn malformed_sql_returns_error_not_panic() {
    let (_f, engine) = open_sales();
    let outcome = engine.query("SELECT * FROM"); // syntactically invalid
    match outcome {
        QueryOutcome::Error { message, sql } => {
            assert!(!message.is_empty());
            assert_eq!(sql, "SELECT * FROM");
        }
        other => panic!("expected Error, got {other:?}"),
    }
}

#[test]
fn nulls_distinct_from_text() {
    let (_f, path) = fixture("a,b\n1,hello\n2,\n");
    let engine = DuckdbEngine::open(&path, &CsvOpts::default()).expect("load");
    let outcome = engine.query("SELECT b FROM t ORDER BY a");
    match outcome {
        QueryOutcome::Rows(t) => {
            // empty CSV field -> DuckDB default is NULL (Q12 default; documented).
            assert_eq!(t.columns()[0].cells[0], Cell::Text("hello".into()));
            assert!(t.columns()[0].cells[1].is_null());
        }
        other => panic!("expected Rows, got {other:?}"),
    }
}

#[test]
fn temporal_and_decimal_cells_render_faithfully() {
    // Regression guard for the `Date32(19372)` / `Decimal(1250.50)` Debug-garbage defect: a DATE
    // and a DECIMAL cell must arrive through `value_ref_to_cell` as DuckDB's canonical text, not
    // the `{:?}` form of the `ValueRef` enum.
    let (_f, engine) = open_sales();
    let outcome = engine.query(
        "SELECT created_at, CAST(amount AS DECIMAL(12,2)) AS amt FROM t ORDER BY id LIMIT 1",
    );
    match outcome {
        QueryOutcome::Rows(t) => {
            assert_eq!(t.columns()[0].cells[0], Cell::Text("2024-03-04".into()));
            assert_eq!(t.columns()[1].cells[0], Cell::Text("12.50".into()));
        }
        other => panic!("expected Rows, got {other:?}"),
    }
}

#[test]
fn timestamp_and_time_cells_render_faithfully() {
    // The other temporal arms: a TIMESTAMP and a TIME value round-trip to DuckDB's canonical text.
    let (_f, engine) = open_sales();
    let outcome = engine.query(
        "SELECT CAST('2023-01-15 12:34:56.5' AS TIMESTAMP) AS ts, CAST('01:02:03' AS TIME) AS tm FROM t LIMIT 1",
    );
    match outcome {
        QueryOutcome::Rows(t) => {
            assert_eq!(
                t.columns()[0].cells[0],
                Cell::Text("2023-01-15 12:34:56.5".into())
            );
            assert_eq!(t.columns()[1].cells[0], Cell::Text("01:02:03".into()));
        }
        other => panic!("expected Rows, got {other:?}"),
    }
}

#[test]
fn distinct_values_for_autocomplete() {
    let (_f, engine) = open_sales();
    let outcome = engine.distinct("status", 10);
    match outcome {
        QueryOutcome::Rows(t) => {
            // two distinct statuses, each with a count column
            assert_eq!(t.row_count(), 2);
            assert_eq!(t.col_count(), 2);
        }
        other => panic!("expected Rows, got {other:?}"),
    }
}

/// The A1 regression guard, in ciq's own engine, modeling ciq's real topology: the **worker
/// thread owns the engine** (DuckDB's `Connection` is `Send` but `!Sync`, so it is moved, not
/// shared); only the `Send + Sync` `InterruptHandle` stays on the dispatcher (main) thread.
/// Confirms (a) an interrupted query comes back `Cancelled`, and (b) the SAME engine/connection
/// still serves a correct query afterward — the behavior the A1 micro-spike proved. A future
/// duckdb bump that regresses reuse turns the build red here.
#[test]
fn interrupt_yields_cancelled_then_connection_reusable() {
    // A bigger table so the self-join blocks long enough to interrupt deterministically.
    let mut csv = String::from("id\n");
    for i in 0..20000 {
        csv.push_str(&i.to_string());
        csv.push('\n');
    }
    let (_f, path) = fixture(&csv);
    let engine = DuckdbEngine::open(&path, &CsvOpts::default()).expect("load");

    // dispatcher holds only the Send+Sync interrupt handle
    let handle = engine.interrupt_handle();

    // worker thread OWNS the engine (moved in), runs a heavy query, hands the engine back
    let (tx, rx) = mpsc::channel::<(DuckdbEngine, QueryOutcome)>();
    thread::spawn(move || {
        // ~400e6-row self-join: never finishes before we interrupt.
        let out = engine.query("SELECT count(*) FROM t a, t b WHERE a.id >= b.id");
        let _ = tx.send((engine, out));
    });

    thread::sleep(Duration::from_millis(300));
    handle.interrupt(); // dispatcher interrupts from its own thread (§0/D4)

    let (engine, out) = rx
        .recv_timeout(Duration::from_secs(30))
        .expect("worker returned");
    assert!(
        matches!(out, QueryOutcome::Cancelled),
        "expected Cancelled, got {out:?}"
    );

    // THE GUARD: the same engine/connection still works after the interrupt.
    match engine.query("SELECT count(*) AS n FROM t") {
        QueryOutcome::Rows(t) => assert_eq!(t.columns()[0].cells[0], Cell::Int(20000)),
        other => panic!("connection not reusable after interrupt: {other:?}"),
    }
}
