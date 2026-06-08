//! Tests for the query-bar [`Editor`] (the `tui_textarea` wrapper): character-granular edits,
//! cursor clamping, UTF-8 safety, the multiline newline path, and the byte<->(row,col) bridge the
//! SQL lexer / autocomplete index the query by.

use super::Editor;

#[test]
fn new_editor_is_empty() {
    let e = Editor::new();
    assert_eq!(e.text(), "");
    assert_eq!(e.cursor(), 0);
    assert_eq!(e.cursor_byte(), 0);
    assert!(e.is_empty());
    assert_eq!(e.line_count(), 1);
}

#[test]
fn with_text_places_cursor_at_end() {
    let e = Editor::with_text("SELECT 1");
    assert_eq!(e.text(), "SELECT 1");
    assert_eq!(e.cursor(), 8);
    assert_eq!(e.cursor_byte(), 8);
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
    assert_eq!(e.line_count(), 1);
}

// --- multiline: Enter inserts a newline; text() joins lines with `\n`. ---

#[test]
fn insert_newline_splits_into_lines() {
    let mut e = Editor::with_text("SELECT *");
    e.insert_newline();
    e.insert_str("FROM t");
    assert_eq!(e.text(), "SELECT *\nFROM t");
    assert_eq!(e.line_count(), 2);
    // cursor is char index into the joined text: "SELECT *" (8) + newline (1) + "FROM t" (6) = 15.
    assert_eq!(e.cursor(), 15);
}

#[test]
fn newline_in_paste_splits_lines() {
    let mut e = Editor::new();
    e.insert_str("a\nb\nc");
    assert_eq!(e.text(), "a\nb\nc");
    assert_eq!(e.line_count(), 3);
}

#[test]
fn move_up_down_between_lines() {
    let mut e = Editor::new();
    e.insert_str("one\ntwo");
    assert!(e.is_on_last_line());
    assert!(!e.is_on_first_line());
    e.move_up();
    assert!(e.is_on_first_line());
    assert!(!e.is_on_last_line());
    e.move_down();
    assert!(e.is_on_last_line());
}

#[test]
fn move_up_on_first_line_is_noop() {
    let mut e = Editor::with_text("only");
    assert!(e.is_on_first_line() && e.is_on_last_line());
    e.move_up();
    assert!(e.is_on_first_line());
}

#[test]
fn backspace_at_line_head_joins_previous_line() {
    let mut e = Editor::new();
    e.insert_str("ab\ncd");
    e.move_home(); // head of line "cd"
    e.backspace(); // joins onto the previous line
    assert_eq!(e.text(), "abcd");
    assert_eq!(e.line_count(), 1);
}

// --- byte <-> (row, col) bridge: cursor_byte() into the joined text, and the inverse. ---

#[test]
fn cursor_byte_accounts_for_newline_bytes_and_multibyte_chars() {
    let mut e = Editor::new();
    // Line 0: "é" (2 bytes). Line 1: "xy" — cursor will sit after the 'x'.
    e.insert_str("é\nxy");
    e.move_left(); // cursor between x and y on line 1
    // byte offset = len("é")=2 + 1 (the `\n`) + len("x")=1 = 4.
    assert_eq!(e.cursor_byte(), 4);
    // char index = 1 (é) + 1 (newline) + 1 (x) = 3.
    assert_eq!(e.cursor(), 3);
}

#[test]
fn set_text_with_byte_cursor_maps_offset_into_multiline_text() {
    let mut e = Editor::new();
    // Bytes of "ab\ncdef": a=0 b=1 \n=2 c=3 d=4 e=5 f=6. Byte offset 6 is between 'e' and 'f' on
    // the second line ("ab"=2 + "\n"=1 + "cde"=3 = 6).
    e.set_text_with_byte_cursor("ab\ncdef", 6);
    assert_eq!(e.text(), "ab\ncdef");
    assert_eq!(e.row_col(), (1, 3)); // line 1, after "cde"
    // Round-trip: cursor_byte() reproduces the byte offset we set.
    assert_eq!(e.cursor_byte(), 6);
    // Inserting here lands at the right place in the multiline text.
    e.insert_char('X');
    assert_eq!(e.text(), "ab\ncdeXf");
}

#[test]
fn set_text_with_byte_cursor_clamps_past_end() {
    let mut e = Editor::with_text("hi");
    e.set_text_with_byte_cursor("short", 999);
    assert_eq!(e.text(), "short");
    assert_eq!(e.cursor(), 5); // clamped to end
}

#[test]
fn set_text_with_byte_cursor_snaps_into_char_boundary() {
    let mut e = Editor::new();
    // "é" is 2 bytes; a byte offset of 1 lands mid-char and must snap to char index 0.
    e.set_text_with_byte_cursor("éx", 1);
    assert_eq!(e.row_col(), (0, 0));
    assert_eq!(e.cursor_byte(), 0);
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

#[test]
fn char_count_matches_joined_text() {
    let mut e = Editor::new();
    e.insert_str("ab\ncd"); // 2 + 1 (newline) + 2 = 5 chars
    assert_eq!(e.char_count(), 5);
}

// --- line-safety of the vim cut primitives: never cross the joining newline ---

/// Place the cursor at `(row, col)` on a multiline buffer via Home + Down/Right walks.
fn at(e: &mut Editor, row: usize, col: usize) {
    // Walk to the top, then down `row` lines, then right `col` columns.
    while !e.is_on_first_line() {
        e.move_up();
    }
    e.move_home();
    for _ in 0..row {
        e.move_down();
    }
    e.move_home();
    for _ in 0..col {
        e.move_right();
    }
}

#[test]
fn cut_col_range_clamps_to_the_current_line() {
    // Line 0 is "ab" (len 2); an end past the line length must NOT cross the newline into "cd".
    let mut e = Editor::with_text("ab\ncd");
    at(&mut e, 0, 0);
    let changed = e.cut_col_range(0, 5);
    assert!(changed);
    assert_eq!(
        e.text(),
        "\ncd",
        "clamped to line 0; the newline + line 1 are untouched"
    );
    assert_eq!(e.line_count(), 2);
}

#[test]
fn delete_to_line_end_at_end_of_nonlast_line_is_a_noop() {
    let mut e = Editor::with_text("one\ntwo");
    at(&mut e, 0, 3); // end of "one"
    let changed = e.delete_to_line_end();
    assert!(!changed, "nothing after the cursor on this line");
    assert_eq!(e.text(), "one\ntwo", "the joining newline is not removed");
}

#[test]
fn delete_to_line_end_removes_only_the_current_line_tail() {
    let mut e = Editor::with_text("hello\nworld");
    at(&mut e, 0, 2); // after "he"
    assert!(e.delete_to_line_end());
    assert_eq!(e.text(), "he\nworld");
}

#[test]
fn delete_char_in_line_at_line_end_is_a_noop() {
    let mut e = Editor::with_text("ab\ncd");
    at(&mut e, 0, 2); // end of "ab"
    let changed = e.delete_char_in_line();
    assert!(!changed, "x at line end deletes nothing (no newline merge)");
    assert_eq!(e.text(), "ab\ncd");
}

#[test]
fn delete_char_in_line_mid_line_removes_the_char() {
    let mut e = Editor::with_text("ab\ncd");
    at(&mut e, 0, 0);
    assert!(e.delete_char_in_line());
    assert_eq!(e.text(), "b\ncd");
}

#[test]
fn delete_whole_line_single_line_clears_text() {
    let mut e = Editor::with_text("hello");
    assert!(e.delete_whole_line());
    assert_eq!(e.text(), "");
    assert_eq!(e.line_count(), 1);
}

#[test]
fn set_text_from_normal_mode_refreshes_the_cursor_style_to_insert() {
    use crate::app::editor::EditorMode;
    use crate::theme;
    let mut e = Editor::with_text("SELECT 1");
    e.set_mode(EditorMode::Normal); // Normal mode -> the colored cursor style
    assert_eq!(e.textarea().cursor_style(), theme::app::cursor_normal());
    // A wholesale set lands the editor in Insert mode; the cursor cell style must follow it, not
    // stay colored for the prior (Normal) mode.
    e.set_text("SELECT 2");
    assert_eq!(e.mode(), EditorMode::Insert);
    assert_eq!(
        e.textarea().cursor_style(),
        theme::app::cursor(),
        "set_text must refresh the cursor style to Insert, not leave it colored for Normal"
    );
}

#[test]
fn delete_whole_line_removes_the_row_on_each_position() {
    // First line.
    let mut e = Editor::with_text("one\ntwo\nthree");
    at(&mut e, 0, 0);
    assert!(e.delete_whole_line());
    assert_eq!(e.text(), "two\nthree");
    // Middle line (mid-column).
    let mut e = Editor::with_text("one\ntwo\nthree");
    at(&mut e, 1, 2);
    assert!(e.delete_whole_line());
    assert_eq!(e.text(), "one\nthree");
    // Last line.
    let mut e = Editor::with_text("one\ntwo");
    at(&mut e, 1, 0);
    assert!(e.delete_whole_line());
    assert_eq!(e.text(), "one");
    assert_eq!(e.line_count(), 1);
}
