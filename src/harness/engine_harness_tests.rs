//! Tests for `EngineHarness`, including the P1.6 no-TTY self-test.

use crate::engine::QueryOutcome;
use crate::harness::engine_harness::EngineHarness;
use crate::schema::ColumnType;

const SALES: &str = "id,status,amount,created_at\n\
1,shipped,12.50,2024-03-04\n\
2,pending,7.00,2024-03-05\n";

#[test]
fn from_csv_loads_and_queries() {
    let h = EngineHarness::from_csv(SALES).expect("load");
    assert_eq!(h.schema().len(), 4);
    assert_eq!(
        h.schema().column_type("created_at"),
        Some(&ColumnType::Date)
    );

    match h.query("SELECT count(*) AS n FROM t") {
        QueryOutcome::Rows(t) => assert_eq!(t.row_count(), 1),
        other => panic!("expected Rows, got {other:?}"),
    }
}

#[test]
fn fixture_outlives_construction() {
    // The temp file must persist after `from_csv` returns so later queries still work.
    let h = EngineHarness::from_csv(SALES).expect("load");
    // multiple queries after construction — would fail if the temp CSV were deleted
    assert!(h.query("SELECT * FROM t").is_rows());
    assert!(h.query("SELECT status FROM t WHERE id = 1").is_rows());
}

#[test]
fn distinct_passthrough() {
    let h = EngineHarness::from_csv(SALES).expect("load");
    assert!(h.distinct("status", 10).is_rows());
}

#[test]
fn error_outcome_surfaces() {
    let h = EngineHarness::from_csv(SALES).expect("load");
    assert!(h.query("SELECT bogus(").is_error());
}

/// P1.6 exit criterion: the harness runs with no controlling terminal. We assert that by
/// running the harness with the `TERM` env var unset for the duration of the call — the
/// engine path touches no terminal, so this must still succeed. (Single-threaded test
/// execution, per the project convention, makes the transient env mutation safe.)
#[test]
fn runs_with_term_unset() {
    let prev = std::env::var_os("TERM");
    // SAFETY: tests run single-threaded (`--test-threads=1`), so no other thread observes
    // this transient env change.
    unsafe {
        std::env::remove_var("TERM");
    }

    let h = EngineHarness::from_csv(SALES).expect("load with no TERM");
    let ok = h.query("SELECT count(*) FROM t").is_rows();

    // restore before asserting, so a failure doesn't leak the unset into other tests
    if let Some(v) = prev {
        unsafe {
            std::env::set_var("TERM", v);
        }
    }
    assert!(ok, "engine harness must work with no controlling terminal");
}
