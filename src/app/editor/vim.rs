//! The vim key-dispatch machine for the query bar (`dev/PLAN.md` §3.1 input UX).
//!
//! Ported from jiq's `src/editor/editor_events.rs`, but re-shaped onto ciq's clean
//! [`Editor`](crate::app::Editor) boundary: where jiq scatters `app.input.textarea.*` calls across
//! `App`-coupled handlers, ciq drives the editor through the small set of vim primitives the
//! `Editor` exposes (motions, line/word deletes, char-column cut and yank, paste). The dispatch is
//! therefore a free function over `&mut Editor` + a neutral [`KeyEvent`](crate::app::KeyEvent) —
//! **headless and `App`-free**, so it is exercised directly against an `Editor` (North Star 2),
//! exactly as jiq's `editor_events_tests.rs` drives `app.input`.
//!
//! ## What this owns vs. what the App owns
//!
//! This function owns *Normal-and-deeper* mode handling (motions, operators, char-search, text
//! objects, the register, and the `i`/`a`/`o` transitions back to Insert). **Insert-mode** keys are
//! still the App's existing query-bar routing ([`App::on_key_query_bar`](crate::app::App)) —
//! typing, Enter (newline, the locked decision), Backspace, autocomplete — so the live-query and
//! autocomplete wiring is unchanged. The App calls [`apply_vim_key`] only when the editor is
//! **not** in Insert mode; the one cross-over is `Esc`, which the App routes here to drop
//! Insert -> Normal.
//!
//! The dispatch knows nothing about what surrounds the editor. Anything that reaches past the
//! buffer comes back in the [`VimOutcome`]: a text change (the App schedules a debounced query), a
//! yank (the App mirrors the register to the system clipboard), or a vertical motion that ran off
//! the buffer's edge (a [`LineExit`], which the Simple-mode form turns into a pane move and the
//! Power-mode bar into the results hand-off).

use crate::app::Editor;
use crate::app::editor::char_search::{
    CharSearchState, SearchDirection, SearchType, find_match_index,
};
use crate::app::editor::mode::{EditorMode, TextObjectScope};
use crate::app::editor::text_objects::TextObjectTarget;
use crate::app::key::{Key, KeyEvent};

/// The edge a vertical vim motion ran off: `j`/`Enter`/`Down` or `G` on the last line, `k`/`Up`
/// or `gg` on the first. In a single-line editor every vertical motion is at an edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineExit {
    Up,
    Down,
    Top,
    Bottom,
}

/// What one vim key did, for the editor's owner to act on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VimOutcome {
    /// The buffer text changed (the App schedules a debounced query).
    pub changed: bool,
    /// A `y` command filled the register (the App mirrors it to the system clipboard).
    pub yanked: bool,
    /// A vertical motion had nowhere to go inside this buffer.
    pub line_exit: Option<LineExit>,
}

impl VimOutcome {
    /// A pure motion or mode flip: nothing for the owner to do.
    pub const NONE: VimOutcome = VimOutcome {
        changed: false,
        yanked: false,
        line_exit: None,
    };

    pub fn changed(changed: bool) -> Self {
        Self {
            changed,
            ..Self::NONE
        }
    }

    pub fn yanked(yanked: bool) -> Self {
        Self {
            yanked,
            ..Self::NONE
        }
    }

    pub fn exit(exit: LineExit) -> Self {
        Self {
            line_exit: Some(exit),
            ..Self::NONE
        }
    }
}

/// Route one key to the editor's current (non-Insert) vim mode. The caller guarantees the editor
/// is not in Insert mode (Insert keys stay on the App's text-editing path). `Esc` from Insert is
/// routed here by the App as well, to drop to Normal.
pub fn apply_vim_key(editor: &mut Editor, ev: &KeyEvent) -> VimOutcome {
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
fn handle_insert_escape(editor: &mut Editor, ev: &KeyEvent) -> VimOutcome {
    if matches!(ev.key, Key::Esc) {
        editor.set_mode(EditorMode::Normal);
        editor.move_left();
    }
    VimOutcome::NONE
}

fn handle_normal(editor: &mut Editor, ev: &KeyEvent) -> VimOutcome {
    // No Ctrl-modified chord is handled in Normal mode here: the App intercepts Ctrl+R (history),
    // Ctrl+P (palette), and Ctrl+A (AI) before the key ever reaches vim dispatch, so binding redo to
    // Ctrl+R here would be dead code. `u` (undo) is reachable; redo has no reachable binding (the
    // history chord owns Ctrl+R) and is intentionally omitted.
    match &ev.key {
        // Motions.
        Key::Char('h') | Key::Left => {
            editor.move_left();
            VimOutcome::NONE
        }
        Key::Char('l') | Key::Right => {
            editor.move_right();
            VimOutcome::NONE
        }
        Key::Char('0') | Key::Char('^') | Key::Home => {
            editor.move_home();
            VimOutcome::NONE
        }
        Key::Char('$') | Key::End => {
            editor.move_end();
            VimOutcome::NONE
        }
        Key::Char('w') => {
            editor.move_word_forward();
            VimOutcome::NONE
        }
        Key::Char('b') => {
            editor.move_word_back();
            VimOutcome::NONE
        }
        Key::Char('e') => {
            editor.move_word_end();
            VimOutcome::NONE
        }
        // Vertical motions across a multiline query (j/k like the arrows). `Enter` is `j` in
        // Normal mode (the locked newline decision only applies to Insert mode). A motion with
        // nowhere to go inside the buffer is reported instead of swallowed, so a form of stacked
        // single-line panes can treat the panes as the lines; the same edge rule serves `gg`/`G`.
        Key::Char('j') | Key::Down | Key::Enter => {
            if editor.is_on_last_line() {
                return VimOutcome::exit(LineExit::Down);
            }
            editor.move_down();
            VimOutcome::NONE
        }
        Key::Char('k') | Key::Up => {
            if editor.is_on_first_line() {
                return VimOutcome::exit(LineExit::Up);
            }
            editor.move_up();
            VimOutcome::NONE
        }
        Key::Char('g') => {
            // `gg` -> top. A bare `g` arms the pending state; the App tracks the two-key `gg`
            // through the Operator('g') slot reused as a "pending g" marker.
            editor.set_mode(EditorMode::Operator('g'));
            VimOutcome::NONE
        }
        Key::Char('G') => {
            if editor.is_on_last_line() {
                return VimOutcome::exit(LineExit::Bottom);
            }
            editor.move_bottom();
            VimOutcome::NONE
        }

        // Insert-mode entries.
        Key::Char('i') => {
            editor.set_mode(EditorMode::Insert);
            VimOutcome::NONE
        }
        Key::Char('a') => {
            editor.move_right();
            editor.set_mode(EditorMode::Insert);
            VimOutcome::NONE
        }
        Key::Char('I') => {
            editor.move_home();
            editor.set_mode(EditorMode::Insert);
            VimOutcome::NONE
        }
        Key::Char('A') => {
            editor.move_end();
            editor.set_mode(EditorMode::Insert);
            VimOutcome::NONE
        }
        Key::Char('o') => {
            let changed = editor.open_line_below();
            editor.set_mode(EditorMode::Insert);
            VimOutcome::changed(changed)
        }
        Key::Char('O') => {
            let changed = editor.open_line_above();
            editor.set_mode(EditorMode::Insert);
            VimOutcome::changed(changed)
        }

        // Single-key edits.
        Key::Char('x') => VimOutcome::changed(editor.delete_char_in_line()),
        Key::Char('X') => VimOutcome::changed(editor.backspace()),
        Key::Char('D') => VimOutcome::changed(editor.delete_to_line_end()),
        Key::Char('C') => {
            let changed = editor.delete_to_line_end();
            editor.set_mode(EditorMode::Insert);
            VimOutcome::changed(changed)
        }

        // The register: `Y` yanks the line (vim's `yy` alias), `p`/`P` paste after/at the cursor.
        Key::Char('Y') => VimOutcome::yanked(editor.yank_whole_line()),
        Key::Char('p') => VimOutcome::changed(editor.paste(true)),
        Key::Char('P') => VimOutcome::changed(editor.paste(false)),

        // Operators (pending a motion / text object).
        Key::Char('d') => {
            editor.set_mode(EditorMode::Operator('d'));
            VimOutcome::NONE
        }
        Key::Char('c') => {
            editor.set_mode(EditorMode::Operator('c'));
            VimOutcome::NONE
        }
        Key::Char('y') => {
            editor.set_mode(EditorMode::Operator('y'));
            VimOutcome::NONE
        }

        // Char search (pending the target char).
        Key::Char('f') => {
            editor.set_mode(EditorMode::CharSearch(
                SearchDirection::Forward,
                SearchType::Find,
            ));
            VimOutcome::NONE
        }
        Key::Char('F') => {
            editor.set_mode(EditorMode::CharSearch(
                SearchDirection::Backward,
                SearchType::Find,
            ));
            VimOutcome::NONE
        }
        Key::Char('t') => {
            editor.set_mode(EditorMode::CharSearch(
                SearchDirection::Forward,
                SearchType::Till,
            ));
            VimOutcome::NONE
        }
        Key::Char('T') => {
            editor.set_mode(EditorMode::CharSearch(
                SearchDirection::Backward,
                SearchType::Till,
            ));
            VimOutcome::NONE
        }
        Key::Char(';') => {
            editor.repeat_char_search(false);
            VimOutcome::NONE
        }
        Key::Char(',') => {
            editor.repeat_char_search(true);
            VimOutcome::NONE
        }

        // Undo.
        Key::Char('u') => VimOutcome::changed(editor.undo()),

        // A bracketed paste inserts in Normal mode too, as vim does with a terminal paste.
        Key::Paste(s) => {
            editor.insert_str(s);
            VimOutcome::changed(!s.is_empty())
        }

        // Esc in Normal is a no-op (stays Normal — standard vim). Everything else is ignored.
        _ => VimOutcome::NONE,
    }
}

fn handle_operator(editor: &mut Editor, operator: char, ev: &KeyEvent) -> VimOutcome {
    // `gg` — a pending `g` (parked in Operator('g')) completes on a second `g`.
    if operator == 'g' {
        editor.set_mode(EditorMode::Normal);
        if !matches!(&ev.key, Key::Char('g')) {
            return VimOutcome::NONE;
        }
        if editor.is_on_first_line() {
            return VimOutcome::exit(LineExit::Top);
        }
        editor.move_top();
        return VimOutcome::NONE;
    }

    // `dd` / `cc` / `yy` — the doubled operator acts on the whole line: `dd` removes the row, `cc`
    // empties it and types there, `yy` copies it.
    if matches!(&ev.key, Key::Char(c) if *c == operator) {
        let outcome = match operator {
            'y' => VimOutcome::yanked(editor.yank_whole_line()),
            'c' => VimOutcome::changed(editor.clear_line()),
            _ => VimOutcome::changed(editor.delete_whole_line()),
        };
        editor.set_mode(mode_after(operator));
        return outcome;
    }

    // Operator + char-search (`df`, `ct`, `yt`, …): arm the pending char-search, remembering the
    // column.
    if let Some((direction, search_type)) = char_search_from_key(&ev.key) {
        let start_col = editor.cursor_col();
        editor.set_mode(EditorMode::OperatorCharSearch(
            operator,
            start_col,
            direction,
            search_type,
        ));
        return VimOutcome::NONE;
    }

    // Operator + text object (`diw`, `ci"`, `yi(`, …): arm the pending text-object state.
    match &ev.key {
        Key::Char('i') => {
            editor.set_mode(EditorMode::TextObject(operator, TextObjectScope::Inner));
            return VimOutcome::NONE;
        }
        Key::Char('a') => {
            editor.set_mode(EditorMode::TextObject(operator, TextObjectScope::Around));
            return VimOutcome::NONE;
        }
        _ => {}
    }

    // Operator + motion (`dw`, `de`, `d$`, `c0`, `yw`, …): span from the cursor across the motion
    // and apply the operator. `de` is inclusive of the char under the new cursor, so it advances
    // one extra column.
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
            // cursor may now sit on a different row. The column-range primitives operate on the
            // *current* line, so reading a column off the wrong row would corrupt a neighbouring
            // line. When the motion crossed lines, clamp it to the original line: a forward motion
            // reaches the line's end, a backward motion its head (vim keeps these operators within
            // the line).
            let (end_row, _) = editor.row_col();
            let (lo, hi) = if end_row != start_row {
                // Re-home the cursor onto the original line so the range lands on the right row.
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
            operate(editor, operator, lo, hi)
        }
        None => {
            // An invalid motion cancels the operator (back to Normal, no edit).
            editor.set_mode(EditorMode::Normal);
            VimOutcome::NONE
        }
    }
}

fn handle_char_search(
    editor: &mut Editor,
    direction: SearchDirection,
    search_type: SearchType,
    ev: &KeyEvent,
) -> VimOutcome {
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
    VimOutcome::NONE
}

fn handle_operator_char_search(
    editor: &mut Editor,
    operator: char,
    start_col: usize,
    direction: SearchDirection,
    search_type: SearchType,
    ev: &KeyEvent,
) -> VimOutcome {
    let target = match ev.key {
        Key::Char(ch) => ch,
        _ => {
            editor.set_mode(EditorMode::Normal);
            return VimOutcome::NONE;
        }
    };

    let line = editor.current_line();
    match operator_char_range(&line, start_col, target, direction, search_type) {
        Some((lo, hi)) => operate(editor, operator, lo, hi),
        None => {
            editor.set_mode(EditorMode::Normal);
            VimOutcome::NONE
        }
    }
}

fn handle_text_object(
    editor: &mut Editor,
    operator: char,
    scope: TextObjectScope,
    ev: &KeyEvent,
) -> VimOutcome {
    if let Key::Char(target_char) = ev.key
        && let Some(target) = TextObjectTarget::from_char(target_char)
        && let Some((lo, hi)) = editor.text_object_bounds(target, scope)
    {
        return operate(editor, operator, lo, hi);
    }
    editor.set_mode(EditorMode::Normal);
    VimOutcome::NONE
}

/// Apply a completed operator to the `[lo, hi)` char span on the current line: `d` and `c` cut it
/// into the register (`c` then enters Insert), `y` copies it and stays in Normal.
fn operate(editor: &mut Editor, operator: char, lo: usize, hi: usize) -> VimOutcome {
    if operator == 'y' {
        let yanked = editor.yank_col_range(lo, hi);
        editor.set_mode(EditorMode::Normal);
        return VimOutcome::yanked(yanked);
    }
    let changed = editor.cut_col_range(lo, hi);
    editor.set_mode(mode_after(operator));
    VimOutcome::changed(changed)
}

/// The mode a completed operator leaves the editor in: `c` goes on to type, `d` and `y` stay put.
fn mode_after(operator: char) -> EditorMode {
    if operator == 'c' {
        EditorMode::Insert
    } else {
        EditorMode::Normal
    }
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

/// The `(start, end)` char-column span (end exclusive) an operator + char-search (`df`/`ct`/`yf`/…)
/// covers, from `cursor_col` to the target. Forward includes the cursor column; backward includes
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

#[cfg(test)]
#[path = "vim_register_tests.rs"]
mod vim_register_tests;

#[cfg(test)]
#[path = "vim_line_exit_tests.rs"]
mod vim_line_exit_tests;
