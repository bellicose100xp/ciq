//! Render tests for the app layer — drives `App::render` through a `ratatui::TestBackend` and
//! asserts on the in-memory cell buffer (headless; no real TTY). Covers each phase's results
//! pane and the three-region layout (query bar / results / status).

use std::sync::mpsc::channel;

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::style::{Color, Modifier};

use crate::app::{App, Focus, Key, KeyEvent};
use crate::engine::InterruptHandle;
use crate::engine::types::{Cell, Column, Table};
use crate::query::worker::types::{ProcessedResult, QueryResponse};
use crate::schema::ColumnType;
use crate::theme;

fn app() -> App {
    let (tx, _rx) = channel();
    let mut app = App::new(tx, InterruptHandle::noop());
    // Force Power mode so the legacy render assertions (single textarea, 1-text-row bar by default)
    // keep their geometry. Simple-mode rendering has dedicated tests in `query_form_tests`; the
    // bulk of `app_render_tests` predates Simple mode.
    app.force_power_mode_for_tests("");
    app
}

fn render(app: &App, w: u16, h: u16) -> String {
    let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
    t.draw(|f| app.render(f)).unwrap();
    t.backend().to_string()
}

/// Whether any cell in the rendered buffer carries the `REVERSED` modifier — the query-bar cursor
/// cell's distinguishing style (`theme::app::cursor()`). Proves the cursor is visible headlessly
/// (a `frame.set_cursor` cursor would leave no styled cell in a `TestBackend` buffer).
fn has_reversed_cell(app: &App, w: u16, h: u16) -> bool {
    let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
    t.draw(|f| app.render(f)).unwrap();
    let buffer = t.backend().buffer();
    buffer
        .content()
        .iter()
        .any(|cell| cell.modifier.contains(Modifier::REVERSED))
}

fn result() -> ProcessedResult {
    let table = Table::new(vec![
        Column::new("id", ColumnType::Int, vec![Cell::Int(7), Cell::Int(8)]),
        Column::new(
            "name",
            ColumnType::Text,
            vec![Cell::Text("ada".into()), Cell::Null],
        ),
    ]);
    let s = table.schema();
    ProcessedResult::new(table, s, 0)
}

#[test]
fn loading_phase_shows_loading_indicator() {
    let screen = render(&app(), 40, 8);
    assert!(screen.contains("loading"), "screen:\n{screen}");
}

#[test]
fn status_line_shows_vim_mode_badge() {
    let mut a = app();
    a.on_loaded("ready");
    // Default mode is Insert — the badge is visible on the status line.
    let screen = render(&a, 40, 8);
    assert!(screen.contains("INSERT"), "INSERT badge missing:\n{screen}");
    // Esc drops to Normal — the badge updates.
    a.on_key(KeyEvent::plain(Key::Esc), 0);
    let screen = render(&a, 40, 8);
    assert!(screen.contains("NORMAL"), "NORMAL badge missing:\n{screen}");
    assert!(
        !screen.contains("INSERT"),
        "stale INSERT badge still shown:\n{screen}"
    );
}

#[test]
fn ready_empty_shows_hint() {
    let mut a = app();
    a.on_loaded("ready");
    let screen = render(&a, 60, 8);
    assert!(
        screen.contains("type a SQL query"),
        "expected empty-state hint, screen:\n{screen}"
    );
}

#[test]
fn populated_grid_renders_header_body_and_null_glyph() {
    let mut a = app();
    a.on_loaded("ready");
    for c in "SELECT * FROM t".chars() {
        a.on_key(KeyEvent::char(c), 0);
    }
    a.tick(150);
    let id = a.latest_request_id();
    a.on_response(QueryResponse::ProcessedSuccess {
        result: result(),
        request_id: id,
        kind: crate::query::worker::types::RequestKind::Main,
    });
    let screen = render(&a, 40, 10);
    assert!(screen.contains("id"), "header id, screen:\n{screen}");
    assert!(screen.contains("name"), "header name, screen:\n{screen}");
    assert!(screen.contains("ada"), "cell value, screen:\n{screen}");
    assert!(screen.contains("NULL"), "null glyph, screen:\n{screen}");
    assert!(screen.contains("2 rows"), "status, screen:\n{screen}");
}

#[test]
fn single_type_annotated_header_and_dialect_summary() {
    use crate::schema::{ColumnMeta, Schema};
    let mut a = app();
    a.set_schema(Schema::new(vec![
        ColumnMeta::new("id", ColumnType::Int),
        ColumnMeta::new("name", ColumnType::Text),
    ]));
    a.set_csv_summary(Some(','), true);
    a.on_loaded("ready");
    for c in "SELECT * FROM t".chars() {
        a.on_key(KeyEvent::char(c), 0);
    }
    // Dismiss the autocomplete popup so it doesn't overlay the header in this render.
    a.on_key(KeyEvent::plain(Key::Esc), 0);
    a.tick(150);
    let id = a.latest_request_id();
    // Cells wide enough that the grid columns admit the decorated `name (badge)` form.
    let table = Table::new(vec![
        Column::new(
            "id",
            ColumnType::Int,
            vec![Cell::Int(10_000_000), Cell::Int(8)],
        ),
        Column::new(
            "name",
            ColumnType::Text,
            vec![Cell::Text("ada lovelace".into()), Cell::Null],
        ),
    ]);
    let s = table.schema();
    a.on_response(QueryResponse::ProcessedSuccess {
        result: ProcessedResult::new(table, s, 0),
        request_id: id,
        kind: crate::query::worker::types::RequestKind::Main,
    });
    let screen = render(&a, 60, 12);

    // The dialect summary shows in the pane border title.
    assert!(
        screen.contains("delim , | header on"),
        "delimiter/header summary in border title, screen:\n{screen}"
    );
    // The single sticky header carries name + type badge.
    assert!(
        screen.contains("id (int)"),
        "header id badge, screen:\n{screen}"
    );
    assert!(
        screen.contains("name (txt)"),
        "header name badge, screen:\n{screen}"
    );
    assert!(screen.contains("ada"), "grid cell value, screen:\n{screen}");
    assert!(screen.contains("2 rows"), "status, screen:\n{screen}");

    // The header row appears EXACTLY ONCE — the old duplicate (a dimmed schema-bar name row + a
    // bold grid header) is gone. There is one line carrying the column names, not two.
    let header_lines = screen.lines().filter(|l| l.contains("name (txt)")).count();
    assert_eq!(
        header_lines, 1,
        "exactly one header row (no duplicate), screen:\n{screen}"
    );
}

#[test]
fn zero_row_result_renders_no_rows_match() {
    let mut a = app();
    a.on_loaded("ready");
    for c in "SELECT * FROM t WHERE id < 0".chars() {
        a.on_key(KeyEvent::char(c), 0);
    }
    a.tick(150);
    let id = a.latest_request_id();
    // An empty result table (zero rows) — a genuine empty *result*.
    let table = Table::new(vec![Column::new("id", ColumnType::Int, vec![])]);
    let s = table.schema();
    a.on_response(QueryResponse::ProcessedSuccess {
        result: ProcessedResult::new(table, s, 0),
        request_id: id,
        kind: crate::query::worker::types::RequestKind::Main,
    });
    let screen = render(&a, 50, 8);
    assert!(
        screen.contains("no rows match"),
        "zero-row empty-state, screen:\n{screen}"
    );
}

#[test]
fn capped_result_renders_row_counter_with_plus_suffix() {
    use crate::app::VIEWPORT_ROW_LIMIT;
    // Keep the worker receiver alive so the dispatch succeeds and the ciq-capped flag is recorded
    // (the cap signal is gated on a *successful* dispatch having applied ciq's viewport LIMIT — a
    // dropped receiver fails the send and never records the flag).
    let (tx, _rx) = std::sync::mpsc::channel();
    let mut a = App::new(tx, InterruptHandle::noop());
    a.force_power_mode_for_tests("");
    a.on_loaded("ready");
    for c in "SELECT * FROM t".chars() {
        a.on_key(KeyEvent::char(c), 0);
    }
    a.on_key(KeyEvent::plain(Key::Esc), 0); // dismiss the autocomplete popup
    a.tick(150);
    let id = a.latest_request_id();
    // A result at the viewport cap — the grid is ciq-truncated.
    let cells: Vec<Cell> = (0..VIEWPORT_ROW_LIMIT as i64).map(Cell::Int).collect();
    let table = Table::new(vec![Column::new("id", ColumnType::Int, cells)]);
    let s = table.schema();
    a.on_response(QueryResponse::ProcessedSuccess {
        result: ProcessedResult::new(table, s, 0),
        request_id: id,
        kind: crate::query::worker::types::RequestKind::Main,
    });
    let screen = render(&a, 60, 12);
    // jiq-style row counter on the results pane top-right border: `1000+` when capped (no
    // separate "showing first N rows" interior banner row anymore).
    assert!(
        screen.contains("1000+"),
        "capped row counter on the results border, screen:\n{screen}"
    );
    assert!(
        !screen.contains("showing first"),
        "no inline truncation banner row, screen:\n{screen}"
    );
    // The grid still renders its header.
    assert!(screen.contains("id"), "grid header, screen:\n{screen}");
}

#[test]
fn uncapped_result_renders_rendered_only_row_counter() {
    // Non-capped results show the bare `<rendered>` count on the results pane top-right border.
    // ciq has no second total to display (no follow-up COUNT(*) query, by design), so the
    // counter carries one fact rather than the tautological `N/N`.
    let mut a = app();
    a.on_loaded("ready");
    for c in "SELECT * FROM t".chars() {
        a.on_key(KeyEvent::char(c), 0);
    }
    a.on_key(KeyEvent::plain(Key::Esc), 0); // dismiss the autocomplete popup
    a.tick(150);
    let id = a.latest_request_id();
    a.on_response(QueryResponse::ProcessedSuccess {
        result: result(),
        request_id: id,
        kind: crate::query::worker::types::RequestKind::Main,
    });
    let screen = render(&a, 60, 12);
    // The counter sits flush against the top-right corner glyph; assert it appears AND that the
    // tautological `N/N` form is gone.
    assert!(
        screen.contains("2┐"),
        "row counter shows just the rendered count flush to the corner, screen:\n{screen}"
    );
    assert!(
        !screen.contains("2/2"),
        "row counter no longer renders the tautological N/N, screen:\n{screen}"
    );
}

#[test]
fn zero_row_result_omits_row_counter_on_border() {
    // Regression: pre-fix the counter rendered `0/0` on the border AND the body showed
    // "no rows match" — duplicate noise. The empty-state body is the canonical zero signal.
    let mut a = app();
    a.on_loaded("ready");
    for c in "SELECT * FROM t WHERE id < 0".chars() {
        a.on_key(KeyEvent::char(c), 0);
    }
    a.on_key(KeyEvent::plain(Key::Esc), 0);
    a.tick(150);
    let id = a.latest_request_id();
    let table = Table::new(vec![Column::new("id", ColumnType::Int, vec![])]);
    let s = table.schema();
    a.on_response(QueryResponse::ProcessedSuccess {
        result: ProcessedResult::new(table, s, 0),
        request_id: id,
        kind: crate::query::worker::types::RequestKind::Main,
    });
    let screen = render(&a, 60, 8);
    assert!(
        screen.contains("no rows match"),
        "empty-state body still rendered, screen:\n{screen}"
    );
    assert!(
        !screen.contains("0/0"),
        "row counter must be omitted on a zero-row result, screen:\n{screen}"
    );
}

#[test]
fn load_error_phase_renders_error_text() {
    let mut a = app();
    a.on_load_error("permission denied");
    let screen = render(&a, 60, 8);
    assert!(
        screen.contains("could not load CSV"),
        "results pane error, screen:\n{screen}"
    );
    assert!(
        screen.contains("permission denied"),
        "status error, screen:\n{screen}"
    );
}

#[test]
fn query_text_appears_in_bar() {
    let mut a = app();
    a.on_loaded("ready");
    a.on_key(KeyEvent::plain(Key::Paste("SELECT 42".into())), 0);
    let screen = render(&a, 40, 6);
    assert!(screen.contains("SELECT 42"), "screen:\n{screen}");
    assert!(screen.contains('>'), "prompt glyph, screen:\n{screen}");
}

#[test]
fn query_bar_sits_at_the_bottom() {
    // The query input is anchored near the bottom of the screen (status line below it), not at
    // the top. The prompt line must fall in the lower portion of the frame.
    let mut a = app();
    a.on_loaded("ready");
    a.on_key(KeyEvent::plain(Key::Paste("SELECT 42".into())), 0);
    let h = 10u16;
    let screen = render(&a, 40, h);
    let lines: Vec<&str> = screen.lines().collect();
    let bar_line = lines
        .iter()
        .position(|l| l.contains("SELECT 42"))
        .expect("query bar line");
    // The query box is bordered; its inner text line is the third-to-last row (the box's bottom
    // border carrying the help hints is below it at h-2, then the status line at h-1). Definitively
    // in the bottom half, not row 0 as it used to be.
    assert_eq!(
        bar_line,
        lines.len() - 3,
        "query text sits just above the box bottom border (help) + status row, screen:\n{screen}"
    );
}

#[test]
fn keyboard_hints_render_centered_on_the_query_box_bottom_border() {
    // Hints live on the box's BOTTOM border, CENTERED (so the legend reads as one compact unit).
    // The top border carries the per-mode badge — left-aligned — separately. With a single-line
    // query the box spans the last three rows above the status line: top border (mode badge),
    // text line, bottom border (hints).
    let mut a = app();
    a.on_loaded("ready");
    a.on_key(KeyEvent::plain(Key::Paste("SELECT 42".into())), 0);
    let h = 10u16;
    let screen = render(&a, 60, h);
    let lines: Vec<&str> = screen.lines().collect();
    let text_line = lines
        .iter()
        .position(|l| l.contains("SELECT 42"))
        .expect("query text line");
    // A hint description ("complete") sits on the row just below the text line — the box's bottom
    // border — not on the text row and not on the status row.
    let hint_line = lines
        .iter()
        .position(|l| l.contains("complete"))
        .expect("help hints on a border row");
    assert_eq!(
        hint_line,
        text_line + 1,
        "hints render on the box bottom border, directly below the query text:\n{screen}"
    );
    // That hint row is the box's bottom border (h-2): the status line is the very last row (h-1).
    assert_eq!(
        hint_line,
        lines.len() - 2,
        "the help-bearing bottom border sits just above the status row:\n{screen}"
    );
    // The mode badge does NOT ride the bottom border anymore — it lives on the TOP border.
    assert!(
        !lines[hint_line].contains("INSERT"),
        "mode badge does NOT ride the bottom border:\n{screen}"
    );
    // The bottom-border hints are CENTERED — the run of hint chars is preceded by leading
    // border/padding chars rather than starting at column 1 (the ratatui Block::title_bottom +
    // Line::centered placement). With a 60-col line and a few short hints the centered legend
    // sits well inside the row, so there is non-empty padding (border `─` glyphs and spaces) on
    // the left of the hints.
    let bottom = lines[hint_line];
    let first_hint_col = bottom
        .find("complete")
        .expect("hint substring on the bottom border");
    assert!(
        first_hint_col > 4,
        "centered hints leave non-trivial left padding (col={first_hint_col}):\n{screen}"
    );
}

#[test]
fn vim_mode_badge_rides_the_query_box_top_border() {
    // The mode badge is on the TOP border of the query box — left-aligned, per-mode color. With a
    // single-line query the box spans rows h-3..=h-1 above the status row: top border (mode), text
    // line, bottom border (hints). The badge text lands on the top-border row.
    let mut a = app();
    a.on_loaded("ready");
    a.on_key(KeyEvent::plain(Key::Paste("SELECT 42".into())), 0);
    let h = 10u16;
    let screen = render(&a, 60, h);
    let lines: Vec<&str> = screen.lines().collect();
    let text_line = lines
        .iter()
        .position(|l| l.contains("SELECT 42"))
        .expect("query text line");
    // The top border row sits directly above the text line.
    let top_border = text_line.checked_sub(1).expect("top border above text");
    assert!(
        lines[top_border].contains("INSERT"),
        "INSERT badge on the box top border:\n{screen}"
    );
    // Switch to Normal mode — the badge text follows.
    a.on_key(KeyEvent::plain(Key::Esc), 0);
    let screen = render(&a, 60, h);
    let lines: Vec<&str> = screen.lines().collect();
    let text_line = lines
        .iter()
        .position(|l| l.contains("SELECT 42"))
        .expect("query text line");
    let top_border = text_line.checked_sub(1).expect("top border above text");
    assert!(
        lines[top_border].contains("NORMAL"),
        "NORMAL badge on the box top border:\n{screen}"
    );
}

#[test]
fn render_does_not_panic_on_tiny_viewport() {
    // Degenerate sizes must never panic (the results pane border can consume the whole area).
    for (w, h) in [(1, 1), (2, 2), (3, 3), (1, 8), (8, 1)] {
        let _ = render(&app(), w, h);
    }
}

#[test]
fn open_popup_overlays_the_screen() {
    use crate::schema::{ColumnMeta, Schema};
    let mut a = app();
    a.set_schema(Schema::new(vec![
        ColumnMeta::new("status", ColumnType::Text),
        ColumnMeta::new("amount", ColumnType::Float),
    ]));
    a.on_loaded("ready");
    // Typing in the SELECT list opens the popup; it must paint over the results area.
    for c in "SELECT st".chars() {
        a.on_key(KeyEvent::char(c), 0);
    }
    assert!(a.autocomplete().is_open());
    let screen = render(&a, 40, 12);
    // The query text in the bar and the popup candidate both render.
    assert!(screen.contains("SELECT st"), "query bar, screen:\n{screen}");
    assert!(
        screen.contains("status"),
        "popup candidate overlaid, screen:\n{screen}"
    );
}

#[test]
fn query_bar_renders_a_visible_cursor_cell() {
    // The textarea paints a reverse-video cursor cell into the buffer (headless-snapshotable),
    // unlike the old hand-rolled bar which drew plain text with no cursor.
    let mut a = app();
    a.on_loaded("ready");
    a.on_key(KeyEvent::plain(Key::Paste("SELECT 42".into())), 0);
    assert!(
        has_reversed_cell(&a, 40, 6),
        "expected a reverse-video cursor cell in the query bar"
    );
}

#[test]
fn empty_query_bar_still_shows_a_cursor() {
    // Even with no text typed, the cursor cell is visible at the start of the (empty) bar.
    let mut a = app();
    a.on_loaded("ready");
    assert!(
        has_reversed_cell(&a, 40, 6),
        "expected a cursor cell even in an empty query bar"
    );
}

#[test]
fn enter_inserts_newline_and_bar_grows_a_row() {
    let mut a = app();
    a.on_loaded("ready");
    // Build a two-line query entirely in Insert mode (the default on focus). Enter inserts a
    // newline in Insert mode (the locked decision); an autocomplete popup, if any, overlays the
    // results pane ABOVE the bar, so it never crowds the bar rows this test inspects. (We avoid Esc
    // here on purpose: with the vim bar, Esc would drop to Normal mode where Enter is the `j`
    // motion, not a newline.)
    for c in "SELECT *".chars() {
        a.on_key(KeyEvent::char(c), 0);
    }
    a.on_key(KeyEvent::plain(Key::Enter), 0); // newline
    for c in "FROM t".chars() {
        a.on_key(KeyEvent::char(c), 0);
    }
    // The query now contains a newline (text() joins lines with \n).
    assert!(a.query().contains('\n'), "query: {:?}", a.query());

    // The query bar occupies two rows now: both line fragments render, on adjacent rows, in the
    // lower portion of the frame (status line is the very last row).
    let h = 10u16;
    let screen = render(&a, 40, h);
    let lines: Vec<&str> = screen.lines().collect();
    // Match the prompt-prefixed bar line (`> SELECT *`) — the empty-state hint above also contains
    // "SELECT *", but only the bar's first line carries the `> ` prompt.
    let row_a = lines
        .iter()
        .position(|l| l.contains("> SELECT *"))
        .expect("first query line");
    let row_b = lines
        .iter()
        .position(|l| l.contains("FROM t") && !l.contains("query"))
        .expect("second query line");
    assert_eq!(row_b, row_a + 1, "second line directly below the first");
    // Box layout: top border, line 1, line 2, bottom border (help hints), status row. So the
    // second query line is the third-to-last row (bottom border + status are below it).
    assert_eq!(
        row_b,
        lines.len() - 3,
        "the multiline query text sits just above the box bottom border + status, screen:\n{screen}"
    );
}

/// Count cells in the rendered buffer whose modifier carries `Modifier::DIM`. A stale-dimmed grid
/// (`result_is_stale == true`) paints every header + body cell with this modifier, so the count
/// strictly increases vs the un-dimmed render of the same grid.
fn count_dim_cells(app: &App, w: u16, h: u16) -> usize {
    let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
    t.draw(|f| app.render(f)).unwrap();
    let buffer = t.backend().buffer();
    buffer
        .content()
        .iter()
        .filter(|cell| cell.modifier.contains(Modifier::DIM))
        .count()
}

/// An int-only result so the rendered grid carries no NULL cells (which already use DIM): every DIM
/// cell counted in the buffer comes purely from the stale-result dim, not from null styling.
fn int_only_result(rows: usize) -> ProcessedResult {
    let cells: Vec<Cell> = (0..rows as i64).map(Cell::Int).collect();
    let table = Table::new(vec![Column::new("id", ColumnType::Int, cells)]);
    let s = table.schema();
    ProcessedResult::new(table, s, 0)
}

#[test]
fn engine_error_keeps_grid_visible_and_dims_its_cells() {
    // Render a successful int-only grid (no NULL cells, so any DIM later is from stale-dim alone),
    // then deliver an engine Error. The rendered grid must STILL be present (the rows survive) and
    // its cells must now carry the DIM modifier — jiq's keep-last-result-dimmed behavior.
    let mut a = app();
    a.on_loaded("ready");
    for c in "SELECT id FROM t".chars() {
        a.on_key(KeyEvent::char(c), 0);
    }
    a.on_key(KeyEvent::plain(Key::Esc), 0); // dismiss the autocomplete popup
    a.tick(150);
    let id = a.latest_request_id();
    a.on_response(QueryResponse::ProcessedSuccess {
        result: int_only_result(3),
        request_id: id,
        kind: crate::query::worker::types::RequestKind::Main,
    });
    let dim_before = count_dim_cells(&a, 40, 10);
    let screen_before = render(&a, 40, 10);
    assert!(
        screen_before.contains("id"),
        "header pre-error, screen:\n{screen_before}"
    );

    // Now type-edit and deliver an engine Error for the new dispatch.
    for c in "x".chars() {
        a.on_key(KeyEvent::char(c), 200);
    }
    a.on_key(KeyEvent::plain(Key::Esc), 200); // dismiss the autocomplete popup
    a.tick(400);
    let id2 = a.latest_request_id();
    a.on_response(QueryResponse::Error {
        message: "Binder Error: Referenced column \"foo\" not found".into(),
        request_id: id2,
        kind: crate::query::worker::types::RequestKind::Main,
    });

    let screen_after = render(&a, 40, 10);
    let dim_after = count_dim_cells(&a, 40, 10);
    // The grid rows are still visible (the header survives the error).
    assert!(
        screen_after.contains("id"),
        "header still visible post-error, screen:\n{screen_after}"
    );
    // The error message is in the status line.
    assert!(
        screen_after.contains("unknown column"),
        "status carries the error, screen:\n{screen_after}"
    );
    // The DIM-cell count strictly increases — the stale-dim was applied to the kept grid.
    assert!(
        dim_after > dim_before,
        "stale grid must have more DIM cells: before={dim_before}, after={dim_after}, screen:\n{screen_after}"
    );
}

#[test]
fn preprocess_reject_keeps_grid_visible_and_dims_its_cells() {
    // Same shape as the engine-error test, but for the preprocess-reject path (a multi-statement
    // bar). The kept grid must dim too; the "read-only"/multi-statement message rides the status.
    let mut a = app();
    a.on_loaded("ready");
    for c in "SELECT id FROM t".chars() {
        a.on_key(KeyEvent::char(c), 0);
    }
    a.on_key(KeyEvent::plain(Key::Esc), 0);
    a.tick(150);
    let id = a.latest_request_id();
    a.on_response(QueryResponse::ProcessedSuccess {
        result: int_only_result(3),
        request_id: id,
        kind: crate::query::worker::types::RequestKind::Main,
    });
    let dim_before = count_dim_cells(&a, 40, 10);

    // Make the bar multi-statement -> preprocess reject (no engine call).
    for c in ";DROP TABLE t".chars() {
        a.on_key(KeyEvent::char(c), 200);
    }
    a.tick(400);

    let screen_after = render(&a, 40, 10);
    let dim_after = count_dim_cells(&a, 40, 10);
    assert!(
        screen_after.contains("id"),
        "header still visible post-reject, screen:\n{screen_after}"
    );
    assert!(
        screen_after.contains("statement") || screen_after.contains("read-only"),
        "status carries the preprocess error, screen:\n{screen_after}"
    );
    assert!(
        dim_after > dim_before,
        "stale grid must have more DIM cells: before={dim_before}, after={dim_after}, screen:\n{screen_after}"
    );
}

#[test]
fn successful_response_after_an_error_clears_dim_in_render() {
    // After an error has dimmed the grid, a subsequent success must restore NORMAL polarity.
    let mut a = app();
    a.on_loaded("ready");
    for c in "SELECT id FROM t".chars() {
        a.on_key(KeyEvent::char(c), 0);
    }
    a.on_key(KeyEvent::plain(Key::Esc), 0);
    a.tick(150);
    let id = a.latest_request_id();
    a.on_response(QueryResponse::ProcessedSuccess {
        result: int_only_result(3),
        request_id: id,
        kind: crate::query::worker::types::RequestKind::Main,
    });
    let dim_baseline = count_dim_cells(&a, 40, 10);

    // Error -> dim.
    for c in "x".chars() {
        a.on_key(KeyEvent::char(c), 200);
    }
    a.on_key(KeyEvent::plain(Key::Esc), 200);
    a.tick(400);
    let id_err = a.latest_request_id();
    a.on_response(QueryResponse::Error {
        message: "Binder Error: Referenced column \"foo\" not found".into(),
        request_id: id_err,
        kind: crate::query::worker::types::RequestKind::Main,
    });
    let dim_after_err = count_dim_cells(&a, 40, 10);
    assert!(dim_after_err > dim_baseline);

    // Success -> dim drops back to baseline (no extra stale-dim cells).
    for c in "y".chars() {
        a.on_key(KeyEvent::char(c), 600);
    }
    a.on_key(KeyEvent::plain(Key::Esc), 600);
    a.tick(800);
    let id_ok = a.latest_request_id();
    a.on_response(QueryResponse::ProcessedSuccess {
        result: int_only_result(2),
        request_id: id_ok,
        kind: crate::query::worker::types::RequestKind::Main,
    });
    let dim_after_ok = count_dim_cells(&a, 40, 10);
    assert!(
        dim_after_ok <= dim_baseline,
        "successful response clears stale-dim: baseline={dim_baseline}, after_err={dim_after_err}, after_ok={dim_after_ok}"
    );
}

#[test]
fn open_palette_overlays_columns_with_checkboxes() {
    use crate::app::SimplePane;
    use crate::schema::{ColumnMeta, Schema};
    // Build a Simple-mode App (the picker is anchored to the SELECT pane in Simple mode).
    let (tx, _rx) = channel();
    let mut a = App::new(tx, InterruptHandle::noop());
    a.set_schema(Schema::new(vec![
        ColumnMeta::new("id", ColumnType::Int),
        ColumnMeta::new("status", ColumnType::Text),
    ]));
    a.on_loaded("ready");
    // Close any post-load autocomplete popup so Ctrl+P actually reaches the palette open path.
    let mut guard = 0;
    while a.autocomplete().is_open() && guard < 4 {
        a.on_key(KeyEvent::new(Key::Esc, crate::app::KeyMods::NONE), 0);
        guard += 1;
    }
    a.query_form_mut().focus(SimplePane::Select);
    let mut guard2 = 0;
    while a.autocomplete().is_open() && guard2 < 4 {
        a.on_key(KeyEvent::new(Key::Esc, crate::app::KeyMods::NONE), 0);
        guard2 += 1;
    }
    // Ctrl+P opens the column palette from the SELECT pane; it overlays the results area with
    // checkbox rows.
    a.on_key(KeyEvent::new(Key::Char('p'), crate::app::KeyMods::CTRL), 0);
    assert!(a.is_palette_open());
    let screen = render(&a, 40, 12);
    assert!(screen.contains("[x]"), "checkbox rows, screen:\n{screen}");
    assert!(screen.contains("status"), "column row, screen:\n{screen}");
    assert!(screen.contains("columns"), "title, screen:\n{screen}");
}

/// Whether any cell in the rendered buffer carries `fg == Color::Rgb(r, g, b)`. Used to prove
/// the bright galaxy palette renders verbatim through `theme::base::*` (a 16-color terminal palette
/// would surface as `Color::Cyan`, not the explicit RGB triple).
fn has_rgb_fg(app: &App, w: u16, h: u16, r: u8, g: u8, b: u8) -> bool {
    let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
    t.draw(|f| app.render(f)).unwrap();
    let buffer = t.backend().buffer();
    buffer
        .content()
        .iter()
        .any(|cell| cell.fg == Color::Rgb(r, g, b))
}

#[test]
fn bright_galaxy_palette_lands_in_the_buffer() {
    // The bright cyan accent (`Color::Rgb(0, 217, 255)`) must surface in the rendered buffer:
    // the focused query box border and/or the help-bar key style both reach for it. Proves the
    // theme rewrite swapped the legacy palette colors (`Color::Cyan`, `Color::DarkGray`) for the
    // verbatim galaxy RGB triples.
    let mut a = app();
    a.on_loaded("ready");
    a.on_key(KeyEvent::plain(Key::Paste("SELECT 42".into())), 0);
    a.on_key(KeyEvent::plain(Key::Esc), 0); // dismiss the autocomplete popup so it doesn't repaint
    assert!(
        has_rgb_fg(&a, 60, 10, 0, 217, 255),
        "bright cyan (0,217,255) must appear somewhere on screen"
    );
    // The TEXT color (236,236,244) drives the cell + description styles — should also land.
    assert!(
        has_rgb_fg(&a, 60, 10, 236, 236, 244),
        "TEXT (236,236,244) must appear (description + cell text)"
    );
}

#[test]
fn focused_query_box_border_uses_bright_cyan() {
    // With focus on the query bar (the default), the box border carries the focused style — the
    // bright-cyan border RGB. Switching focus to the results pane swaps the colors: the box border
    // dims, the results pane border picks up the bright cyan.
    let mut a = app();
    a.on_loaded("ready");
    // QueryBar focused -> the box border is bright cyan.
    let mut t = Terminal::new(TestBackend::new(60, 10)).unwrap();
    t.draw(|f| a.render(f)).unwrap();
    let buf = t.backend().buffer();
    // The box's top-left corner cell carries the border style. With single-line query, box
    // occupies rows h-3..=h-1; top-left corner is at column 0, row h-3 = 7.
    let corner = buf.cell((0, 7)).unwrap();
    let focused_fg = match theme::border::focused().fg {
        Some(c) => c,
        None => panic!("focused border style must carry an fg color"),
    };
    let unfocused_fg = match theme::border::unfocused().fg {
        Some(c) => c,
        None => panic!("unfocused border style must carry an fg color"),
    };
    assert_eq!(
        corner.fg, focused_fg,
        "focused query-box border carries the focused-cyan fg"
    );
    assert_ne!(
        focused_fg, unfocused_fg,
        "focused vs unfocused borders must differ in color"
    );
    assert_eq!(a.focus(), Focus::QueryBar, "default focus is the query bar");
}

#[test]
fn unfocused_query_box_border_uses_muted_slate() {
    // Hand focus to the results pane (Down from the query bar with one line, after dismissing the
    // autocomplete popup). The query-box border now uses the unfocused (muted) style; the results
    // pane border uses focused.
    let mut a = app();
    a.on_loaded("ready");
    // Push a result on so navigation lands inside the grid (otherwise empty-state takes focus).
    for c in "SELECT * FROM t".chars() {
        a.on_key(KeyEvent::char(c), 0);
    }
    a.on_key(KeyEvent::plain(Key::Esc), 0); // dismiss popup -> Normal mode
    // Esc again drops to a state where re-running might re-open; force Insert mode and dismiss
    // popup before the focus transfer.
    a.tick(150);
    let id = a.latest_request_id();
    a.on_response(QueryResponse::ProcessedSuccess {
        result: result(),
        request_id: id,
        kind: crate::query::worker::types::RequestKind::Main,
    });
    // After Esc the editor is Normal; press 'i' to return to Insert, then dismiss any popup the
    // refresh re-opened, then Down to hand focus to the results pane.
    a.on_key(KeyEvent::char('i'), 0);
    if a.autocomplete().is_open() {
        a.on_key(KeyEvent::plain(Key::Esc), 0);
        a.on_key(KeyEvent::char('i'), 0); // back to Insert
    }
    if a.autocomplete().is_open() {
        a.on_key(KeyEvent::plain(Key::Esc), 0);
    }
    a.on_key(KeyEvent::plain(Key::Down), 0); // hand focus to the results pane
    assert_eq!(a.focus(), Focus::Results);

    let mut t = Terminal::new(TestBackend::new(60, 10)).unwrap();
    t.draw(|f| a.render(f)).unwrap();
    let buf = t.backend().buffer();
    let focused_fg = theme::border::focused().fg.unwrap();
    let unfocused_fg = theme::border::unfocused().fg.unwrap();
    // Query box top-left corner at row 7 — should now be the unfocused color.
    let qbox_corner = buf.cell((0, 7)).unwrap();
    assert_eq!(
        qbox_corner.fg, unfocused_fg,
        "unfocused query-box border uses the muted slate"
    );
    // The results pane top-left corner sits at row 0 col 0 — focused now.
    let results_corner = buf.cell((0, 0)).unwrap();
    assert_eq!(
        results_corner.fg, focused_fg,
        "focused results pane border uses the bright cyan"
    );
}
