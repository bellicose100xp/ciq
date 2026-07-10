//! `App`-shell tests for the `Ctrl+O` output-on-exit and `Ctrl+W` save-to-CSV chords: the exit
//! action flag, the popup open/close/route, filename editing, and the actual write to a tempdir.
//! Split out of `app_tests.rs` like the other per-feature test files.

use crate::app::{App, ExitAction, Key, KeyEvent, KeyMods};
use crate::engine::types::{Cell, Column, Table};
use crate::query::worker::types::{ProcessedResult, QueryRequest, QueryResponse, RequestKind};
use crate::schema::ColumnType;

use super::{loaded_app, type_str};

fn ctrl(c: char) -> KeyEvent {
    KeyEvent::new(Key::Char(c), KeyMods::CTRL)
}

fn two_row_result() -> ProcessedResult {
    let table = Table::new(vec![
        Column::new("id", ColumnType::Int, vec![Cell::Int(1), Cell::Int(2)]),
        Column::new(
            "region",
            ColumnType::Text,
            vec![Cell::Text("EU".into()), Cell::Text("NA".into())],
        ),
    ]);
    let schema = table.schema();
    ProcessedResult::new(table, schema, 0)
}

/// A loaded app with a two-row result on screen and any autocomplete popup dismissed.
fn app_with_result() -> (App, std::sync::mpsc::Receiver<QueryRequest>) {
    let (mut app, rx) = loaded_app();
    type_str(&mut app, "SELECT * FROM t", 0);
    app.tick(150);
    let id = app.latest_request_id();
    app.on_response(QueryResponse::ProcessedSuccess {
        result: two_row_result(),
        request_id: id,
        kind: RequestKind::Main,
    });
    if app.autocomplete().is_open() {
        app.on_key(KeyEvent::plain(Key::Esc), 200);
    }
    (app, rx)
}

// --- Ctrl+O: quit and print the result to the scrollback ---

#[test]
fn ctrl_o_quits_and_sets_print_action() {
    let (mut app, _rx) = app_with_result();
    let quit = app.on_key(ctrl('o'), 300);
    assert!(quit, "Ctrl+O quits");
    assert_eq!(app.exit_action(), Some(ExitAction::PrintResult));
}

#[test]
fn ctrl_o_without_result_quits_plain() {
    // Before any result lands, Ctrl+O still quits but sets no print action (nothing to print).
    let (mut app, _rx) = loaded_app();
    let quit = app.on_key(ctrl('o'), 0);
    assert!(quit);
    assert_eq!(app.exit_action(), None);
}

// --- Ctrl+W: the save-to-CSV popup ---

#[test]
fn ctrl_w_opens_popup_with_default_filename() {
    let (mut app, _rx) = app_with_result();
    app.configure_save(Some("sales".to_string()), None);
    app.on_key(ctrl('w'), 300);
    assert!(app.is_save_open());
    assert_eq!(app.save().filename(), "sales-out.csv");
}

#[test]
fn ctrl_w_without_result_is_noop() {
    let (mut app, _rx) = loaded_app();
    app.on_key(ctrl('w'), 0);
    assert!(!app.is_save_open());
}

#[test]
fn esc_closes_the_save_popup() {
    let (mut app, _rx) = app_with_result();
    app.on_key(ctrl('w'), 300);
    assert!(app.is_save_open());
    app.on_key(KeyEvent::plain(Key::Esc), 300);
    assert!(!app.is_save_open());
}

#[test]
fn typing_edits_the_filename_while_open() {
    let (mut app, _rx) = app_with_result();
    app.configure_save(Some("data".to_string()), None);
    app.on_key(ctrl('w'), 300);
    // Clear the default, then type a fresh name.
    for _ in 0.."data-out.csv".len() {
        app.on_key(KeyEvent::plain(Key::Backspace), 300);
    }
    type_str(&mut app, "mine", 300);
    assert_eq!(app.save().filename(), "mine");
}

#[test]
fn open_save_leaves_no_other_popup_open() {
    // Opening the save popup must not leave another overlay open underneath it (overlays are
    // mutually exclusive). Whatever popups routing allows to be open when Ctrl+W fires, the save
    // popup takes over cleanly.
    let (mut app, _rx) = app_with_result();
    app.on_key(ctrl('w'), 300);
    assert!(app.is_save_open());
    assert!(!app.autocomplete().is_open());
    assert!(!app.is_history_open());
    assert!(!app.is_ai_open());
    assert!(!app.is_palette_open());
    assert!(!app.is_facet_open());
}

#[test]
fn ctrl_c_quits_from_the_save_popup() {
    let (mut app, _rx) = app_with_result();
    app.on_key(ctrl('w'), 300);
    let quit = app.on_key(ctrl('c'), 300);
    assert!(quit);
}

#[test]
fn enter_writes_csv_and_reports_status() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("out.csv");
    let (mut app, _rx) = app_with_result();
    app.on_key(ctrl('w'), 300);
    // Replace the default name with the tempdir path.
    for _ in 0..app.save().filename().len() {
        app.on_key(KeyEvent::plain(Key::Backspace), 300);
    }
    type_str(&mut app, &path.to_string_lossy(), 300);
    app.on_key(KeyEvent::plain(Key::Enter), 300);
    assert!(!app.is_save_open(), "popup closes after a successful write");
    let written = std::fs::read_to_string(&path).expect("file written");
    assert_eq!(written, "id,region\n1,EU\n2,NA\n");
    assert!(
        app.status().contains("saved 2 rows"),
        "status: {}",
        app.status()
    );
}

#[test]
fn ctrl_w_works_while_search_is_still_editing() {
    // The search bar in editing mode captures the keyboard; Ctrl+W must still open the save
    // popup (confirming the in-progress filter first) rather than being swallowed.
    let (mut app, _rx) = app_with_result();
    app.on_key(ctrl('f'), 300);
    type_str(&mut app, "EU", 300); // still editing — no Enter
    app.on_key(ctrl('w'), 300);
    assert!(app.is_save_open(), "Ctrl+W opens the save popup mid-edit");
    assert!(
        app.search().is_confirmed(),
        "the in-progress filter is confirmed, not dropped"
    );
}

#[test]
fn ctrl_o_works_while_search_is_still_editing() {
    let (mut app, _rx) = app_with_result();
    app.on_key(ctrl('f'), 300);
    type_str(&mut app, "EU", 300); // still editing — no Enter
    let quit = app.on_key(ctrl('o'), 300);
    assert!(quit, "Ctrl+O quits mid-edit");
    assert_eq!(app.exit_action(), Some(ExitAction::PrintResult));
    // The filter survives into the exit print: only the EU row is displayed.
    let rows = app.display_rows().expect("displayed rows");
    assert_eq!(rows.row_count(), 1, "the print sees the filtered view");
}

#[test]
fn save_from_editing_search_writes_the_filtered_view() {
    // End-to-end: filter (unconfirmed) -> Ctrl+W -> Enter writes only the filtered rows.
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("mid-edit.csv");
    let (mut app, _rx) = app_with_result();
    app.on_key(ctrl('f'), 300);
    type_str(&mut app, "EU", 300); // still editing — no Enter
    app.on_key(ctrl('w'), 300);
    for _ in 0..app.save().filename().len() {
        app.on_key(KeyEvent::plain(Key::Backspace), 300);
    }
    type_str(&mut app, &path.to_string_lossy(), 300);
    app.on_key(KeyEvent::plain(Key::Enter), 300);
    let written = std::fs::read_to_string(&path).expect("file written");
    assert_eq!(written, "id,region\n1,EU\n");
}

#[test]
fn enter_writes_the_filtered_view() {
    // A Ctrl+F filter narrows the result; the save writes what's on screen, not the full result.
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("filtered.csv");
    let (mut app, _rx) = app_with_result();
    // Filter to rows containing "EU".
    app.on_key(ctrl('f'), 300);
    type_str(&mut app, "EU", 300);
    app.on_key(KeyEvent::plain(Key::Enter), 300); // confirm the filter
    app.on_key(ctrl('w'), 300);
    for _ in 0..app.save().filename().len() {
        app.on_key(KeyEvent::plain(Key::Backspace), 300);
    }
    type_str(&mut app, &path.to_string_lossy(), 300);
    app.on_key(KeyEvent::plain(Key::Enter), 300);
    let written = std::fs::read_to_string(&path).expect("file written");
    assert_eq!(
        written, "id,region\n1,EU\n",
        "only the filtered row is saved"
    );
}
