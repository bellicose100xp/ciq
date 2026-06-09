//! `App`-shell tests for the column palette (P4.4/P4.5, §0/D3): ownership byte-compare,
//! seed/replace, and the Ctrl+P popup wiring. Split out of `app_tests.rs` to keep each test file
//! under the 1000-line limit; the shared App helpers live in the parent (`super`).

use crate::app::{Key, KeyEvent, KeyMods};

use super::{app, drain, loaded_app, test_schema, type_str};

// --- column palette ownership detection (P4.4, §0/D3) ---

#[test]
fn no_palette_without_a_schema() {
    let (app, _rx) = app();
    assert!(app.palette().is_none());
    assert!(!app.palette_owns_query());
    assert!(!app.is_palette_open());
}

#[test]
fn set_schema_builds_the_palette() {
    let (app, _rx) = loaded_app();
    let palette = app.palette().expect("palette built from schema");
    // The pick universe mirrors the schema columns in table order.
    let names: Vec<&str> = palette
        .all_columns()
        .iter()
        .map(|c| c.name.as_str())
        .collect();
    assert_eq!(names, vec!["id", "status", "amount", "order"]);
}

#[test]
fn seed_palette_query_makes_the_common_path_palette_owned() {
    // Opening a file with nothing typed pre-seeds the bar with the palette's own emission, so the
    // palette OWNS the query (equal byte-compare => live).
    let (mut app, rx) = loaded_app();
    assert_eq!(app.query(), "", "bar empty until seeded");
    let seeded = app.seed_palette_query(0);
    assert!(seeded);
    assert_eq!(app.query(), "SELECT * FROM t LIMIT 1000");
    assert!(app.palette_owns_query(), "equal => palette owns it (live)");
    // The seed schedules the query; it dispatches on the debounce tick.
    assert!(app.tick(150));
    let sent = drain(&rx);
    assert_eq!(sent.len(), 1);
    assert!(sent[0].contains("SELECT * FROM t LIMIT 1000"));
}

#[test]
fn seed_does_not_clobber_a_query_typed_during_load() {
    // If the user typed during load, the seed must NOT overwrite their query.
    let (mut app, _rx) = loaded_app();
    type_str(&mut app, "SELECT id FROM t", 0);
    let seeded = app.seed_palette_query(0);
    assert!(!seeded, "non-empty bar is left untouched");
    assert_eq!(app.query(), "SELECT id FROM t");
    assert!(!app.palette_owns_query(), "hand-typed => palette disabled");
}

#[test]
fn hand_typing_after_seed_disables_the_palette() {
    // Equal => live; a single edit diverges the bar from the last emitted => palette disabled,
    // detected purely by byte-compare (no SQL parsing).
    let (mut app, _rx) = loaded_app();
    app.seed_palette_query(0);
    assert!(app.palette_owns_query());
    type_str(&mut app, " WHERE id > 5", 0); // user appends -> diverges
    assert!(
        !app.palette_owns_query(),
        "different bar text => palette no longer owns it (offer Replace)"
    );
}

#[test]
fn replace_query_with_palette_snaps_to_generated_query() {
    // The "Replace?" affordance: a user who hand-typed SQL accepts Replace; the bar snaps to the
    // palette's generated query (discarding their text — the documented UX cliff) and the palette
    // owns it again.
    let (mut app, rx) = loaded_app();
    type_str(&mut app, "SELECT id FROM t WHERE status = 'EU'", 0);
    assert!(!app.palette_owns_query());
    // With nothing checked in the palette, Replace snaps to the generated `SELECT *` — discarding
    // the hand-typed WHERE (the documented UX cliff, §0/D3).
    let installed = app.replace_query_with_palette(0).expect("palette present");
    assert_eq!(installed, "SELECT * FROM t LIMIT 1000");
    assert_eq!(app.query(), "SELECT * FROM t LIMIT 1000");
    assert!(
        app.palette_owns_query(),
        "after Replace the palette owns the query again"
    );
    let _ = drain(&rx);
}

// --- column palette popup wiring (P4.5, §0/D3) ---

/// Ctrl+P key event.
fn ctrl_k() -> KeyEvent {
    KeyEvent::new(Key::Char('p'), KeyMods::CTRL)
}

#[test]
fn ctrl_k_opens_palette_when_loaded() {
    let (mut app, _rx) = loaded_app();
    assert!(!app.is_palette_open());
    let quit = app.on_key(ctrl_k(), 0);
    assert!(!quit);
    assert!(app.is_palette_open());
}

#[test]
fn ctrl_k_is_a_noop_without_a_schema() {
    let (mut app, _rx) = app();
    app.on_loaded("ready"); // ready but no schema => no palette
    app.on_key(ctrl_k(), 0);
    assert!(!app.is_palette_open());
}

#[test]
fn ctrl_k_is_a_noop_while_loading() {
    let (mut app, _rx) = app();
    app.set_schema(test_schema()); // palette built, but still Loading
    app.on_key(ctrl_k(), 0);
    assert!(!app.is_palette_open(), "palette must not open before Ready");
}

#[test]
fn esc_closes_palette_does_not_quit() {
    let (mut app, _rx) = loaded_app();
    app.on_key(ctrl_k(), 0);
    assert!(app.is_palette_open());
    let quit = app.on_key(KeyEvent::plain(Key::Esc), 0);
    assert!(!quit, "Esc closes the palette, does not quit");
    assert!(!app.is_palette_open());
}

#[test]
fn ctrl_c_quits_even_with_palette_open() {
    let (mut app, _rx) = loaded_app();
    app.on_key(ctrl_k(), 0);
    let quit = app.on_key(KeyEvent::new(Key::Char('c'), KeyMods::CTRL), 0);
    assert!(quit, "Ctrl-C quits from the palette too");
}

#[test]
fn space_toggles_cursor_column() {
    let (mut app, _rx) = loaded_app();
    app.on_key(ctrl_k(), 0);
    // Cursor starts on column 0 (id). Space toggles it checked.
    app.on_key(KeyEvent::char(' '), 0);
    assert_eq!(app.palette().unwrap().checked(), &[0]);
    // Space again unchecks.
    app.on_key(KeyEvent::char(' '), 0);
    assert!(app.palette().unwrap().checked().is_empty());
}

#[test]
fn arrows_move_cursor_and_typing_filters() {
    let (mut app, _rx) = loaded_app();
    app.on_key(ctrl_k(), 0);
    // Down moves the cursor to status (index 1), Space checks it.
    app.on_key(KeyEvent::plain(Key::Down), 0);
    app.on_key(KeyEvent::char(' '), 0);
    assert_eq!(app.palette().unwrap().checked(), &[1]);
    // Typing filters the list (the needle, not the bar).
    app.on_key(KeyEvent::char('a'), 0);
    assert_eq!(app.palette().unwrap().needle(), "a");
    // The query bar is untouched while the palette is open.
    assert_eq!(app.query(), "");
    // Backspace pops the needle.
    app.on_key(KeyEvent::plain(Key::Backspace), 0);
    assert_eq!(app.palette().unwrap().needle(), "");
}

#[test]
fn left_right_reorder_the_checked_projection() {
    let (mut app, _rx) = loaded_app();
    app.on_key(ctrl_k(), 0);
    // Check id (0) then status (1): selection order [0, 1].
    app.on_key(KeyEvent::char(' '), 0); // id (cursor at 0)
    app.on_key(KeyEvent::plain(Key::Down), 0);
    app.on_key(KeyEvent::char(' '), 0); // status (cursor at 1)
    assert_eq!(app.palette().unwrap().checked(), &[0, 1]);
    // Cursor is on status (index 1, checked). Left moves it earlier in the projection: [1, 0].
    app.on_key(KeyEvent::plain(Key::Left), 0);
    assert_eq!(app.palette().unwrap().checked(), &[1, 0]);
    // Right moves it back later: [0, 1].
    app.on_key(KeyEvent::plain(Key::Right), 0);
    assert_eq!(app.palette().unwrap().checked(), &[0, 1]);
}

#[test]
fn enter_emits_palette_query_and_closes() {
    let (mut app, rx) = loaded_app();
    app.on_key(ctrl_k(), 0);
    // Check status (1) then id (0): projection order [status, id] -> emit reflects selection order.
    app.on_key(KeyEvent::plain(Key::Down), 0); // cursor -> status
    app.on_key(KeyEvent::char(' '), 0); // check status
    app.on_key(KeyEvent::plain(Key::Up), 0); // cursor -> id
    app.on_key(KeyEvent::char(' '), 0); // check id  => [1, 0]
    app.on_key(KeyEvent::plain(Key::Enter), 0);
    assert!(!app.is_palette_open(), "Enter closes the palette");
    assert_eq!(app.query(), "SELECT status, id FROM t LIMIT 1000");
    assert!(
        app.palette_owns_query(),
        "the emitted query is palette-owned"
    );
    // Enter scheduled the query; it dispatches on the debounce tick (the normal worker path).
    assert!(app.tick(150));
    let sent = drain(&rx);
    assert_eq!(sent.len(), 1);
    assert!(sent[0].contains("SELECT status, id FROM t"));
}

#[test]
fn opening_palette_closes_the_autocomplete_popup() {
    let (mut app, _rx) = loaded_app();
    // Open the autocomplete popup by typing a partial column.
    type_str(&mut app, "SELECT st", 0);
    assert!(app.autocomplete().is_open());
    app.on_key(ctrl_k(), 0);
    assert!(app.is_palette_open());
    assert!(
        !app.autocomplete().is_open(),
        "the two overlays are mutually exclusive"
    );
}
