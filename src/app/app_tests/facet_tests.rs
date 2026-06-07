//! `App`-shell tests for instant facets (P4.6, §6.5): the `f` chord in the results pane dispatches
//! a facet fetch on its own lane, the response fills the popup (never the grid), stale/other-column
//! responses are ignored, and the popup's key routing (Esc/Ctrl-C/other-key). Split out of
//! `app_tests.rs` to keep each test file under the 1000-line limit; the shared App helpers live in
//! the parent (`super`).

use std::sync::mpsc::Receiver;

use crate::app::{App, Focus, Key, KeyEvent, KeyMods};
use crate::engine::types::{Cell, Column, Table};
use crate::query::worker::types::{ProcessedResult, QueryRequest, QueryResponse, RequestKind};
use crate::schema::ColumnType;

use super::{loaded_app, two_row_result, type_str};

use crate::query::worker::types::RequestKind as RK;

/// Put a result on screen and move focus to the results pane (where the `f` chord lives). Uses
/// `two_row_result` (columns `id`, `region`); `id` (column 0) resolves against `test_schema`.
fn app_with_result_in_results_focus() -> (App, Receiver<QueryRequest>) {
    let (mut app, rx) = loaded_app();
    type_str(&mut app, "SELECT * FROM t", 0);
    app.tick(150);
    let id = app.latest_request_id();
    app.on_response(QueryResponse::ProcessedSuccess {
        result: two_row_result(),
        request_id: id,
        kind: RequestKind::Main,
    });
    // Dismiss any autocomplete popup left open by the typed query (it would consume Down to move
    // the selection rather than handing focus off).
    if app.autocomplete().is_open() {
        app.on_key(KeyEvent::plain(Key::Esc), 200);
    }
    // Move focus from the query bar to the results pane (Down hands off).
    app.on_key(KeyEvent::plain(Key::Down), 200);
    assert_eq!(app.focus(), Focus::Results);
    (app, rx)
}

/// The facet-fetch request the App dispatched for `column`, if any (drains the channel).
fn facet_request(rx: &Receiver<QueryRequest>) -> Option<QueryRequest> {
    let mut found = None;
    while let Ok(r) = rx.try_recv() {
        if matches!(&r.kind, RK::Facet { column } if column == "id") {
            found = Some(r);
        }
    }
    found
}

/// A facet summary response for `column` (the numeric MIN/MAX/distinct/nulls shape).
fn facet_summary_response(column: &str, request_id: u64) -> QueryResponse {
    let table = Table::new(vec![
        Column::new("mn", ColumnType::Int, vec![Cell::Int(1)]),
        Column::new("mx", ColumnType::Int, vec![Cell::Int(2)]),
        Column::new("distinct_count", ColumnType::Int, vec![Cell::Int(2)]),
        Column::new("null_count", ColumnType::Int, vec![Cell::Int(0)]),
    ]);
    let schema = table.schema();
    QueryResponse::ProcessedSuccess {
        result: ProcessedResult::new(table, schema, 0),
        request_id,
        kind: RequestKind::Facet {
            column: column.into(),
        },
    }
}

#[test]
fn f_in_results_dispatches_a_facet_fetch_and_opens_pending_popup() {
    let (mut app, rx) = app_with_result_in_results_focus();
    app.on_key(KeyEvent::char('f'), 300);
    // A facet fetch for the focused column (`id`, the leftmost visible) was dispatched, tagged Facet.
    let req = facet_request(&rx).expect("a Facet fetch for `id`");
    assert!(req.query.contains(r#"min("id")"#), "got: {}", req.query);
    // The popup opened pending (no result until the worker responds).
    assert!(app.is_facet_open());
    assert_eq!(app.facet().unwrap().column(), "id");
    assert!(!app.facet().unwrap().is_ready());
}

#[test]
fn facet_response_fills_the_popup_not_the_grid() {
    let (mut app, rx) = app_with_result_in_results_focus();
    app.on_key(KeyEvent::char('f'), 300);
    let id = facet_request(&rx).expect("facet fetch").request_id;
    let grid_before = app.result().unwrap().rows.row_count();

    // The worker returns the facet stats; routing fills the popup, NOT the grid.
    let changed = app.on_response(facet_summary_response("id", id));
    assert!(!changed, "a facet fetch must not change the visible grid");
    assert_eq!(
        app.result().unwrap().rows.row_count(),
        grid_before,
        "the grid is unchanged by a facet"
    );
    // The popup is now ready with the parsed min/max/distinct/null.
    let facet = app.facet().unwrap();
    assert!(facet.is_ready());
    let result = facet.result().unwrap();
    assert_eq!(result.distinct(), 2);
    assert_eq!(result.nulls(), 0);
    match result {
        crate::facets::FacetResult::Summary { min, max, .. } => {
            assert_eq!(min.as_deref(), Some("1"));
            assert_eq!(max.as_deref(), Some("2"));
        }
        _ => panic!("int column => summary facet"),
    }
}

#[test]
fn facet_for_a_different_column_is_ignored() {
    // A stale facet response (for a column the popup is no longer showing) must not overwrite it.
    let (mut app, rx) = app_with_result_in_results_focus();
    app.on_key(KeyEvent::char('f'), 300);
    let _ = facet_request(&rx);
    // A response for `region` (not the open `id` facet) is ignored.
    let resp = facet_summary_response("region", 1);
    app.on_response(resp);
    assert!(
        !app.facet().unwrap().is_ready(),
        "a facet for a different column does not fill the `id` popup"
    );
}

#[test]
fn esc_closes_the_facet_popup_without_quitting() {
    let (mut app, rx) = app_with_result_in_results_focus();
    app.on_key(KeyEvent::char('f'), 300);
    let id = facet_request(&rx).expect("facet fetch").request_id;
    app.on_response(facet_summary_response("id", id));
    assert!(app.is_facet_open());
    let quit = app.on_key(KeyEvent::plain(Key::Esc), 400);
    assert!(!quit, "Esc closes the facet, does not quit");
    assert!(!app.is_facet_open());
}

#[test]
fn ctrl_c_quits_even_with_facet_open() {
    let (mut app, rx) = app_with_result_in_results_focus();
    app.on_key(KeyEvent::char('f'), 300);
    let _ = facet_request(&rx);
    let quit = app.on_key(KeyEvent::new(Key::Char('c'), KeyMods::CTRL), 400);
    assert!(quit, "Ctrl-C quits from the facet popup too");
}

#[test]
fn other_key_dismisses_the_facet_and_resumes_normal_routing() {
    // Any non-Esc key closes the facet and falls through to normal routing (e.g. Down scrolls).
    let (mut app, rx) = app_with_result_in_results_focus();
    app.on_key(KeyEvent::char('f'), 300);
    let id = facet_request(&rx).expect("facet fetch").request_id;
    app.on_response(facet_summary_response("id", id));
    assert!(app.is_facet_open());
    // Down both closes the facet and is then handled by the results pane.
    app.on_key(KeyEvent::plain(Key::Down), 400);
    assert!(!app.is_facet_open(), "a non-Esc key dismisses the facet");
}

#[test]
fn f_is_a_noop_in_the_query_bar() {
    // The `f` chord only opens a facet from the results pane; in the query bar it types `f`.
    let (mut app, _rx) = loaded_app();
    type_str(&mut app, "f", 0);
    assert_eq!(app.query(), "f", "f in the bar is a literal character");
    assert!(!app.is_facet_open());
}

#[test]
fn f_is_a_noop_without_a_result() {
    // No result on screen => no focused column => `f` does nothing.
    let (mut app, _rx) = loaded_app();
    app.on_key(KeyEvent::plain(Key::Down), 0); // try to focus results (no result yet)
    app.on_key(KeyEvent::char('f'), 0);
    assert!(!app.is_facet_open());
}

#[test]
fn facet_fetch_does_not_disturb_the_main_in_flight_gate() {
    // A facet rides its own lane: dispatching it must not bump the main request_id or clear/touch
    // the main in-flight bookkeeping.
    let (mut app, rx) = app_with_result_in_results_focus();
    let main_id_before = app.latest_request_id();
    assert!(!app.is_query_in_flight(), "main result already landed");
    app.on_key(KeyEvent::char('f'), 300);
    let _ = facet_request(&rx);
    assert_eq!(
        app.latest_request_id(),
        main_id_before,
        "a facet fetch must not bump the main request_id"
    );
}
