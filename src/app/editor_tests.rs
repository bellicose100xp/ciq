//! Tests for the pure query-bar [`Editor`] — character-granular edits, cursor clamping, and
//! UTF-8 safety (no byte-boundary slicing on multi-byte input).

use super::Editor;

#[test]
fn new_editor_is_empty() {
    let e = Editor::new();
    assert_eq!(e.text(), "");
    assert_eq!(e.cursor(), 0);
    assert!(e.is_empty());
}

#[test]
fn with_text_places_cursor_at_end() {
    let e = Editor::with_text("SELECT 1");
    assert_eq!(e.text(), "SELECT 1");
    assert_eq!(e.cursor(), 8);
}

#[test]
fn insert_char_advances_cursor() {
    let mut e = Editor::new();
    e.insert_char('a');
    e.insert_char('b');
    assert_eq!(e.text(), "ab");
    assert_eq!(e.cursor(), 2);
}

#[test]
fn insert_char_mid_buffer() {
    let mut e = Editor::with_text("ac");
    e.move_left(); // cursor between a and c (index 1)
    e.insert_char('b');
    assert_eq!(e.text(), "abc");
    assert_eq!(e.cursor(), 2);
}

#[test]
fn insert_str_inserts_at_cursor_and_advances() {
    let mut e = Editor::with_text("SELECT ");
    e.insert_str("* FROM t");
    assert_eq!(e.text(), "SELECT * FROM t");
    assert_eq!(e.cursor(), "SELECT * FROM t".chars().count());
}

#[test]
fn insert_str_empty_is_noop() {
    let mut e = Editor::with_text("x");
    e.insert_str("");
    assert_eq!(e.text(), "x");
    assert_eq!(e.cursor(), 1);
}

#[test]
fn backspace_removes_before_cursor() {
    let mut e = Editor::with_text("abc");
    e.backspace();
    assert_eq!(e.text(), "ab");
    assert_eq!(e.cursor(), 2);
}

#[test]
fn backspace_at_start_is_noop() {
    let mut e = Editor::with_text("abc");
    e.move_home();
    e.backspace();
    assert_eq!(e.text(), "abc");
    assert_eq!(e.cursor(), 0);
}

#[test]
fn delete_removes_at_cursor() {
    let mut e = Editor::with_text("abc");
    e.move_home();
    e.delete();
    assert_eq!(e.text(), "bc");
    assert_eq!(e.cursor(), 0);
}

#[test]
fn delete_at_end_is_noop() {
    let mut e = Editor::with_text("abc");
    e.delete();
    assert_eq!(e.text(), "abc");
    assert_eq!(e.cursor(), 3);
}

#[test]
fn cursor_movement_clamps() {
    let mut e = Editor::with_text("ab");
    e.move_left();
    e.move_left();
    e.move_left(); // clamps at 0
    assert_eq!(e.cursor(), 0);
    e.move_right();
    e.move_right();
    e.move_right(); // clamps at char_count
    assert_eq!(e.cursor(), 2);
}

#[test]
fn home_and_end() {
    let mut e = Editor::with_text("hello");
    e.move_home();
    assert_eq!(e.cursor(), 0);
    e.move_end();
    assert_eq!(e.cursor(), 5);
}

#[test]
fn set_text_and_clear() {
    let mut e = Editor::with_text("old");
    e.set_text("brand new");
    assert_eq!(e.text(), "brand new");
    assert_eq!(e.cursor(), 9);
    e.clear();
    assert_eq!(e.text(), "");
    assert_eq!(e.cursor(), 0);
}

// --- UTF-8 safety: cursor is a CHAR index; mutations never split a code point. ---

#[test]
fn multibyte_insert_and_backspace() {
    let mut e = Editor::new();
    e.insert_char('é'); // 2 bytes
    e.insert_char('日'); // 3 bytes
    assert_eq!(e.text(), "é日");
    assert_eq!(e.cursor(), 2);
    e.backspace();
    assert_eq!(e.text(), "é");
    assert_eq!(e.cursor(), 1);
    e.backspace();
    assert_eq!(e.text(), "");
    assert_eq!(e.cursor(), 0);
}

#[test]
fn multibyte_mid_insert() {
    let mut e = Editor::with_text("aé"); // a=1 byte, é=2 bytes; 2 chars
    e.move_left(); // between a and é (char index 1)
    e.insert_char('日');
    assert_eq!(e.text(), "a日é");
    assert_eq!(e.cursor(), 2);
}

#[test]
fn multibyte_delete_at_cursor() {
    let mut e = Editor::with_text("é日x");
    e.move_home();
    e.delete(); // removes the leading é, not a stray byte
    assert_eq!(e.text(), "日x");
    assert_eq!(e.cursor(), 0);
}
