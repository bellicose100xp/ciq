//! App-level integration tests for the Simple/Power query-bar redesign — the user-visible
//! behavior on launch (defaults, dispatch from the composed SQL, Ctrl+Q toggle, Tab pane focus,
//! click-to-focus). These build an App in its **production default** (Simple mode) — the rest of
//! `app_tests` forces Power mode to preserve legacy textarea-shaped semantics.

use std::sync::mpsc::{Receiver, channel};

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::style::Modifier;

use crate::app::{App, Focus, Key, KeyEvent, KeyMods, QueryMode, SimplePane};
use crate::engine::InterruptHandle;
use crate::query::worker::types::QueryRequest;

fn app() -> (App, Receiver<QueryRequest>) {
    let (tx, rx) = channel();
    // No `force_power_mode_for_tests` — exercise the production default.
    (App::new(tx, InterruptHandle::noop()), rx)
}

fn ctrl(k: Key) -> KeyEvent {
    KeyEvent::new(k, KeyMods::CTRL)
}

fn drain(rx: &Receiver<QueryRequest>) -> Vec<String> {
    let mut out = Vec::new();
    while let Ok(r) = rx.try_recv() {
        out.push(r.query);
    }
    out
}

// ── Launch defaults ──────────────────────────────────────────────────────────────────────────────

#[test]
fn default_launch_is_simple_mode_with_cursor_in_where() {
    let (app, _rx) = app();
    assert_eq!(app.query_form().mode(), QueryMode::Simple);
    assert_eq!(app.query_form().focused_pane(), SimplePane::Where);
    assert_eq!(app.focus(), Focus::QueryBar);
    assert_eq!(app.query_form().text(SimplePane::Select), "*");
    assert_eq!(app.query_form().text(SimplePane::Where), "");
    assert_eq!(app.query_form().text(SimplePane::GroupBy), "");
    assert_eq!(app.query_form().text(SimplePane::OrderBy), "");
    assert_eq!(app.query_form().text(SimplePane::Limit), "1000");
}

#[test]
fn default_query_is_select_star_from_t_limit_1000() {
    let (app, _rx) = app();
    assert_eq!(app.query(), "SELECT * FROM t LIMIT 1000");
}

// ── Typing into WHERE filters live ───────────────────────────────────────────────────────────────

#[test]
fn typing_into_where_dispatches_composed_filter() {
    let (mut app, rx) = app();
    app.on_loaded("ready");
    // Type a predicate into the focused WHERE pane.
    for c in "region='EU'".chars() {
        app.on_key(KeyEvent::char(c), 0);
    }
    // Drive the debouncer past the window so the dispatch fires.
    let _ = app.tick(150);
    let queries = drain(&rx);
    assert_eq!(queries.len(), 1, "exactly one debounced dispatch");
    assert!(
        queries[0].contains("WHERE region='EU'"),
        "composed dispatch carries the typed predicate: {queries:?}"
    );
    assert!(
        queries[0].starts_with("SELECT * FROM t"),
        "composed dispatch keeps the implicit FROM: {queries:?}"
    );
}

// ── Tab cycles pane focus ───────────────────────────────────────────────────────────────────────

#[test]
fn tab_cycles_through_panes_in_simple_mode() {
    let (mut app, _rx) = app();
    assert_eq!(app.query_form().focused_pane(), SimplePane::Where);
    app.on_key(KeyEvent::plain(Key::Tab), 0);
    assert_eq!(app.query_form().focused_pane(), SimplePane::GroupBy);
    app.on_key(KeyEvent::plain(Key::Tab), 0);
    assert_eq!(app.query_form().focused_pane(), SimplePane::OrderBy);
    app.on_key(KeyEvent::plain(Key::Tab), 0);
    assert_eq!(app.query_form().focused_pane(), SimplePane::Limit);
    app.on_key(KeyEvent::plain(Key::Tab), 0);
    assert_eq!(
        app.query_form().focused_pane(),
        SimplePane::Select,
        "wraps around"
    );
}

#[test]
fn shift_tab_cycles_in_reverse() {
    let (mut app, _rx) = app();
    let shift_tab = KeyEvent::new(
        Key::Tab,
        KeyMods {
            shift: true,
            ..KeyMods::NONE
        },
    );
    app.on_key(shift_tab, 0);
    assert_eq!(app.query_form().focused_pane(), SimplePane::Select);
}

// ── Down on the LIMIT pane hands off to results ─────────────────────────────────────────────────

#[test]
fn down_on_limit_pane_focuses_results() {
    let (mut app, _rx) = app();
    app.query_form_mut().focus(SimplePane::Limit);
    app.on_key(KeyEvent::plain(Key::Down), 0);
    assert_eq!(app.focus(), Focus::Results);
}

// ── Ctrl+Q toggles modes, preserving context ────────────────────────────────────────────────────

#[test]
fn ctrl_q_simple_to_power_loads_composed_sql_into_textarea() {
    let (mut app, _rx) = app();
    // Type a where clause first so the toggle has non-default content to preserve.
    for c in "id > 5".chars() {
        app.on_key(KeyEvent::char(c), 0);
    }
    app.on_key(ctrl(Key::Char('q')), 100);
    assert_eq!(app.query_form().mode(), QueryMode::Power);
    let power_text = app.query_form().power().text();
    assert!(
        power_text.contains("WHERE id > 5"),
        "the composed SQL carries the WHERE clause into Power: {power_text:?}"
    );
}

#[test]
fn ctrl_q_power_to_simple_redistributes_a_clean_select() {
    let (mut app, _rx) = app();
    // Flip to Power, type a clean SELECT, flip back.
    app.on_key(ctrl(Key::Char('q')), 0);
    assert_eq!(app.query_form().mode(), QueryMode::Power);
    app.query_form_mut()
        .power_mut()
        .set_text("SELECT id, name FROM t WHERE region = 'EU' ORDER BY id LIMIT 50");
    app.on_key(ctrl(Key::Char('q')), 100);
    assert_eq!(app.query_form().mode(), QueryMode::Simple);
    assert_eq!(app.query_form().text(SimplePane::Select), "id, name");
    assert_eq!(app.query_form().text(SimplePane::Where), "region = 'EU'");
    assert_eq!(app.query_form().text(SimplePane::OrderBy), "id");
    assert_eq!(app.query_form().text(SimplePane::Limit), "50");
}

#[test]
fn ctrl_q_power_to_simple_refuses_a_join_with_clear_status() {
    let (mut app, _rx) = app();
    app.on_key(ctrl(Key::Char('q')), 0);
    app.query_form_mut()
        .power_mut()
        .set_text("SELECT * FROM t JOIN u ON t.id = u.id");
    app.on_key(ctrl(Key::Char('q')), 100);
    // The simplifier refused; the form stays in Power and the status names the reason.
    assert_eq!(app.query_form().mode(), QueryMode::Power);
    assert!(
        app.status().to_lowercase().contains("simplify"),
        "status names the simplifier refusal: {:?}",
        app.status()
    );
}

// ── LIMIT-pane validation: non-numeric blocks dispatch but does NOT dim the prior result ────────

#[test]
fn invalid_limit_does_not_dispatch_or_dim_prior_result() {
    let (mut app, rx) = app();
    app.on_loaded("ready");
    // First, get a successful dispatch (a typed char in WHERE schedules + ticks past the window).
    app.on_key(KeyEvent::char('1'), 0);
    let _ = app.tick(150);
    let pre = drain(&rx);
    assert!(!pre.is_empty(), "typed-then-tick dispatch fired");
    // Type junk into LIMIT.
    app.query_form_mut().focus(SimplePane::Limit);
    // Clear the existing "1000" so we end up with "abc".
    app.input_editor_mut().move_end();
    for _ in 0..4 {
        app.input_editor_mut().backspace();
    }
    for c in "abc".chars() {
        app.on_key(KeyEvent::char(c), 200);
    }
    // Drive past the debounce: dispatch_current refuses, status carries the error, NO dispatch.
    let _ = app.tick(400);
    let post = drain(&rx);
    assert!(post.is_empty(), "invalid LIMIT refuses dispatch: {post:?}");
    assert!(
        app.status().to_lowercase().contains("limit"),
        "status names the LIMIT validation error: {:?}",
        app.status()
    );
    // Pane-validation must NOT mark the prior result stale (only engine-pipeline errors do).
    assert!(
        !app.result_is_stale(),
        "invalid-LIMIT pane validation does NOT dim the last good result"
    );
}

// ── Cursor only on the focused pane ──────────────────────────────────────────────────────────────

/// Count cells in the rendered buffer carrying `Modifier::REVERSED` — the cursor-cell signature
/// (`theme::app::cursor`). One per visible cursor; zero on an unfocused single-line pane.
fn count_reversed_cells(app: &App, w: u16, h: u16) -> usize {
    let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
    t.draw(|f| app.render(f)).unwrap();
    t.backend()
        .buffer()
        .content()
        .iter()
        .filter(|c| c.modifier.contains(Modifier::REVERSED))
        .count()
}

#[test]
fn only_focused_pane_shows_a_cursor_cell() {
    let (app, _rx) = app();
    // Default focus = WHERE. Render a comfortably-sized backend (the box is 7 rows tall + the
    // results pane + status above it; 10 rows is plenty for the bar to appear).
    // Five panes in Simple mode would each paint a reverse-video cursor cell if the cursor weren't
    // suppressed on the unfocused four — assert we see exactly one.
    let reversed = count_reversed_cells(&app, 60, 14);
    assert_eq!(
        reversed, 1,
        "expected exactly ONE reverse-video cursor cell (focused pane), got {reversed}"
    );
}

#[test]
fn focused_cursor_follows_focus_change() {
    let (mut app, _rx) = app();
    // Focus moves to LIMIT — still exactly one cursor cell, just in a different pane.
    app.query_form_mut().focus(SimplePane::Limit);
    let reversed = count_reversed_cells(&app, 60, 14);
    assert_eq!(
        reversed, 1,
        "after focus change, still exactly ONE cursor cell"
    );
}

// ── Up navigation in Simple closed-popup ─────────────────────────────────────────────────────────

#[test]
fn up_in_simple_focuses_previous_pane() {
    let (mut app, _rx) = app();
    // Default WHERE -> Up should land on SELECT.
    app.on_key(KeyEvent::new(Key::Up, KeyMods::NONE), 0);
    assert_eq!(app.query_form().focused_pane(), SimplePane::Select);
    // Up on SELECT stays put (no surface above the bar).
    app.on_key(KeyEvent::new(Key::Up, KeyMods::NONE), 0);
    assert_eq!(app.query_form().focused_pane(), SimplePane::Select);
}
