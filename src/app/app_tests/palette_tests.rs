//! `App`-shell tests for the SELECT-pane column-picker popup (user-locked redesign 2026-06-09):
//! Ctrl+P scope (anchored to the SELECT pane), bidirectional sync (toggle → SELECT pane rewrites
//! immediately + grid filters), and the in-popup keymap (Space/Tab toggle, ↑↓ nav, Enter/Esc
//! close, Ctrl+A all, Ctrl+X none, Ctrl+I invert).

use std::sync::mpsc::{Receiver, channel};

use crate::app::{App, Focus, Key, KeyEvent, KeyMods, QueryMode, SimplePane};
use crate::engine::InterruptHandle;
use crate::query::worker::types::QueryRequest;
use crate::schema::{ColumnMeta, ColumnType, Schema};

// --- fixtures ---

/// A fixed test schema for the palette-popup tests: 4 columns including a keyword-colliding name
/// (`order`) so the quoting path is exercised on emit.
fn test_schema() -> Schema {
    Schema::new(vec![
        ColumnMeta::new("id", ColumnType::Int),
        ColumnMeta::new("status", ColumnType::Text),
        ColumnMeta::new("amount", ColumnType::Float),
        ColumnMeta::new("order", ColumnType::Int),
    ])
}

/// Build a Simple-mode App with the test schema loaded and ready for queries. Default focus is
/// WHERE; tests that need SELECT focus call `app.query_form_mut().focus(SimplePane::Select)`
/// explicitly.
fn loaded_simple_app() -> (App, Receiver<QueryRequest>) {
    let (tx, rx) = channel();
    let mut app = App::new(tx, InterruptHandle::noop());
    app.set_schema(test_schema());
    app.on_loaded("ready");
    // Close any popup that may have opened from the post-load autocomplete refresh; the popup-
    // closed bare-Insert state is what every test below assumes.
    let mut guard = 0;
    while app.autocomplete().is_open() && guard < 4 {
        app.on_key(KeyEvent::new(Key::Esc, KeyMods::NONE), 0);
        guard += 1;
    }
    (app, rx)
}

fn focus_select(app: &mut App) {
    app.query_form_mut().focus(SimplePane::Select);
    // Selecting the SELECT pane re-runs the autocomplete refresh; close it before the test starts
    // sending Ctrl+P so the test can assert on `is_palette_open()`.
    let mut guard = 0;
    while app.autocomplete().is_open() && guard < 4 {
        app.on_key(KeyEvent::new(Key::Esc, KeyMods::NONE), 0);
        guard += 1;
    }
}

fn ctrl(k: Key) -> KeyEvent {
    KeyEvent::new(k, KeyMods::CTRL)
}

fn ctrl_p() -> KeyEvent {
    ctrl(Key::Char('p'))
}

// --- scope: Ctrl+P only opens the popup with focus on the SELECT pane in Simple mode ---

#[test]
fn ctrl_p_opens_popup_when_focus_is_on_select_pane() {
    let (mut app, _rx) = loaded_simple_app();
    focus_select(&mut app);
    assert_eq!(app.query_form().focused_pane(), SimplePane::Select);
    let quit = app.on_key(ctrl_p(), 0);
    assert!(!quit);
    assert!(app.is_palette_open(), "Ctrl+P opens the popup from SELECT");
}

#[test]
fn ctrl_p_is_a_noop_when_focus_is_on_where() {
    let (mut app, _rx) = loaded_simple_app();
    assert_eq!(app.query_form().focused_pane(), SimplePane::Where);
    app.on_key(ctrl_p(), 0);
    assert!(
        !app.is_palette_open(),
        "Ctrl+P is a no-op outside the SELECT pane"
    );
}

#[test]
fn ctrl_p_is_a_noop_in_power_mode() {
    let (mut app, _rx) = loaded_simple_app();
    // Flip to Power.
    app.on_key(ctrl(Key::Char('q')), 0);
    assert_eq!(app.query_form().mode(), QueryMode::Power);
    app.on_key(ctrl_p(), 0);
    assert!(!app.is_palette_open(), "Ctrl+P is a no-op in Power mode");
}

// --- bidirectional sync: toggle rewrites SELECT immediately ---

#[test]
fn opening_popup_pre_checks_against_the_select_pane_text() {
    let (mut app, _rx) = loaded_simple_app();
    // Default SELECT pane is `*` -> all checked.
    focus_select(&mut app);
    app.on_key(ctrl_p(), 0);
    let pal = app.palette().unwrap();
    assert_eq!(pal.checked_set().len(), 4, "* => all checked");
}

#[test]
fn opening_popup_with_a_subset_checks_only_those_columns() {
    let (mut app, _rx) = loaded_simple_app();
    app.query_form_mut()
        .set_text(SimplePane::Select, "id, status");
    focus_select(&mut app);
    app.on_key(ctrl_p(), 0);
    let pal = app.palette().unwrap();
    let checked: Vec<usize> = pal.checked_set().iter().copied().collect();
    assert_eq!(checked, vec![0, 1], "only id+status checked");
}

#[test]
fn space_toggle_rewrites_select_immediately() {
    let (mut app, _rx) = loaded_simple_app();
    // Pre-set: only id checked.
    app.query_form_mut().set_text(SimplePane::Select, "id");
    focus_select(&mut app);
    app.on_key(ctrl_p(), 0);
    // Cursor at row 0 (id). Space toggles id off -> SELECT pane becomes empty (composer falls
    // back to `*`).
    app.on_key(KeyEvent::char(' '), 0);
    assert_eq!(app.query_form().text(SimplePane::Select), "");
    // Toggle id back on (now the only checked) -> SELECT pane becomes "id".
    app.on_key(KeyEvent::char(' '), 0);
    assert_eq!(app.query_form().text(SimplePane::Select), "id");
}

#[test]
fn tab_toggle_is_alias_for_space() {
    let (mut app, _rx) = loaded_simple_app();
    app.query_form_mut().set_text(SimplePane::Select, "id");
    focus_select(&mut app);
    app.on_key(ctrl_p(), 0);
    // Cursor at row 0 (id); Tab toggles it off (SELECT pane empties).
    app.on_key(KeyEvent::plain(Key::Tab), 0);
    assert_eq!(app.query_form().text(SimplePane::Select), "");
}

#[test]
fn ctrl_a_select_all_writes_star() {
    let (mut app, _rx) = loaded_simple_app();
    app.query_form_mut().set_text(SimplePane::Select, "id");
    focus_select(&mut app);
    app.on_key(ctrl_p(), 0);
    app.on_key(ctrl(Key::Char('a')), 0);
    assert_eq!(app.query_form().text(SimplePane::Select), "*");
}

#[test]
fn ctrl_x_deselect_all_empties_select() {
    let (mut app, _rx) = loaded_simple_app();
    focus_select(&mut app);
    app.on_key(ctrl_p(), 0);
    // Default open => all checked. Ctrl+X deselects all -> SELECT becomes empty.
    app.on_key(ctrl(Key::Char('x')), 0);
    assert_eq!(app.query_form().text(SimplePane::Select), "");
}

#[test]
fn ctrl_i_invert_flips_the_selection() {
    let (mut app, _rx) = loaded_simple_app();
    app.query_form_mut().set_text(SimplePane::Select, "id");
    focus_select(&mut app);
    app.on_key(ctrl_p(), 0);
    // Pre-checked: {id}. Invert -> {status, amount, order}. Schema-order projection.
    app.on_key(ctrl(Key::Char('i')), 0);
    assert_eq!(
        app.query_form().text(SimplePane::Select),
        "status, amount, \"order\""
    );
}

// --- close ---

#[test]
fn enter_closes_popup_without_changing_select() {
    let (mut app, _rx) = loaded_simple_app();
    app.query_form_mut().set_text(SimplePane::Select, "id");
    focus_select(&mut app);
    app.on_key(ctrl_p(), 0);
    assert!(app.is_palette_open());
    app.on_key(KeyEvent::plain(Key::Enter), 0);
    assert!(!app.is_palette_open(), "Enter closes the popup");
    assert_eq!(
        app.query_form().text(SimplePane::Select),
        "id",
        "Enter does not edit SELECT — toggles already wrote it"
    );
}

#[test]
fn esc_closes_popup() {
    let (mut app, _rx) = loaded_simple_app();
    focus_select(&mut app);
    app.on_key(ctrl_p(), 0);
    assert!(app.is_palette_open());
    let quit = app.on_key(KeyEvent::plain(Key::Esc), 0);
    assert!(!quit, "Esc closes the popup, does not quit");
    assert!(!app.is_palette_open());
}

#[test]
fn ctrl_c_quits_even_with_palette_open() {
    let (mut app, _rx) = loaded_simple_app();
    focus_select(&mut app);
    app.on_key(ctrl_p(), 0);
    let quit = app.on_key(ctrl(Key::Char('c')), 0);
    assert!(quit, "Ctrl+C quits from the popup too");
}

// --- nav: bounded, no wrap ---

#[test]
fn down_advances_cursor_then_stops_at_last_row() {
    let (mut app, _rx) = loaded_simple_app();
    focus_select(&mut app);
    app.on_key(ctrl_p(), 0);
    for _ in 0..3 {
        app.on_key(KeyEvent::plain(Key::Down), 0);
    }
    assert_eq!(app.palette().unwrap().cursor(), 3);
    // Past the last row is a no-op.
    for _ in 0..5 {
        app.on_key(KeyEvent::plain(Key::Down), 0);
    }
    assert_eq!(app.palette().unwrap().cursor(), 3, "bounded; no wrap");
}

#[test]
fn up_retreats_cursor_then_stops_at_first_row() {
    let (mut app, _rx) = loaded_simple_app();
    focus_select(&mut app);
    app.on_key(ctrl_p(), 0);
    app.on_key(KeyEvent::plain(Key::Down), 0);
    app.on_key(KeyEvent::plain(Key::Down), 0);
    assert_eq!(app.palette().unwrap().cursor(), 2);
    for _ in 0..5 {
        app.on_key(KeyEvent::plain(Key::Up), 0);
    }
    assert_eq!(app.palette().unwrap().cursor(), 0, "bounded; no wrap");
}

// --- popup overlay invariants ---

#[test]
fn opening_palette_closes_the_autocomplete_popup() {
    let (mut app, _rx) = loaded_simple_app();
    focus_select(&mut app);
    // Re-open autocomplete by typing a printable char into SELECT then re-establishing focus;
    // the simpler approach is to just re-trigger the post-edit refresh. Type a char that opens
    // the popup, then issue Ctrl+P.
    app.on_key(KeyEvent::char(' '), 0);
    let _ = app.autocomplete().is_open();
    // Force the popup open via the public refresh path so the precondition is deterministic.
    app.refresh_autocomplete();
    let was_open = app.autocomplete().is_open();
    app.on_key(ctrl_p(), 0);
    if was_open {
        assert!(
            !app.autocomplete().is_open(),
            "the two overlays are mutually exclusive when palette opens"
        );
    }
    // Palette opening doesn't depend on whether autocomplete was open — it just has to NOT
    // co-exist with autocomplete after the open.
    assert!(app.is_palette_open());
}

#[test]
fn focus_helper_compiles_and_does_not_panic_on_default_app() {
    // Basic smoke that the helpers used elsewhere stay healthy as App evolves.
    let (mut app, _rx) = loaded_simple_app();
    assert_eq!(app.focus(), Focus::QueryBar);
    focus_select(&mut app);
}
