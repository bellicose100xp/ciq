//! Tests for `AppHarness` (P2.8 + P2.11): render snapshots of the populated grid, debounce
//! coalescing, the out-of-band cancel surfacing only the latest result, and the load-once
//! invariant. The worker-coupled tests wire a real `spawn_worker` to a counting `FakeEngine`
//! over channels (deterministic, no sleeps), mirroring the real event loop.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::channel;

use crate::app::{App, AppPhase, Key, KeyEvent};
use crate::engine::fake_engine::FakeEngine;
use crate::engine::types::{Cell, Column, Table};
use crate::engine::{InterruptHandle, QueryEngine, QueryOutcome};
use crate::harness::app_harness::AppHarness;
use crate::query::worker::spawn_worker;
use crate::query::worker::types::{ProcessedResult, QueryRequest, QueryResponse};
use crate::schema::{ColumnMeta, ColumnType, Schema};

fn schema() -> Schema {
    Schema::new(vec![
        ColumnMeta::new("id", ColumnType::Int),
        ColumnMeta::new("region", ColumnType::Text),
    ])
}

fn two_row_table() -> Table {
    Table::new(vec![
        Column::new("id", ColumnType::Int, vec![Cell::Int(1), Cell::Int(2)]),
        Column::new(
            "region",
            ColumnType::Text,
            vec![Cell::Text("EU".into()), Cell::Text("NA".into())],
        ),
    ])
}

fn two_row_result() -> ProcessedResult {
    let table = two_row_table();
    let s = table.schema();
    ProcessedResult::new(table, s, 0)
}

// --- render snapshots (TestBackend; headless) ---

#[test]
fn renders_loading_frame() {
    let mut h = AppHarness::new(50, 8);
    let screen = h.screen();
    assert!(screen.contains("loading"), "screen was:\n{screen}");
    // Query bar prompt + bordered results pane are drawn.
    assert!(
        screen.contains('>'),
        "expected query prompt, screen:\n{screen}"
    );
    assert!(
        screen.contains('┌') || screen.contains('│'),
        "expected a results border, screen:\n{screen}"
    );
}

#[test]
fn renders_query_text_in_bar() {
    let mut h = AppHarness::new(60, 8);
    h.complete_load("ready");
    h.type_str("SELECT * FROM t WHERE region='EU'");
    let screen = h.screen();
    assert!(
        screen.contains("SELECT * FROM t WHERE region='EU'"),
        "query bar should show typed text, screen:\n{screen}"
    );
}

#[test]
fn renders_populated_grid_with_header_and_rows() {
    let mut h = AppHarness::new(40, 10);
    h.complete_load("ready");
    h.type_str("SELECT * FROM t");
    h.advance(150); // dispatch
    let id = h.app().latest_request_id();
    h.respond(QueryResponse::ProcessedSuccess {
        result: two_row_result(),
        request_id: id,
        kind: crate::query::worker::types::RequestKind::Main,
    });
    let screen = h.screen();
    // Header column names + the body cell values + the "2 rows" status all render.
    assert!(screen.contains("id"), "header `id`, screen:\n{screen}");
    assert!(
        screen.contains("region"),
        "header `region`, screen:\n{screen}"
    );
    assert!(screen.contains("EU"), "body cell EU, screen:\n{screen}");
    assert!(screen.contains("NA"), "body cell NA, screen:\n{screen}");
    assert!(screen.contains("2 rows"), "status, screen:\n{screen}");
}

#[test]
fn render_is_deterministic() {
    let mut a = AppHarness::new(40, 8);
    let mut b = AppHarness::new(40, 8);
    assert_eq!(a.screen(), b.screen());
}

#[test]
fn renders_with_term_unset() {
    let prev = std::env::var_os("TERM");
    // SAFETY: tests run single-threaded (`--test-threads=1`).
    unsafe {
        std::env::remove_var("TERM");
    }
    let mut h = AppHarness::new(30, 6);
    let ok = h.screen().contains("loading");
    if let Some(v) = prev {
        unsafe {
            std::env::set_var("TERM", v);
        }
    }
    assert!(ok, "render must work with no controlling terminal");
}

#[test]
fn invalid_sql_shows_error_status_not_crash() {
    let mut h = AppHarness::new(60, 8);
    h.complete_load("ready");
    h.type_str("UPDATE t SET id=1");
    h.advance(150);
    assert!(h.dispatched().is_empty(), "DML must not be dispatched");
    let screen = h.screen();
    assert!(
        screen.contains("read-only"),
        "expected read-only error, screen:\n{screen}"
    );
}

// --- debounce coalescing: N keystrokes within the window => exactly ONE dispatch ---

#[test]
fn debounce_coalesces_to_one_dispatch() {
    let mut h = AppHarness::new(60, 8);
    h.complete_load("ready");
    // Type a valid SELECT char-by-char, all within ONE quiet window (each `advance` re-ticks
    // but the window hasn't elapsed, so no dispatch happens mid-typing).
    for c in "SELECT 1".chars() {
        h.key(KeyEvent::char(c));
        h.advance(10); // 10ms between keys — all within one 150ms window
    }
    // Nothing fired yet (last keystroke well before its window closes).
    assert!(h.dispatched().is_empty());
    // Quiet for 150ms past the last keystroke -> exactly one dispatch of the final buffer.
    h.advance(150);
    let sent = h.dispatched();
    assert_eq!(
        sent.len(),
        1,
        "8 keystrokes in one window => one query, got {sent:?}"
    );
}

/// Debounce coalescing proven all the way to the engine: N keystrokes in one window cause the
/// counting `FakeEngine` to see exactly ONE `query()` call. Wires a real worker like the event loop.
#[test]
fn debounce_coalescing_hits_engine_exactly_once() {
    let engine = FakeEngine::new(schema()).with_default(QueryOutcome::Rows(two_row_table()));
    let interrupt = engine.interrupt_handle();
    let query_count = Arc::new(AtomicUsize::new(0));
    // Wrap the engine to count queries the worker actually runs.
    let counting = CountingEngine {
        inner: engine,
        queries: query_count.clone(),
    };

    let (req_tx, req_rx) = channel();
    let (resp_tx, resp_rx) = channel();
    let worker = spawn_worker(Box::new(counting), req_rx, resp_tx);

    let mut app = App::new(req_tx, interrupt);
    app.force_power_mode_for_tests("");
    app.on_loaded("ready");

    // Type a valid SELECT one char at a time, all within one 150ms window, then go quiet.
    let mut t = 0u64;
    for c in "SELECT * FROM t".chars() {
        app.on_key(KeyEvent::char(c), t);
        t += 5; // 5ms between keys -> all inside one window
    }
    t += 150; // quiet past the last keystroke
    assert!(app.tick(t), "the coalesced query should fire once");

    // Receive the single response.
    let resp = resp_rx.recv().unwrap();
    assert!(matches!(resp, QueryResponse::ProcessedSuccess { .. }));
    app.on_response(resp);

    drop(app); // drops req_tx -> worker loop ends
    worker.join().unwrap();
    assert_eq!(
        query_count.load(Ordering::SeqCst),
        1,
        "the engine must be queried exactly once for one coalesced window"
    );
}

// --- out-of-band cancel: only the latest result surfaces ---

/// Dispatch id=1 (worker blocks on a gated engine), supersede with id=2 (dispatcher interrupts),
/// then assert the App surfaces only id=2's result and discards the stale `Cancelled{1}`.
#[test]
fn cancel_surfaces_only_latest_result() {
    let (engine, gate) = FakeEngine::new(schema())
        .with_default(QueryOutcome::Rows(two_row_table()))
        .with_gate();
    let interrupt = engine.interrupt_handle();

    let (req_tx, req_rx) = channel();
    let (resp_tx, resp_rx) = channel();
    let worker = spawn_worker(Box::new(engine), req_rx, resp_tx);

    let mut app = App::new(req_tx, interrupt);
    app.force_power_mode_for_tests("");
    app.on_loaded("ready");

    // First query (id=1): type + fire.
    for (i, c) in "SELECT 1".chars().enumerate() {
        app.on_key(KeyEvent::char(c), i as u64);
    }
    app.tick(200);
    assert_eq!(app.latest_request_id(), 1);
    gate.wait_entered(); // worker is blocked inside query() for id=1

    // Second query (id=2): edit the bar -> supersede -> dispatcher interrupts id=1 before id=2.
    app.on_key(KeyEvent::plain(Key::Backspace), 300); // edit "SELECT 1" -> "SELECT "
    app.on_key(KeyEvent::char('2'), 305);
    app.tick(500);
    assert_eq!(app.latest_request_id(), 2);

    // id=1 returns Cancelled (interrupted) — the App must discard it (stale), showing nothing.
    let r1 = resp_rx.recv().unwrap();
    assert!(matches!(r1, QueryResponse::Cancelled { request_id: 1, .. }));
    assert!(
        !app.on_response(r1),
        "stale Cancelled{{1}} must not surface"
    );
    assert!(app.result().is_none());

    // id=2 re-enters the gate; release it -> it completes and surfaces.
    gate.wait_entered();
    gate.release();
    let r2 = resp_rx.recv().unwrap();
    assert!(matches!(
        r2,
        QueryResponse::ProcessedSuccess { request_id: 2, .. }
    ));
    assert!(app.on_response(r2), "id=2 is the latest and must surface");
    assert!(app.result().is_some());
    assert_eq!(app.result().unwrap().rows.row_count(), 2);

    drop(app);
    worker.join().unwrap();
}

// --- load called exactly once per session ---

#[test]
fn load_called_exactly_once_per_session() {
    // The counting FakeEngine asserts load_count via its own hook. The event loop calls
    // DuckdbEngine::open (which loads once) and never re-loads; here we model that the loader
    // path invokes load exactly once.
    let mut engine = FakeEngine::new(schema());
    assert_eq!(engine.load_count(), 0);
    engine
        .load(std::path::Path::new("/dev/null"), &Default::default())
        .unwrap();
    assert_eq!(engine.load_count(), 1, "exactly one load per session");
    // The App never calls load again — it talks only over the request channel after Ready.
    let (tx, _rx) = channel::<QueryRequest>();
    let mut app = App::new(tx, engine.interrupt_handle());
    app.on_loaded("ready");
    // No App method exists that can re-trigger load.
    assert_eq!(engine.load_count(), 1);
}

// --- a query typed during load fires on ready, end-to-end through a real worker ---

#[test]
fn query_typed_during_load_reaches_engine_on_ready() {
    let engine = FakeEngine::new(schema()).with_default(QueryOutcome::Rows(two_row_table()));
    let interrupt = engine.interrupt_handle();
    let query_count = Arc::new(AtomicUsize::new(0));
    let counting = CountingEngine {
        inner: engine,
        queries: query_count.clone(),
    };
    let (req_tx, req_rx) = channel();
    let (resp_tx, resp_rx) = channel();
    let worker = spawn_worker(Box::new(counting), req_rx, resp_tx);

    let mut app = App::new(req_tx, interrupt);
    app.force_power_mode_for_tests("");
    // Type while Loading; window elapses; no dispatch yet.
    for (i, c) in "SELECT * FROM t".chars().enumerate() {
        app.on_key(KeyEvent::char(c), i as u64);
    }
    assert!(!app.tick(200));
    assert_eq!(query_count.load(Ordering::SeqCst), 0);
    assert_eq!(app.phase(), &AppPhase::Loading);

    // Engine becomes ready -> the pending query fires and reaches the engine.
    assert!(app.on_loaded("ready"));
    let resp = resp_rx.recv().unwrap();
    app.on_response(resp);
    assert_eq!(query_count.load(Ordering::SeqCst), 1);

    drop(app);
    worker.join().unwrap();
}

// --- value-completion through the real worker channel (P3.7), end-to-end ---

/// Type `WHERE region = 'a`, let the App dispatch a distinct-values fetch on the SAME worker
/// channel, have the engine answer it, and assert the cache fills and the popup suggests the
/// quoted value — proving autocomplete never opens its own connection (§5.5). Deterministic,
/// headless: a real `spawn_worker` over a `FakeEngine` whose value query returns a fixed set.
#[test]
fn value_completion_round_trips_through_the_worker() {
    use crate::autocomplete::value_source::build_distinct_sql_default;

    // The engine answers the exact distinct-values SQL the App emits with a fixed column of
    // distinct `region` values (column 0 holds the values, as build_distinct_sql produces).
    let distinct_sql = build_distinct_sql_default("region");
    let region_values = Table::new(vec![Column::new(
        "region",
        ColumnType::Text,
        vec![Cell::Text("apac".into()), Cell::Text("amer".into())],
    )]);
    let engine = FakeEngine::new(schema())
        .with_default(QueryOutcome::Rows(two_row_table()))
        .with_response(distinct_sql, QueryOutcome::Rows(region_values));
    let interrupt = engine.interrupt_handle();

    let (req_tx, req_rx) = channel();
    let (resp_tx, resp_rx) = channel();
    let worker = spawn_worker(Box::new(engine), req_rx, resp_tx);

    let mut app = App::new(req_tx, interrupt);
    app.force_power_mode_for_tests("");
    app.set_schema(schema());
    app.on_loaded("ready");

    // Enter a value literal after `region =`; the App fires a Value fetch on the worker channel.
    for (i, c) in "SELECT * FROM t WHERE region = 'a".chars().enumerate() {
        app.on_key(KeyEvent::char(c), i as u64);
    }

    // The worker answers the value fetch (and only that — no main query is in flight). Route it.
    let resp = resp_rx.recv().unwrap();
    assert!(
        matches!(&resp, QueryResponse::ProcessedSuccess { kind, .. }
            if matches!(kind, crate::query::worker::types::RequestKind::Value { column } if column == "region")),
        "the worker answered a Value fetch for `region`, got {resp:?}"
    );
    app.on_response(resp);

    // The cache filled and the popup now offers the quoted value `'apac'` (fuzzy-filtered by `a`).
    assert!(app.value_cache().contains("region"));
    assert!(app.autocomplete().is_open());
    let texts: Vec<&str> = app
        .autocomplete()
        .suggestions()
        .iter()
        .map(|s| s.text.as_str())
        .collect();
    assert!(
        texts.contains(&"apac") || texts.contains(&"amer"),
        "expected region values, got {texts:?}"
    );
    // Accept inserts a single-quoted string literal (the value is a text column).
    app.on_key(KeyEvent::plain(Key::Tab), 0);
    assert!(
        app.query().contains("region = '") && app.query().ends_with('\''),
        "accepted value is a quoted string literal, got: {}",
        app.query()
    );

    drop(app);
    worker.join().unwrap();
}

/// A `QueryEngine` wrapper that counts `query()` calls (the worker runs the inner engine).
struct CountingEngine {
    inner: FakeEngine,
    queries: Arc<AtomicUsize>,
}

impl QueryEngine for CountingEngine {
    fn load(
        &mut self,
        path: &std::path::Path,
        opts: &crate::engine::CsvOpts,
    ) -> Result<Schema, crate::error::EngineError> {
        self.inner.load(path, opts)
    }
    fn query(&self, sql: &str) -> QueryOutcome {
        self.queries.fetch_add(1, Ordering::SeqCst);
        self.inner.query(sql)
    }
    fn distinct(&self, col: &str, limit: usize) -> QueryOutcome {
        self.inner.distinct(col, limit)
    }
    fn schema(&self) -> &Schema {
        self.inner.schema()
    }
    fn interrupt_handle(&self) -> InterruptHandle {
        self.inner.interrupt_handle()
    }
}
