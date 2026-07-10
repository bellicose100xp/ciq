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

// --- scroll over the results pane ---

#[test]
fn scroll_down_over_results_scrolls_the_grid_body() {
    let mut app = app_with_result_on_bar(50);
    let (_w, _h) = render_and_record(&app);
    // A wheel-down over the pane body (row 5 is inside the grid body) scrolls by the wheel step.
    app.on_mouse(MouseEvent::ScrollDown { col: 5, row: 5 }, 0);
    assert_eq!(app.v_row_offset(), 3, "one wheel notch = 3 rows");
    app.on_mouse(MouseEvent::ScrollDown { col: 5, row: 5 }, 0);
    assert_eq!(app.v_row_offset(), 6);
    // Wheel-up backs it off.
    app.on_mouse(MouseEvent::ScrollUp { col: 5, row: 5 }, 0);
    assert_eq!(app.v_row_offset(), 3);
}

#[test]
fn scroll_up_clamps_at_top() {
    let mut app = app_with_result_on_bar(50);
    render_and_record(&app);
    app.on_mouse(MouseEvent::ScrollUp { col: 5, row: 5 }, 0);
    assert_eq!(app.v_row_offset(), 0, "clamps at the top");
}

#[test]
fn scroll_down_clamps_at_last_row() {
    let mut app = app_with_result_on_bar(2); // body_len-1 == 1
    render_and_record(&app);
    app.on_mouse(MouseEvent::ScrollDown { col: 5, row: 5 }, 0);
    app.on_mouse(MouseEvent::ScrollDown { col: 5, row: 5 }, 0);
    assert_eq!(app.v_row_offset(), 1, "clamps at last row");
}

#[test]
fn horizontal_swipe_over_a_narrow_grid_clamps_to_zero() {
    // Two narrow columns ("id (int)"=8 + gutter 2 + "name (txt)"=10 = 20 chars total) easily
    // fit the 80-char viewport, so the right-edge cap (`total - viewport/2`) saturates at 0 —
    // there is nothing to scroll. The trackpad cannot drag the user into empty space.
    let mut app = app_with_result_on_bar(5);
    render_and_record(&app);
    app.on_mouse(MouseEvent::ScrollRight { col: 5, row: 5 }, 0);
    assert_eq!(app.h_char_offset(), 0, "right-cap pins narrow grid at 0");
    assert_eq!(app.h_col_offset(), 0);
    app.on_mouse(MouseEvent::ScrollLeft { col: 5, row: 5 }, 0);
    assert_eq!(app.h_char_offset(), 0, "clamps at 0 going left");
    assert_eq!(app.h_col_offset(), 0);
}

/// A loaded App with a wide enough result for the trackpad to actually slide. Eight columns
/// (header label "col_NN (int)" ≈ 13 chars each, sample cells fit) gives a total grid width
/// well past the 80-col viewport, so the right-edge cap leaves room for several notches.
fn app_with_wide_result_on_bar() -> App {
    use crate::engine::{Cell, Column, Table};
    use crate::query::worker::types::ProcessedResult;
    use crate::schema::ColumnType;

    let (mut app, _rx) = loaded_app();
    type_query(&mut app, "SELECT * FROM t");
    app.tick(150);
    let id = app.latest_request_id();
    let cols: Vec<Column> = (0..8)
        .map(|i| {
            let cells: Vec<Cell> = (0..3).map(|r| Cell::Int((i * 100 + r) as i64)).collect();
            Column::new(format!("col_{i:02}"), ColumnType::Int, cells)
        })
        .collect();
    let table = Table::new(cols);
    let schema = table.schema();
    let result = ProcessedResult::new(table, schema, 0);
    app.on_response(QueryResponse::ProcessedSuccess {
        result,
        request_id: id,
        kind: RequestKind::Main,
    });
    app
}

#[test]
fn trackpad_swipe_right_advances_h_char_offset_smoothly() {
    let mut app = app_with_wide_result_on_bar();
    render_and_record(&app);
    let before_chars = app.h_char_offset();
    let before_col = app.h_col_offset();
    app.on_mouse(MouseEvent::ScrollRight { col: 5, row: 5 }, 0);
    let after_chars = app.h_char_offset();
    // One notch = CHAR_SCROLL_STEP (3) chars of slide; the first column is wider than 3, so
    // h_col_offset stays put — the trackpad slid INSIDE the leftmost visible column.
    assert_eq!(
        after_chars,
        before_chars.saturating_add(3),
        "one notch slides 3 chars"
    );
    assert_eq!(
        app.h_col_offset(),
        before_col,
        "still inside the leftmost column"
    );
}

#[test]
fn trackpad_swipe_eventually_drops_a_column() {
    let mut app = app_with_wide_result_on_bar();
    render_and_record(&app);
    // Swipe right enough notches to cross the first column's full width plus the gutter so
    // h_col_offset bumps from 0 to 1. Each notch slides 3 chars; after enough notches the
    // first column (≈13 + 2 = 15 char left-edge for col 1) is fully off-screen.
    for _ in 0..6 {
        app.on_mouse(MouseEvent::ScrollRight { col: 5, row: 5 }, 0);
    }
    assert!(
        app.h_col_offset() >= 1,
        "after enough notches h_col_offset advances to drop col 0; got {}",
        app.h_col_offset()
    );
    // h_char_offset is still synced with how many chars have been slid.
    assert_eq!(app.h_char_offset(), 18, "6 notches × 3 chars = 18");
}

#[test]
fn trackpad_swipe_left_clamps_at_zero() {
    let mut app = app_with_wide_result_on_bar();
    render_and_record(&app);
    app.on_mouse(MouseEvent::ScrollLeft { col: 5, row: 5 }, 0);
    assert_eq!(app.h_char_offset(), 0);
    assert_eq!(app.h_col_offset(), 0);
}

#[test]
fn keyboard_left_after_partial_trackpad_slide_snaps_to_a_column_boundary() {
    let mut app = app_with_wide_result_on_bar();
    render_and_record(&app);
    // Slide partially into col 0 (3 chars).
    app.on_mouse(MouseEvent::ScrollRight { col: 5, row: 5 }, 0);
    assert_eq!(app.h_char_offset(), 3);
    assert_eq!(app.h_col_offset(), 0);
    // Move focus into the results pane so the keyboard ←/→ handler fires.
    app.on_key(
        KeyEvent::new(Key::Char('t'), super::super::KeyMods::CTRL),
        0,
    ); // Ctrl+T -> Results
    app.on_key(KeyEvent::plain(Key::Left), 0);
    // Left from col 0 stays at col 0; h_char_offset snaps back to the column's left edge (0).
    assert_eq!(app.h_char_offset(), 0, "snapped back to col 0 left edge");
    assert_eq!(app.h_col_offset(), 0);
}

#[test]
fn keyboard_right_after_no_slide_advances_one_full_column() {
    let mut app = app_with_wide_result_on_bar();
    render_and_record(&app);
    app.on_key(
        KeyEvent::new(Key::Char('t'), super::super::KeyMods::CTRL),
        0,
    ); // focus Results
    app.on_key(KeyEvent::plain(Key::Right), 0);
    // h_col_offset advanced to 1; h_char_offset snapped to col 1's left edge.
    assert_eq!(app.h_col_offset(), 1);
    assert!(
        app.h_char_offset() > 0,
        "h_char_offset is the cumulative left-edge of col 1, not 0"
    );
}

// --- click to focus / position ---

#[test]
fn click_in_results_pane_focuses_results() {
    let mut app = app_with_result_on_bar(20);
    render_and_record(&app);
    assert_eq!(app.focus(), Focus::QueryBar);
    app.on_mouse(MouseEvent::Click { col: 5, row: 5 }, 0); // inside the grid body
    assert_eq!(app.focus(), Focus::Results);
}

#[test]
fn click_in_query_bar_focuses_bar_and_positions_cursor() {
    let mut app = app_with_result_on_bar(20);
    // Put a known query in the bar and move focus to results first.
    // The bar already holds "SELECT * FROM t" (15 chars). Render to record regions.
    let (_w, h) = render_and_record(&app);
    app.on_mouse(MouseEvent::Click { col: 5, row: 5 }, 0); // focus results
    assert_eq!(app.focus(), Focus::Results);
    // The query box is bordered; its inner text row is h - 3 (below it: the box bottom border with
    // the help hints at h-2, then the status row at h-1). Click on that text row inside the text.
    let bar_row = h - 3;
    // The box left border (col 0) + the `> ` prompt (cols 1-2) precede the text, so text col = x-3.
    // Click at screen col 8 -> text col 5, landing the cursor at char 5 ("T").
    app.on_mouse(
        MouseEvent::Click {
            col: 8,
            row: bar_row,
        },
        0,
    );
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
    app.on_mouse(
        MouseEvent::Click {
            col: 70,
            row: bar_row,
        },
        0,
    );
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
    app.on_mouse(
        MouseEvent::Click {
            col: 5,
            row: bar_line2,
        },
        0,
    );
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
    app.on_mouse(
        MouseEvent::Drag {
            col: 5,
            row: bar_row,
        },
        0,
    ); // box border (0) + prompt (1-2): screen col 5 -> text col 2
    assert_eq!(app.focus(), Focus::QueryBar);
    assert_eq!(app.editor().cursor(), 2);
}

#[test]
fn click_in_query_bar_during_load_error_is_frozen() {
    let (mut app, _rx) = {
        use crate::engine::InterruptHandle;
        use std::sync::mpsc::channel;
        let (tx, rx) = channel();
        let mut app = App::new(tx, InterruptHandle::noop());
        app.force_power_mode_for_tests("");
        (app, rx)
    };
    app.on_load_error("boom");
    let (_w, h) = render_and_record(&app);
    let bar_row = h - 3;
    // A click on the frozen bar must not move focus into editing or position a cursor.
    app.on_mouse(
        MouseEvent::Click {
            col: 5,
            row: bar_row,
        },
        0,
    );
    assert_eq!(app.query(), "", "frozen bar takes no cursor change");
}

#[test]
fn click_in_results_with_no_result_does_not_focus() {
    let (mut app, _rx) = {
        use crate::engine::InterruptHandle;
        use std::sync::mpsc::channel;
        let (tx, rx) = channel();
        let mut app = App::new(tx, InterruptHandle::noop());
        app.force_power_mode_for_tests("");
        (app, rx)
    };
    app.set_schema(test_schema());
    app.on_loaded("ready");
    render_and_record(&app);
    app.on_mouse(MouseEvent::Click { col: 5, row: 5 }, 0);
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
    app.on_mouse(MouseEvent::ScrollDown { col: 5, row: 100 }, 0);
    assert_eq!(app.v_row_offset(), 3);
}

// --- popup scroll + click selection ---

#[test]
fn scroll_over_autocomplete_popup_moves_selection_by_wheel_rows() {
    let (mut app, _rx) = loaded_app();
    type_query(&mut app, "SELECT "); // empty partial -> all columns
    assert!(app.autocomplete().is_open());
    render_and_record(&app);
    assert_eq!(app.autocomplete().selected(), 0);
    // The popup anchors just above the bar; find a cell inside it and scroll.
    let (kind, rect) = app.layout_regions().popup.expect("popup recorded");
    assert_eq!(kind, crate::app::PopupKind::Autocomplete);
    let inner_row = rect.y + 1;
    // One wheel notch advances the selection by `WHEEL_ROWS` (3), matching the grid's per-tick
    // grain so the felt rate is consistent. The popup's auto-scroll then keeps the cursor inside
    // the visible window with the SCROLLOFF margin.
    app.on_mouse(
        MouseEvent::ScrollDown {
            col: rect.x + 1,
            row: inner_row,
        },
        0,
    );
    assert_eq!(
        app.autocomplete().selected(),
        3,
        "wheel-down advances selection by WHEEL_ROWS"
    );
    app.on_mouse(
        MouseEvent::ScrollUp {
            col: rect.x + 1,
            row: inner_row,
        },
        0,
    );
    assert_eq!(
        app.autocomplete().selected(),
        0,
        "wheel-up retreats selection by WHEEL_ROWS, bounded at 0 (no wrap)"
    );
}

#[test]
fn wheel_up_at_top_of_autocomplete_popup_clamps_at_zero() {
    let (mut app, _rx) = loaded_app();
    type_query(&mut app, "SELECT ");
    assert!(app.autocomplete().is_open());
    render_and_record(&app);
    assert_eq!(app.autocomplete().selected(), 0);
    let (_, rect) = app.layout_regions().popup.expect("popup recorded");
    app.on_mouse(
        MouseEvent::ScrollUp {
            col: rect.x + 1,
            row: rect.y + 1,
        },
        0,
    );
    assert_eq!(
        app.autocomplete().selected(),
        0,
        "wheel-up at the top is a bounded no-op, never wraps"
    );
}

#[test]
fn wheel_down_past_end_of_autocomplete_popup_clamps_at_last() {
    let (mut app, _rx) = loaded_app();
    type_query(&mut app, "SELECT ");
    assert!(app.autocomplete().is_open());
    render_and_record(&app);
    let last = app.autocomplete().len() - 1;
    let (_, rect) = app.layout_regions().popup.expect("popup recorded");
    // Spam wheel-down past the end of the list. Each tick advances by WHEEL_ROWS but the
    // bounded per-step select_next clamps at `last`, so no wrap.
    for _ in 0..(last + 5) {
        app.on_mouse(
            MouseEvent::ScrollDown {
                col: rect.x + 1,
                row: rect.y + 1,
            },
            0,
        );
    }
    assert_eq!(
        app.autocomplete().selected(),
        last,
        "wheel-down past the end is bounded at the last entry; no wrap"
    );
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
    app.on_mouse(
        MouseEvent::Click {
            col: rect.x + 1,
            row: third_row,
        },
        0,
    );
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
    app.on_mouse(
        MouseEvent::Click {
            col: rect.x + 1,
            row: rect.y + 1,
        },
        0,
    );
    assert_eq!(
        app.autocomplete().selected(),
        start,
        "clicking the first visible row selects the scrolled-window index (start + 0), not 0"
    );
}

// (Removed: `click_in_blank_band_of_needle_filtered_palette_is_bounded` covered the obsolete
// fuzzy-needle filter; the picker no longer filters in-popup. The popup-region-equals-drawn-rows
// invariant is now trivially true since every column is always drawn.)

// --- hover (Move events) ---

#[test]
fn move_over_a_grid_row_sets_the_hover_and_moving_off_clears_it() {
    let mut app = app_with_result_on_bar(10);
    render_and_record(&app);
    let rect = app.layout_regions().results_pane.expect("pane recorded");
    // Body row 0 sits below the top border (1) + sticky header (1).
    let body_top = rect.y + 2;
    app.on_mouse(
        MouseEvent::Move {
            col: 5,
            row: body_top + 2,
        },
        0,
    );
    assert_eq!(
        app.hover(),
        Some(crate::app::HoverTarget::GridRow(2)),
        "hover lands on the absolute body row"
    );
    // Moving onto the sticky header (no body row there) clears it.
    app.on_mouse(
        MouseEvent::Move {
            col: 5,
            row: rect.y + 1,
        },
        0,
    );
    assert_eq!(app.hover(), None);
}

#[test]
fn grid_hover_folds_in_the_vertical_scroll() {
    let mut app = app_with_result_on_bar(50);
    render_and_record(&app);
    app.on_mouse(MouseEvent::ScrollDown { col: 5, row: 5 }, 0); // v_row_offset = 3
    let rect = app.layout_regions().results_pane.expect("pane recorded");
    app.on_mouse(
        MouseEvent::Move {
            col: 5,
            row: rect.y + 2,
        },
        0,
    );
    assert_eq!(
        app.hover(),
        Some(crate::app::HoverTarget::GridRow(3)),
        "hover on the first visible row is offset by the scroll"
    );
}

#[test]
fn hover_below_a_short_result_highlights_nothing() {
    let mut app = app_with_result_on_bar(2);
    render_and_record(&app);
    let rect = app.layout_regions().results_pane.expect("pane recorded");
    // Row index 5 is inside the pane but past the 2-row result.
    app.on_mouse(
        MouseEvent::Move {
            col: 5,
            row: rect.y + 2 + 5,
        },
        0,
    );
    assert_eq!(app.hover(), None, "no data row under the pointer");
}

#[test]
fn move_over_an_autocomplete_row_sets_popup_hover() {
    let (mut app, _rx) = loaded_app();
    type_query(&mut app, "SELECT ");
    assert!(app.autocomplete().is_open());
    render_and_record(&app);
    let (kind, rect) = app.layout_regions().popup.expect("popup recorded");
    app.on_mouse(
        MouseEvent::Move {
            col: rect.x + 1,
            row: rect.y + 1 + 1,
        },
        0,
    );
    assert_eq!(
        app.hover(),
        Some(crate::app::HoverTarget::PopupRow(kind, 1)),
        "hover lands on the popup row under the pointer"
    );
    // The popup border row carries no hover.
    app.on_mouse(
        MouseEvent::Move {
            col: rect.x + 1,
            row: rect.y,
        },
        0,
    );
    assert_eq!(app.hover(), None);
}

#[test]
fn hover_past_the_popup_list_end_highlights_nothing() {
    let (mut app, _rx) = loaded_app();
    type_query(&mut app, "SELECT id"); // narrow list (usually 1 candidate)
    if !app.autocomplete().is_open() {
        return; // nothing to hover; scenario not reachable with this schema
    }
    render_and_record(&app);
    let (_kind, rect) = app.layout_regions().popup.expect("popup recorded");
    let len = app.autocomplete().len();
    let inner_rows = rect.height.saturating_sub(2) as usize;
    if len >= inner_rows {
        return; // no blank band to test
    }
    app.on_mouse(
        MouseEvent::Move {
            col: rect.x + 1,
            row: rect.y + 1 + len as u16,
        },
        0,
    );
    assert_eq!(app.hover(), None, "a blank popup row hovers nothing");
}

// --- double-click ---

#[test]
fn double_click_on_autocomplete_row_accepts_the_suggestion() {
    let (mut app, _rx) = loaded_app();
    type_query(&mut app, "SELECT ");
    assert!(app.autocomplete().is_open());
    render_and_record(&app);
    let (_kind, rect) = app.layout_regions().popup.expect("popup recorded");
    let cell = (rect.x + 1, rect.y + 1 + 1); // second row
    app.on_mouse(
        MouseEvent::Click {
            col: cell.0,
            row: cell.1,
        },
        100,
    );
    assert!(app.autocomplete().is_open(), "first click only selects");
    assert_eq!(app.autocomplete().selected(), 1);
    let before = app.query().to_string();
    app.on_mouse(
        MouseEvent::Click {
            col: cell.0,
            row: cell.1,
        },
        300,
    );
    assert!(
        !app.autocomplete().is_open(),
        "second fast click accepts and closes the popup"
    );
    assert_ne!(app.query(), before, "the suggestion was inserted");
}

#[test]
fn slow_second_click_on_autocomplete_row_does_not_accept() {
    let (mut app, _rx) = loaded_app();
    type_query(&mut app, "SELECT ");
    assert!(app.autocomplete().is_open());
    render_and_record(&app);
    let (_kind, rect) = app.layout_regions().popup.expect("popup recorded");
    let cell = (rect.x + 1, rect.y + 1);
    app.on_mouse(
        MouseEvent::Click {
            col: cell.0,
            row: cell.1,
        },
        0,
    );
    app.on_mouse(
        MouseEvent::Click {
            col: cell.0,
            row: cell.1,
        },
        401,
    );
    assert!(
        app.autocomplete().is_open(),
        "past the 400ms threshold the pair never forms"
    );
}

#[test]
fn scroll_between_two_clicks_invalidates_the_double() {
    let (mut app, _rx) = loaded_app();
    type_query(&mut app, "SELECT ");
    assert!(app.autocomplete().is_open());
    render_and_record(&app);
    let (_kind, rect) = app.layout_regions().popup.expect("popup recorded");
    let cell = (rect.x + 1, rect.y + 1);
    app.on_mouse(
        MouseEvent::Click {
            col: cell.0,
            row: cell.1,
        },
        0,
    );
    app.on_mouse(MouseEvent::ScrollDown { col: 2, row: 2 }, 50);
    app.on_mouse(
        MouseEvent::Click {
            col: cell.0,
            row: cell.1,
        },
        100,
    );
    assert!(
        app.autocomplete().is_open(),
        "a scroll between the clicks resets the pair"
    );
}

// --- history popup mouse ---

/// A loaded app with one history entry and the history popup open (via the real Ctrl+R chord).
fn app_with_history_open() -> (
    App,
    std::sync::mpsc::Receiver<crate::query::worker::types::QueryRequest>,
) {
    let (mut app, rx) = loaded_app();
    type_query(&mut app, "SELECT * FROM t");
    app.tick(150); // dispatch records the query in history
    // Clear the bar so the recall observable is a bar-text change (and the needle seed is empty —
    // an unfiltered list).
    for _ in 0..40 {
        app.on_key(KeyEvent::plain(Key::Backspace), 200);
    }
    app.on_key(
        KeyEvent::new(Key::Char('r'), crate::app::KeyMods::CTRL),
        300,
    );
    assert!(app.is_history_open());
    (app, rx)
}

#[test]
fn click_on_history_row_recalls_the_entry() {
    let (mut app, _rx) = app_with_history_open();
    render_and_record(&app);
    let (kind, rect) = app.layout_regions().popup.expect("popup recorded");
    assert_eq!(kind, crate::app::PopupKind::History);
    app.on_mouse(
        MouseEvent::Click {
            col: rect.x + 1,
            row: rect.y + 1,
        },
        400,
    );
    assert!(!app.is_history_open(), "recall closes the popup");
    assert_eq!(
        app.query(),
        "SELECT * FROM t",
        "the clicked entry landed in the bar"
    );
}

#[test]
fn click_outside_the_history_popup_dismisses_without_recall() {
    let (mut app, _rx) = app_with_history_open();
    render_and_record(&app);
    let bar = app.query().to_string();
    // Click far away (the results pane area, row 2).
    app.on_mouse(MouseEvent::Click { col: 2, row: 2 }, 400);
    assert!(!app.is_history_open(), "click-outside dismisses");
    assert_eq!(app.query(), bar, "no recall on dismiss");
    assert_eq!(
        app.focus(),
        Focus::QueryBar,
        "the dismissing click is swallowed (does not also focus the grid)"
    );
}

// --- palette popup mouse ---

/// A loaded app still in the production default Simple mode (the shared `loaded_app` forces Power
/// for the legacy tests, so this builds its own), autocomplete dismissed.
fn simple_mode_app() -> (
    App,
    std::sync::mpsc::Receiver<crate::query::worker::types::QueryRequest>,
) {
    use crate::engine::InterruptHandle;
    use std::sync::mpsc::channel;
    let (tx, rx) = channel();
    let mut app = App::new(tx, InterruptHandle::noop());
    app.set_schema(test_schema());
    app.on_loaded("ready");
    let mut guard = 0;
    while app.autocomplete().is_open() && guard < 4 {
        app.on_key(KeyEvent::plain(Key::Esc), 0);
        guard += 1;
    }
    (app, rx)
}

/// A loaded app in Simple mode with the palette open on the SELECT pane (the real Ctrl+P chord).
fn app_with_palette_open() -> (
    App,
    std::sync::mpsc::Receiver<crate::query::worker::types::QueryRequest>,
) {
    let (mut app, rx) = simple_mode_app();
    app.query_form_mut().focus(crate::app::SimplePane::Select);
    let mut guard = 0;
    while app.autocomplete().is_open() && guard < 4 {
        app.on_key(KeyEvent::plain(Key::Esc), 0);
        guard += 1;
    }
    app.on_key(KeyEvent::new(Key::Char('p'), crate::app::KeyMods::CTRL), 0);
    assert!(app.is_palette_open());
    (app, rx)
}

#[test]
fn double_click_on_palette_row_toggles_the_column() {
    let (mut app, _rx) = app_with_palette_open();
    render_and_record(&app);
    let (kind, rect) = app.layout_regions().popup.expect("popup recorded");
    assert_eq!(kind, crate::app::PopupKind::Palette);
    let checked_before = app.palette().unwrap().is_checked(0);
    let cell = (rect.x + 1, rect.y + 1);
    app.on_mouse(
        MouseEvent::Click {
            col: cell.0,
            row: cell.1,
        },
        100,
    );
    assert_eq!(
        app.palette().unwrap().is_checked(0),
        checked_before,
        "a single click only moves the cursor"
    );
    assert_eq!(app.palette().unwrap().cursor(), 0);
    app.on_mouse(
        MouseEvent::Click {
            col: cell.0 + 3,
            row: cell.1,
        },
        300,
    );
    assert_eq!(
        app.palette().unwrap().is_checked(0),
        !checked_before,
        "a fast second click on the same row toggles (SameRow granularity tolerates column jitter)"
    );
}

// --- facet click-dismiss ---

#[test]
fn click_anywhere_dismisses_an_open_facet() {
    let (mut app, rx) = loaded_app();
    type_query(&mut app, "SELECT * FROM t");
    app.tick(150);
    let id = app.latest_request_id();
    app.on_response(QueryResponse::ProcessedSuccess {
        result: wide_result(3),
        request_id: id,
        kind: RequestKind::Main,
    });
    if app.autocomplete().is_open() {
        app.on_key(KeyEvent::plain(Key::Esc), 200);
    }
    app.on_key(KeyEvent::plain(Key::Down), 200); // focus results
    assert_eq!(app.focus(), Focus::Results);
    app.on_key(KeyEvent::char('f'), 250); // open the facet (pending)
    let _ = rx; // the facet fetch request is not asserted here
    assert!(app.is_facet_open());
    render_and_record(&app);
    app.on_mouse(MouseEvent::Click { col: 2, row: 2 }, 300);
    assert!(!app.is_facet_open(), "any click dismisses the facet");
}

// --- drag never activates ---

#[test]
fn drag_over_a_history_row_selects_but_does_not_recall() {
    let (mut app, _rx) = app_with_history_open();
    render_and_record(&app);
    let (_kind, rect) = app.layout_regions().popup.expect("popup recorded");
    app.on_mouse(
        MouseEvent::Drag {
            col: rect.x + 1,
            row: rect.y + 1,
        },
        400,
    );
    assert!(
        app.is_history_open(),
        "a drag over a history row must not recall/close"
    );
}

// --- search bar mouse ---

/// A loaded app with the Ctrl+F search bar open in editing mode and a typed needle.
fn app_with_search_editing() -> App {
    let mut app = app_with_result_on_bar(20);
    if app.autocomplete().is_open() {
        app.on_key(KeyEvent::plain(Key::Esc), 0);
    }
    app.on_key(
        KeyEvent::new(Key::Char('f'), super::super::KeyMods::CTRL),
        0,
    );
    assert!(app.search().is_editing());
    // "0" matches the wide_result cells (Int columns counting from 0).
    app.on_key(KeyEvent::char('0'), 0);
    assert!(app.search().is_filtering());
    app
}

#[test]
fn click_on_confirmed_search_bar_reenters_editing() {
    let mut app = app_with_search_editing();
    app.on_key(KeyEvent::plain(Key::Enter), 0); // confirm
    assert!(app.search().is_confirmed());
    render_and_record(&app);
    let rect = app.layout_regions().search_bar.expect("bar recorded");
    app.on_mouse(
        MouseEvent::Click {
            col: rect.x + 2,
            row: rect.y + 1,
        },
        0,
    );
    assert!(
        app.search().is_editing(),
        "a click on the confirmed bar re-enters needle editing (Ctrl+F parity)"
    );
    // Typing now edits the needle again.
    app.on_key(KeyEvent::char('1'), 0);
    assert_eq!(app.search().needle(), "01");
}

#[test]
fn click_on_search_bar_while_editing_keeps_editing() {
    let mut app = app_with_search_editing();
    render_and_record(&app);
    let rect = app.layout_regions().search_bar.expect("bar recorded");
    app.on_mouse(
        MouseEvent::Click {
            col: rect.x + 2,
            row: rect.y + 1,
        },
        0,
    );
    assert!(
        app.search().is_editing(),
        "already editing: click is a no-op"
    );
    assert_eq!(app.search().needle(), "0", "the needle is untouched");
}

#[test]
fn click_on_query_bar_while_search_editing_confirms_and_moves_typing_to_the_bar() {
    let mut app = app_with_search_editing();
    let (_w, h) = render_and_record(&app);
    let bar_row = h - 3; // the query box inner text row
    app.on_mouse(
        MouseEvent::Click {
            col: 8,
            row: bar_row,
        },
        0,
    );
    assert!(
        app.search().is_confirmed(),
        "clicking the query bar confirms the non-empty needle (Enter parity)"
    );
    assert_eq!(app.focus(), Focus::QueryBar);
    let before = app.query().to_string();
    app.on_key(KeyEvent::char('x'), 0);
    assert_ne!(
        app.query(),
        before,
        "typing lands in the query bar, not the needle"
    );
    assert_eq!(
        app.search().needle(),
        "0",
        "the needle stopped capturing keys"
    );
}

#[test]
fn click_on_query_bar_with_empty_needle_closes_the_search() {
    let mut app = app_with_result_on_bar(20);
    if app.autocomplete().is_open() {
        app.on_key(KeyEvent::plain(Key::Esc), 0);
    }
    app.on_key(
        KeyEvent::new(Key::Char('f'), super::super::KeyMods::CTRL),
        0,
    );
    assert!(app.search().is_editing());
    let (_w, h) = render_and_record(&app);
    let bar_row = h - 3;
    app.on_mouse(
        MouseEvent::Click {
            col: 8,
            row: bar_row,
        },
        0,
    );
    assert!(
        !app.search().is_visible(),
        "an empty needle has nothing to freeze — the bar closes (Enter parity)"
    );
    assert_eq!(app.focus(), Focus::QueryBar);
}

#[test]
fn click_on_results_while_search_editing_confirms_the_filter() {
    let mut app = app_with_search_editing();
    render_and_record(&app);
    app.on_mouse(MouseEvent::Click { col: 5, row: 5 }, 0); // inside the grid body
    assert!(
        app.search().is_confirmed(),
        "a grid click freezes the filter and resumes navigation"
    );
    assert_eq!(app.focus(), Focus::Results);
}

#[test]
fn search_bar_region_is_absent_when_the_bar_is_closed() {
    let app = app_with_result_on_bar(5);
    render_and_record(&app);
    assert_eq!(
        app.layout_regions().search_bar,
        None,
        "no phantom click target when Ctrl+F is closed"
    );
}

// --- Simple-mode click column mapping ---

#[test]
fn simple_mode_pane_click_maps_columns_past_the_label_gutter() {
    // Simple mode reserves a 9-char label column (SIMPLE_LABEL_WIDTH) left of the editor text —
    // the click→text-col mapping must subtract it (not the Power-mode 2-char prompt).
    let (mut app, _rx) = simple_mode_app();
    // The WHERE pane is the default focus; type into it so there is text to land in.
    type_query(&mut app, "id > 5");
    if app.autocomplete().is_open() {
        app.on_key(KeyEvent::plain(Key::Esc), 0);
    }
    render_and_record(&app);
    let bar = app.layout_regions().query_bar.expect("bar recorded");
    // Click the WHERE pane (row index 1) at text column 2: screen x = bar.x + label(9) + 2.
    app.on_mouse(
        MouseEvent::Click {
            col: bar.x + 9 + 2,
            row: bar.y + 1,
        },
        0,
    );
    assert_eq!(app.focus(), Focus::QueryBar);
    assert_eq!(
        app.query_form().focused_pane(),
        crate::app::SimplePane::Where
    );
    assert_eq!(
        app.editor().cursor(),
        2,
        "text col = screen col - label width (9), not - prompt width (2)"
    );
    // A click on the label gutter clamps to column 0.
    app.on_mouse(
        MouseEvent::Click {
            col: bar.x + 3,
            row: bar.y + 1,
        },
        0,
    );
    assert_eq!(app.editor().cursor(), 0, "label-gutter click clamps to 0");
}
