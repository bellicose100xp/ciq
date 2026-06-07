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
    });
    let screen = render(&a, 40, 10);
    assert!(screen.contains("id"), "header id, screen:\n{screen}");
    assert!(screen.contains("name"), "header name, screen:\n{screen}");
    assert!(screen.contains("ada"), "cell value, screen:\n{screen}");
    assert!(screen.contains("NULL"), "null glyph, screen:\n{screen}");
    assert!(screen.contains("2 rows"), "status, screen:\n{screen}");
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
