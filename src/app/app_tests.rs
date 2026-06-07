//! Tests for the `App` shell — event routing, the debounce-fire wiring, response handling
//! (stale-discard), invalid-SQL handling, scroll, and the load state machine (P2.8 + P2.11).
//!
//! These drive the App through its headless surface (`on_key` / `tick` / `on_response` /
//! `on_loaded` / `on_load_error`) with synthetic `KeyEvent`s and explicit `u64` time — no
//! terminal, no wall clock. Worker-coupled behaviors (debounce coalescing against a counting
//! engine, out-of-band cancel) live in `harness/app_harness_tests.rs` where a real worker is
//! wired.

use std::sync::mpsc::{Receiver, channel};

use super::{App, AppPhase, Focus, Key, KeyEvent, VIEWPORT_ROW_LIMIT};
use crate::engine::InterruptHandle;
use crate::engine::types::{Cell, Column, Table};
use crate::query::worker::types::{ProcessedResult, QueryRequest, QueryResponse, RequestKind};
use crate::schema::ColumnType;

/// Build an App over a request channel whose receiver the test keeps (to inspect dispatches).
fn app() -> (App, Receiver<QueryRequest>) {
    let (tx, rx) = channel();
    (App::new(tx, InterruptHandle::noop()), rx)
}

fn type_str(app: &mut App, s: &str, now_ms: u64) {
    for c in s.chars() {
        app.on_key(KeyEvent::char(c), now_ms);
    }
}

fn two_row_result() -> ProcessedResult {
    let table = Table::new(vec![
        Column::new("id", ColumnType::Int, vec![Cell::Int(1), Cell::Int(2)]),
        Column::new(
            "region",
            ColumnType::Text,
            vec![Cell::Text("EU".into()), Cell::Text("NA".into())],
        ),
    ]);
    let schema = table.schema();
    ProcessedResult::new(table, schema, 0)
}

fn drain(rx: &Receiver<QueryRequest>) -> Vec<String> {
    let mut out = Vec::new();
    while let Ok(r) = rx.try_recv() {
        out.push(r.query);
    }
    out
}

// --- initial state ---

#[test]
fn new_app_is_loading_on_query_bar() {
    let (app, _rx) = app();
    assert_eq!(app.phase(), &AppPhase::Loading);
    assert_eq!(app.focus(), Focus::QueryBar);
    assert_eq!(app.query(), "");
}

// --- editor routing ---

#[test]
fn typing_updates_query_buffer() {
    let (mut app, _rx) = app();
    type_str(&mut app, "SELECT 1", 0);
    assert_eq!(app.query(), "SELECT 1");
    assert_eq!(app.editor().cursor(), 8);
}

#[test]
fn backspace_and_cursor_keys_route_to_editor() {
    let (mut app, _rx) = app();
    type_str(&mut app, "abc", 0);
    app.on_key(KeyEvent::plain(Key::Backspace), 0);
    assert_eq!(app.query(), "ab");
    app.on_key(KeyEvent::plain(Key::Home), 0);
    assert_eq!(app.editor().cursor(), 0);
}

#[test]
fn paste_inserts_decoded_payload() {
    let (mut app, _rx) = app();
    app.on_key(KeyEvent::plain(Key::Paste("SELECT * FROM t".into())), 0);
    assert_eq!(app.query(), "SELECT * FROM t");
}

#[test]
fn quit_keys_signal_quit() {
    let (mut a, _rx) = app();
    assert!(a.on_key(KeyEvent::plain(Key::Esc), 0));
    let (mut b, _rx2) = app();
    assert!(b.on_key(KeyEvent::new(Key::Char('c'), super::KeyMods::CTRL), 0));
}

// --- debounce-fire wiring (P2.8) ---

#[test]
fn typing_then_tick_past_window_dispatches_wrapped_sql() {
    let (mut app, rx) = app();
    app.on_loaded("ready"); // engine ready
    type_str(&mut app, "SELECT * FROM t", 0);
    // Before the window elapses, nothing fires.
    assert!(!app.tick(100));
    assert!(drain(&rx).is_empty());
    // After 150ms quiet, exactly one query is dispatched, LIMIT-wrapped.
    assert!(app.tick(150));
    let sent = drain(&rx);
    assert_eq!(sent.len(), 1);
    assert!(
        sent[0].contains(&format!("LIMIT {VIEWPORT_ROW_LIMIT}")),
        "expected viewport LIMIT wrap, got: {}",
        sent[0]
    );
    assert_eq!(app.phase(), &AppPhase::Querying);
}

#[test]
fn cursor_only_keys_do_not_reschedule() {
    let (mut app, rx) = app();
    app.on_loaded("ready");
    type_str(&mut app, "SELECT 1", 0);
    app.tick(150); // fires once
    assert_eq!(drain(&rx).len(), 1);
    // A pure cursor move at a later time must NOT schedule a new query.
    app.on_key(KeyEvent::plain(Key::Left), 200);
    assert!(!app.tick(400));
    assert!(drain(&rx).is_empty());
}

#[test]
fn empty_query_clears_result_and_does_not_dispatch() {
    let (mut app, rx) = app();
    app.on_loaded("ready");
    // Type then delete back to empty.
    type_str(&mut app, "x", 0);
    app.on_key(KeyEvent::plain(Key::Backspace), 0);
    assert!(!app.tick(150));
    assert!(drain(&rx).is_empty());
    assert_eq!(app.phase(), &AppPhase::Ready);
    assert!(app.result().is_none());
}

// --- invalid SQL -> status error, no dispatch, no crash ---

#[test]
fn invalid_grammar_sets_status_error_and_does_not_dispatch() {
    let (mut app, rx) = app();
    app.on_loaded("ready");
    type_str(&mut app, "DROP TABLE t", 0);
    assert!(!app.tick(150));
    assert!(drain(&rx).is_empty(), "DML must not reach the engine");
    assert_eq!(app.status(), "read-only SELECT queries only");
    assert_eq!(app.phase(), &AppPhase::Ready);
}

#[test]
fn multi_statement_rejected() {
    let (mut app, rx) = app();
    app.on_loaded("ready");
    type_str(&mut app, "SELECT 1; SELECT 2", 0);
    assert!(!app.tick(150));
    assert!(drain(&rx).is_empty());
    assert_eq!(app.status(), "single statement only");
}

// --- response handling + stale-discard ---

#[test]
fn processed_success_updates_result_and_status() {
    let (mut app, rx) = app();
    app.on_loaded("ready");
    type_str(&mut app, "SELECT * FROM t", 0);
    app.tick(150);
    let _ = drain(&rx);
    let id = app.latest_request_id();
    assert!(app.on_response(QueryResponse::ProcessedSuccess {
        result: two_row_result(),
        request_id: id,
        kind: RequestKind::Main,
    }));
    assert!(app.result().is_some());
    assert_eq!(app.result().unwrap().rows.row_count(), 2);
    assert_eq!(app.status(), "2 rows");
    assert_eq!(app.phase(), &AppPhase::Ready);
}

#[test]
fn stale_response_is_discarded() {
    let (mut app, rx) = app();
    app.on_loaded("ready");
    // Dispatch twice so latest_id == 2 (both valid SELECTs).
    type_str(&mut app, "SELECT 1", 0);
    app.tick(150);
    type_str(&mut app, " WHERE 1=1", 200);
    app.tick(400);
    let _ = drain(&rx);
    assert_eq!(app.latest_request_id(), 2);
    // A response for the older id=1 must be dropped (no result set, no state change).
    let changed = app.on_response(QueryResponse::ProcessedSuccess {
        result: two_row_result(),
        request_id: 1,
        kind: RequestKind::Main,
    });
    assert!(!changed, "stale id=1 response must be discarded");
    assert!(app.result().is_none());
}

#[test]
fn cancelled_response_shows_nothing() {
    let (mut app, _rx) = app();
    app.on_loaded("ready");
    type_str(&mut app, "SELECT 1", 0);
    app.tick(150);
    let id = app.latest_request_id();
    assert!(!app.on_response(QueryResponse::Cancelled { request_id: id }));
    assert!(app.result().is_none());
}

#[test]
fn engine_error_response_enhances_to_status_no_crash() {
    let (mut app, _rx) = app();
    app.on_loaded("ready");
    type_str(&mut app, "SELECT bogus", 0);
    app.tick(150);
    let id = app.latest_request_id();
    let changed = app.on_response(QueryResponse::Error {
        message: "Binder Error: Referenced column \"bogus\" not found".into(),
        request_id: id,
        kind: RequestKind::Main,
    });
    assert!(changed);
    assert_eq!(app.status(), "unknown column: \"bogus\"");
    assert_eq!(app.phase(), &AppPhase::Ready);
}

#[test]
fn per_request_panic_error_surfaces_under_its_real_id() {
    // A per-request engine panic arrives as Error under the query's real id (never 0) and is
    // applied like any other current-id Error.
    let (mut app, _rx) = app();
    app.on_loaded("ready");
    type_str(&mut app, "SELECT 1", 0);
    app.tick(150);
    let id = app.latest_request_id();
    let changed = app.on_response(QueryResponse::Error {
        message: "query panicked: boom".into(),
        request_id: id,
        kind: RequestKind::Main,
    });
    assert!(changed);
    assert_eq!(app.phase(), &AppPhase::Ready);
    assert!(app.status().contains("boom"));
}

#[test]
fn superseded_panic_error_is_stale_discarded() {
    // A panic Error for an older id is stale-discarded exactly like any other superseded
    // response — there is no immediate-apply special case for panics.
    let (mut app, rx) = app();
    app.on_loaded("ready");
    type_str(&mut app, "SELECT 1", 0);
    app.tick(150);
    type_str(&mut app, " WHERE 1=1", 200);
    app.tick(400);
    let _ = drain(&rx);
    assert_eq!(app.latest_request_id(), 2);
    let changed = app.on_response(QueryResponse::Error {
        message: "query panicked: boom".into(),
        request_id: 1,
        kind: RequestKind::Main,
    });
    assert!(
        !changed,
        "a panic Error for a superseded id must be discarded"
    );
}

// --- scroll (focus handoff + offsets) ---

#[test]
fn down_from_query_bar_focuses_results_and_scrolls() {
    let (mut app, _rx) = app();
    app.on_loaded("ready");
    type_str(&mut app, "SELECT * FROM t", 0);
    app.tick(150);
    let id = app.latest_request_id();
    app.on_response(QueryResponse::ProcessedSuccess {
        result: two_row_result(),
        request_id: id,
        kind: RequestKind::Main,
    });
    // Down moves focus to the results pane.
    app.on_key(KeyEvent::plain(Key::Down), 200);
    assert_eq!(app.focus(), Focus::Results);
    // Down in results scrolls the body (clamped to body_len-1 == 1).
    app.on_key(KeyEvent::plain(Key::Down), 200);
    assert_eq!(app.v_row_offset(), 1);
    app.on_key(KeyEvent::plain(Key::Down), 200);
    assert_eq!(app.v_row_offset(), 1, "scroll clamps at the last row");
}

#[test]
fn up_at_top_of_results_returns_focus_to_query_bar() {
    let (mut app, _rx) = app();
    app.on_loaded("ready");
    type_str(&mut app, "SELECT 1", 0);
    app.tick(150);
    let id = app.latest_request_id();
    app.on_response(QueryResponse::ProcessedSuccess {
        result: two_row_result(),
        request_id: id,
        kind: RequestKind::Main,
    });
    app.on_key(KeyEvent::plain(Key::Down), 0); // focus results
    assert_eq!(app.focus(), Focus::Results);
    app.on_key(KeyEvent::plain(Key::Up), 0); // at top -> back to bar
    assert_eq!(app.focus(), Focus::QueryBar);
}

// --- load state machine (P2.11) ---

#[test]
fn load_completion_transitions_loading_to_ready() {
    let (mut app, _rx) = app();
    assert_eq!(app.phase(), &AppPhase::Loading);
    assert!(!app.on_loaded("loaded: 3 columns"));
    assert_eq!(app.phase(), &AppPhase::Ready);
    assert_eq!(app.status(), "loaded: 3 columns");
}

#[test]
fn query_typed_during_load_fires_on_ready() {
    let (mut app, rx) = app();
    // Editable during load: type a query and let the debounce window elapse while still Loading.
    type_str(&mut app, "SELECT * FROM t", 0);
    assert!(!app.tick(150), "must not dispatch while Loading");
    assert!(drain(&rx).is_empty());
    // Now the engine becomes ready: the pending query fires immediately.
    let fired = app.on_loaded("ready");
    assert!(fired, "the query typed during load must fire on Ready");
    let sent = drain(&rx);
    assert_eq!(sent.len(), 1);
    assert!(sent[0].contains("SELECT * FROM t"));
    assert_eq!(app.phase(), &AppPhase::Querying);
}

#[test]
fn no_pending_query_does_not_fire_on_ready() {
    let (mut app, rx) = app();
    let fired = app.on_loaded("ready");
    assert!(!fired);
    assert!(drain(&rx).is_empty());
}

#[test]
fn load_error_freezes_bar_and_sets_error_status() {
    let (mut app, _rx) = app();
    app.on_load_error("file not found");
    assert!(matches!(app.phase(), AppPhase::LoadError(_)));
    assert_eq!(app.status(), "load error: file not found");
    // The query bar is frozen: typing has no effect.
    type_str(&mut app, "SELECT 1", 0);
    assert_eq!(app.query(), "");
}

// --- belt-and-suspenders: never dispatch while loading even if ticked ---

#[test]
fn tick_while_loading_never_dispatches() {
    let (mut app, rx) = app();
    type_str(&mut app, "SELECT 1", 0);
    for t in (150..1000).step_by(50) {
        assert!(!app.tick(t));
    }
    assert!(drain(&rx).is_empty());
}

// --- remaining editor keys routed through the query bar ---

#[test]
fn delete_right_end_editor_keys_route() {
    let (mut app, _rx) = app();
    type_str(&mut app, "abc", 0);
    app.on_key(KeyEvent::plain(Key::Home), 0);
    app.on_key(KeyEvent::plain(Key::Delete), 0); // removes 'a'
    assert_eq!(app.query(), "bc");
    app.on_key(KeyEvent::plain(Key::Right), 0); // cursor 0 -> 1
    app.on_key(KeyEvent::plain(Key::End), 0); // cursor -> end
    assert_eq!(app.editor().cursor(), 2);
}

// --- results-pane navigation (all scroll branches) ---

/// A result with `n` rows and two columns, for exercising scroll bounds.
fn wide_result(rows: usize) -> ProcessedResult {
    let ids: Vec<Cell> = (0..rows as i64).map(Cell::Int).collect();
    let names: Vec<Cell> = (0..rows).map(|i| Cell::Text(format!("r{i}"))).collect();
    let table = Table::new(vec![
        Column::new("id", ColumnType::Int, ids),
        Column::new("name", ColumnType::Text, names),
    ]);
    let schema = table.schema();
    ProcessedResult::new(table, schema, 0)
}

fn app_with_result(rows: usize) -> (App, std::sync::mpsc::Receiver<QueryRequest>) {
    let (mut app, rx) = app();
    app.on_loaded("ready");
    type_str(&mut app, "SELECT * FROM t", 0);
    app.tick(150);
    let id = app.latest_request_id();
    app.on_response(QueryResponse::ProcessedSuccess {
        result: wide_result(rows),
        request_id: id,
        kind: RequestKind::Main,
    });
    app.on_key(KeyEvent::plain(Key::Down), 200); // focus results
    (app, rx)
}

#[test]
fn page_down_and_page_up_scroll_in_tens_clamped() {
    let (mut app, _rx) = app_with_result(30);
    app.on_key(KeyEvent::plain(Key::PageDown), 0);
    assert_eq!(app.v_row_offset(), 10);
    app.on_key(KeyEvent::plain(Key::PageDown), 0);
    assert_eq!(app.v_row_offset(), 20);
    app.on_key(KeyEvent::plain(Key::PageDown), 0);
    assert_eq!(app.v_row_offset(), 29, "clamps at last row (body_len-1)");
    app.on_key(KeyEvent::plain(Key::PageUp), 0);
    assert_eq!(app.v_row_offset(), 19);
}

#[test]
fn up_decrements_then_returns_focus_at_top() {
    let (mut app, _rx) = app_with_result(30);
    app.on_key(KeyEvent::plain(Key::Down), 0); // offset 1
    app.on_key(KeyEvent::plain(Key::Down), 0); // offset 2
    assert_eq!(app.v_row_offset(), 2);
    app.on_key(KeyEvent::plain(Key::Up), 0); // offset 1 (decrement, still focused)
    assert_eq!(app.v_row_offset(), 1);
    assert_eq!(app.focus(), Focus::Results);
}

#[test]
fn home_in_results_jumps_to_top() {
    let (mut app, _rx) = app_with_result(30);
    app.on_key(KeyEvent::plain(Key::PageDown), 0);
    assert_eq!(app.v_row_offset(), 10);
    app.on_key(KeyEvent::plain(Key::Home), 0);
    assert_eq!(app.v_row_offset(), 0);
}

#[test]
fn left_right_scroll_columns_clamped() {
    let (mut app, _rx) = app_with_result(5); // 2 columns -> h_col_offset max 1
    app.on_key(KeyEvent::plain(Key::Right), 0);
    assert_eq!(app.h_col_offset(), 1);
    app.on_key(KeyEvent::plain(Key::Right), 0);
    assert_eq!(app.h_col_offset(), 1, "clamps at last column (col_count-1)");
    app.on_key(KeyEvent::plain(Key::Left), 0);
    assert_eq!(app.h_col_offset(), 0);
    app.on_key(KeyEvent::plain(Key::Left), 0);
    assert_eq!(app.h_col_offset(), 0, "clamps at 0");
}

// --- set_interrupt swaps the handle (the load-completion install path) ---

#[test]
fn set_interrupt_installs_handle_without_panicking() {
    let (mut app, _rx) = app();
    // The placeholder is a no-op; installing a fresh no-op handle must be a clean swap.
    app.set_interrupt(InterruptHandle::noop());
    // A subsequent dispatch path still works (no in-flight interrupt fires, but the call is live).
    app.on_loaded("ready");
    type_str(&mut app, "SELECT 1", 0);
    assert!(app.tick(150));
}

// --- autocomplete popup wiring (P3.6) ---

use crate::schema::{ColumnMeta, Schema};

/// A fixed test schema with a keyword-colliding column (`order`) and a low-cardinality `status`.
fn test_schema() -> Schema {
    Schema::new(vec![
        ColumnMeta::new("id", ColumnType::Int),
        ColumnMeta::new("status", ColumnType::Text),
        ColumnMeta::new("amount", ColumnType::Float),
        ColumnMeta::new("order", ColumnType::Int),
    ])
}

/// An App loaded with the test schema (popup has its candidate source) and ready for queries.
fn loaded_app() -> (App, Receiver<QueryRequest>) {
    let (mut app, rx) = app();
    app.set_schema(test_schema());
    app.on_loaded("ready");
    (app, rx)
}

/// A value-fetch response for `column` returning a single-column table of the given values, as the
/// `build_distinct_sql` query would (column 0 = the values).
fn value_response(column: &str, values: &[&str], request_id: u64) -> QueryResponse {
    let cells = values.iter().map(|v| Cell::Text((*v).into())).collect();
    let table = Table::new(vec![Column::new(column, ColumnType::Text, cells)]);
    let schema = table.schema();
    QueryResponse::ProcessedSuccess {
        result: ProcessedResult::new(table, schema, 0),
        request_id,
        kind: RequestKind::Value {
            column: column.into(),
        },
    }
}

#[test]
fn popup_stays_closed_without_a_schema() {
    let (mut app, _rx) = app();
    app.on_loaded("ready"); // ready but no schema installed
    type_str(&mut app, "SELECT st", 0);
    assert!(
        !app.autocomplete().is_open(),
        "no schema => no candidates => closed popup"
    );
}

#[test]
fn typing_in_select_list_opens_popup_with_column_candidates() {
    let (mut app, _rx) = loaded_app();
    type_str(&mut app, "SELECT st", 0);
    assert!(app.autocomplete().is_open());
    let texts: Vec<&str> = app
        .autocomplete()
        .suggestions()
        .iter()
        .map(|s| s.text.as_str())
        .collect();
    assert!(
        texts.contains(&"status"),
        "expected `status`, got {texts:?}"
    );
}

#[test]
fn tab_inserts_selected_suggestion() {
    let (mut app, _rx) = loaded_app();
    type_str(&mut app, "SELECT st", 0);
    assert!(app.autocomplete().is_open());
    // `status` is the prefix match and the first candidate; Tab accepts it.
    app.on_key(KeyEvent::plain(Key::Tab), 0);
    assert_eq!(app.query(), "SELECT status");
    assert!(!app.autocomplete().is_open(), "popup closes after accept");
}

#[test]
fn tab_on_keyword_collision_column_inserts_quoted() {
    let (mut app, _rx) = loaded_app();
    type_str(&mut app, "SELECT or", 0);
    assert!(app.autocomplete().is_open());
    // The first prefix match `order` collides with a keyword -> inserted quoted.
    app.on_key(KeyEvent::plain(Key::Tab), 0);
    assert_eq!(app.query(), "SELECT \"order\"");
}

#[test]
fn arrows_move_selection_while_popup_open() {
    let (mut app, _rx) = loaded_app();
    type_str(&mut app, "SELECT ", 0); // empty partial -> all columns, in order
    assert!(app.autocomplete().is_open());
    assert_eq!(app.autocomplete().selected(), 0);
    app.on_key(KeyEvent::plain(Key::Down), 0);
    assert_eq!(app.autocomplete().selected(), 1);
    app.on_key(KeyEvent::plain(Key::Up), 0);
    assert_eq!(app.autocomplete().selected(), 0);
    // Down arrow is consumed by the popup, NOT a focus handoff to the results pane.
    assert_eq!(app.focus(), Focus::QueryBar);
}

#[test]
fn esc_dismisses_popup_without_quitting() {
    let (mut app, _rx) = loaded_app();
    type_str(&mut app, "SELECT st", 0);
    assert!(app.autocomplete().is_open());
    let quit = app.on_key(KeyEvent::plain(Key::Esc), 0);
    assert!(
        !quit,
        "Esc closes the popup, does not quit, while it is open"
    );
    assert!(!app.autocomplete().is_open());
    // A second Esc (popup now closed) quits as usual.
    assert!(app.on_key(KeyEvent::plain(Key::Esc), 0));
}

// --- value-completion through the worker (P3.7) ---

#[test]
fn value_position_dispatches_a_value_fetch_through_the_worker() {
    let (mut app, rx) = loaded_app();
    // Cursor enters a value literal after `status =`.
    type_str(&mut app, "SELECT * FROM t WHERE status = '", 0);
    // The App issued a distinct-values fetch on the same channel, tagged as a Value request.
    let reqs: Vec<QueryRequest> = {
        let mut v = Vec::new();
        while let Ok(r) = rx.try_recv() {
            v.push(r);
        }
        v
    };
    let value_req = reqs
        .iter()
        .find(|r| matches!(&r.kind, RequestKind::Value { column } if column == "status"))
        .expect("a Value fetch for `status` should be dispatched");
    assert!(
        value_req.query.contains("\"status\""),
        "the distinct SQL targets the quoted column, got: {}",
        value_req.query
    );
}

#[test]
fn value_response_fills_cache_and_suggests_quoted_value() {
    let (mut app, rx) = loaded_app();
    type_str(&mut app, "SELECT * FROM t WHERE status = 'a", 0);
    // Drain to find the value-fetch id.
    let mut value_id = None;
    while let Ok(r) = rx.try_recv() {
        if let RequestKind::Value { column } = &r.kind
            && column == "status"
        {
            value_id = Some(r.request_id);
        }
    }
    let value_id = value_id.expect("value fetch dispatched");

    // The worker returns the distinct values; routing fills the cache (not the grid).
    let changed = app.on_response(value_response(
        "status",
        &["active", "archived", "pending"],
        value_id,
    ));
    assert!(!changed, "a value fetch must not change the visible grid");
    assert!(
        app.result().is_none(),
        "value fetch does not become a result"
    );
    assert!(app.value_cache().contains("status"));

    // The popup now offers `active` (fuzzy-filtered by the partial `a`); accepting it inserts the
    // quoted string literal.
    assert!(app.autocomplete().is_open(), "popup re-opens with values");
    let texts: Vec<&str> = app
        .autocomplete()
        .suggestions()
        .iter()
        .map(|s| s.text.as_str())
        .collect();
    assert!(
        texts.contains(&"active"),
        "expected `active`, got {texts:?}"
    );
    app.on_key(KeyEvent::plain(Key::Tab), 0);
    assert_eq!(app.query(), "SELECT * FROM t WHERE status = 'active'");
}

#[test]
fn cached_column_does_not_refetch_values() {
    let (mut app, rx) = loaded_app();
    // First value-position keystroke triggers a fetch.
    type_str(&mut app, "SELECT * FROM t WHERE status = '", 0);
    let id = {
        let mut found = None;
        while let Ok(r) = rx.try_recv() {
            if let RequestKind::Value { column } = &r.kind
                && column == "status"
            {
                found = Some(r.request_id);
            }
        }
        found.expect("first fetch")
    };
    app.on_response(value_response("status", &["active"], id));

    // Typing more inside the same value literal must NOT issue another fetch (cache hit).
    type_str(&mut app, "ac", 0);
    let refetched = {
        let mut any = false;
        while let Ok(r) = rx.try_recv() {
            if matches!(&r.kind, RequestKind::Value { column } if column == "status") {
                any = true;
            }
        }
        any
    };
    assert!(!refetched, "a cached column must not be re-fetched");
}
