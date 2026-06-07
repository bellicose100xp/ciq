//! Tests for the worker thread — drives `spawn_worker` with a `FakeEngine` over real channels
//! (deterministic; no sleeps, no TTY). Covers the three `QueryOutcome` → `QueryResponse`
//! mappings and panic isolation (a panic in the engine becomes an `Error`, the harness
//! survives).

use std::path::Path;
use std::sync::mpsc::channel;

use crate::engine::fake_engine::FakeEngine;
use crate::engine::types::{Cell, Column, InterruptHandle, Interruptible, QueryOutcome, Table};
use crate::engine::{CsvOpts, QueryEngine};
use crate::query::worker::spawn_worker;
use crate::query::worker::types::{QueryRequest, QueryResponse};
use crate::schema::{ColumnMeta, ColumnType, Schema};

fn schema() -> Schema {
    Schema::new(vec![
        ColumnMeta::new("id", ColumnType::Int),
        ColumnMeta::new("status", ColumnType::Text),
    ])
}

fn two_row_table() -> Table {
    Table::new(vec![
        Column::new("id", ColumnType::Int, vec![Cell::Int(1), Cell::Int(2)]),
        Column::new(
            "status",
            ColumnType::Text,
            vec![Cell::Text("active".into()), Cell::Text("idle".into())],
        ),
    ])
}

#[test]
fn rows_outcome_becomes_processed_success() {
    let engine =
        Box::new(FakeEngine::new(schema()).with_default(QueryOutcome::Rows(two_row_table())));
    let (req_tx, req_rx) = channel();
    let (resp_tx, resp_rx) = channel();
    let handle = spawn_worker(engine, req_rx, resp_tx);

    req_tx
        .send(QueryRequest::new("SELECT * FROM t", 1))
        .unwrap();
    let resp = resp_rx.recv().unwrap();
    match resp {
        QueryResponse::ProcessedSuccess { result, request_id } => {
            assert_eq!(request_id, 1);
            assert_eq!(result.rows.row_count(), 2);
            assert_eq!(result.schema.len(), 2);
            assert_eq!(result.grid.body.len(), 2, "one grid body line per row");
        }
        other => panic!("expected ProcessedSuccess, got {other:?}"),
    }

    drop(req_tx); // close the channel so the loop ends
    handle.join().unwrap();
}

#[test]
fn error_outcome_becomes_error_response() {
    let engine = Box::new(FakeEngine::new(schema()).with_response(
        "bad sql",
        QueryOutcome::Error {
            message: "Parser Error: syntax error".into(),
            sql: "bad sql".into(),
        },
    ));
    let (req_tx, req_rx) = channel();
    let (resp_tx, resp_rx) = channel();
    let handle = spawn_worker(engine, req_rx, resp_tx);

    req_tx.send(QueryRequest::new("bad sql", 9)).unwrap();
    match resp_rx.recv().unwrap() {
        QueryResponse::Error {
            message,
            request_id,
        } => {
            assert_eq!(request_id, 9);
            assert!(message.contains("Parser Error"));
        }
        other => panic!("expected Error, got {other:?}"),
    }

    drop(req_tx);
    handle.join().unwrap();
}

#[test]
fn cancelled_outcome_becomes_cancelled_response() {
    let engine =
        Box::new(FakeEngine::new(schema()).with_default(QueryOutcome::Rows(two_row_table())));
    let handle_to_interrupt = engine.interrupt_handle();
    let (req_tx, req_rx) = channel();
    let (resp_tx, resp_rx) = channel();
    let worker = spawn_worker(engine, req_rx, resp_tx);

    // Flip the interrupt flag so the FakeEngine returns Cancelled for the next query, then send.
    handle_to_interrupt.interrupt();
    req_tx.send(QueryRequest::new("SELECT 1", 3)).unwrap();
    match resp_rx.recv().unwrap() {
        QueryResponse::Cancelled { request_id } => assert_eq!(request_id, 3),
        other => panic!("expected Cancelled, got {other:?}"),
    }

    drop(req_tx);
    worker.join().unwrap();
}

/// An engine whose `query` panics — used to prove panic isolation.
struct PanickingEngine {
    schema: Schema,
}

impl QueryEngine for PanickingEngine {
    fn load(&mut self, _path: &Path, _opts: &CsvOpts) -> Result<Schema, crate::error::EngineError> {
        Ok(self.schema.clone())
    }
    fn query(&self, _sql: &str) -> QueryOutcome {
        panic!("boom inside the engine");
    }
    fn distinct(&self, _col: &str, _limit: usize) -> QueryOutcome {
        QueryOutcome::Rows(Table::default())
    }
    fn schema(&self) -> &Schema {
        &self.schema
    }
    fn interrupt_handle(&self) -> InterruptHandle {
        struct NoopInterrupt;
        impl Interruptible for NoopInterrupt {
            fn interrupt(&self) {}
        }
        InterruptHandle::new(std::sync::Arc::new(NoopInterrupt))
    }
}

#[test]
fn engine_panic_becomes_error_and_worker_survives() {
    let engine = Box::new(PanickingEngine { schema: schema() });
    let (req_tx, req_rx) = channel();
    let (resp_tx, resp_rx) = channel();
    let worker = spawn_worker(engine, req_rx, resp_tx);

    // First request panics inside the engine -> per-request catch turns it into an Error for id 1.
    req_tx.send(QueryRequest::new("SELECT 1", 1)).unwrap();
    match resp_rx.recv().unwrap() {
        QueryResponse::Error { request_id, .. } => assert_eq!(request_id, 1),
        other => panic!("expected Error from panicking query, got {other:?}"),
    }

    // The worker must still be alive to serve a second request (loop not torn down).
    req_tx.send(QueryRequest::new("SELECT 2", 2)).unwrap();
    match resp_rx.recv().unwrap() {
        QueryResponse::Error { request_id, .. } => assert_eq!(request_id, 2),
        other => panic!("expected Error from second panicking query, got {other:?}"),
    }

    drop(req_tx);
    worker.join().unwrap();
}

#[test]
fn multiple_requests_processed_in_order() {
    let engine =
        Box::new(FakeEngine::new(schema()).with_default(QueryOutcome::Rows(two_row_table())));
    let (req_tx, req_rx) = channel();
    let (resp_tx, resp_rx) = channel();
    let worker = spawn_worker(engine, req_rx, resp_tx);

    for id in 1..=3 {
        req_tx
            .send(QueryRequest::new("SELECT * FROM t", id))
            .unwrap();
    }
    for id in 1..=3 {
        assert_eq!(resp_rx.recv().unwrap().request_id(), id);
    }

    drop(req_tx);
    worker.join().unwrap();
}
