//! Tests for the worker channel types — construction and an exhaustive match over the three
//! `QueryResponse` variants (the contract the App's dispatch loop relies on being total).

use crate::engine::types::{Cell, Column, Table};
use crate::query::worker::types::{ProcessedResult, QueryRequest, QueryResponse, RequestKind};
use crate::schema::{ColumnMeta, ColumnType, Schema};

fn sample_table() -> Table {
    Table::new(vec![
        Column::new("id", ColumnType::Int, vec![Cell::Int(1), Cell::Int(2)]),
        Column::new(
            "status",
            ColumnType::Text,
            vec![Cell::Text("ok".into()), Cell::Null],
        ),
    ])
}

fn sample_processed() -> ProcessedResult {
    let table = sample_table();
    let schema = table.schema();
    ProcessedResult::new(table, schema, 7)
}

#[test]
fn request_holds_query_and_id_no_cancel_token() {
    let req = QueryRequest::new("SELECT * FROM t", 42);
    assert_eq!(req.query, "SELECT * FROM t");
    assert_eq!(req.request_id, 42);
}

#[test]
fn processed_result_carries_rows_schema_and_time() {
    let p = sample_processed();
    assert_eq!(p.rows.row_count(), 2);
    assert_eq!(p.rows.col_count(), 2);
    assert_eq!(p.schema.len(), 2);
    assert_eq!(
        p.schema.column_type("id"),
        Some(&ColumnType::Int),
        "schema is derived from the result columns"
    );
    assert_eq!(p.execution_time_ms, 7);
}

#[test]
fn processed_result_schema_matches_manual_schema() {
    let p = sample_processed();
    let expected = Schema::new(vec![
        ColumnMeta::new("id", ColumnType::Int),
        ColumnMeta::new("status", ColumnType::Text),
    ]);
    assert_eq!(p.schema, expected);
}

#[test]
fn response_request_id_for_each_variant() {
    let success = QueryResponse::ProcessedSuccess {
        result: sample_processed(),
        request_id: 5,
        kind: RequestKind::Main,
    };
    let error = QueryResponse::Error {
        message: "bad sql".into(),
        request_id: 6,
        kind: RequestKind::Main,
    };
    let cancelled = QueryResponse::Cancelled {
        request_id: 7,
        kind: RequestKind::Main,
    };

    assert_eq!(success.request_id(), 5);
    assert_eq!(error.request_id(), 6);
    assert_eq!(cancelled.request_id(), 7);
}

#[test]
fn error_response_carries_the_querys_real_id() {
    // Every Error (including a per-request engine panic) is correlated by the real request_id
    // of the query it answers — there is no special id-0 marker.
    let resp = QueryResponse::Error {
        message: "query panicked: boom".into(),
        request_id: 42,
        kind: RequestKind::Main,
    };
    assert_eq!(resp.request_id(), 42);
}

#[test]
fn exhaustive_match_over_all_response_variants() {
    // The dispatch loop relies on this match being total; the test enumerates every arm so a
    // future variant addition fails to compile here (and forces a conscious update).
    for resp in [
        QueryResponse::ProcessedSuccess {
            result: sample_processed(),
            request_id: 1,
            kind: RequestKind::Main,
        },
        QueryResponse::Error {
            message: "x".into(),
            request_id: 2,
            kind: RequestKind::Main,
        },
        QueryResponse::Cancelled {
            request_id: 3,
            kind: RequestKind::Main,
        },
    ] {
        let label = match resp {
            QueryResponse::ProcessedSuccess {
                result, request_id, ..
            } => {
                assert_eq!(result.rows.row_count(), 2);
                format!("success:{request_id}")
            }
            QueryResponse::Error {
                message,
                request_id,
                ..
            } => format!("error:{request_id}:{message}"),
            QueryResponse::Cancelled { request_id, .. } => format!("cancelled:{request_id}"),
        };
        assert!(!label.is_empty());
    }
}
