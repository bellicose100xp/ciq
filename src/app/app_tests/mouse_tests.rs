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

/// A loaded app whose schema has more columns than the popup's visible window (so a scrolled list
/// is reachable). Distinct, single-letter-prefixed names keep the autocomplete order stable.
fn wide_schema_app() -> (
    App,
    std::sync::mpsc::Receiver<crate::query::worker::types::QueryRequest>,
) {
    use crate::engine::InterruptHandle;
    use crate::schema::{ColumnMeta, ColumnType, Schema};
    use std::sync::mpsc::channel;
    let (tx, rx) = channel();
    let mut app = App::new(tx, InterruptHandle::noop());
    let cols: Vec<ColumnMeta> = (0..12)
        .map(|i| ColumnMeta::new(format!("col_{i:02}"), ColumnType::Int))
        .collect();
    app.set_schema(Schema::new(cols));
    app.on_loaded("ready");
    (app, rx)
}

/// A needle that fuzzy-matches exactly one column of [`wide_schema_app`] (the digits of `col_00`,
/// which only `col_00` contains as a subsequence).
fn first_column_unique_needle(_app: &App) -> &'static str {
    "00"
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
    // The query box is bordered; its inner text row is h - 3 (below it: the box bottom border with
    // the help hints at h-2, then the status row at h-1). Click on that text row inside the text.
    let bar_row = h - 3;
    // The box left border (col 0) + the `> ` prompt (cols 1-2) precede the text, so text col = x-3.
    // Click at screen col 8 -> text col 5, landing the cursor at char 5 ("T").
    app.on_mouse(MouseEvent::Click {
        col: 8,
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
fn click_on_second_line_of_multiline_bar_positions_cursor_on_that_line() {
    let mut app = app_with_result_on_bar(20);
    // Make the bar two lines: "SELECT * FROM t" then "WHERE id > 0". Dismiss the autocomplete popup
    // first so Enter inserts a newline rather than accepting a suggestion.
    if app.autocomplete().is_open() {
        app.on_key(KeyEvent::plain(Key::Esc), 0);
    }
    app.on_key(KeyEvent::plain(Key::Enter), 0);
    type_query(&mut app, "WHERE id > 0");
    if app.autocomplete().is_open() {
        app.on_key(KeyEvent::plain(Key::Esc), 0);
    }
    let (_w, h) = render_and_record(&app);
    // Layout: results(Min1) + bordered box(text 2 + 2 border = 4) + status(1). The box rows are
    // h-5 (top border), h-4 (line 1), h-3 (line 2), h-2 (bottom border = help hints); status at h-1.
    let bar_line2 = h - 3; // the second visual line ("WHERE id > 0")
    // Box left border (col 0) + `> ` prompt (cols 1-2): text col = x-3. Click screen col 5 ->
    // text col 2 on the second line ("WHERE id > 0").
    app.on_mouse(MouseEvent::Click {
        col: 5,
        row: bar_line2,
    });
    assert_eq!(app.focus(), Focus::QueryBar);
    assert_eq!(
        app.editor().row_col(),
        (1, 2),
        "cursor lands on the clicked line (1) at the clicked column (2), not line 0"
    );
}

#[test]
fn drag_in_query_bar_positions_cursor_like_a_click() {
    let mut app = app_with_result_on_bar(20);
    let (_w, h) = render_and_record(&app);
    let bar_row = h - 3;
    app.on_mouse(MouseEvent::Drag {
        col: 5,
        row: bar_row,
    }); // box border (0) + prompt (1-2): screen col 5 -> text col 2
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

#[test]
fn click_on_scrolled_autocomplete_row_selects_the_visible_index_not_the_absolute_row() {
    // With more candidates than the visible window, scroll the selection past the window, then
    // click the FIRST visible row. It must select the off-screen-anchored absolute index
    // (start + 0), not absolute index 0. Regression for the dropped scroll-offset.
    let (mut app, _rx) = wide_schema_app();
    type_query(&mut app, "SELECT "); // all columns (more than MAX_VISIBLE_ROWS=8)
    assert!(app.autocomplete().is_open());
    assert!(
        app.autocomplete().len() > 8,
        "need a list longer than the 8-row window, got {}",
        app.autocomplete().len()
    );
    // Press Down 9 times so the selection (index 9) scrolls the window down (start becomes 2).
    for _ in 0..9 {
        app.on_key(KeyEvent::plain(Key::Down), 0);
    }
    assert_eq!(app.autocomplete().selected(), 9);
    render_and_record(&app);
    let (_kind, rect) = app.layout_regions().popup.expect("popup recorded");
    let visible = rect.height.saturating_sub(2) as usize;
    let start = crate::scroll_window::scroll_offset(9, app.autocomplete().len(), visible);
    assert!(start > 0, "the list must have scrolled (start={start})");
    // Click the first visible row (inner row 0).
    app.on_mouse(MouseEvent::Click {
        col: rect.x + 1,
        row: rect.y + 1,
    });
    assert_eq!(
        app.autocomplete().selected(),
        start,
        "clicking the first visible row selects the scrolled-window index (start + 0), not 0"
    );
}

#[test]
fn click_in_blank_band_of_needle_filtered_palette_is_bounded() {
    // A needle that filters the palette to one column must size the recorded popup region to the
    // filtered count, so a click below the single drawn row does not resolve into the box as a
    // phantom row (region-vs-drawn geometry must match). Regression for the all_columns sizing.
    let (mut app, _rx) = wide_schema_app();
    app.on_key(KeyEvent::new(Key::Char('k'), crate::app::KeyMods::CTRL), 0);
    assert!(app.is_palette_open());
    // Filter to columns containing the subsequence of the first column's distinctive needle.
    let needle = first_column_unique_needle(&app);
    for c in needle.chars() {
        app.on_key(KeyEvent::char(c), 0);
    }
    let filtered = app.palette().unwrap().filtered_indices().len();
    assert_eq!(filtered, 1, "needle should narrow to exactly one column");
    render_and_record(&app);
    let (_kind, rect) = app.layout_regions().popup.expect("popup recorded");
    // The box inner height now equals the filtered count (1), so there is exactly one inner row.
    assert_eq!(
        rect.height.saturating_sub(2),
        1,
        "the popup region is sized to the filtered count, not the full column count"
    );
}
