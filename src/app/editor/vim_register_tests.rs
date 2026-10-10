//! Tests for the vim register: the `y` operator (`yy`/`Y`, `yw`, `y$`, `yiw`, `yi"`, `yf`), the
//! `p`/`P` pastes, and what `d`/`c` leave behind for a following `p`. Driven straight at an
//! [`Editor`](crate::app::Editor), no `App` — the clipboard mirror is the App's job and is pinned
//! in `app_tests/vim_app_tests.rs`.

use crate::app::Editor;
use crate::app::editor::VimOutcome;
use crate::app::editor::mode::EditorMode;
use crate::app::key::KeyEvent;

/// An editor pre-filled with `text`, dropped to Normal mode with the cursor at the line start.
fn normal(text: &str) -> Editor {
    let mut e = Editor::with_text(text);
    e.set_mode(EditorMode::Normal);
    e.move_top();
    e
}

fn at_col(e: &mut Editor, col: usize) {
    e.move_home();
    for _ in 0..col {
        e.move_right();
    }
}

fn ch(e: &mut Editor, c: char) -> VimOutcome {
    e.on_vim_key(&KeyEvent::char(c))
}

// --- yanks ---

#[test]
fn yy_fills_the_register_without_touching_the_text() {
    let mut e = normal("SELECT a FROM t");
    at_col(&mut e, 3);
    ch(&mut e, 'y');
    assert_eq!(e.mode(), EditorMode::Operator('y'));
    let outcome = ch(&mut e, 'y');
    assert_eq!(outcome, VimOutcome::yanked(true));
    assert_eq!(e.yank_text(), "SELECT a FROM t");
    assert_eq!(e.text(), "SELECT a FROM t");
    assert_eq!(e.cursor_col(), 3, "yy leaves the cursor where it was");
    assert_eq!(e.mode(), EditorMode::Normal);
}

#[test]
fn shift_y_is_yy() {
    let mut e = normal("abc");
    assert_eq!(ch(&mut e, 'Y'), VimOutcome::yanked(true));
    assert_eq!(e.yank_text(), "abc");
}

#[test]
fn yy_on_an_empty_line_yanks_nothing() {
    let mut e = normal("");
    ch(&mut e, 'y');
    assert_eq!(ch(&mut e, 'y'), VimOutcome::NONE);
    assert_eq!(e.yank_text(), "");
}

#[test]
fn yy_on_a_multiline_buffer_takes_only_the_cursor_line() {
    let mut e = normal("first\nsecond");
    ch(&mut e, 'j');
    ch(&mut e, 'y');
    ch(&mut e, 'y');
    assert_eq!(e.yank_text(), "second");
    assert_eq!(e.text(), "first\nsecond");
}

#[test]
fn yw_yanks_the_word_and_keeps_the_cursor_at_the_start() {
    let mut e = normal("foo bar");
    ch(&mut e, 'y');
    assert_eq!(ch(&mut e, 'w'), VimOutcome::yanked(true));
    assert_eq!(e.yank_text(), "foo ");
    assert_eq!(e.text(), "foo bar");
    assert_eq!(e.cursor_col(), 0);
}

#[test]
fn yb_yanks_backward_and_moves_the_cursor_to_the_span_start() {
    let mut e = normal("foo bar");
    at_col(&mut e, 4);
    ch(&mut e, 'y');
    ch(&mut e, 'b');
    assert_eq!(e.yank_text(), "foo ");
    assert_eq!(
        e.cursor_col(),
        0,
        "vim parks the cursor at the start of a backward yank"
    );
}

#[test]
fn y_dollar_yanks_to_the_line_end() {
    let mut e = normal("foo bar");
    at_col(&mut e, 4);
    ch(&mut e, 'y');
    ch(&mut e, '$');
    assert_eq!(e.yank_text(), "bar");
    assert_eq!(e.cursor_col(), 4);
}

#[test]
fn yiw_yanks_the_inner_word() {
    let mut e = normal("foo bar baz");
    at_col(&mut e, 5);
    ch(&mut e, 'y');
    ch(&mut e, 'i');
    assert_eq!(
        e.mode(),
        EditorMode::TextObject('y', crate::app::editor::TextObjectScope::Inner)
    );
    assert_eq!(ch(&mut e, 'w'), VimOutcome::yanked(true));
    assert_eq!(e.yank_text(), "bar");
    assert_eq!(e.text(), "foo bar baz");
    assert_eq!(e.mode(), EditorMode::Normal);
}

#[test]
fn yi_quote_yanks_inside_the_literal() {
    let mut e = normal("x = 'hello'");
    at_col(&mut e, 6);
    ch(&mut e, 'y');
    ch(&mut e, 'i');
    ch(&mut e, '\'');
    assert_eq!(e.yank_text(), "hello");
}

#[test]
fn yf_yanks_through_the_target() {
    let mut e = normal("a, b, c");
    ch(&mut e, 'y');
    ch(&mut e, 'f');
    assert_eq!(ch(&mut e, ','), VimOutcome::yanked(true));
    assert_eq!(e.yank_text(), "a,");
    assert_eq!(e.text(), "a, b, c");
}

#[test]
fn yf_with_a_missing_target_yanks_nothing() {
    let mut e = normal("abc");
    ch(&mut e, 'y');
    ch(&mut e, 'f');
    assert_eq!(ch(&mut e, 'z'), VimOutcome::NONE);
    assert_eq!(e.yank_text(), "");
    assert_eq!(e.mode(), EditorMode::Normal);
}

#[test]
fn yw_on_the_last_word_of_a_nonlast_line_stays_within_the_line() {
    let mut e = normal("foo\nbar");
    ch(&mut e, 'y');
    ch(&mut e, 'w');
    assert_eq!(e.yank_text(), "foo");
    assert_eq!(
        e.row_col(),
        (0, 0),
        "the cursor is re-homed onto the original line"
    );
}

#[test]
fn an_invalid_yank_motion_cancels_the_operator() {
    let mut e = normal("abc");
    ch(&mut e, 'y');
    assert_eq!(ch(&mut e, 'z'), VimOutcome::NONE);
    assert_eq!(e.mode(), EditorMode::Normal);
    assert_eq!(e.yank_text(), "");
}

// --- pastes ---

#[test]
fn p_pastes_after_the_cursor_and_lands_on_the_last_pasted_char() {
    let mut e = normal("ab");
    e.set_yank_text("XY");
    assert_eq!(ch(&mut e, 'p'), VimOutcome::changed(true));
    assert_eq!(e.text(), "aXYb");
    assert_eq!(e.cursor_col(), 2, "on the Y");
    assert_eq!(e.mode(), EditorMode::Normal);
}

#[test]
fn shift_p_pastes_at_the_cursor() {
    let mut e = normal("ab");
    e.set_yank_text("XY");
    at_col(&mut e, 1);
    assert_eq!(ch(&mut e, 'P'), VimOutcome::changed(true));
    assert_eq!(e.text(), "aXYb");
    assert_eq!(e.cursor_col(), 2);
}

#[test]
fn p_at_the_line_end_appends() {
    let mut e = normal("ab");
    e.set_yank_text("Z");
    at_col(&mut e, 2); // the end-of-line column
    ch(&mut e, 'p');
    assert_eq!(e.text(), "abZ");
}

#[test]
fn p_with_an_empty_register_is_a_noop() {
    let mut e = normal("ab");
    assert_eq!(ch(&mut e, 'p'), VimOutcome::NONE);
    assert_eq!(e.text(), "ab");
}

#[test]
fn yy_then_p_duplicates_the_line_inline() {
    // ciq's register is text-only: a yanked line pastes inline, never as a new row (a SQL bar has
    // no use for line-wise pastes and the Simple panes cannot hold a second row).
    let mut e = normal("abc");
    ch(&mut e, 'y');
    ch(&mut e, 'y');
    ch(&mut e, '$');
    ch(&mut e, 'p');
    assert_eq!(e.text(), "abcabc");
    assert_eq!(e.line_count(), 1);
}

#[test]
fn dw_then_p_moves_the_word() {
    let mut e = normal("foo bar");
    ch(&mut e, 'd');
    ch(&mut e, 'w');
    assert_eq!(e.text(), "bar");
    assert_eq!(e.yank_text(), "foo ");
    ch(&mut e, '$');
    ch(&mut e, 'p');
    assert_eq!(e.text(), "barfoo ");
}

#[test]
fn dd_leaves_the_line_text_in_the_register() {
    let mut e = normal("first\nsecond\nthird");
    ch(&mut e, 'j');
    ch(&mut e, 'd');
    ch(&mut e, 'd');
    assert_eq!(e.text(), "first\nthird");
    assert_eq!(
        e.yank_text(),
        "second",
        "no joining newline in the register"
    );
    ch(&mut e, 'p');
    assert_eq!(e.text(), "first\ntsecondhird");
}

#[test]
fn cc_leaves_the_line_text_in_the_register_too() {
    let mut e = normal("abc");
    ch(&mut e, 'c');
    ch(&mut e, 'c');
    assert_eq!(e.text(), "");
    assert_eq!(e.yank_text(), "abc");
    assert_eq!(e.mode(), EditorMode::Insert);
}

#[test]
fn ci_quote_then_p_pastes_the_old_literal() {
    let mut e = normal("x = 'old'");
    at_col(&mut e, 6);
    ch(&mut e, 'c');
    ch(&mut e, 'i');
    ch(&mut e, '\'');
    assert_eq!(e.text(), "x = ''");
    assert_eq!(e.yank_text(), "old");
}

#[test]
fn p_in_a_single_line_editor_flattens_a_multiline_register() {
    let mut e = Editor::single_line();
    e.set_text("ab");
    e.set_mode(EditorMode::Normal);
    e.set_yank_text("x\ny");
    ch(&mut e, 'p');
    assert_eq!(e.text(), "abx y");
    assert_eq!(e.line_count(), 1);
}

#[test]
fn p_in_a_multiline_editor_keeps_the_register_rows() {
    let mut e = normal("ab");
    e.set_yank_text("x\ny");
    ch(&mut e, '$');
    ch(&mut e, 'p');
    assert_eq!(e.text(), "abx\ny");
    assert_eq!(e.line_count(), 2);
}

#[test]
fn set_text_keeps_the_register() {
    let mut e = normal("abc");
    ch(&mut e, 'y');
    ch(&mut e, 'y');
    e.set_text("replaced");
    assert_eq!(
        e.yank_text(),
        "abc",
        "a wholesale set does not clear the register"
    );
}
