//! Engine round-trip for facets (P4.6, §6.5): `build_facet_sql` → run on a **real** `DuckdbEngine`
//! over a fixture → parse the result `Table` through `FacetState` → assert the stats.
//!
//! This is the agent-checkable wiring proof the brief calls for: the emitted SQL is not just
//! string-asserted (that's `facet_query_tests`) but actually executed and the response parsed,
//! confirming the SQL is valid DuckDB and the parser reads the columns it gets back. It runs on the
//! same engine the worker owns — no second connection.

use crate::engine::QueryOutcome;
use crate::facets::facet_query::build_facet_sql;
use crate::facets::facet_state::{FacetResult, FacetState};
use crate::harness::engine_harness::EngineHarness;
use crate::schema::ColumnType;

/// `region` repeats so the histogram has real frequencies; `score` has a NULL row.
const FIXTURE: &str = "id,region,score\n\
1,EU,10\n\
2,EU,30\n\
3,NA,20\n\
4,EU,\n\
5,APAC,40\n";

#[test]
fn numeric_facet_round_trips_through_the_engine() {
    let h = EngineHarness::from_csv(FIXTURE).expect("load");
    assert_eq!(h.schema().column_type("score"), Some(&ColumnType::Int));

    let sql = build_facet_sql("score", h.schema());
    let table = match h.query(&sql) {
        QueryOutcome::Rows(t) => t,
        other => panic!("facet SQL must be valid + return rows, got {other:?}"),
    };

    let mut state = FacetState::pending("score", ColumnType::Int);
    state.apply_result(&table);
    match state.result().expect("ready") {
        FacetResult::Summary {
            min,
            max,
            distinct,
            nulls,
        } => {
            assert_eq!(min.as_deref(), Some("10"));
            assert_eq!(max.as_deref(), Some("40"));
            assert_eq!(*distinct, 4, "10/20/30/40 distinct (the NULL excluded)");
            assert_eq!(*nulls, 1, "one NULL score row");
        }
        other => panic!("int column => summary facet, got {other:?}"),
    }
}

#[test]
fn text_facet_histogram_round_trips_with_stable_order() {
    let h = EngineHarness::from_csv(FIXTURE).expect("load");
    assert_eq!(h.schema().column_type("region"), Some(&ColumnType::Text));

    let sql = build_facet_sql("region", h.schema());
    let table = match h.query(&sql) {
        QueryOutcome::Rows(t) => t,
        other => panic!("facet SQL must be valid + return rows, got {other:?}"),
    };

    let mut state = FacetState::pending("region", ColumnType::Text);
    state.apply_result(&table);
    match state.result().expect("ready") {
        FacetResult::Histogram {
            bars,
            distinct,
            nulls,
        } => {
            // Stable top-K order: count DESC, then value ASC. EU=3 leads; APAC and NA tie at 1, so
            // the value tie-break puts APAC before NA — deterministic, never flips.
            let pairs: Vec<(&str, u64)> =
                bars.iter().map(|b| (b.value.as_str(), b.count)).collect();
            assert_eq!(pairs, vec![("EU", 3), ("APAC", 1), ("NA", 1)]);
            assert_eq!(*distinct, 3, "EU/NA/APAC");
            assert_eq!(*nulls, 0, "region has no NULLs");
        }
        other => panic!("text column => histogram facet, got {other:?}"),
    }
}

#[test]
fn all_null_text_histogram_reports_real_null_count() {
    // Regression guard: an entirely-NULL text column must report its true null count, not 0. The
    // `stats LEFT JOIN bars` shape carries the count on the sentinel row even with zero bars.
    let h = EngineHarness::from_csv("id,note\n1,\n2,\n3,\n4,\n5,\n").expect("load");
    assert_eq!(h.schema().column_type("note"), Some(&ColumnType::Text));

    let sql = build_facet_sql("note", h.schema());
    let table = match h.query(&sql) {
        QueryOutcome::Rows(t) => t,
        other => panic!("facet SQL must be valid + return rows, got {other:?}"),
    };

    let mut state = FacetState::pending("note", ColumnType::Text);
    state.apply_result(&table);
    match state.result().expect("ready") {
        FacetResult::Histogram {
            bars,
            distinct,
            nulls,
        } => {
            assert!(bars.is_empty(), "no non-null values => no bars");
            assert_eq!(*distinct, 0, "no distinct non-null values");
            assert_eq!(*nulls, 5, "all five rows are NULL — the lost-count bug");
        }
        other => panic!("text column => histogram facet, got {other:?}"),
    }
}

#[test]
fn facet_for_keyword_column_is_valid_sql() {
    // A reserved-word column name must be quoted so the facet SQL parses (the shared sql_ident
    // escaper). `order` is reserved.
    let h = EngineHarness::from_csv("id,order\n1,a\n2,a\n3,b\n").expect("load");
    let sql = build_facet_sql("order", h.schema());
    let table = match h.query(&sql) {
        QueryOutcome::Rows(t) => t,
        other => panic!("quoted-keyword facet must be valid SQL, got {other:?}"),
    };
    let mut state = FacetState::pending("order", ColumnType::Text);
    state.apply_result(&table);
    assert_eq!(state.result().unwrap().distinct(), 2, "a, b");
}
