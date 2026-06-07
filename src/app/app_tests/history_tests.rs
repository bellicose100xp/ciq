//! `App`-shell tests for query history (P5.2, §7.6): the `Ctrl+R` chord opens the popup, a
//! dispatched query is recorded (deduped), the popup's key routing (navigate / filter / recall /
//! close / quit), and recall dropping the selected entry into the bar so it fires through the
//! normal debounce + dispatch path. Split out of `app_tests.rs` to keep each file under the
//! 1000-line limit; the shared App helpers live in the parent (`super`).
//!
//! Everything is driven through the public `on_key` / `tick` surface — including clearing the bar
//! (backspace) — so the tests exercise exactly the path a real session takes (no test-only bar
//! setter is introduced).

use crate::app::{App, Key, KeyEvent, KeyMods};

use super::{drain, loaded_app, type_str};

fn ctrl(c: char) -> KeyEvent {
    KeyEvent::new(Key::Char(c), KeyMods::CTRL)
}

/// Clear the query bar by issuing enough backspaces, then type `sql` — the "replace the bar"
/// gesture, all through `on_key`.
fn set_bar(app: &mut App, sql: &str, now_ms: u64) {
    for _ in 0..200 {
        app.on_key(KeyEvent::plain(Key::Backspace), now_ms);
    }
    type_str(app, sql, now_ms);
}

/// Set the bar to `sql` and fire the debounce (the felt "I ran this" moment).
fn run_query(app: &mut App, sql: &str, now_ms: u64) {
    set_bar(app, sql, now_ms);
    app.tick(now_ms + 150);
}

// --- recording on dispatch ---

#[test]
fn dispatched_query_is_recorded() {
    let (mut app, _rx) = loaded_app();
    run_query(&mut app, "SELECT * FROM t", 0);
    assert_eq!(app.history().entries(), &["SELECT * FROM t".to_string()]);
}

#[test]
fn history_records_newest_first() {
    let (mut app, _rx) = loaded_app();
    run_query(&mut app, "SELECT id FROM t", 0);
    run_query(&mut app, "SELECT amount FROM t", 400);
    assert_eq!(
        app.history().entries(),
        &[
            "SELECT amount FROM t".to_string(),
            "SELECT id FROM t".to_string(),
        ]
    );
}

#[test]
fn consecutive_duplicate_query_is_not_double_recorded() {
    let (mut app, _rx) = loaded_app();
    run_query(&mut app, "SELECT * FROM t", 0);
    // Re-run the same bar text: the ring dedupes the consecutive duplicate.
    app.tick(1000);
    assert_eq!(app.history().total_count(), 1);
}

#[test]
fn non_consecutive_duplicate_moves_to_front_no_growth() {
    let (mut app, _rx) = loaded_app();
    run_query(&mut app, "SELECT a FROM t", 0);
    run_query(&mut app, "SELECT b FROM t", 400);
    run_query(&mut app, "SELECT a FROM t", 800); // re-run the older one
    assert_eq!(app.history().total_count(), 2);
    assert_eq!(
        app.history().entries().first().map(String::as_str),
        Some("SELECT a FROM t")
    );
}

// --- the Ctrl+R chord ---

#[test]
fn ctrl_r_opens_history_popup() {
    let (mut app, _rx) = loaded_app();
    run_query(&mut app, "SELECT * FROM t", 0);
    assert!(!app.is_history_open());
    app.on_key(ctrl('r'), 200);
    assert!(app.is_history_open());
    assert!(app.history().is_visible());
}

#[test]
fn ctrl_r_is_noop_while_loading() {
    // A fresh App is in Loading; the chord must not open the popup.
    let (mut app, _rx) = super::app();
    app.on_key(ctrl('r'), 0);
    assert!(!app.is_history_open());
}

#[test]
fn ctrl_r_seeds_needle_with_bar_text() {
    let (mut app, _rx) = loaded_app();
    run_query(&mut app, "SELECT id FROM t", 0);
    set_bar(&mut app, "SELECT id", 300);
    app.on_key(ctrl('r'), 400);
    assert_eq!(app.history().needle(), "SELECT id");
}

// --- popup key routing ---

#[test]
fn esc_closes_popup_without_recall() {
    let (mut app, _rx) = loaded_app();
    run_query(&mut app, "SELECT * FROM t", 0);
    app.on_key(ctrl('r'), 200);
    let bar_before = app.query().to_string();
    let quit = app.on_key(KeyEvent::plain(Key::Esc), 250);
    assert!(!quit, "Esc closes the popup, never quits");
    assert!(!app.is_history_open());
    assert_eq!(app.query(), bar_before, "Esc does not recall");
}

#[test]
fn ctrl_c_in_popup_quits() {
    let (mut app, _rx) = loaded_app();
    run_query(&mut app, "SELECT * FROM t", 0);
    app.on_key(ctrl('r'), 200);
    assert!(app.on_key(ctrl('c'), 250), "Ctrl-C quits from the popup");
}

#[test]
fn typing_in_popup_filters() {
    let (mut app, _rx) = loaded_app();
    run_query(&mut app, "SELECT id FROM t", 0);
    run_query(&mut app, "SELECT count(*) FROM t", 400);
    set_bar(&mut app, "", 600); // clear so the open needle starts empty
    app.on_key(ctrl('r'), 700);
    assert_eq!(app.history().filtered_count(), 2);
    for c in "count".chars() {
        app.on_key(KeyEvent::char(c), 750);
    }
    assert_eq!(app.history().filtered_count(), 1);
    app.on_key(KeyEvent::plain(Key::Backspace), 760);
    assert_eq!(app.history().filtered_count(), 1); // "coun" still matches count(*)
}

// --- recall through the normal path ---

#[test]
fn enter_recalls_into_bar_and_fires_normal_path() {
    let (mut app, rx) = loaded_app();
    run_query(&mut app, "SELECT id FROM t", 0);
    run_query(&mut app, "SELECT amount FROM t", 400);
    let _ = drain(&rx); // clear dispatches from the two runs

    // Clear the bar so the popup opens with an empty needle (all entries visible), then recall.
    set_bar(&mut app, "", 600);
    app.on_key(ctrl('r'), 700);
    // Newest-first: cursor starts on "SELECT amount FROM t"; step to the older "SELECT id".
    app.on_key(KeyEvent::plain(Key::Down), 710);
    app.on_key(KeyEvent::plain(Key::Enter), 720);

    assert!(!app.is_history_open(), "Enter closes the popup");
    assert_eq!(
        app.query(),
        "SELECT id FROM t",
        "the recalled SQL is in the bar"
    );

    app.tick(900);
    let dispatched = drain(&rx);
    assert!(
        dispatched.iter().any(|q| q.contains("SELECT id FROM t")),
        "recalled SQL flows through the normal LIMIT-wrap + dispatch path: {dispatched:?}"
    );
}

#[test]
fn recalled_sql_is_limit_wrapped_like_any_query() {
    let (mut app, rx) = loaded_app();
    run_query(&mut app, "SELECT id FROM t", 0);
    let _ = drain(&rx);
    set_bar(&mut app, "", 300);
    app.on_key(ctrl('r'), 400);
    app.on_key(KeyEvent::plain(Key::Enter), 410); // recall the (only) entry
    app.tick(600);
    let dispatched = drain(&rx);
    // The dispatched SQL is the LIMIT-wrapped form (the read-only guard + viewport cap applied),
    // not the raw recalled text — proving recall is not a privileged bypass.
    assert!(
        dispatched.iter().any(|q| q.contains("LIMIT")),
        "recalled SQL goes through prepare_interactive: {dispatched:?}"
    );
}

#[test]
fn enter_on_empty_history_just_closes() {
    let (mut app, _rx) = loaded_app();
    app.on_key(ctrl('r'), 0);
    app.on_key(KeyEvent::plain(Key::Enter), 10);
    assert!(!app.is_history_open());
}

// --- on-disk persistence (tempdir; never $HOME) ---

#[test]
fn configured_history_persists_to_disk() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("history");
    let (mut app, _rx) = loaded_app();
    app.configure_history(Some(path.clone()), 100, true);

    run_query(&mut app, "SELECT id FROM t", 0);
    run_query(&mut app, "SELECT amount FROM t", 400);

    let on_disk = crate::history::storage::load(&path);
    assert_eq!(
        on_disk,
        vec![
            "SELECT amount FROM t".to_string(),
            "SELECT id FROM t".to_string(),
        ],
        "queries persist to the configured file, newest first"
    );
}

#[test]
fn configure_history_seeds_ring_from_disk() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("history");
    crate::history::storage::save(&path, &["prior 1".to_string(), "prior 2".to_string()], 100)
        .unwrap();

    let (mut app, _rx) = loaded_app();
    app.configure_history(Some(path), 100, true);
    assert_eq!(
        app.history().entries(),
        &["prior 1".to_string(), "prior 2".to_string()],
        "the ring is seeded from the on-disk file at configure time"
    );
}

#[test]
fn disabled_persistence_keeps_ring_session_only() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("history");
    let (mut app, _rx) = loaded_app();
    app.configure_history(Some(path.clone()), 100, false); // persistence OFF

    run_query(&mut app, "SELECT id FROM t", 0);

    // The in-memory ring still records...
    assert_eq!(app.history().total_count(), 1);
    // ...but nothing was written to disk.
    assert!(
        crate::history::storage::load(&path).is_empty(),
        "persistence disabled: the file stays empty"
    );
}
