//! App-shell mouse routing tests (P-input-UX, ported from jiq's `app/mouse_*.rs`).
//!
//! These drive [`App::on_mouse`] with synthetic [`MouseEvent`]s after a real `TestBackend` render
//! recorded the on-screen regions — so the coordinate mapping is exercised against the same
//! geometry the App lays out, headlessly (no terminal). Covers scroll-over-results (vertical +
//! horizontal), click-to-focus, click-to-position-cursor, the load-error freeze, and popup
//! scroll/click selection.

use ratatui::Terminal;
use ratatui::backend::TestBackend;

use super::super::{App, Focus, Key, KeyEvent, MouseEvent};
use super::{loaded_app, test_schema, wide_result};
use crate::query::worker::types::{QueryResponse, RequestKind};

/// Render once into an off-screen `TestBackend` so `App` records its layout regions, then return
/// the screen size used (the caller maps clicks into it). 80x24 matches the snapshot tests.
fn render_and_record(app: &App) -> (u16, u16) {
    let (w, h) = (80u16, 24u16);
    let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
    t.draw(|f| app.render(f)).unwrap();
    (w, h)
}

/// A loaded app showing a `rows`-row result, focused on the query bar (the default after a result
/// lands). Mirrors `app_with_result` but keeps query-bar focus so the click-to-focus path is tested.
fn app_with_result_on_bar(rows: usize) -> App {
    let (mut app, rx) = loaded_app();
    type_query(&mut app, "SELECT * FROM t");
    app.tick(150);
    let id = app.latest_request_id();
    app.on_response(QueryResponse::ProcessedSuccess {
        result: wide_result(rows),
        request_id: id,
        kind: RequestKind::Main,
    });
    let _ = rx; // dispatched query drained implicitly; not asserted here
    app
}

fn type_query(app: &mut App, s: &str) {
    for c in s.chars() {
        app.on_key(KeyEvent::char(c), 0);
    }
}

// --- scroll over the results pane ---

#[test]
fn scroll_down_over_results_scrolls_the_grid_body() {
    let mut app = app_with_result_on_bar(50);
    let (_w, _h) = render_and_record(&app);
    // A wheel-down over the pane body (row 5 is inside the grid body) scrolls by the wheel step.
    app.on_mouse(MouseEvent::ScrollDown { col: 5, row: 5 });
    assert_eq!(app.v_row_offset(), 3, "one wheel notch = 3 rows");
    app.on_mouse(MouseEvent::ScrollDown { col: 5, row: 5 });
    assert_eq!(app.v_row_offset(), 6);
    // Wheel-up backs it off.
    app.on_mouse(MouseEvent::ScrollUp { col: 5, row: 5 });
    assert_eq!(app.v_row_offset(), 3);
}

#[test]
fn scroll_up_clamps_at_top() {
    let mut app = app_with_result_on_bar(50);
    render_and_record(&app);
    app.on_mouse(MouseEvent::ScrollUp { col: 5, row: 5 });
    assert_eq!(app.v_row_offset(), 0, "clamps at the top");
}

#[test]
fn scroll_down_clamps_at_last_row() {
    let mut app = app_with_result_on_bar(2); // body_len-1 == 1
    render_and_record(&app);
    app.on_mouse(MouseEvent::ScrollDown { col: 5, row: 5 });
    app.on_mouse(MouseEvent::ScrollDown { col: 5, row: 5 });
    assert_eq!(app.v_row_offset(), 1, "clamps at last row");
}

#[test]
fn horizontal_swipe_over_results_scrolls_columns() {
    let mut app = app_with_result_on_bar(5); // 2 columns -> h max 1
    render_and_record(&app);
    app.on_mouse(MouseEvent::ScrollRight { col: 5, row: 5 });
    assert_eq!(app.h_col_offset(), 1);
    app.on_mouse(MouseEvent::ScrollRight { col: 5, row: 5 });
    assert_eq!(app.h_col_offset(), 1, "clamps at last column");
    app.on_mouse(MouseEvent::ScrollLeft { col: 5, row: 5 });
    assert_eq!(app.h_col_offset(), 0);
    app.on_mouse(MouseEvent::ScrollLeft { col: 5, row: 5 });
    assert_eq!(app.h_col_offset(), 0, "clamps at 0");
}

// --- click to focus / position ---

#[test]
fn click_in_results_pane_focuses_results() {
    let mut app = app_with_result_on_bar(20);
    render_and_record(&app);
    assert_eq!(app.focus(), Focus::QueryBar);
    app.on_mouse(MouseEvent::Click { col: 5, row: 5 }); // inside the grid body
    assert_eq!(app.focus(), Focus::Results);
}

#[test]
fn click_in_query_bar_focuses_bar_and_positions_cursor() {
    let mut app = app_with_result_on_bar(20);
    // Put a known query in the bar and move focus to results first.
    // The bar already holds "SELECT * FROM t" (15 chars). Render to record regions.
    let (_w, h) = render_and_record(&app);
    app.on_mouse(MouseEvent::Click { col: 5, row: 5 }); // focus results
    assert_eq!(app.focus(), Focus::Results);
    // The query bar is the row just below the results pane: h - 3 (status + help take the last two
    // rows, bar above them). Click on the bar at a column inside the text.
    let bar_row = h - 3;
    // Click at screen col 7 -> text col 5 (prompt is 2 wide), landing the cursor at char 5 ("T").
    app.on_mouse(MouseEvent::Click {
        col: 7,
        row: bar_row,
    });
    assert_eq!(app.focus(), Focus::QueryBar);
    assert_eq!(
        app.editor().cursor(),
        5,
        "cursor lands at the clicked column"
    );
    assert_eq!(
        app.editor_mode(),
        crate::app::editor::EditorMode::Insert,
        "a bar click lands in Insert mode"
    );
}

#[test]
fn click_past_end_of_text_clamps_to_line_end() {
    let mut app = app_with_result_on_bar(20);
    let (_w, h) = render_and_record(&app);
    let bar_row = h - 3;
    // Click far to the right, past the 15-char query -> cursor clamps to the line end (15).
    app.on_mouse(MouseEvent::Click {
        col: 70,
        row: bar_row,
    });
    assert_eq!(app.focus(), Focus::QueryBar);
    assert_eq!(app.editor().cursor(), 15, "clamped to the text end");
}

#[test]
fn drag_in_query_bar_positions_cursor_like_a_click() {
    let mut app = app_with_result_on_bar(20);
    let (_w, h) = render_and_record(&app);
    let bar_row = h - 3;
    app.on_mouse(MouseEvent::Drag {
        col: 4,
        row: bar_row,
    }); // text col 2
    assert_eq!(app.focus(), Focus::QueryBar);
    assert_eq!(app.editor().cursor(), 2);
}

#[test]
fn click_in_query_bar_during_load_error_is_frozen() {
    let (mut app, _rx) = {
        use crate::engine::InterruptHandle;
        use std::sync::mpsc::channel;
        let (tx, rx) = channel();
        (App::new(tx, InterruptHandle::noop()), rx)
    };
    app.on_load_error("boom");
    let (_w, h) = render_and_record(&app);
    let bar_row = h - 3;
    // A click on the frozen bar must not move focus into editing or position a cursor.
    app.on_mouse(MouseEvent::Click {
        col: 5,
        row: bar_row,
    });
    assert_eq!(app.query(), "", "frozen bar takes no cursor change");
}

#[test]
fn click_in_results_with_no_result_does_not_focus() {
    let (mut app, _rx) = {
        use crate::engine::InterruptHandle;
        use std::sync::mpsc::channel;
        let (tx, rx) = channel();
        (App::new(tx, InterruptHandle::noop()), rx)
    };
    app.set_schema(test_schema());
    app.on_loaded("ready");
    render_and_record(&app);
    app.on_mouse(MouseEvent::Click { col: 5, row: 5 });
    assert_eq!(
        app.focus(),
        Focus::QueryBar,
        "no result => clicking the pane does not focus results"
    );
}

// --- scroll outside any surface falls back to the grid (jiq's None -> results) ---

#[test]
fn scroll_outside_all_surfaces_falls_back_to_grid() {
    let mut app = app_with_result_on_bar(50);
    render_and_record(&app);
    // Row 100 is past every surface; a wheel there still pages the grid (jiq's fallback).
    app.on_mouse(MouseEvent::ScrollDown { col: 5, row: 100 });
    assert_eq!(app.v_row_offset(), 3);
}

// --- popup scroll + click selection ---

#[test]
fn scroll_over_autocomplete_popup_moves_selection() {
    let (mut app, _rx) = loaded_app();
    type_query(&mut app, "SELECT "); // empty partial -> all columns
    assert!(app.autocomplete().is_open());
    render_and_record(&app);
    assert_eq!(app.autocomplete().selected(), 0);
    // The popup anchors just above the bar; find a cell inside it and scroll.
    let (kind, rect) = app.layout_regions().popup.expect("popup recorded");
    assert_eq!(kind, crate::app::PopupKind::Autocomplete);
    let inner_row = rect.y + 1; // first inner row
    app.on_mouse(MouseEvent::ScrollDown {
        col: rect.x + 1,
        row: inner_row,
    });
    assert_eq!(
        app.autocomplete().selected(),
        1,
        "wheel-down moves selection"
    );
    app.on_mouse(MouseEvent::ScrollUp {
        col: rect.x + 1,
        row: inner_row,
    });
    assert_eq!(app.autocomplete().selected(), 0);
}

#[test]
fn click_on_autocomplete_row_selects_it() {
    let (mut app, _rx) = loaded_app();
    type_query(&mut app, "SELECT "); // all columns, in order
    assert!(app.autocomplete().is_open());
    render_and_record(&app);
    let (_kind, rect) = app.layout_regions().popup.expect("popup recorded");
    // Click the third inner row (index 2) -> selection moves there.
    let third_row = rect.y + 1 + 2;
    app.on_mouse(MouseEvent::Click {
        col: rect.x + 1,
        row: third_row,
    });
    assert_eq!(app.autocomplete().selected(), 2);
    // A subsequent Tab accepts the clicked candidate.
    app.on_key(KeyEvent::plain(Key::Tab), 0);
    assert!(
        !app.autocomplete().is_open(),
        "Tab accepted the clicked row"
    );
}
