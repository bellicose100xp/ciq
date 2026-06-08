//! Tests for the vim key-dispatch machine ([`apply_vim_key`]), driven directly against an
//! [`Editor`](crate::app::Editor) — no `App`, no terminal (North Star 2). Mirrors jiq's
//! `editor_events_tests.rs` table-driven style: every mode transition + each motion/edit is a case.

use crate::app::Editor;
use crate::app::editor::mode::EditorMode;
use crate::app::key::{Key, KeyEvent, KeyMods};

/// An editor pre-filled with `text`, dropped to Normal mode with the cursor at the very top
/// (first line, line start).
fn normal(text: &str) -> Editor {
    let mut e = Editor::with_text(text);
    e.set_mode(EditorMode::Normal);
    e.move_top();
    e
}

/// Move the cursor to char column `col` on the current line.
fn at_col(e: &mut Editor, col: usize) {
    e.move_home();
    for _ in 0..col {
        e.move_right();
    }
}

/// Feed a plain printable key.
fn ch(e: &mut Editor, c: char) -> bool {
    e.on_vim_key(&KeyEvent::char(c))
}

/// Feed a plain non-char key.
fn key(e: &mut Editor, k: Key) -> bool {
    e.on_vim_key(&KeyEvent::plain(k))
}

// --- mode transitions ---

#[test]
fn esc_from_insert_enters_normal_and_steps_left() {
    let mut e = Editor::with_text("abc"); // Insert by default, cursor at end (col 3)
    assert_eq!(e.mode(), EditorMode::Insert);
    key(&mut e, Key::Esc);
    assert_eq!(e.mode(), EditorMode::Normal);
    assert_eq!(e.cursor_col(), 2); // stepped left off the just-typed column
}

#[test]
fn esc_in_normal_is_noop() {
    let mut e = normal("abc");
    key(&mut e, Key::Esc);
    assert_eq!(e.mode(), EditorMode::Normal);
}

#[test]
fn i_enters_insert_at_cursor() {
    let mut e = normal("abc");
    ch(&mut e, 'i');
    assert_eq!(e.mode(), EditorMode::Insert);
    assert_eq!(e.cursor_col(), 0);
}

#[test]
fn a_enters_insert_after_cursor() {
    let mut e = normal("abc");
    ch(&mut e, 'a');
    assert_eq!(e.mode(), EditorMode::Insert);
    assert_eq!(e.cursor_col(), 1);
}

#[test]
fn shift_i_enters_insert_at_line_start() {
    let mut e = normal("  abc");
    at_col(&mut e, 4);
    ch(&mut e, 'I');
    assert_eq!(e.mode(), EditorMode::Insert);
    assert_eq!(e.cursor_col(), 0);
}

#[test]
fn shift_a_enters_insert_at_line_end() {
    let mut e = normal("abc");
    ch(&mut e, 'A');
    assert_eq!(e.mode(), EditorMode::Insert);
    assert_eq!(e.cursor_col(), 3);
}

#[test]
fn o_opens_line_below_in_insert() {
    let mut e = normal("abc");
    let changed = ch(&mut e, 'o');
    assert!(changed);
    assert_eq!(e.mode(), EditorMode::Insert);
    assert_eq!(e.text(), "abc\n");
    assert_eq!(e.line_count(), 2);
}

#[test]
fn shift_o_opens_line_above_in_insert() {
    let mut e = normal("abc");
    let changed = ch(&mut e, 'O');
    assert!(changed);
    assert_eq!(e.mode(), EditorMode::Insert);
    assert_eq!(e.text(), "\nabc");
    assert_eq!(e.row_col(), (0, 0));
}

// --- motions (h/j/k/l, w/b/e, 0/^/$, gg/G) ---

#[test]
fn hl_move_left_right() {
    let mut e = normal("abc");
    ch(&mut e, 'l');
    assert_eq!(e.cursor_col(), 1);
    ch(&mut e, 'l');
    assert_eq!(e.cursor_col(), 2);
    ch(&mut e, 'h');
    assert_eq!(e.cursor_col(), 1);
}

#[test]
fn word_motions() {
    let mut e = normal("foo bar baz");
    ch(&mut e, 'w');
    assert_eq!(e.cursor_col(), 4); // start of "bar"
    ch(&mut e, 'e');
    assert_eq!(e.cursor_col(), 6); // end of "bar"
    ch(&mut e, 'b');
    assert_eq!(e.cursor_col(), 4); // back to start of "bar"
}

#[test]
fn zero_and_dollar_line_ends() {
    let mut e = normal("hello");
    ch(&mut e, '$');
    // `$` moves to the end-of-line position (col == line length), as jiq's `CursorMove::End` does.
    assert_eq!(e.cursor_col(), 5);
    ch(&mut e, '0');
    assert_eq!(e.cursor_col(), 0);
}

#[test]
fn caret_moves_to_line_start() {
    let mut e = normal("abc");
    ch(&mut e, '$');
    ch(&mut e, '^');
    assert_eq!(e.cursor_col(), 0);
}

#[test]
fn jk_move_between_lines() {
    let mut e = normal("one\ntwo");
    assert!(e.is_on_first_line());
    ch(&mut e, 'j');
    assert!(e.is_on_last_line());
    ch(&mut e, 'k');
    assert!(e.is_on_first_line());
}

#[test]
fn enter_in_normal_is_down_motion_not_newline() {
    let mut e = normal("one\ntwo");
    let changed = key(&mut e, Key::Enter);
    assert!(
        !changed,
        "Enter in Normal mode is the `j` motion, not a newline"
    );
    assert!(e.is_on_last_line());
    assert_eq!(e.text(), "one\ntwo");
}

#[test]
fn gg_goes_to_top_and_g_goes_to_bottom() {
    let mut e = normal("one\ntwo\nthree");
    ch(&mut e, 'G');
    assert!(e.is_on_last_line());
    // `gg` is two keys: a bare `g` arms the pending state, the second `g` jumps to the top.
    ch(&mut e, 'g');
    assert_eq!(e.mode(), EditorMode::Operator('g'));
    ch(&mut e, 'g');
    assert_eq!(e.mode(), EditorMode::Normal);
    assert!(e.is_on_first_line());
}

#[test]
fn lone_g_then_other_key_cancels() {
    let mut e = normal("abc");
    ch(&mut e, 'g');
    assert_eq!(e.mode(), EditorMode::Operator('g'));
    ch(&mut e, 'x'); // not a second g -> cancels pending, back to Normal
    assert_eq!(e.mode(), EditorMode::Normal);
    assert_eq!(e.text(), "abc"); // x did NOT delete (it was consumed cancelling the g)
}

// --- single-key edits (x, X, D, C) ---

#[test]
fn x_deletes_char_under_cursor() {
    let mut e = normal("abc");
    let changed = ch(&mut e, 'x');
    assert!(changed);
    assert_eq!(e.text(), "bc");
}

#[test]
fn shift_x_deletes_char_before_cursor() {
    let mut e = normal("abc");
    at_col(&mut e, 1);
    let changed = ch(&mut e, 'X');
    assert!(changed);
    assert_eq!(e.text(), "bc");
}

#[test]
fn shift_d_deletes_to_line_end() {
    let mut e = normal("hello world");
    at_col(&mut e, 5);
    let changed = ch(&mut e, 'D');
    assert!(changed);
    assert_eq!(e.text(), "hello");
}

#[test]
fn shift_c_deletes_to_line_end_and_enters_insert() {
    let mut e = normal("hello world");
    at_col(&mut e, 5);
    let changed = ch(&mut e, 'C');
    assert!(changed);
    assert_eq!(e.text(), "hello");
    assert_eq!(e.mode(), EditorMode::Insert);
}

// --- operator + motion (dw/de/db/d$/d0, cw) ---

#[test]
fn dw_deletes_word_forward() {
    let mut e = normal("foo bar");
    let changed = ch(&mut e, 'd');
    assert_eq!(e.mode(), EditorMode::Operator('d'));
    let changed2 = ch(&mut e, 'w');
    assert!(changed || changed2);
    assert_eq!(e.text(), "bar");
    assert_eq!(e.mode(), EditorMode::Normal);
}

#[test]
fn de_deletes_to_word_end_inclusive() {
    let mut e = normal("foo bar");
    ch(&mut e, 'd');
    ch(&mut e, 'e');
    // `de` from the start of "foo" removes "foo" (inclusive of the word-end char).
    assert_eq!(e.text(), " bar");
}

#[test]
fn db_deletes_word_backward() {
    let mut e = normal("foo bar");
    e.move_end(); // col 7 (end)
    ch(&mut e, 'd');
    ch(&mut e, 'b');
    assert!(e.text().starts_with("foo "));
}

#[test]
fn d_dollar_deletes_to_line_end() {
    let mut e = normal("hello world");
    at_col(&mut e, 5);
    ch(&mut e, 'd');
    ch(&mut e, '$');
    assert_eq!(e.text(), "hello");
}

#[test]
fn d_zero_deletes_to_line_start() {
    let mut e = normal("hello world");
    at_col(&mut e, 6);
    ch(&mut e, 'd');
    ch(&mut e, '0');
    assert_eq!(e.text(), "world");
}

#[test]
fn cw_changes_word_and_enters_insert() {
    let mut e = normal("foo bar");
    ch(&mut e, 'c');
    ch(&mut e, 'w');
    assert_eq!(e.mode(), EditorMode::Insert);
    assert!(!e.text().starts_with("foo"));
}

#[test]
fn operator_invalid_motion_cancels() {
    let mut e = normal("foo");
    ch(&mut e, 'd');
    assert_eq!(e.mode(), EditorMode::Operator('d'));
    let changed = ch(&mut e, 'z'); // not a motion
    assert!(!changed);
    assert_eq!(e.mode(), EditorMode::Normal);
    assert_eq!(e.text(), "foo");
}

// --- dd / cc (doubled operator) ---

#[test]
fn dd_deletes_whole_line() {
    let mut e = normal("hello");
    ch(&mut e, 'd');
    let changed = ch(&mut e, 'd');
    assert!(changed);
    assert_eq!(e.text(), "");
    assert_eq!(e.mode(), EditorMode::Normal);
}

#[test]
fn cc_clears_line_and_enters_insert() {
    let mut e = normal("hello");
    ch(&mut e, 'c');
    ch(&mut e, 'c');
    assert_eq!(e.text(), "");
    assert_eq!(e.mode(), EditorMode::Insert);
}

// --- char search (f/F/t/T) + repeat (; ,) ---

#[test]
fn f_finds_char_forward() {
    let mut e = normal("hello world");
    ch(&mut e, 'f');
    assert!(matches!(e.mode(), EditorMode::CharSearch(..)));
    ch(&mut e, 'w');
    assert_eq!(e.mode(), EditorMode::Normal);
    assert_eq!(e.cursor_col(), 6); // on the 'w'
}

#[test]
fn t_stops_before_char() {
    let mut e = normal("hello world");
    ch(&mut e, 't');
    ch(&mut e, 'w');
    assert_eq!(e.cursor_col(), 5); // the space just before 'w'
}

#[test]
fn shift_f_finds_char_backward() {
    let mut e = normal("hello world");
    e.move_end(); // col 10 ('d')
    ch(&mut e, 'F');
    ch(&mut e, 'o');
    assert_eq!(e.cursor_col(), 7);
}

#[test]
fn semicolon_repeats_last_char_search() {
    let mut e = normal("a.b.c.d");
    ch(&mut e, 'f');
    ch(&mut e, '.');
    assert_eq!(e.cursor_col(), 1); // first '.'
    ch(&mut e, ';');
    assert_eq!(e.cursor_col(), 3); // second '.'
    ch(&mut e, ';');
    assert_eq!(e.cursor_col(), 5); // third '.'
}

#[test]
fn comma_repeats_last_char_search_reversed() {
    let mut e = normal("a.b.c.d");
    ch(&mut e, 'f');
    ch(&mut e, '.'); // col 1
    ch(&mut e, ';'); // col 3
    ch(&mut e, ','); // reversed -> back to col 1
    assert_eq!(e.cursor_col(), 1);
}

#[test]
fn char_search_missing_target_stays_put() {
    let mut e = normal("abc");
    ch(&mut e, 'f');
    ch(&mut e, 'z'); // not present
    assert_eq!(e.cursor_col(), 0);
    assert_eq!(e.mode(), EditorMode::Normal);
}

// --- operator + char search (df/dt/ct) ---

#[test]
fn df_deletes_through_target() {
    let mut e = normal("hello world");
    ch(&mut e, 'd');
    ch(&mut e, 'f');
    ch(&mut e, ' '); // delete up to and including the space
    assert_eq!(e.text(), "world");
    assert_eq!(e.mode(), EditorMode::Normal);
}

#[test]
fn dt_deletes_up_to_target() {
    let mut e = normal("hello world");
    ch(&mut e, 'd');
    ch(&mut e, 't');
    ch(&mut e, ' '); // delete up to (not including) the space
    assert_eq!(e.text(), " world");
}

#[test]
fn ct_changes_up_to_target_and_enters_insert() {
    let mut e = normal("hello world");
    ch(&mut e, 'c');
    ch(&mut e, 't');
    ch(&mut e, ' ');
    assert_eq!(e.text(), " world");
    assert_eq!(e.mode(), EditorMode::Insert);
}

#[test]
fn df_missing_target_cancels() {
    let mut e = normal("hello");
    ch(&mut e, 'd');
    ch(&mut e, 'f');
    ch(&mut e, 'z'); // not present
    assert_eq!(e.text(), "hello");
    assert_eq!(e.mode(), EditorMode::Normal);
}

// --- operator + text object (diw/ciw/di"/da(/ci') ---

#[test]
fn diw_deletes_inner_word() {
    let mut e = normal("foo bar baz");
    at_col(&mut e, 5); // inside "bar"
    ch(&mut e, 'd');
    ch(&mut e, 'i');
    assert!(matches!(e.mode(), EditorMode::TextObject(..)));
    ch(&mut e, 'w');
    assert_eq!(e.text(), "foo  baz");
    assert_eq!(e.mode(), EditorMode::Normal);
}

#[test]
fn ci_single_quote_changes_inner_literal() {
    let mut e = normal("region = 'EU'");
    at_col(&mut e, 11); // inside 'EU'
    ch(&mut e, 'c');
    ch(&mut e, 'i');
    ch(&mut e, '\'');
    assert_eq!(e.text(), "region = ''");
    assert_eq!(e.mode(), EditorMode::Insert);
}

#[test]
fn da_paren_deletes_around_args() {
    let mut e = normal("count(id)");
    at_col(&mut e, 7); // inside (id)
    ch(&mut e, 'd');
    ch(&mut e, 'a');
    ch(&mut e, '(');
    assert_eq!(e.text(), "count");
    assert_eq!(e.mode(), EditorMode::Normal);
}

#[test]
fn text_object_not_found_cancels() {
    let mut e = normal("a b");
    at_col(&mut e, 1); // on the space, not a word char
    ch(&mut e, 'd');
    ch(&mut e, 'i');
    ch(&mut e, 'w');
    assert_eq!(e.text(), "a b");
    assert_eq!(e.mode(), EditorMode::Normal);
}

#[test]
fn text_object_invalid_target_cancels() {
    let mut e = normal("foo");
    ch(&mut e, 'd');
    ch(&mut e, 'i');
    ch(&mut e, 'z'); // not a text-object char
    assert_eq!(e.text(), "foo");
    assert_eq!(e.mode(), EditorMode::Normal);
}

// --- undo / redo ---

#[test]
fn u_undoes_and_ctrl_r_redoes() {
    let mut e = normal("hello");
    ch(&mut e, 'x'); // delete 'h' -> "ello"
    assert_eq!(e.text(), "ello");
    let undone = ch(&mut e, 'u');
    assert!(undone);
    assert_eq!(e.text(), "hello");
    let redone = e.on_vim_key(&KeyEvent::new(Key::Char('r'), KeyMods::CTRL));
    assert!(redone);
    assert_eq!(e.text(), "ello");
}

// --- pure motions report no text change ---

#[test]
fn pure_motion_reports_no_change() {
    let mut e = normal("hello");
    assert!(!ch(&mut e, 'l'));
    assert!(!ch(&mut e, 'w'));
    assert!(!ch(&mut e, '$'));
    assert!(!ch(&mut e, 'i')); // mode flip only
}
