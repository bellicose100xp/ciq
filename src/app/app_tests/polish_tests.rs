//! App-level wiring tests for the P5.3 polish: empty-state selection, the large-result truncation
//! banner, and schema-aware error enhancement (did-you-mean). These drive the headless App surface
//! (`on_loaded` / `on_key` / `tick` / `on_response`) — no terminal, no wall clock.

use std::sync::mpsc::{Receiver, channel};

use crate::app::{App, VIEWPORT_ROW_LIMIT};
use crate::engine::InterruptHandle;
use crate::engine::types::{Cell, Column, Table};
use crate::query::worker::types::{ProcessedResult, QueryResponse, RequestKind};
use crate::schema::{ColumnMeta, ColumnType, Schema};

fn app() -> (App, Receiver<crate::query::worker::types::QueryRequest>) {
    let (tx, rx) = channel();
    (App::new(tx, InterruptHandle::noop()), rx)
}

fn type_str(app: &mut App, s: &str, now_ms: u64) {
    for c in s.chars() {
        app.on_key(crate::app::KeyEvent::char(c), now_ms);
    }
}

/// A one-column `id` result with `n` rows.
fn id_result(n: usize) -> ProcessedResult {
    let cells: Vec<Cell> = (0..n as i64).map(Cell::Int).collect();
    let table = Table::new(vec![Column::new("id", ColumnType::Int, cells)]);
    let schema = table.schema();
    ProcessedResult::new(table, schema, 0)
}

/// Run `query` through the App and land `result` as its accepted response. Returns the App ready
/// to be inspected.
fn run_query(query: &str, result: ProcessedResult) -> App {
    let (mut app, _rx) = app();
    app.on_loaded("ready");
    type_str(&mut app, query, 0);
    app.tick(150);
    let id = app.latest_request_id();
    app.on_response(QueryResponse::ProcessedSuccess {
        result,
        request_id: id,
        kind: RequestKind::Main,
    });
    app
}

// --- empty-state selection ---

#[test]
fn empty_state_is_no_query_hint_before_first_query() {
    let (mut app, _rx) = app();
    app.on_loaded("ready");
    assert_eq!(
        app.empty_state(),
        Some("type a SQL query above (e.g. SELECT * FROM t)")
    );
}

#[test]
fn empty_state_is_loading_while_parsing() {
    let (app, _rx) = app();
    assert_eq!(app.empty_state(), Some("loading CSV…"));
}

#[test]
fn empty_state_is_no_rows_match_on_zero_row_result() {
    let app = run_query("SELECT * FROM t WHERE id < 0", id_result(0));
    assert_eq!(app.empty_state(), Some("no rows match"));
}

#[test]
fn empty_state_is_none_when_grid_is_populated() {
    let app = run_query("SELECT * FROM t", id_result(3));
    assert_eq!(app.empty_state(), None);
}

#[test]
fn clearing_the_bar_returns_to_the_initial_hint_not_no_rows() {
    // Run a zero-row query (-> "no rows match"), then clear the bar -> back to the initial hint.
    let (mut app, _rx) = app();
    app.on_loaded("ready");
    type_str(&mut app, "SELECT * FROM t WHERE id < 0", 0);
    app.tick(150);
    let id = app.latest_request_id();
    app.on_response(QueryResponse::ProcessedSuccess {
        result: id_result(0),
        request_id: id,
        kind: RequestKind::Main,
    });
    assert_eq!(app.empty_state(), Some("no rows match"));

    // Select-all-and-delete by emptying the editor, then fire the (empty) debounce.
    for _ in 0.."SELECT * FROM t WHERE id < 0".len() {
        app.on_key(crate::app::KeyEvent::plain(crate::app::Key::Backspace), 200);
    }
    app.tick(400);
    assert_eq!(
        app.empty_state(),
        Some("type a SQL query above (e.g. SELECT * FROM t)")
    );
}

// --- [general] row_limit wiring (the configured cap drives the dispatched LIMIT + the banner) ---

#[test]
fn configured_row_limit_changes_the_dispatched_limit() {
    let (mut app, rx) = app();
    app.configure_general(50);
    assert_eq!(app.viewport_row_limit(), 50);
    app.on_loaded("ready");
    type_str(&mut app, "SELECT * FROM t", 0);
    app.tick(150);
    let sent: Vec<String> = {
        let mut v = Vec::new();
        while let Ok(r) = rx.try_recv() {
            v.push(r.query);
        }
        v
    };
    assert_eq!(sent.len(), 1);
    assert!(
        sent[0].contains("LIMIT 50"),
        "the configured row_limit must drive the viewport LIMIT, got: {}",
        sent[0]
    );
}

#[test]
fn configured_row_limit_drives_the_truncation_banner_cap() {
    // A configured cap of 50: a 50-row bare-SELECT result hits the cap and shows the banner.
    let (mut app, _rx) = app();
    app.configure_general(50);
    app.on_loaded("ready");
    type_str(&mut app, "SELECT * FROM t", 0);
    app.tick(150);
    let id = app.latest_request_id();
    app.on_response(QueryResponse::ProcessedSuccess {
        result: id_result(50),
        request_id: id,
        kind: RequestKind::Main,
    });
    assert_eq!(
        app.truncation_banner(),
        Some("showing first 50 rows (use --output to export all)".to_string())
    );
}

#[test]
fn configure_general_clamps_zero_to_one() {
    let (mut app, _rx) = app();
    app.configure_general(0);
    assert_eq!(app.viewport_row_limit(), 1);
}

// --- truncation banner ---

#[test]
fn truncation_banner_shows_when_bare_select_hits_the_cap() {
    let app = run_query("SELECT * FROM t", id_result(VIEWPORT_ROW_LIMIT));
    assert_eq!(
        app.truncation_banner(),
        Some(format!(
            "showing first {VIEWPORT_ROW_LIMIT} rows (use --output to export all)"
        ))
    );
}

#[test]
fn no_truncation_banner_under_the_cap() {
    let app = run_query("SELECT * FROM t", id_result(VIEWPORT_ROW_LIMIT - 1));
    assert_eq!(app.truncation_banner(), None);
}

#[test]
fn no_truncation_banner_when_user_supplied_their_own_limit() {
    // The user wrote LIMIT 1000; a full 1000-row result is their intent, not a ciq cap.
    let q = format!("SELECT * FROM t LIMIT {VIEWPORT_ROW_LIMIT}");
    let app = run_query(&q, id_result(VIEWPORT_ROW_LIMIT));
    assert_eq!(app.truncation_banner(), None);
}

#[test]
fn no_truncation_banner_without_a_result() {
    let (mut app, _rx) = app();
    app.on_loaded("ready");
    assert_eq!(app.truncation_banner(), None);
}

// --- schema-aware error enhancement (did-you-mean) ---

#[test]
fn unknown_column_error_gets_did_you_mean_against_schema() {
    let (mut app, _rx) = app();
    app.set_schema(Schema::new(vec![
        ColumnMeta::new("region", ColumnType::Text),
        ColumnMeta::new("amount", ColumnType::Float),
    ]));
    app.on_loaded("ready");
    type_str(&mut app, "SELECT reigon FROM t", 0);
    app.tick(150);
    let id = app.latest_request_id();
    app.on_response(QueryResponse::Error {
        message: "Binder Error: Referenced column \"reigon\" not found in FROM clause!".into(),
        request_id: id,
        kind: RequestKind::Main,
    });
    assert_eq!(
        app.status(),
        "unknown column: \"reigon\" — did you mean \"region\"?"
    );
}

#[test]
fn engine_error_after_a_success_clears_the_stale_grid_and_banner() {
    // Run a bare SELECT that fills (and caps) the grid, then a query that the engine rejects (an
    // unknown column). The error must drop the last-good grid + its truncation banner so they aren't
    // left painted under the error message — matching set_query_error / empty_state's contract.
    let (mut app, _rx) = app();
    app.set_schema(Schema::new(vec![ColumnMeta::new("id", ColumnType::Int)]));
    app.on_loaded("ready");

    // (1) a successful bare SELECT that hits the viewport cap -> grid + truncation banner.
    type_str(&mut app, "SELECT * FROM t", 0);
    app.tick(150);
    let id = app.latest_request_id();
    app.on_response(QueryResponse::ProcessedSuccess {
        result: id_result(VIEWPORT_ROW_LIMIT),
        request_id: id,
        kind: RequestKind::Main,
    });
    assert!(app.result().is_some());
    assert!(app.truncation_banner().is_some());

    // (2) edit to an unknown-column query; the engine returns Error.
    type_str(&mut app, "x", 200); // any edit that re-dispatches
    app.tick(400);
    let id2 = app.latest_request_id();
    app.on_response(QueryResponse::Error {
        message: "Binder Error: Referenced column \"foo\" not found in FROM clause!".into(),
        request_id: id2,
        kind: RequestKind::Main,
    });

    // The error is in the status line; the stale grid AND its banner are gone, and the pane falls
    // back to the neutral empty-state (no grid).
    assert!(app.status().contains("unknown column"));
    assert!(app.result().is_none(), "error must clear the stale grid");
    assert_eq!(
        app.truncation_banner(),
        None,
        "stale banner must be cleared"
    );
    assert!(app.empty_state().is_some(), "the pane shows an empty-state");
}

#[test]
fn preprocess_reject_after_a_success_clears_the_stale_grid() {
    // The other error path: a preprocess rejection (DML) after a successful grid must also drop the
    // stale result so no grid lingers under the "read-only" status.
    let app0 = run_query("SELECT * FROM t", id_result(3));
    let mut app = app0;
    type_str(&mut app, ";DROP TABLE t", 200); // makes the bar multi-statement
    app.tick(400);
    assert!(
        app.status().contains("statement") || app.status().contains("read-only"),
        "status: {}",
        app.status()
    );
    assert!(
        app.result().is_none(),
        "a preprocess reject must clear the stale grid"
    );
}

#[test]
fn error_without_schema_falls_back_to_plain_enhance() {
    let (mut app, _rx) = app();
    app.on_loaded("ready"); // no schema set
    type_str(&mut app, "SELECT reigon FROM t", 0);
    app.tick(150);
    let id = app.latest_request_id();
    app.on_response(QueryResponse::Error {
        message: "Binder Error: Referenced column \"reigon\" not found in FROM clause!".into(),
        request_id: id,
        kind: RequestKind::Main,
    });
    assert_eq!(app.status(), "unknown column: \"reigon\"");
}
