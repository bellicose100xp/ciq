//! Tests for the bottom keyboard-shortcut help bar (`dev/PLAN.md` §4.1).
//!
//! Two layers, matching the module's purity split:
//!  - the pure [`get_context_hints`] / [`mode_label`] are table-tested — one row per
//!    focus / vim-mode / open-popup context (the hard-floor contract);
//!  - [`render_line`] is exercised through a `ratatui::TestBackend` snapshot (headless): the right
//!    hints + the mode badge land on the bottom row, and a narrow terminal drops trailing hints
//!    rather than overflowing.

use std::sync::mpsc::channel;

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;

use crate::app::help_line::{get_context_hints, mode_label, render_line};
use crate::app::{App, Key, KeyEvent, KeyMods};
use crate::engine::InterruptHandle;
use crate::schema::{ColumnMeta, ColumnType, Schema};

fn app() -> App {
    let (tx, _rx) = channel();
    let mut app = App::new(tx, InterruptHandle::noop());
    // Help-bar hint tests assert on Power-mode chord set (the bulk surface). Simple-mode hints
    // (which lead with `Alt+↑↓ panes` and use `Tab \t`) have their own dedicated tests.
    app.force_power_mode_for_tests("");
    app
}

fn test_schema() -> Schema {
    Schema::new(vec![
        ColumnMeta::new("id", ColumnType::Int),
        ColumnMeta::new("status", ColumnType::Text),
    ])
}

/// An App loaded + ready with a schema (so the popups/palette have their candidate source).
fn loaded_app() -> App {
    let mut a = app();
    a.set_schema(test_schema());
    a.on_loaded("ready");
    a
}

/// Drive `a` into vim Normal mode with the autocomplete popup closed — the state where the
/// Normal-mode query-bar hints actually show. With a schema loaded, a query-bar edit re-opens the
/// autocomplete popup (almost every cursor context has candidates), and an open popup intercepts
/// keys ahead of the focus routing — so a single `Esc` only ever closes the popup. The deterministic
/// path: type a query (popup open), then `Esc` until the editor is Normal *and* the popup is closed.
fn query_bar_normal_no_popup() -> App {
    let mut a = loaded_app();
    for c in "SELECT * FROM t".chars() {
        a.on_key(KeyEvent::char(c), 0);
    }
    // Each Esc either closes the open popup (staying in the current mode) or, with the popup closed,
    // drops Insert -> Normal (which re-opens the popup via the post-edit refresh). Press Esc until
    // the editor is Normal *and* the popup is closed (it converges in a few presses).
    use crate::app::editor::EditorMode;
    for _ in 0..8 {
        if a.editor_mode() == EditorMode::Normal && !a.autocomplete().is_open() {
            break;
        }
        a.on_key(KeyEvent::plain(Key::Esc), 0);
    }
    assert_eq!(a.editor_mode(), EditorMode::Normal);
    assert!(
        !a.autocomplete().is_open(),
        "popup must be closed for Normal-mode hints"
    );
    a
}

/// Render only the help bar over a one-row area `width` wide and return the bottom row's text.
fn render_help(app: &App, width: u16) -> String {
    let mut t = Terminal::new(TestBackend::new(width, 1)).unwrap();
    t.draw(|f| render_line(app, f, Rect::new(0, 0, width, 1)))
        .unwrap();
    t.backend().to_string()
}

/// Whether `hints` contains a pair whose key is `key`.
fn has_key(hints: &[(&'static str, &'static str)], key: &str) -> bool {
    hints.iter().any(|(k, _)| *k == key)
}

// --- context hint table (pure) ---

#[test]
fn query_bar_insert_mode_hints() {
    let app = loaded_app(); // defaults to QueryBar + Insert (Power, since the test helper forces it)
    let hints = get_context_hints(&app);
    assert!(has_key(&hints, "Tab"), "Tab chord: {hints:?}");
    assert!(
        has_key(&hints, "Ctrl+P"),
        "columns palette chord: {hints:?}"
    );
    assert!(has_key(&hints, "Ctrl+A"), "AI chord: {hints:?}");
    assert!(has_key(&hints, "Ctrl+R"), "history chord: {hints:?}");
    assert!(has_key(&hints, "Ctrl+T"), "focus-toggle chord: {hints:?}");
    assert!(has_key(&hints, "Ctrl+C"), "quit chord: {hints:?}");
    // The mode badge says INSERT.
    assert_eq!(mode_label(&app).as_deref(), Some("INSERT"));
}

#[test]
fn query_bar_normal_mode_hints() {
    let app = query_bar_normal_no_popup();
    let hints = get_context_hints(&app);
    assert!(has_key(&hints, "hjkl"), "vim motion hint: {hints:?}");
    assert!(has_key(&hints, "i"), "insert-mode hint: {hints:?}");
    assert!(
        has_key(&hints, "Ctrl+P"),
        "columns reachable in Normal: {hints:?}"
    );
    assert!(
        has_key(&hints, "Ctrl+T"),
        "focus-toggle reachable in Normal: {hints:?}"
    );
    // No live "complete" affordance in Normal mode (the popup is an Insert-mode concern).
    assert!(
        !has_key(&hints, "Tab"),
        "no Tab-complete in Normal: {hints:?}"
    );
    assert_eq!(mode_label(&app).as_deref(), Some("NORMAL"));
}

#[test]
fn results_pane_hints() {
    let mut app = loaded_app();
    // A trailing space lists all columns and opens the autocomplete popup; dismiss it, then hand
    // focus to the results pane (Down from the single last line).
    for c in "SELECT 1".chars() {
        app.on_key(KeyEvent::char(c), 0);
    }
    if app.autocomplete().is_open() {
        app.on_key(KeyEvent::plain(Key::Esc), 0);
    }
    app.on_key(KeyEvent::plain(Key::Down), 0); // hands off to Results
    assert_eq!(app.focus(), crate::app::Focus::Results);
    let hints = get_context_hints(&app);
    assert!(has_key(&hints, "Up/Down"), "scroll hint: {hints:?}");
    assert!(has_key(&hints, "PgUp/PgDn"), "page hint: {hints:?}");
    assert!(
        has_key(&hints, "Left/Right"),
        "column scroll hint: {hints:?}"
    );
    assert!(has_key(&hints, "f"), "facet chord: {hints:?}");
    assert!(
        has_key(&hints, "Ctrl+T"),
        "focus-toggle to query bar: {hints:?}"
    );
    assert!(has_key(&hints, "Ctrl+C"), "quit chord: {hints:?}");
    // No mode badge in the results pane (no editing mode applies there).
    assert_eq!(mode_label(&app), None);
}

#[test]
fn autocomplete_popup_hints() {
    let mut app = loaded_app();
    for c in "SELECT st".chars() {
        app.on_key(KeyEvent::char(c), 0);
    }
    assert!(app.autocomplete().is_open());
    let hints = get_context_hints(&app);
    assert!(has_key(&hints, "Tab"), "complete chord: {hints:?}");
    assert!(has_key(&hints, "Up/Down"), "select chord: {hints:?}");
    assert!(has_key(&hints, "Esc"), "dismiss chord: {hints:?}");
}

#[test]
fn palette_popup_hints() {
    let mut app = loaded_app();
    app.on_key(KeyEvent::new(Key::Char('p'), KeyMods::CTRL), 0);
    assert!(app.is_palette_open());
    let hints = get_context_hints(&app);
    assert!(has_key(&hints, "Space"), "toggle chord: {hints:?}");
    assert!(has_key(&hints, "Left/Right"), "reorder chord: {hints:?}");
    assert!(has_key(&hints, "Enter"), "apply chord: {hints:?}");
    assert!(has_key(&hints, "Esc"), "close chord: {hints:?}");
}

#[test]
fn history_popup_hints() {
    let mut app = loaded_app();
    app.on_key(KeyEvent::new(Key::Char('r'), KeyMods::CTRL), 0);
    assert!(app.is_history_open());
    let hints = get_context_hints(&app);
    assert!(has_key(&hints, "Up/Down"), "select chord: {hints:?}");
    assert!(has_key(&hints, "Enter"), "recall chord: {hints:?}");
    assert!(has_key(&hints, "Esc"), "close chord: {hints:?}");
}

#[test]
fn ai_popup_hints() {
    let mut app = loaded_app();
    let (tx, _rx) = channel();
    app.set_ai_channel(tx); // wire the AI feature so Ctrl+A opens the popup
    app.on_key(KeyEvent::new(Key::Char('a'), KeyMods::CTRL), 0);
    assert!(app.is_ai_open());
    let hints = get_context_hints(&app);
    assert!(has_key(&hints, "Enter"), "generate chord: {hints:?}");
    assert!(has_key(&hints, "Esc"), "close chord: {hints:?}");
}

#[test]
fn facet_popup_hints() {
    use crate::app::Focus;
    use crate::engine::types::{Cell, Column, Table};
    use crate::query::worker::types::{ProcessedResult, QueryResponse, RequestKind};
    use crate::schema::ColumnType;

    // Keep the request receiver alive so the facet dispatch (which rides the worker channel)
    // succeeds — `loaded_app` drops it, which would silently no-op `open_facet`.
    let (tx, _rx) = channel();
    let mut app = App::new(tx, InterruptHandle::noop());
    app.force_power_mode_for_tests("");
    app.set_schema(test_schema());
    app.on_loaded("ready");
    // Put a result on screen so the `f` chord has a focused column to facet (`id` resolves against
    // test_schema), then move focus to the results pane and press `f`.
    for c in "SELECT * FROM t".chars() {
        app.on_key(KeyEvent::char(c), 0);
    }
    app.tick(150);
    let id = app.latest_request_id();
    let table = Table::new(vec![Column::new(
        "id",
        ColumnType::Int,
        vec![Cell::Int(1), Cell::Int(2)],
    )]);
    let schema = table.schema();
    app.on_response(QueryResponse::ProcessedSuccess {
        result: ProcessedResult::new(table, schema, 0),
        request_id: id,
        kind: RequestKind::Main,
    });
    if app.autocomplete().is_open() {
        app.on_key(KeyEvent::plain(Key::Esc), 0);
    }
    app.on_key(KeyEvent::plain(Key::Down), 0); // focus results
    assert_eq!(app.focus(), Focus::Results);
    app.on_key(KeyEvent::char('f'), 0); // open the facet popup
    assert!(app.is_facet_open(), "the `f` chord opened a facet");

    let hints = get_context_hints(&app);
    // The facet-open context shows Esc/Ctrl+C (and NOT the results-pane or query-bar hints).
    assert!(has_key(&hints, "Esc"), "facet close chord: {hints:?}");
    assert!(has_key(&hints, "Ctrl+C"), "facet quit chord: {hints:?}");
    assert!(
        !has_key(&hints, "Tab") && !has_key(&hints, "PgUp/PgDn"),
        "the facet branch fired, not the query-bar or results-pane branch: {hints:?}"
    );
    // No mode badge when a facet popup is open (the query bar is not the focused editing surface).
    assert_eq!(mode_label(&app), None);
}

// --- render (snapshot) ---

#[test]
fn render_shows_a_hint() {
    // The help row no longer carries the mode badge — that lives on the query box's TOP border
    // (`app_render::render_query_box`). The hints themselves still render with their bullet.
    let app = loaded_app();
    let row = render_help(&app, 80);
    assert!(
        !row.contains("INSERT"),
        "mode badge does NOT live on the help row anymore:\n{row}"
    );
    assert!(
        row.contains("complete"),
        "a hint description renders:\n{row}"
    );
    assert!(
        row.contains('\u{2022}'),
        "the bullet separator renders:\n{row}"
    );
}

#[test]
fn render_updates_with_mode_hints() {
    // Switching vim mode swaps the hint set (Normal-mode `move` replaces Insert-mode `complete`),
    // even though the badge itself rides the box top border instead of the help row.
    let app = query_bar_normal_no_popup();
    let row = render_help(&app, 80);
    assert!(
        !row.contains("NORMAL"),
        "mode badge does NOT live on the help row:\n{row}"
    );
    assert!(row.contains("move"), "a vim hint renders:\n{row}");
}

#[test]
fn render_drops_trailing_hints_on_a_narrow_terminal() {
    let app = loaded_app();
    // Wide enough for the first hint or two; the low-priority trailing hints must be dropped,
    // not clipped mid-word. (The mode badge no longer competes for the row's width.)
    let row = render_help(&app, 22);
    assert!(
        row.contains("Tab"),
        "the highest-priority hint survives:\n{row}"
    );
    assert!(
        !row.contains("quit"),
        "a low-priority trailing hint is dropped on a narrow bar:\n{row}"
    );
}

#[test]
fn render_no_panic_on_zero_or_tiny_width() {
    // Degenerate widths must never panic and must never clip a partial word.
    for w in [1u16, 2, 3, 5] {
        let _ = render_help(&loaded_app(), w);
    }
}

/// Pair lookup for the description (paired with `has_key`).
fn pair_for<'a>(
    hints: &'a [(&'static str, &'static str)],
    key: &str,
) -> Option<&'a (&'static str, &'static str)> {
    hints.iter().find(|(k, _)| *k == key)
}

#[test]
fn simple_mode_query_bar_insert_hints_show_pane_nav_and_tab_tab() {
    // Simple is the production default; build the App without forcing Power.
    let (tx, _rx) = channel();
    let mut app = App::new(tx, InterruptHandle::noop());
    app.set_schema(test_schema());
    app.on_loaded("ready");
    // Close any popup that may be open from the post-load refresh so the bar's bottom hints
    // reflect Simple+Insert, not the popup-open context. Esc with the popup open closes the
    // popup without changing vim mode.
    let mut guard = 0;
    while app.autocomplete().is_open() && guard < 4 {
        app.on_key(KeyEvent::new(Key::Esc, KeyMods::NONE), 0);
        guard += 1;
    }
    assert!(
        !app.autocomplete().is_open(),
        "popup must be closed for the bare Insert-mode hints"
    );
    let hints = get_context_hints(&app);
    // Pane-nav lives on Alt+Up/Down (the leading hint); Tab inserts a literal tab.
    assert!(
        has_key(&hints, "Alt+\u{2191}\u{2193}"),
        "Simple-mode pane-nav chord: {hints:?}"
    );
    let tab = pair_for(&hints, "Tab").expect("Tab present in Simple-mode Insert hints");
    assert_eq!(
        tab.1, "\\t",
        "Simple-mode Tab inserts a literal tab now, not 'next pane': {hints:?}"
    );
    assert!(
        has_key(&hints, "Ctrl+Q"),
        "Ctrl+Q (SQL toggle) chord present: {hints:?}"
    );
}

#[test]
fn autocomplete_popup_hint_descriptions_say_accept_select_close() {
    let mut app = loaded_app();
    for c in "SELECT st".chars() {
        app.on_key(KeyEvent::char(c), 0);
    }
    assert!(app.autocomplete().is_open());
    let hints = get_context_hints(&app);
    let tab = pair_for(&hints, "Tab").expect("Tab present in popup-open hints");
    assert_eq!(tab.1, "accept", "Tab description: {hints:?}");
    let updown = pair_for(&hints, "Up/Down").expect("Up/Down present");
    assert_eq!(updown.1, "select", "Up/Down description: {hints:?}");
    let esc = pair_for(&hints, "Esc").expect("Esc present");
    assert_eq!(esc.1, "close", "Esc description: {hints:?}");
}
