//! Render tests for the app layer — drives `App::render` through a `ratatui::TestBackend` and
//! asserts on the in-memory cell buffer (headless; no real TTY). Covers each phase's results
//! pane and the three-region layout (query bar / results / status).

use std::sync::mpsc::channel;

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::style::Modifier;

use crate::app::{App, Key, KeyEvent};
use crate::engine::InterruptHandle;
use crate::engine::types::{Cell, Column, Table};
use crate::query::worker::types::{ProcessedResult, QueryResponse};
use crate::schema::ColumnType;

fn app() -> App {
    let (tx, _rx) = channel();
    App::new(tx, InterruptHandle::noop())
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
fn capped_result_renders_truncation_banner() {
    use crate::app::VIEWPORT_ROW_LIMIT;
    // Keep the worker receiver alive so the dispatch succeeds and the ciq-capped flag is recorded
    // (the banner is gated on a *successful* dispatch having applied ciq's viewport LIMIT — a
    // dropped receiver fails the send and never records the flag).
    let (tx, _rx) = std::sync::mpsc::channel();
    let mut a = App::new(tx, InterruptHandle::noop());
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
    assert!(
        screen.contains("showing first 1000 rows"),
        "truncation banner, screen:\n{screen}"
    );
    // The grid still renders its header below the banner.
    assert!(screen.contains("id"), "grid header, screen:\n{screen}");
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
    // Bar is the second-to-last row (status line is the very last); definitively in the bottom
    // half, not row 0 as it used to be.
    assert_eq!(
        bar_line,
        lines.len() - 2,
        "query bar is the second-to-last row (status below it), screen:\n{screen}"
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
    // Two query rows + a status row at the bottom: the second query line is the third-to-last row.
    assert_eq!(
        row_b,
        lines.len() - 2,
        "the multiline bar sits just above the status line, screen:\n{screen}"
    );
}

#[test]
fn open_palette_overlays_columns_with_checkboxes() {
    use crate::schema::{ColumnMeta, Schema};
    let mut a = app();
    a.set_schema(Schema::new(vec![
        ColumnMeta::new("id", ColumnType::Int),
        ColumnMeta::new("status", ColumnType::Text),
    ]));
    a.on_loaded("ready");
    // Ctrl+K opens the column palette; it overlays the results area with checkbox rows.
    a.on_key(KeyEvent::new(Key::Char('k'), crate::app::KeyMods::CTRL), 0);
    assert!(a.is_palette_open());
    let screen = render(&a, 40, 12);
    assert!(screen.contains("[ ]"), "checkbox rows, screen:\n{screen}");
    assert!(screen.contains("status"), "column row, screen:\n{screen}");
    assert!(screen.contains("columns"), "title, screen:\n{screen}");
}
