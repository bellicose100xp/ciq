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
    // (which lead with `Alt+↑↓ panes` and gate `Ctrl+P` to the SELECT pane) have their own
    // dedicated tests below.
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
    // Tab-complete is an intuitive autocomplete idiom — no longer surfaced.
    assert!(
        !has_key(&hints, "Tab"),
        "no Tab hint (intuitive autocomplete idiom): {hints:?}"
    );
    // Ctrl+P (columns palette) is anchored to the SELECT pane in Simple mode now; in Power mode
    // it's not a hint at all. So this Power-mode test should NOT find it.
    assert!(
        !has_key(&hints, "Ctrl+P"),
        "no Ctrl+P hint in Power mode: {hints:?}"
    );
    assert!(has_key(&hints, "Ctrl+A"), "AI chord: {hints:?}");
    assert!(has_key(&hints, "Ctrl+R"), "history chord: {hints:?}");
    assert!(has_key(&hints, "Ctrl+T"), "focus-toggle chord: {hints:?}");
    assert!(has_key(&hints, "Ctrl+Q"), "SQL-mode chord: {hints:?}");
    assert!(has_key(&hints, "Ctrl+C"), "quit chord: {hints:?}");
    // The mode badge announces INSERT — the bottom hints no longer carry an `Esc vim` hint.
    assert!(
        !has_key(&hints, "Esc"),
        "no `Esc vim` hint (mode badge announces it): {hints:?}"
    );
    assert_eq!(mode_label(&app).as_deref(), Some("INSERT"));
}

#[test]
fn query_bar_normal_mode_hints() {
    let app = query_bar_normal_no_popup();
    let hints = get_context_hints(&app);
    // hjkl/i are obvious to vim users and the TOP-border badge already announces NORMAL — the
    // Normal-mode hint set now carries only the non-obvious feature chords.
    assert!(
        !has_key(&hints, "hjkl"),
        "no hjkl hint (obvious vim motion): {hints:?}"
    );
    assert!(
        !has_key(&hints, "i"),
        "no `i` hint (obvious vim insert): {hints:?}"
    );
    // Ctrl+P (columns palette) is anchored to SELECT-pane focus in Simple mode; it's not in the
    // generic Normal-mode hint set anymore.
    assert!(
        !has_key(&hints, "Ctrl+P"),
        "no Ctrl+P in Normal hints (anchored to SELECT pane): {hints:?}"
    );
    assert!(
        has_key(&hints, "Ctrl+R"),
        "history reachable in Normal: {hints:?}"
    );
    assert!(
        has_key(&hints, "Ctrl+T"),
        "focus-toggle reachable in Normal: {hints:?}"
    );
    assert!(has_key(&hints, "Ctrl+C"), "quit chord: {hints:?}");
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
    // Arrow/PgUp-PgDn/Home scrolling is intuitive — those hints were dropped. Only the
    // non-obvious chords remain.
    assert!(
        !has_key(&hints, "Up/Down"),
        "no scroll hint (intuitive): {hints:?}"
    );
    assert!(
        !has_key(&hints, "PgUp/PgDn"),
        "no page hint (intuitive): {hints:?}"
    );
    assert!(
        !has_key(&hints, "Left/Right"),
        "no column-scroll hint (intuitive): {hints:?}"
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
    // Tab-accept, ↑↓-select, Esc-close are intuitive autocomplete idioms — the popup-open
    // context carries only the universal quit chord.
    assert!(
        !has_key(&hints, "Tab"),
        "no Tab hint (intuitive): {hints:?}"
    );
    assert!(
        !has_key(&hints, "Up/Down"),
        "no select hint (intuitive): {hints:?}"
    );
    assert!(
        !has_key(&hints, "Esc"),
        "no close hint (intuitive): {hints:?}"
    );
    assert!(has_key(&hints, "Ctrl+C"), "quit chord: {hints:?}");
}

#[test]
fn palette_popup_hints() {
    // The palette popup is anchored to the SELECT pane in Simple mode; build a Simple-mode App
    // and focus SELECT so Ctrl+P opens the popup.
    use crate::app::SimplePane;
    let (tx, _rx) = channel();
    let mut app = App::new(tx, InterruptHandle::noop());
    app.set_schema(test_schema());
    app.on_loaded("ready");
    // Close any post-load autocomplete popup so Ctrl+P actually opens the palette.
    let mut guard = 0;
    while app.autocomplete().is_open() && guard < 4 {
        app.on_key(KeyEvent::new(Key::Esc, KeyMods::NONE), 0);
        guard += 1;
    }
    app.query_form_mut().focus(SimplePane::Select);
    // Refresh-on-focus may re-open autocomplete; close it again before opening the palette.
    let mut guard2 = 0;
    while app.autocomplete().is_open() && guard2 < 4 {
        app.on_key(KeyEvent::new(Key::Esc, KeyMods::NONE), 0);
        guard2 += 1;
    }
    app.on_key(KeyEvent::new(Key::Char('p'), KeyMods::CTRL), 0);
    assert!(app.is_palette_open());
    let hints = get_context_hints(&app);
    // Space/Tab-toggle, ↑↓-nav, Enter/Esc-close are intuitive — only the non-obvious bulk ops
    // (plus quit) show.
    assert!(
        !has_key(&hints, "Space/Tab"),
        "no toggle hint (intuitive): {hints:?}"
    );
    assert!(
        !has_key(&hints, "Up/Down"),
        "no nav hint (intuitive): {hints:?}"
    );
    assert!(
        !has_key(&hints, "Enter/Esc"),
        "no close hint (intuitive): {hints:?}"
    );
    assert!(has_key(&hints, "Ctrl+A"), "select-all chord: {hints:?}");
    assert!(has_key(&hints, "Ctrl+X"), "deselect-all chord: {hints:?}");
    assert!(has_key(&hints, "Ctrl+I"), "invert chord: {hints:?}");
    assert!(has_key(&hints, "Ctrl+C"), "quit chord: {hints:?}");
}

#[test]
fn history_popup_hints() {
    let mut app = loaded_app();
    app.on_key(KeyEvent::new(Key::Char('r'), KeyMods::CTRL), 0);
    assert!(app.is_history_open());
    let hints = get_context_hints(&app);
    // ↑↓-select, Enter-recall, type-to-filter, Esc-close are intuitive — only quit shows.
    assert!(
        !has_key(&hints, "Up/Down"),
        "no select hint (intuitive): {hints:?}"
    );
    assert!(
        !has_key(&hints, "Enter"),
        "no recall hint (intuitive): {hints:?}"
    );
    assert!(
        !has_key(&hints, "Esc"),
        "no close hint (intuitive): {hints:?}"
    );
    assert!(has_key(&hints, "Ctrl+C"), "quit chord: {hints:?}");
}

#[test]
fn ai_popup_hints() {
    let mut app = loaded_app();
    let (tx, _rx) = channel();
    app.set_ai_channel(tx); // wire the AI feature so Ctrl+A opens the popup
    app.on_key(KeyEvent::new(Key::Char('a'), KeyMods::CTRL), 0);
    assert!(app.is_ai_open());
    let hints = get_context_hints(&app);
    // Enter-generate and Esc-close are intuitive — only quit shows.
    assert!(
        !has_key(&hints, "Enter"),
        "no generate hint (intuitive): {hints:?}"
    );
    assert!(
        !has_key(&hints, "Esc"),
        "no close hint (intuitive): {hints:?}"
    );
    assert!(has_key(&hints, "Ctrl+C"), "quit chord: {hints:?}");
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
    // The facet-open context shows only the universal quit chord (Esc-close is intuitive and was
    // dropped), and NOT the results-pane or query-bar hints.
    assert!(
        !has_key(&hints, "Esc"),
        "no facet close hint (intuitive): {hints:?}"
    );
    assert!(has_key(&hints, "Ctrl+C"), "facet quit chord: {hints:?}");
    assert!(
        !has_key(&hints, "Tab") && !has_key(&hints, "PgUp/PgDn") && !has_key(&hints, "f"),
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
        row.contains("history"),
        "a hint description renders:\n{row}"
    );
    assert!(
        row.contains('\u{2022}'),
        "the bullet separator renders:\n{row}"
    );
}

#[test]
fn render_updates_with_mode_hints() {
    // Switching vim mode swaps the hint set. Both Insert and Normal now share the feature chords
    // (the obvious motion/typing keys are dropped), so the assertion is just that the bar renders
    // a feature hint and not the mode badge (which rides the box top border).
    let app = query_bar_normal_no_popup();
    let row = render_help(&app, 80);
    assert!(
        !row.contains("NORMAL"),
        "mode badge does NOT live on the help row:\n{row}"
    );
    assert!(row.contains("history"), "a feature hint renders:\n{row}");
}

#[test]
fn render_drops_trailing_hints_on_a_narrow_terminal() {
    let app = loaded_app();
    // Wide enough for the first hint or two; the low-priority trailing hints must be dropped,
    // not clipped mid-word. (The mode badge no longer competes for the row's width.) The
    // Power-insert set leads with `Ctrl+A AI` and ends with `Ctrl+C quit`.
    let row = render_help(&app, 22);
    assert!(
        row.contains("AI"),
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
fn simple_mode_query_bar_insert_hints_lead_with_pane_nav() {
    // Simple is the production default; build the App without forcing Power. Default focus is the
    // WHERE pane (cursor parks there on launch).
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
    // Pane-nav lives on Alt+Up/Down (the leading hint).
    assert!(
        has_key(&hints, "Alt+\u{2191}\u{2193}"),
        "Simple-mode pane-nav chord: {hints:?}"
    );
    // The mode badge (TOP border) announces INSERT, so the bottom hints no longer carry
    // `Tab \t` or `Esc vim`.
    assert!(
        !has_key(&hints, "Tab"),
        "no Tab hint in Simple Insert (mode badge announces literal tab): {hints:?}"
    );
    assert!(
        !has_key(&hints, "Esc"),
        "no `Esc vim` hint (mode badge announces it): {hints:?}"
    );
    // Ctrl+P (columns) is anchored to the SELECT pane; default focus is WHERE, so the hint is absent.
    assert!(
        !has_key(&hints, "Ctrl+P"),
        "Ctrl+P columns hint hidden when focus is not on SELECT pane: {hints:?}"
    );
    assert!(
        has_key(&hints, "Ctrl+Q"),
        "Ctrl+Q (SQL toggle) chord present: {hints:?}"
    );
}

#[test]
fn simple_mode_select_pane_focus_shows_ctrl_p_columns_hint() {
    // Ctrl+P is anchored to the SELECT pane; the hint should appear only when SELECT has focus.
    use crate::app::SimplePane;
    let (tx, _rx) = channel();
    let mut app = App::new(tx, InterruptHandle::noop());
    app.set_schema(test_schema());
    app.on_loaded("ready");
    // Close any popup so the bare Insert-mode hint set applies.
    let mut guard = 0;
    while app.autocomplete().is_open() && guard < 4 {
        app.on_key(KeyEvent::new(Key::Esc, KeyMods::NONE), 0);
        guard += 1;
    }
    // Move focus from the default WHERE pane to the SELECT pane via Alt+Up.
    app.on_key(
        KeyEvent::new(
            Key::Up,
            KeyMods {
                alt: true,
                ..KeyMods::NONE
            },
        ),
        0,
    );
    assert_eq!(app.query_form().focused_pane(), SimplePane::Select);
    // The post-focus refresh may re-open the autocomplete popup against the SELECT pane's `*`;
    // close it so the bare Insert-mode hint set applies (popup-open returns its own hints).
    let mut guard2 = 0;
    while app.autocomplete().is_open() && guard2 < 4 {
        app.on_key(KeyEvent::new(Key::Esc, KeyMods::NONE), 0);
        guard2 += 1;
    }
    let hints = get_context_hints(&app);
    assert!(
        has_key(&hints, "Ctrl+P"),
        "Ctrl+P columns hint appears with SELECT focus: {hints:?}"
    );
}

#[test]
fn autocomplete_popup_help_line_carries_only_quit() {
    // The autocomplete popup's intuitive keys (Tab accept / ↑↓ select / Esc close) are not spelled
    // out in the main help-line branch — only the universal quit chord remains. (The popup's own
    // bottom-border renderer surfaces the one CONTEXTUAL `Ctrl+P multi-select` when SELECT-focused;
    // that's covered in autocomplete_render_tests.)
    let mut app = loaded_app();
    for c in "SELECT st".chars() {
        app.on_key(KeyEvent::char(c), 0);
    }
    assert!(app.autocomplete().is_open());
    let hints = get_context_hints(&app);
    assert!(pair_for(&hints, "Tab").is_none(), "no Tab hint: {hints:?}");
    assert!(
        pair_for(&hints, "Up/Down").is_none(),
        "no Up/Down hint: {hints:?}"
    );
    assert!(pair_for(&hints, "Esc").is_none(), "no Esc hint: {hints:?}");
    assert!(has_key(&hints, "Ctrl+C"), "quit chord: {hints:?}");
}
