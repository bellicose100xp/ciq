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

// ── Pane navigation: Alt+J/K and Alt+↑/↓ (bounded, no wrap) ─────────────────────────────────────

fn alt(k: Key) -> KeyEvent {
    KeyEvent::new(
        k,
        KeyMods {
            alt: true,
            ..KeyMods::NONE
        },
    )
}

#[test]
fn alt_j_walks_panes_forward_and_stops_at_limit() {
    let (mut app, _rx) = app();
    assert_eq!(app.query_form().focused_pane(), SimplePane::Where);
    app.on_key(alt(Key::Char('j')), 0);
    assert_eq!(app.query_form().focused_pane(), SimplePane::GroupBy);
    app.on_key(alt(Key::Char('j')), 0);
    assert_eq!(app.query_form().focused_pane(), SimplePane::OrderBy);
    app.on_key(alt(Key::Char('j')), 0);
    assert_eq!(app.query_form().focused_pane(), SimplePane::Limit);
    app.on_key(alt(Key::Char('j')), 0);
    assert_eq!(
        app.query_form().focused_pane(),
        SimplePane::Limit,
        "bounded: stops at LIMIT, no wrap"
    );
}

#[test]
fn alt_k_walks_panes_backward_and_stops_at_select() {
    let (mut app, _rx) = app();
    // Default focus is WHERE.
    app.on_key(alt(Key::Char('k')), 0);
    assert_eq!(app.query_form().focused_pane(), SimplePane::Select);
    app.on_key(alt(Key::Char('k')), 0);
    assert_eq!(
        app.query_form().focused_pane(),
        SimplePane::Select,
        "bounded: stops at SELECT, no wrap"
    );
}

#[test]
fn alt_down_is_alias_for_alt_j() {
    let (mut app, _rx) = app();
    app.on_key(alt(Key::Down), 0);
    assert_eq!(app.query_form().focused_pane(), SimplePane::GroupBy);
}

#[test]
fn alt_up_is_alias_for_alt_k() {
    let (mut app, _rx) = app();
    app.on_key(alt(Key::Up), 0);
    assert_eq!(app.query_form().focused_pane(), SimplePane::Select);
}

// ── Tab popup-closed inserts a literal \t into the focused pane ─────────────────────────────────

#[test]
fn tab_popup_closed_inserts_a_literal_tab() {
    let (mut app, _rx) = app();
    // Default focus is WHERE; no schema is loaded so the popup never opens.
    app.on_key(KeyEvent::plain(Key::Tab), 0);
    assert_eq!(
        app.query_form().text(SimplePane::Where),
        "\t",
        "Tab inserts a literal tab when the popup is closed"
    );
    assert_eq!(
        app.query_form().focused_pane(),
        SimplePane::Where,
        "Tab does NOT change pane focus anymore"
    );
}

// ── Plain Up / Down in the bar are no-ops in Simple mode ────────────────────────────────────────

#[test]
fn plain_down_on_limit_stays_on_limit() {
    let (mut app, _rx) = app();
    app.query_form_mut().focus(SimplePane::Limit);
    app.on_key(KeyEvent::plain(Key::Down), 0);
    assert_eq!(
        app.focus(),
        Focus::QueryBar,
        "plain Down does NOT hand focus to Results anymore"
    );
    assert_eq!(
        app.query_form().focused_pane(),
        SimplePane::Limit,
        "plain Down on LIMIT is a no-op"
    );
}

#[test]
fn plain_up_in_any_pane_is_a_noop() {
    let (mut app, _rx) = app();
    // Default focus is WHERE.
    app.on_key(KeyEvent::plain(Key::Up), 0);
    assert_eq!(
        app.query_form().focused_pane(),
        SimplePane::Where,
        "plain Up does NOT cycle to the previous pane anymore"
    );
}

// ── Ctrl+T toggles focus between query bar and results ──────────────────────────────────────────

#[test]
fn ctrl_t_toggles_focus_between_query_bar_and_results() {
    let (mut app, _rx) = app();
    assert_eq!(app.focus(), Focus::QueryBar);
    let ctrl_t = ctrl(Key::Char('t'));
    app.on_key(ctrl_t.clone(), 0);
    assert_eq!(app.focus(), Focus::Results);
    app.on_key(ctrl_t.clone(), 0);
    assert_eq!(app.focus(), Focus::QueryBar);
}

#[test]
fn ctrl_t_preserves_pane_focus_across_round_trip() {
    let (mut app, _rx) = app();
    app.query_form_mut().focus(SimplePane::OrderBy);
    let ctrl_t = ctrl(Key::Char('t'));
    app.on_key(ctrl_t.clone(), 0);
    app.on_key(ctrl_t, 0);
    assert_eq!(app.focus(), Focus::QueryBar);
    assert_eq!(
        app.query_form().focused_pane(),
        SimplePane::OrderBy,
        "Ctrl+T round-trip preserves which Simple pane was focused"
    );
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
