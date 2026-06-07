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
