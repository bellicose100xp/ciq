//! Render tests for the app layer — drives `App::render` through a `ratatui::TestBackend` and
//! asserts on the in-memory cell buffer (headless; no real TTY). Covers each phase's results
//! pane and the three-region layout (query bar / results / status).

use std::sync::mpsc::channel;

use ratatui::Terminal;
use ratatui::backend::TestBackend;

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
fn schema_bar_and_summary_render_above_grid() {
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
    // Dismiss the autocomplete popup so it doesn't overlay the schema bar in this render.
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
    // The schema bar shows name + type badge above the grid.
    assert!(
        screen.contains("id (int)"),
        "schema-bar id badge, screen:\n{screen}"
    );
    assert!(
        screen.contains("name (txt)"),
        "schema-bar name badge, screen:\n{screen}"
    );
    // The grid still renders its body cells below the bar (one extra reserved row didn't crowd
    // them out).
    assert!(screen.contains("ada"), "grid cell value, screen:\n{screen}");
    assert!(screen.contains("2 rows"), "status, screen:\n{screen}");

    // The schema bar's `id (int)` lands ABOVE the grid's bare `id` header row: the first line on
    // which the decorated badge appears precedes the first line on which the bare header appears.
    let bar_line = screen
        .lines()
        .position(|l| l.contains("id (int)"))
        .expect("schema bar line");
    let header_line = screen
        .lines()
        .position(|l| l.contains("id") && !l.contains("id (int)") && !l.contains("SELECT"))
        .expect("grid header line");
    assert!(
        bar_line < header_line,
        "schema bar (line {bar_line}) above grid header (line {header_line}), screen:\n{screen}"
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
