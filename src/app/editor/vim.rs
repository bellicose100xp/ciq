//! The vim key-dispatch machine for the query bar (`dev/PLAN.md` §3.1 input UX).
//!
//! Ported from jiq's `src/editor/editor_events.rs`, but re-shaped onto ciq's clean
//! [`Editor`](crate::app::Editor) boundary: where jiq scatters `app.input.textarea.*` calls across
//! `App`-coupled handlers, ciq drives the editor through the small set of vim primitives the
//! `Editor` exposes (motions, line/word deletes, char-column cut). The dispatch is therefore a
//! free function over `&mut Editor` + a neutral [`KeyEvent`](crate::app::KeyEvent) — **headless and
//! `App`-free**, so it is exercised directly against an `Editor` (North Star 2), exactly as jiq's
//! `editor_events_tests.rs` drives `app.input`.
//!
//! ## What this owns vs. what the App owns
//!
//! This function owns *Normal-and-deeper* mode handling (motions, operators, char-search, text
//! objects, and the `i`/`a`/`o` transitions back to Insert). **Insert-mode** keys are still the
//! App's existing query-bar routing ([`App::on_key_query_bar`](crate::app::App)) — typing, Enter
//! (newline, the locked decision), Backspace, autocomplete — so the live-query and autocomplete
//! wiring is unchanged. The App calls [`apply_vim_key`] only when the editor is **not** in Insert
//! mode; the one cross-over is `Esc`, which the App routes here to drop Insert -> Normal.
//!
//! Returns `true` when the buffer text changed (so the App schedules a debounced query), `false`
//! for pure motions / mode flips.

use crate::app::Editor;
use crate::app::editor::char_search::{
    CharSearchState, SearchDirection, SearchType, find_match_index,
};
use crate::app::editor::mode::{EditorMode, TextObjectScope};
use crate::app::editor::text_objects::TextObjectTarget;
use crate::app::key::{Key, KeyEvent};

/// Route one key to the editor's current (non-Insert) vim mode. The caller guarantees the editor
/// is not in Insert mode (Insert keys stay on the App's text-editing path). `Esc` from Insert is
/// routed here by the App as well, to drop to Normal.
///
/// Returns `true` if the buffer text changed.
pub fn apply_vim_key(editor: &mut Editor, ev: &KeyEvent) -> bool {
    match editor.mode() {
        EditorMode::Insert => handle_insert_escape(editor, ev),
        EditorMode::Normal => handle_normal(editor, ev),
        EditorMode::Operator(op) => handle_operator(editor, op, ev),
        EditorMode::CharSearch(dir, st) => handle_char_search(editor, dir, st, ev),
        EditorMode::OperatorCharSearch(op, start, dir, st) => {
            handle_operator_char_search(editor, op, start, dir, st, ev)
        }
        EditorMode::TextObject(op, scope) => handle_text_object(editor, op, scope, ev),
    }
}

/// The only Insert-mode key vim handles here: `Esc` drops to Normal (and steps the cursor left,
/// matching vim's "cursor moves off the just-typed column"). Anything else is a no-op (the App's
/// Insert routing handles real typing).
fn handle_insert_escape(editor: &mut Editor, ev: &KeyEvent) -> bool {
    if matches!(ev.key, Key::Esc) {
        editor.set_mode(EditorMode::Normal);
        editor.move_left();
    }
    false
}

fn handle_normal(editor: &mut Editor, ev: &KeyEvent) -> bool {
    // No Ctrl-modified chord is handled in Normal mode here: the App intercepts Ctrl+R (history),
    // Ctrl+P (palette), and Ctrl+A (AI) before the key ever reaches vim dispatch, so binding redo to
    // Ctrl+R here would be dead code. `u` (undo) is reachable; redo has no reachable binding (the
    // history chord owns Ctrl+R) and is intentionally omitted.
    match &ev.key {
        // Motions.
        Key::Char('h') | Key::Left => {
            editor.move_left();
            false
        }
        Key::Char('l') | Key::Right => {
            editor.move_right();
            false
        }
        Key::Char('0') | Key::Char('^') | Key::Home => {
            editor.move_home();
            false
        }
        Key::Char('$') | Key::End => {
            editor.move_end();
            false
        }
        Key::Char('w') => {
            editor.move_word_forward();
            false
        }
        Key::Char('b') => {
            editor.move_word_back();
            false
        }
        Key::Char('e') => {
            editor.move_word_end();
            false
        }
        // Vertical motions across a multiline query (j/k like the arrows). `Enter` is `j` in
        // Normal mode (the locked newline decision only applies to Insert mode).
        Key::Char('j') | Key::Down | Key::Enter => {
            editor.move_down();
            false
        }
        Key::Char('k') | Key::Up => {
            editor.move_up();
            false
        }
        Key::Char('g') => {
            // `gg` -> top. A bare `g` arms the pending state; the App tracks the two-key `gg`
            // through the Operator('g') slot reused as a "pending g" marker.
            editor.set_mode(EditorMode::Operator('g'));
            false
        }
        Key::Char('G') => {
            editor.move_bottom();
            false
        }

        // Insert-mode entries.
        Key::Char('i') => {
            editor.set_mode(EditorMode::Insert);
            false
        }
        Key::Char('a') => {
            editor.move_right();
            editor.set_mode(EditorMode::Insert);
            false
        }
        Key::Char('I') => {
            editor.move_home();
            editor.set_mode(EditorMode::Insert);
            false
        }
        Key::Char('A') => {
            editor.move_end();
            editor.set_mode(EditorMode::Insert);
            false
        }
        Key::Char('o') => {
            editor.open_line_below();
            editor.set_mode(EditorMode::Insert);
            true
        }
        Key::Char('O') => {
            editor.open_line_above();
            editor.set_mode(EditorMode::Insert);
            true
        }

        // Single-key edits.
        Key::Char('x') => editor.delete_char_in_line(),
        Key::Char('X') => editor.backspace(),
        Key::Char('D') => editor.delete_to_line_end(),
        Key::Char('C') => {
            let changed = editor.delete_to_line_end();
            editor.set_mode(EditorMode::Insert);
            changed
        }

        // Operators (pending a motion / text object).
        Key::Char('d') => {
            editor.set_mode(EditorMode::Operator('d'));
            false
        }
        Key::Char('c') => {
            editor.set_mode(EditorMode::Operator('c'));
            false
        }

        // Char search (pending the target char).
        Key::Char('f') => {
            editor.set_mode(EditorMode::CharSearch(
                SearchDirection::Forward,
                SearchType::Find,
            ));
            false
        }
        Key::Char('F') => {
            editor.set_mode(EditorMode::CharSearch(
                SearchDirection::Backward,
                SearchType::Find,
            ));
            false
        }
        Key::Char('t') => {
            editor.set_mode(EditorMode::CharSearch(
                SearchDirection::Forward,
                SearchType::Till,
            ));
            false
        }
        Key::Char('T') => {
            editor.set_mode(EditorMode::CharSearch(
                SearchDirection::Backward,
                SearchType::Till,
            ));
            false
        }
        Key::Char(';') => {
            editor.repeat_char_search(false);
            false
        }
        Key::Char(',') => {
            editor.repeat_char_search(true);
            false
        }

        // Undo.
        Key::Char('u') => editor.undo(),

        // Esc in Normal is a no-op (stays Normal — standard vim). Everything else is ignored.
        _ => false,
    }
}

fn handle_operator(editor: &mut Editor, operator: char, ev: &KeyEvent) -> bool {
    // `gg` — a pending `g` (parked in Operator('g')) completes on a second `g`.
    if operator == 'g' {
        editor.set_mode(EditorMode::Normal);
        if matches!(&ev.key, Key::Char('g')) {
            editor.move_top();
        }
        return false;
    }

    // `dd` / `cc` — the doubled operator deletes the whole line.
    if matches!(&ev.key, Key::Char(c) if *c == operator) {
        let changed = editor.delete_whole_line();
        editor.set_mode(if operator == 'c' {
            EditorMode::Insert
        } else {
            EditorMode::Normal
        });
        return changed;
    }

    // Operator + char-search (`df`, `ct`, …): arm the pending char-search, remembering the column.
    if let Some((direction, search_type)) = char_search_from_key(&ev.key) {
        let start_col = editor.cursor_col();
        editor.set_mode(EditorMode::OperatorCharSearch(
            operator,
            start_col,
            direction,
            search_type,
        ));
        return false;
    }

    // Operator + text object (`diw`, `ci"`, …): arm the pending text-object state.
    match &ev.key {
        Key::Char('i') => {
            editor.set_mode(EditorMode::TextObject(operator, TextObjectScope::Inner));
            return false;
        }
        Key::Char('a') => {
            editor.set_mode(EditorMode::TextObject(operator, TextObjectScope::Around));
            return false;
        }
        _ => {}
    }

    // Operator + motion (`dw`, `de`, `d$`, `c0`, …): select from the cursor across the motion and
    // cut. `de` is inclusive of the char under the new cursor, so it advances one extra column.
    let (start_row, start_col) = editor.row_col();
    let line_len = editor.current_line().chars().count();
    let motion_end = match &ev.key {
        Key::Char('w') => {
            editor.move_word_forward();
            Some(editor.cursor_col())
        }
        Key::Char('b') => {
            editor.move_word_back();
            Some(editor.cursor_col())
        }
        Key::Char('e') => {
            editor.move_word_end();
            Some(editor.cursor_col() + 1)
        }
        Key::Char('0') | Key::Char('^') | Key::Home => {
            editor.move_home();
            Some(editor.cursor_col())
        }
        Key::Char('$') | Key::End => {
            editor.move_end();
            Some(editor.cursor_col())
        }
        Key::Char('h') | Key::Left => {
            editor.move_left();
            Some(editor.cursor_col())
        }
        Key::Char('l') | Key::Right => {
            editor.move_right();
            Some(editor.cursor_col())
        }
        _ => None,
    };

    match motion_end {
        Some(end) => {
            // tui-textarea's word motions (and Forward/Back) WRAP across line boundaries, so the
            // cursor may now sit on a different row. `cut_col_range` operates on the *current* line,
            // so reading a column off the wrong row would corrupt a neighbouring line. When the
            // motion crossed lines, clamp it to the original line: a forward motion deletes to the
            // line's end, a backward motion to its head (vim keeps these operators within the line).
            let (end_row, _) = editor.row_col();
            let (lo, hi) = if end_row != start_row {
                // Re-home the cursor onto the original line so `cut_col_range` cuts the right row.
                editor.move_to_row_col(start_row, start_col);
                if end_row > start_row {
                    (start_col, line_len) // forward wrap -> to end of the original line
                } else {
                    (0, start_col) // backward wrap -> to the head of the original line
                }
            } else if start_col <= end {
                (start_col, end)
            } else {
                (end, start_col)
            };
            let changed = editor.cut_col_range(lo, hi);
            editor.set_mode(if operator == 'c' {
                EditorMode::Insert
            } else {
                EditorMode::Normal
            });
            changed
        }
        None => {
            // An invalid motion cancels the operator (back to Normal, no edit).
            editor.set_mode(EditorMode::Normal);
            false
        }
    }
}

fn handle_char_search(
    editor: &mut Editor,
    direction: SearchDirection,
    search_type: SearchType,
    ev: &KeyEvent,
) -> bool {
    if let Key::Char(target) = ev.key {
        let found = editor.char_search(target, direction, search_type);
        if found {
            editor.set_last_char_search(CharSearchState {
                character: target,
                direction,
                search_type,
            });
        }
    }
    editor.set_mode(EditorMode::Normal);
    false
}

fn handle_operator_char_search(
    editor: &mut Editor,
    operator: char,
    start_col: usize,
    direction: SearchDirection,
    search_type: SearchType,
    ev: &KeyEvent,
) -> bool {
    let target = match ev.key {
        Key::Char(ch) => ch,
        _ => {
            editor.set_mode(EditorMode::Normal);
            return false;
        }
    };

    let line = editor.current_line();
    let range = operator_char_range(&line, start_col, target, direction, search_type);
    match range {
        Some((lo, hi)) => {
            let changed = editor.cut_col_range(lo, hi);
            editor.set_mode(if operator == 'c' {
                EditorMode::Insert
            } else {
                EditorMode::Normal
            });
            changed
        }
        None => {
            editor.set_mode(EditorMode::Normal);
            false
        }
    }
}

fn handle_text_object(
    editor: &mut Editor,
    operator: char,
    scope: TextObjectScope,
    ev: &KeyEvent,
) -> bool {
    if let Key::Char(target_char) = ev.key
        && let Some(target) = TextObjectTarget::from_char(target_char)
        && editor.cut_text_object(target, scope)
    {
        editor.set_mode(if operator == 'c' {
            EditorMode::Insert
        } else {
            EditorMode::Normal
        });
        return true;
    }
    editor.set_mode(EditorMode::Normal);
    false
}

fn char_search_from_key(key: &Key) -> Option<(SearchDirection, SearchType)> {
    match key {
        Key::Char('f') => Some((SearchDirection::Forward, SearchType::Find)),
        Key::Char('F') => Some((SearchDirection::Backward, SearchType::Find)),
        Key::Char('t') => Some((SearchDirection::Forward, SearchType::Till)),
        Key::Char('T') => Some((SearchDirection::Backward, SearchType::Till)),
        _ => None,
    }
}

/// The `(start, end)` char-column span (end exclusive) an operator + char-search (`df`/`ct`/…)
/// deletes, from `cursor_col` to the target. Forward includes the cursor column; backward includes
/// the column the cursor sits on. `None` when the target isn't found in range.
fn operator_char_range(
    text: &str,
    cursor_col: usize,
    target: char,
    direction: SearchDirection,
    search_type: SearchType,
) -> Option<(usize, usize)> {
    let len = text.chars().count();
    if len == 0 || cursor_col >= len {
        return None;
    }
    // Share the char-scan with the motion path (the single hard-floor source); the operator range
    // only differs in how it turns the match index into a `[start, end)` span.
    let match_index = find_match_index(text, cursor_col, target, direction)?;
    let (start, end) = match direction {
        SearchDirection::Forward => {
            let end = match search_type {
                SearchType::Find => match_index + 1,
                SearchType::Till => match_index,
            };
            (cursor_col, end)
        }
        SearchDirection::Backward => {
            let start = match search_type {
                SearchType::Find => match_index,
                SearchType::Till => match_index + 1,
            };
            (start, cursor_col + 1)
        }
    };
    (start < end).then_some((start, end))
}

#[cfg(test)]
#[path = "vim_tests.rs"]
mod vim_tests;
