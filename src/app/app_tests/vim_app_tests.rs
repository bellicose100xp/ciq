//! `App`-shell tests for the vim pieces that reach outside the editor: Normal-mode `/` opening
//! the row search, the yank-to-clipboard hand-off, and the Power-mode line exits. The Simple-mode
//! pane motions live in `simple_mode_tests.rs`; the editor-level dispatch in `editor/vim_*_tests.rs`.

use std::sync::mpsc::{Receiver, channel};

use super::{two_row_result, type_str};
use crate::app::editor::EditorMode;
use crate::app::{App, Focus, Key, KeyEvent};
use crate::engine::InterruptHandle;
use crate::query::worker::types::{QueryRequest, QueryResponse, RequestKind};

/// A Power-mode App (the multiline textarea) that is loaded and ready but has no schema, so typing
/// never opens the autocomplete popup and the first `Esc` drops straight to Normal mode.
fn app() -> (App, Receiver<QueryRequest>) {
    let (tx, rx) = channel();
    let mut app = App::new(tx, InterruptHandle::noop());
    app.force_power_mode_for_tests("");
    app.on_loaded("ready");
    (app, rx)
}

fn esc(app: &mut App) {
    app.on_key(KeyEvent::plain(Key::Esc), 0);
}

/// Dispatch the typed query and answer it so the search has rows to filter.
fn with_result(app: &mut App) {
    app.tick(150);
    let id = app.latest_request_id();
    app.on_response(QueryResponse::ProcessedSuccess {
        result: two_row_result(),
        request_id: id,
        kind: RequestKind::Main,
    });
}

#[test]
fn slash_in_normal_mode_opens_the_row_search() {
    let (mut app, _rx) = app();
    type_str(&mut app, "SELECT * FROM t", 0);
    with_result(&mut app);
    esc(&mut app);
    assert_eq!(app.editor_mode(), EditorMode::Normal);
    app.on_key(KeyEvent::char('/'), 0);
    assert!(app.search().is_visible(), "`/` opened the search bar");
    assert!(app.search().is_editing());
    assert_eq!(app.query(), "SELECT * FROM t", "no `/` landed in the query");
}

#[test]
fn a_shifted_slash_in_normal_mode_still_opens_the_search() {
    // Layouts that reach `/` through Shift deliver the char with the shift flag set.
    use crate::app::KeyMods;
    let (mut app, _rx) = app();
    type_str(&mut app, "SELECT 1", 0);
    with_result(&mut app);
    esc(&mut app);
    let shifted = KeyMods {
        shift: true,
        ..KeyMods::NONE
    };
    app.on_key(KeyEvent::new(Key::Char('/'), shifted), 0);
    assert!(app.search().is_visible());
}

#[test]
fn slash_in_insert_mode_is_text() {
    let (mut app, _rx) = app();
    type_str(&mut app, "1 ", 0);
    with_result(&mut app);
    app.on_key(KeyEvent::char('/'), 0);
    assert!(!app.search().is_visible());
    assert_eq!(app.query(), "1 /");
}

#[test]
fn slash_in_normal_mode_without_a_result_does_nothing() {
    let (mut app, _rx) = app();
    type_str(&mut app, "SELECT 1", 0);
    esc(&mut app);
    app.on_key(KeyEvent::char('/'), 0);
    assert!(!app.search().is_visible(), "nothing to filter yet");
    assert_eq!(app.query(), "SELECT 1");
}

#[test]
fn a_yank_queues_the_register_for_the_clipboard_once() {
    let (mut app, _rx) = app();
    type_str(&mut app, "SELECT a FROM t", 0);
    esc(&mut app);
    assert!(
        app.take_clipboard_text().is_none(),
        "nothing queued before a yank"
    );
    app.on_key(KeyEvent::char('y'), 0);
    app.on_key(KeyEvent::char('y'), 0);
    assert_eq!(
        app.take_clipboard_text().as_deref(),
        Some("SELECT a FROM t")
    );
    assert!(app.take_clipboard_text().is_none(), "taking drains it");
    assert_eq!(app.query(), "SELECT a FROM t", "yy leaves the text alone");
}

#[test]
fn a_cut_fills_the_register_but_not_the_clipboard() {
    let (mut app, _rx) = app();
    type_str(&mut app, "foo bar", 0);
    esc(&mut app);
    app.on_key(KeyEvent::char('0'), 0);
    app.on_key(KeyEvent::char('d'), 0);
    app.on_key(KeyEvent::char('w'), 0);
    assert_eq!(app.query(), "bar");
    assert!(app.take_clipboard_text().is_none(), "deletes stay in-app");
    app.on_key(KeyEvent::char('$'), 0);
    app.on_key(KeyEvent::char('p'), 0);
    assert_eq!(app.query(), "barfoo ");
}

#[test]
fn power_normal_j_on_the_last_line_hands_focus_to_results() {
    let (mut app, _rx) = app();
    type_str(&mut app, "SELECT *", 0);
    app.on_key(KeyEvent::plain(Key::Enter), 0);
    type_str(&mut app, "FROM t", 0);
    esc(&mut app);
    app.on_key(KeyEvent::char('k'), 0);
    assert_eq!(app.editor().row_col().0, 0);
    assert_eq!(app.focus(), Focus::QueryBar);
    app.on_key(KeyEvent::char('k'), 0);
    assert_eq!(
        app.focus(),
        Focus::QueryBar,
        "k on the first line has nowhere to go"
    );
    app.on_key(KeyEvent::char('j'), 0);
    assert_eq!(
        app.editor().row_col().0,
        1,
        "j inside the buffer moves a line"
    );
    assert_eq!(app.focus(), Focus::QueryBar);
    app.on_key(KeyEvent::char('j'), 0);
    assert_eq!(
        app.focus(),
        Focus::Results,
        "j off the last line hands focus to the grid"
    );
}

#[test]
fn power_normal_shift_g_stays_in_the_bar() {
    let (mut app, _rx) = app();
    type_str(&mut app, "SELECT 1", 0);
    esc(&mut app);
    app.on_key(KeyEvent::char('G'), 0);
    app.on_key(KeyEvent::char('g'), 0);
    app.on_key(KeyEvent::char('g'), 0);
    assert_eq!(app.focus(), Focus::QueryBar);
    assert_eq!(app.editor_mode(), EditorMode::Normal);
}
