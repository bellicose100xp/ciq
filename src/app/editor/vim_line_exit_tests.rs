//! Tests for the edge of the buffer: which vertical vim motions report a
//! [`LineExit`](crate::app::editor::LineExit) and which move inside the buffer instead, and how the
//! single-line editor keeps every vim command on one row. Headless, straight at an
//! [`Editor`](crate::app::Editor); the pane moves those exits drive live in
//! `app_tests/simple_mode_tests.rs`.

use crate::app::Editor;
use crate::app::editor::mode::EditorMode;
use crate::app::editor::{LineExit, VimOutcome};
use crate::app::key::{Key, KeyEvent};

fn normal(text: &str) -> Editor {
    let mut e = Editor::with_text(text);
    e.set_mode(EditorMode::Normal);
    e.move_top();
    e
}

fn single_line(text: &str) -> Editor {
    let mut e = Editor::single_line();
    e.set_text(text);
    e.set_mode(EditorMode::Normal);
    e.move_home();
    e
}

fn ch(e: &mut Editor, c: char) -> VimOutcome {
    e.on_vim_key(&KeyEvent::char(c))
}

fn key(e: &mut Editor, k: Key) -> VimOutcome {
    e.on_vim_key(&KeyEvent::plain(k))
}

// --- multiline buffers: motions inside the buffer never exit ---

#[test]
fn j_inside_a_multiline_buffer_moves_down_without_an_exit() {
    let mut e = normal("a\nb\nc");
    assert_eq!(ch(&mut e, 'j'), VimOutcome::NONE);
    assert_eq!(e.row_col(), (1, 0));
}

#[test]
fn j_on_the_last_line_reports_a_down_exit() {
    let mut e = normal("a\nb");
    ch(&mut e, 'j');
    assert_eq!(ch(&mut e, 'j'), VimOutcome::exit(LineExit::Down));
    assert_eq!(e.row_col(), (1, 0), "the cursor stays put");
}

#[test]
fn down_and_enter_are_j_at_the_edge_too() {
    let mut e = normal("only");
    assert_eq!(key(&mut e, Key::Down), VimOutcome::exit(LineExit::Down));
    assert_eq!(key(&mut e, Key::Enter), VimOutcome::exit(LineExit::Down));
    assert_eq!(
        e.text(),
        "only",
        "Enter in Normal mode never inserts a newline"
    );
}

#[test]
fn k_on_the_first_line_reports_an_up_exit() {
    let mut e = normal("a\nb");
    assert_eq!(ch(&mut e, 'k'), VimOutcome::exit(LineExit::Up));
    assert_eq!(key(&mut e, Key::Up), VimOutcome::exit(LineExit::Up));
    ch(&mut e, 'j');
    assert_eq!(
        ch(&mut e, 'k'),
        VimOutcome::NONE,
        "k from line 2 moves inside the buffer"
    );
}

#[test]
fn gg_and_shift_g_in_a_multiline_buffer_jump_inside_it() {
    let mut e = normal("a\nb\nc");
    assert_eq!(ch(&mut e, 'G'), VimOutcome::NONE);
    assert_eq!(e.row_col(), (2, 0));
    ch(&mut e, 'g');
    assert_eq!(ch(&mut e, 'g'), VimOutcome::NONE);
    assert_eq!(e.row_col(), (0, 0));
}

#[test]
fn gg_and_shift_g_at_their_own_edge_report_exits_in_any_editor() {
    // The edge rule is the same for every vertical motion: `G` on the last line and `gg` on the
    // first have nowhere to go, so a one-row Power buffer reports the exit too (its owner decides
    // what, if anything, that means; Power ignores Top/Bottom).
    let mut e = normal("one");
    assert_eq!(ch(&mut e, 'G'), VimOutcome::exit(LineExit::Bottom));
    ch(&mut e, 'g');
    assert_eq!(ch(&mut e, 'g'), VimOutcome::exit(LineExit::Top));
    assert_eq!(e.mode(), EditorMode::Normal);
}

// --- single-line editors: every vertical motion is an exit ---

#[test]
fn single_line_j_and_k_report_exits_without_moving() {
    let mut e = single_line("abc");
    e.move_right();
    assert_eq!(ch(&mut e, 'j'), VimOutcome::exit(LineExit::Down));
    assert_eq!(ch(&mut e, 'k'), VimOutcome::exit(LineExit::Up));
    assert_eq!(e.cursor_col(), 1);
    assert_eq!(e.mode(), EditorMode::Normal);
}

#[test]
fn single_line_gg_reports_a_top_exit() {
    let mut e = single_line("abc");
    assert_eq!(ch(&mut e, 'g'), VimOutcome::NONE);
    assert_eq!(e.mode(), EditorMode::Operator('g'));
    assert_eq!(ch(&mut e, 'g'), VimOutcome::exit(LineExit::Top));
    assert_eq!(e.mode(), EditorMode::Normal);
}

#[test]
fn single_line_shift_g_reports_a_bottom_exit() {
    let mut e = single_line("abc");
    assert_eq!(ch(&mut e, 'G'), VimOutcome::exit(LineExit::Bottom));
}

#[test]
fn a_lone_g_followed_by_another_key_cancels_without_an_exit() {
    let mut e = single_line("abc");
    ch(&mut e, 'g');
    assert_eq!(ch(&mut e, 'x'), VimOutcome::NONE);
    assert_eq!(
        e.text(),
        "abc",
        "the cancelling key is swallowed, not executed"
    );
    assert_eq!(e.mode(), EditorMode::Normal);
}

// --- single-line editors: o / O cannot open a row ---

#[test]
fn single_line_o_enters_insert_at_the_line_end_without_a_newline() {
    let mut e = single_line("abc");
    assert_eq!(ch(&mut e, 'o'), VimOutcome::NONE);
    assert_eq!(e.text(), "abc");
    assert_eq!(e.line_count(), 1);
    assert_eq!(e.mode(), EditorMode::Insert);
    assert_eq!(e.cursor_col(), 3);
}

#[test]
fn single_line_shift_o_enters_insert_at_the_line_start_without_a_newline() {
    let mut e = single_line("abc");
    e.move_end();
    assert_eq!(ch(&mut e, 'O'), VimOutcome::NONE);
    assert_eq!(e.text(), "abc");
    assert_eq!(e.mode(), EditorMode::Insert);
    assert_eq!(e.cursor_col(), 0);
}

#[test]
fn multiline_o_still_opens_a_row_and_reports_the_change() {
    let mut e = normal("abc");
    let outcome = ch(&mut e, 'o');
    assert!(outcome.changed);
    assert_eq!(e.text(), "abc\n");
    assert_eq!(e.row_col(), (1, 0));
}

// --- the single-line editor outside vim ---

#[test]
fn single_line_editor_ignores_enter() {
    let mut e = Editor::single_line();
    e.insert_str("abc");
    e.insert_newline();
    assert_eq!(e.text(), "abc");
    assert_eq!(e.line_count(), 1);
}

#[test]
fn single_line_editor_flattens_pasted_newlines_to_spaces() {
    let mut e = Editor::single_line();
    // Char for char: a CRLF becomes two spaces, so byte offsets into the source stay valid.
    e.insert_str("a\nb\r\nc");
    assert_eq!(e.text(), "a b  c");
    assert_eq!(e.line_count(), 1);
    assert_eq!(e.cursor_col(), 6);
}

#[test]
fn single_line_editor_flattens_a_wholesale_set() {
    let mut e = Editor::single_line();
    e.set_text("x > 1\n  AND y < 2");
    assert_eq!(e.text(), "x > 1   AND y < 2");
    assert_eq!(e.line_count(), 1);
}

#[test]
fn single_line_set_text_with_byte_cursor_maps_the_offset_onto_the_flattened_text() {
    let mut e = Editor::single_line();
    // "ab\ncd" with the cursor after the "c": byte 4 in the source, column 4 once flattened.
    e.set_text_with_byte_cursor("ab\ncd", 4);
    assert_eq!(e.text(), "ab cd");
    assert_eq!(e.cursor_col(), 4);
    // A CRLF source: byte 5 is still "after the c" because each newline byte became one space.
    e.set_text_with_byte_cursor("ab\r\ncd", 5);
    assert_eq!(e.text(), "ab  cd");
    assert_eq!(e.cursor_col(), 5);
}

#[test]
fn a_paste_in_normal_mode_inserts_at_the_cursor() {
    let mut e = normal("ab");
    e.move_end();
    let outcome = e.on_vim_key(&KeyEvent::plain(Key::Paste("XY".into())));
    assert!(outcome.changed);
    assert_eq!(e.text(), "abXY");
    assert_eq!(e.mode(), EditorMode::Normal);
    let outcome = single_line("ab").on_vim_key(&KeyEvent::plain(Key::Paste("x\ny".into())));
    assert!(outcome.changed);
}

#[test]
fn a_paste_in_a_single_line_pane_in_normal_mode_stays_on_one_row() {
    let mut e = single_line("ab");
    e.move_end();
    e.on_vim_key(&KeyEvent::plain(Key::Paste("x\ny".into())));
    assert_eq!(e.text(), "abx y");
    assert_eq!(e.line_count(), 1);
}

#[test]
fn multiline_editor_is_untouched_by_the_single_line_rules() {
    let mut e = Editor::new();
    assert!(!e.is_single_line());
    e.insert_str("a\nb");
    e.insert_newline();
    assert_eq!(e.text(), "a\nb\n");
    assert_eq!(e.line_count(), 3);
}

#[test]
fn yank_col_range_copies_without_mutating_and_parks_the_cursor() {
    let mut e = normal("hello world");
    e.move_end();
    assert!(e.yank_col_range(6, 11));
    assert_eq!(e.yank_text(), "world");
    assert_eq!(e.text(), "hello world");
    assert_eq!(e.cursor_col(), 6);
    assert!(!e.yank_col_range(3, 3), "an empty span yanks nothing");
    assert!(
        !e.yank_col_range(20, 30),
        "a span past the line end clamps to nothing"
    );
}
