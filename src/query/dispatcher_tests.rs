//! Tests for the out-of-band cancel dispatcher (P2.5 / §0/D4).
//!
//! The headline test wires a real worker over channels to a **gated** `FakeEngine`: dispatch
//! id=1 (worker blocks in `query()`), dispatch id=2 (dispatcher fires `interrupt()`), then
//! assert the worker emits `Cancelled{1}` and `ProcessedSuccess{2}`, that only id=2 surfaces
//! (the stale `Cancelled{1}` is drained via stale-discard), and that the interrupt was issued
//! **from the dispatcher thread**, never the worker thread. Fully deterministic: ordering is
//! pinned by the gate's entered/release rendezvous channels, never by sleeps.

use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};
use std::thread::ThreadId;

use crate::engine::fake_engine::FakeEngine;
use crate::engine::types::{Cell, Column, Interruptible, QueryOutcome, Table};
use crate::engine::{InterruptHandle, QueryEngine};
use crate::query::dispatcher::Dispatcher;
use crate::query::worker::spawn_worker;
use crate::query::worker::types::{QueryResponse, RequestKind};
use crate::schema::{ColumnMeta, ColumnType, Schema};

fn schema() -> Schema {
    Schema::new(vec![ColumnMeta::new("id", ColumnType::Int)])
}

fn one_row_table() -> Table {
    Table::new(vec![Column::new(
        "id",
        ColumnType::Int,
        vec![Cell::Int(42)],
    )])
}

/// An `Interruptible` that records the `ThreadId` of every `.interrupt()` caller, then delegates
/// to the engine's real handle (which flips the cancel flag + releases the gate).
struct RecordingInterrupt {
    inner: InterruptHandle,
    callers: Arc<Mutex<Vec<ThreadId>>>,
}

impl Interruptible for RecordingInterrupt {
    fn interrupt(&self) {
        self.callers
            .lock()
            .unwrap()
            .push(std::thread::current().id());
        self.inner.interrupt();
    }
}

#[test]
fn dispatch_without_in_flight_does_not_interrupt() {
    let engine = FakeEngine::new(schema()).with_default(QueryOutcome::Rows(one_row_table()));
    let callers = Arc::new(Mutex::new(Vec::new()));
    let recording = InterruptHandle::new(Arc::new(RecordingInterrupt {
        inner: engine.interrupt_handle(),
        callers: callers.clone(),
    }));
    let (req_tx, _req_rx) = channel();
    let mut dispatcher = Dispatcher::new(req_tx, recording);

    assert!(!dispatcher.in_flight());
    let id = dispatcher.dispatch("SELECT 1").unwrap();
    assert_eq!(id, 1);
    assert!(dispatcher.in_flight());
    assert!(
        callers.lock().unwrap().is_empty(),
        "no interrupt fires when nothing was in-flight"
    );
}

#[test]
fn dispatch_facet_rides_its_own_lane_without_interrupt_or_main_bump() {
    // A facet fetch (P4.6) is out of the main lane: it must not interrupt an in-flight main query,
    // must not bump the main request_id, and rides its own monotonic facet id. Its request is tagged
    // RequestKind::Facet with the column so the App routes it to the popup.
    let engine = FakeEngine::new(schema()).with_default(QueryOutcome::Rows(one_row_table()));
    let callers = Arc::new(Mutex::new(Vec::new()));
    let recording = InterruptHandle::new(Arc::new(RecordingInterrupt {
        inner: engine.interrupt_handle(),
        callers: callers.clone(),
    }));
    let (req_tx, req_rx) = channel();
    let mut dispatcher = Dispatcher::new(req_tx, recording);

    // A main query is in-flight.
    let main_id = dispatcher.dispatch("SELECT 1").unwrap();
    assert_eq!(main_id, 1);
    assert!(dispatcher.in_flight());

    // A facet fetch rides its own lane: own id (starts at 1, independent), no interrupt, no main bump.
    let facet_id = dispatcher
        .dispatch_facet("SELECT min(id) FROM t", "id")
        .unwrap();
    assert_eq!(facet_id, 1, "facet lane is independent of the main lane");
    assert_eq!(
        dispatcher.latest_id(),
        main_id,
        "a facet fetch must not bump the main request_id"
    );
    assert!(
        callers.lock().unwrap().is_empty(),
        "dispatching a facet must not interrupt the in-flight main query"
    );

    // The request that went out is tagged Facet with the column.
    let reqs: Vec<_> = std::iter::from_fn(|| req_rx.try_recv().ok()).collect();
    let facet_req = reqs
        .iter()
        .find(|r| matches!(&r.kind, RequestKind::Facet { column } if column == "id"))
        .expect("a Facet request for `id` was sent");
    assert!(
        facet_req.query.contains("min(id)"),
        "got: {}",
        facet_req.query
    );

    // A second facet fetch advances only the facet lane.
    let facet_id2 = dispatcher
        .dispatch_facet("SELECT min(id) FROM t", "id")
        .unwrap();
    assert_eq!(facet_id2, 2);
    assert_eq!(dispatcher.latest_id(), main_id, "still no main bump");
}

#[test]
fn second_dispatch_interrupts_first_from_dispatcher_thread() {
    let (engine, gate) = FakeEngine::new(schema())
        .with_default(QueryOutcome::Rows(one_row_table()))
        .with_gate();
    let callers = Arc::new(Mutex::new(Vec::new()));
    let recording = InterruptHandle::new(Arc::new(RecordingInterrupt {
        inner: engine.interrupt_handle(),
        callers: callers.clone(),
    }));

    let (req_tx, req_rx) = channel();
    let (resp_tx, resp_rx) = channel();
    let worker = spawn_worker(Box::new(engine), req_rx, resp_tx);
    let worker_thread_id = worker.thread().id();
    let dispatcher_thread_id = std::thread::current().id();

    let mut dispatcher = Dispatcher::new(req_tx, recording);

    // Dispatch id=1; wait until the worker is truly blocked inside query() before superseding.
    let id1 = dispatcher.dispatch("SELECT 1").unwrap();
    assert_eq!(id1, 1);
    gate.wait_entered();

    // Dispatch id=2; the dispatcher must fire interrupt() (from THIS thread) before sending.
    let id2 = dispatcher.dispatch("SELECT 2").unwrap();
    assert_eq!(id2, 2);

    // id=1 unblocks (interrupted) and comes back Cancelled — stale, so the App drains it.
    let r1 = resp_rx.recv().unwrap();
    assert!(
        matches!(r1, QueryResponse::Cancelled { request_id: 1, .. }),
        "id=1 should come back Cancelled, got {r1:?}"
    );
    assert!(
        dispatcher.is_stale(r1.request_id()),
        "the Cancelled{{1}} is stale and must be drained, not surfaced"
    );
    assert!(!dispatcher.accept(r1.request_id()));

    // id=2 re-enters the gate; release it so it completes normally and surfaces.
    gate.wait_entered();
    gate.release();
    let r2 = resp_rx.recv().unwrap();
    match &r2 {
        QueryResponse::ProcessedSuccess {
            request_id, result, ..
        } => {
            assert_eq!(*request_id, 2);
            assert_eq!(result.rows.row_count(), 1);
        }
        other => panic!("id=2 should ProcessedSuccess, got {other:?}"),
    }
    assert!(
        dispatcher.accept(r2.request_id()),
        "id=2 is the latest and must be accepted"
    );
    assert!(!dispatcher.in_flight());

    // The interrupt was issued exactly once, from the dispatcher thread, never the worker.
    let recorded = callers.lock().unwrap().clone();
    assert_eq!(recorded.len(), 1, "exactly one interrupt fired");
    assert_eq!(
        recorded[0], dispatcher_thread_id,
        "interrupt must be issued from the dispatcher thread"
    );
    assert_ne!(
        recorded[0], worker_thread_id,
        "interrupt must NOT come from the worker thread (it is blocked inside query)"
    );

    drop(dispatcher);
    worker.join().unwrap();
}
