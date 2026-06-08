//! The query-bar text editor — a thin wrapper around [`tui_textarea::TextArea`] giving ciq a
//! **multiline** query box with a visible cursor cell.
//!
//! `dev/PLAN.md` §3.1 (the `editor: mutate query buffer + cursor` node). Ported from jiq's
//! `input/input_state.rs` (same crate version) and re-justified on ciq's merits: a single-line
//! hand-rolled buffer cannot show a cursor in a headless `TestBackend` snapshot and cannot grow to
//! the multiline editing the next (vim) stage needs. The textarea renders a styled cursor cell
//! straight into the cell buffer, so the cursor is visible both in the real terminal and in a
//! `TestBackend` snapshot — no `frame.set_cursor` (which a `TestBackend` does not capture).
//!
//! This is a **seam**, not pure core: it owns a render widget. The byte<->(row,col) bridge below
//! is the load-bearing logic (the SQL lexer / autocomplete / palette all index the query by a byte
//! offset into the joined text) and is covered by its tests, but the editor as a whole is mostly
//! delegation to the textarea, so it is off the pure-core hard floor (`dev/core-modules.txt`).
//!
//! ## Newline policy (locked decision)
//!
//! Enter inserts a newline; there is no "submit" key — queries run live on debounce. [`text`]
//! joins the textarea's lines with `\n`, and the SQL preprocess/lexer already tolerate newlines as
//! whitespace, so a multiline query flows through the debounced query loop unchanged. (In vim
//! **Normal** mode, Enter is the `j` motion, not a newline — see [`vim`].)
//!
//! ## Vim layer (ported from jiq)
//!
//! The editor is **modal** like jiq's: it carries an [`EditorMode`] (default [`Insert`] so casual
//! typing works), and `Esc` drops to [`Normal`] for vim navigation/edits. The pure decision logic
//! (the mode machine, char-search column math, text-object bounds) lives in the [`mode`],
//! [`char_search`], and [`text_objects`] submodules; the key dispatch is [`vim::apply_vim_key`],
//! which drives the small set of vim primitives the `Editor` exposes below
//! (`move_word_*`, `delete_whole_line`, `cut_col_range`, …). All of it is headless and `App`-free.
//!
//! [`Insert`]: EditorMode::Insert
//! [`Normal`]: EditorMode::Normal

use ratatui::style::Style;
use tui_textarea::{CursorMove, TextArea};

use crate::theme;

pub mod char_search;
pub mod mode;
pub mod text_objects;
pub mod vim;

pub use char_search::{CharSearchState, SearchDirection, SearchType};
pub use mode::{EditorMode, TextObjectScope};
pub use text_objects::TextObjectTarget;

/// A multiline text buffer + visible cursor, backed by [`tui_textarea::TextArea`], with a vim mode
/// machine on top (`Insert` by default; `Esc` -> `Normal`).
pub struct Editor {
    textarea: TextArea<'static>,
    /// The current vim editing mode. `Insert` until `Esc`; see [`vim`].
    mode: EditorMode,
    /// The last `f`/`F`/`t`/`T` char-search, for `;` (repeat) and `,` (repeat reversed).
    last_char_search: Option<CharSearchState>,
}

impl Default for Editor {
    fn default() -> Self {
        Self::new()
    }
}

impl Editor {
    pub fn new() -> Self {
        let mut textarea = TextArea::default();
        style_textarea(&mut textarea);
        Self {
            textarea,
            mode: EditorMode::Insert,
            last_char_search: None,
        }
    }

    /// An editor pre-filled with `text`, cursor at the end.
    pub fn with_text(text: impl Into<String>) -> Self {
        let mut e = Self::new();
        e.set_text(text);
        e
    }

    /// The full query: the textarea's lines joined with `\n` (the string the preprocess, the SQL
    /// lexer, and autocomplete all operate on). Owned because the lines are joined on demand.
    pub fn text(&self) -> String {
        self.textarea.lines().join("\n")
    }

    /// The render widget. The render layer blits `&TextArea` (which implements `Widget`); the
    /// cursor cell is painted into the buffer here, not via `frame.set_cursor`.
    pub fn textarea(&self) -> &TextArea<'static> {
        &self.textarea
    }

    /// The cursor as a `(row, col)` pair of **character** indices into the corresponding line.
    pub fn row_col(&self) -> (usize, usize) {
        self.textarea.cursor()
    }

    /// The cursor position as a **character** index into the joined [`text`]. For a single-line
    /// query this equals the column; across lines each `\n` counts as one character.
    pub fn cursor(&self) -> usize {
        let (row, col) = self.textarea.cursor();
        let lines = self.textarea.lines();
        let mut idx = 0;
        for line in lines.iter().take(row) {
            idx += line.chars().count() + 1; // +1 for the joining newline
        }
        idx + col
    }

    /// Number of characters in the joined buffer (the max cursor position).
    pub fn char_count(&self) -> usize {
        self.text().chars().count()
    }

    /// Whether the buffer is empty (no characters on any line).
    pub fn is_empty(&self) -> bool {
        self.textarea.is_empty()
    }

    /// The cursor's **byte** offset into the joined [`text`] — the index the byte-oriented SQL
    /// lexer, clause-context detector, and autocomplete insertion all use. Always a char boundary
    /// (it is built up from whole lines plus a char-counted prefix of the cursor line).
    pub fn cursor_byte(&self) -> usize {
        let (row, col) = self.textarea.cursor();
        let lines = self.textarea.lines();
        let mut offset = 0;
        for line in lines.iter().take(row) {
            offset += line.len() + 1; // +1 for the joining `\n` byte
        }
        if let Some(line) = lines.get(row) {
            offset += byte_offset_of_char(line, col);
        }
        offset
    }

    /// Set the buffer and place the cursor at the given **byte** offset into the joined text
    /// (snapped to the nearest char boundary at or before it). Used by autocomplete insertion,
    /// which computes a new text + byte cursor; the textarea cursor is a `(row, col)` char pair, so
    /// convert here.
    pub fn set_text_with_byte_cursor(&mut self, text: impl Into<String>, byte_cursor: usize) {
        let text = text.into();
        let (row, col) = row_col_of_byte(&text, byte_cursor);
        self.replace_all(&text);
        self.move_to_row_col(row, col);
    }

    /// Insert a single character at the cursor, advancing it past the inserted char.
    pub fn insert_char(&mut self, c: char) {
        self.textarea.insert_char(c);
    }

    /// Insert a whole string at the cursor (the decoded-paste / multi-char path), advancing the
    /// cursor past all inserted characters. Embedded newlines split into multiple lines.
    pub fn insert_str(&mut self, s: &str) {
        if s.is_empty() {
            return;
        }
        self.textarea.insert_str(s);
    }

    /// Insert a newline at the cursor (the Enter / Shift+Enter key — newline universally, since
    /// queries run live on debounce and there is no submit key).
    pub fn insert_newline(&mut self) {
        self.textarea.insert_newline();
    }

    /// Delete the character before the cursor (Backspace / vim `X`). No-op at the very start of the
    /// buffer. At the head of a non-first line this joins it onto the previous line. Returns whether
    /// anything was removed.
    pub fn backspace(&mut self) -> bool {
        self.textarea.delete_char()
    }

    /// Delete the character at the cursor (Delete / vim `x`). No-op at the very end of the buffer.
    /// At the end of a non-last line this pulls the next line up. Returns whether anything was
    /// removed.
    pub fn delete(&mut self) -> bool {
        self.textarea.delete_next_char()
    }

    /// Move the cursor one character left (wrapping to the end of the previous line). Clamped at
    /// the start of the buffer.
    pub fn move_left(&mut self) {
        self.textarea.move_cursor(CursorMove::Back);
    }

    /// Move the cursor one character right (wrapping to the start of the next line). Clamped at the
    /// end of the buffer.
    pub fn move_right(&mut self) {
        self.textarea.move_cursor(CursorMove::Forward);
    }

    /// Move the cursor to the start of the current line.
    pub fn move_home(&mut self) {
        self.textarea.move_cursor(CursorMove::Head);
    }

    /// Move the cursor to the end of the current line.
    pub fn move_end(&mut self) {
        self.textarea.move_cursor(CursorMove::End);
    }

    /// Move the cursor up one line (within a multiline query). No-op on the first line.
    pub fn move_up(&mut self) {
        self.textarea.move_cursor(CursorMove::Up);
    }

    /// Move the cursor down one line (within a multiline query). No-op on the last line.
    pub fn move_down(&mut self) {
        self.textarea.move_cursor(CursorMove::Down);
    }

    /// Whether the cursor is on the first line of the buffer.
    pub fn is_on_first_line(&self) -> bool {
        self.textarea.cursor().0 == 0
    }

    /// Whether the cursor is on the last line of the buffer (the single-line case is always true).
    pub fn is_on_last_line(&self) -> bool {
        self.textarea.cursor().0 + 1 >= self.textarea.lines().len()
    }

    /// The number of lines in the buffer (>= 1). The render layer sizes the bar from this.
    pub fn line_count(&self) -> usize {
        self.textarea.lines().len()
    }

    // --- vim mode machine (ported from jiq; see the `vim` submodule) ---

    /// The current vim editing mode (`Insert` until `Esc`). Surfaced in the status line so the mode
    /// is visible.
    pub fn mode(&self) -> EditorMode {
        self.mode
    }

    /// Set the vim mode (used by [`vim`] transitions and by the App's focus reset), updating the
    /// cursor cell color so the mode is legible at the cursor itself (vim's block-cursor signal).
    pub fn set_mode(&mut self, mode: EditorMode) {
        self.mode = mode;
        self.textarea.set_cursor_style(cursor_style_for(mode));
    }

    /// Reset to Insert mode — called when the query bar regains focus or is set wholesale, so the
    /// user always lands in the typing mode they expect.
    pub fn reset_to_insert(&mut self) {
        self.set_mode(EditorMode::Insert);
    }

    /// Route one key through the vim mode machine. The App calls this for every non-Insert mode
    /// key, plus `Esc` from Insert (to drop to Normal). Returns `true` if the buffer text changed.
    pub fn on_vim_key(&mut self, ev: &crate::app::key::KeyEvent) -> bool {
        vim::apply_vim_key(self, ev)
    }

    /// The cursor's char column within its current line (0-based). Vim motions and the char-column
    /// cut operate per line.
    pub fn cursor_col(&self) -> usize {
        self.textarea.cursor().1
    }

    /// The text of the cursor's current line (owned; the vim char-search / text-object math reads
    /// it as a `&str`).
    pub fn current_line(&self) -> String {
        let (row, _) = self.textarea.cursor();
        self.textarea.lines().get(row).cloned().unwrap_or_default()
    }

    /// The last char-search, if any (for the App / tests).
    pub fn last_char_search(&self) -> Option<CharSearchState> {
        self.last_char_search
    }

    /// Record the last char-search (so `;` / `,` can repeat it).
    pub fn set_last_char_search(&mut self, search: CharSearchState) {
        self.last_char_search = Some(search);
    }

    /// Move to the next word boundary (`w`).
    pub fn move_word_forward(&mut self) {
        self.textarea.move_cursor(CursorMove::WordForward);
    }

    /// Move to the previous word boundary (`b`).
    pub fn move_word_back(&mut self) {
        self.textarea.move_cursor(CursorMove::WordBack);
    }

    /// Move to the end of the current/next word (`e`).
    pub fn move_word_end(&mut self) {
        self.textarea.move_cursor(CursorMove::WordEnd);
    }

    /// Move to the very top of the buffer (`gg`): first line, line start.
    pub fn move_top(&mut self) {
        self.textarea.move_cursor(CursorMove::Top);
        self.textarea.move_cursor(CursorMove::Head);
    }

    /// Move to the very bottom of the buffer (`G`): last line, line start.
    pub fn move_bottom(&mut self) {
        self.textarea.move_cursor(CursorMove::Bottom);
        self.textarea.move_cursor(CursorMove::Head);
    }

    /// Open a new empty line below the cursor and move onto it (`o`).
    pub fn open_line_below(&mut self) {
        self.textarea.move_cursor(CursorMove::End);
        self.textarea.insert_newline();
    }

    /// Open a new empty line above the cursor and move onto it (`O`).
    pub fn open_line_above(&mut self) {
        self.textarea.move_cursor(CursorMove::Head);
        self.textarea.insert_newline();
        self.textarea.move_cursor(CursorMove::Up);
    }

    /// Delete from the cursor to the end of the line (`D` / `d$` / `C`). Returns whether anything
    /// was removed.
    pub fn delete_to_line_end(&mut self) -> bool {
        self.textarea.delete_line_by_end()
    }

    /// Delete the cursor's whole line content (`dd` / `cc`). On a single-line buffer this clears
    /// the line. Returns whether anything was removed.
    pub fn delete_whole_line(&mut self) -> bool {
        let head = self.textarea.delete_line_by_head();
        let end = self.textarea.delete_line_by_end();
        head || end
    }

    /// Move the cursor `target` char-search and return whether the target was found. Translates the
    /// pure column result from [`char_search`] into a textarea cursor move.
    pub fn char_search(
        &mut self,
        target: char,
        direction: SearchDirection,
        search_type: SearchType,
    ) -> bool {
        let line = self.current_line();
        let cursor_col = self.cursor_col();
        match char_search::find_char_position(&line, cursor_col, target, direction, search_type) {
            Some(new_col) => {
                self.set_cursor_col(new_col);
                true
            }
            None => false,
        }
    }

    /// Repeat the last char-search (`;`), or reversed (`,`). No-op if there was none.
    pub fn repeat_char_search(&mut self, reverse: bool) {
        let Some(search) = self.last_char_search else {
            return;
        };
        let direction = if reverse {
            search.direction.opposite()
        } else {
            search.direction
        };
        self.char_search(search.character, direction, search.search_type);
    }

    /// Cut the `[start, end)` char-column span on the current line (used by operator + motion /
    /// char-search). Leaves the cursor at `start`. Returns whether anything was removed.
    pub fn cut_col_range(&mut self, start: usize, end: usize) -> bool {
        if start >= end {
            return false;
        }
        self.textarea.cancel_selection();
        self.set_cursor_col(start);
        self.textarea.start_selection();
        for _ in start..end {
            self.textarea.move_cursor(CursorMove::Forward);
        }
        self.textarea.cut()
    }

    /// Cut the text object (`diw`, `ci"`, `da(`, …) around the cursor. Returns whether one was
    /// found and removed. Translates the pure bounds from [`text_objects`] into a char-column cut.
    pub fn cut_text_object(&mut self, target: TextObjectTarget, scope: TextObjectScope) -> bool {
        let line = self.current_line();
        let cursor_col = self.cursor_col();
        match text_objects::find_text_object_bounds(&line, cursor_col, target, scope) {
            Some((start, end)) => self.cut_col_range(start, end),
            None => false,
        }
    }

    /// Undo the last edit (`u`). Returns whether the buffer changed.
    pub fn undo(&mut self) -> bool {
        self.textarea.undo()
    }

    /// Redo the last undone edit (`Ctrl-r`). Returns whether the buffer changed.
    pub fn redo(&mut self) -> bool {
        self.textarea.redo()
    }

    /// Move the cursor to char column `col` within its current line (clamped by the textarea).
    fn set_cursor_col(&mut self, col: usize) {
        self.textarea.move_cursor(CursorMove::Head);
        for _ in 0..col {
            self.textarea.move_cursor(CursorMove::Forward);
        }
    }

    /// Replace the entire buffer with `text`, placing the cursor at the end. Used when the palette
    /// / history / AI sets the query wholesale; resets to Insert mode so the user lands in the
    /// typing mode they expect after a wholesale set.
    pub fn set_text(&mut self, text: impl Into<String>) {
        let text = text.into();
        self.replace_all(&text);
        self.textarea.move_cursor(CursorMove::Bottom);
        self.textarea.move_cursor(CursorMove::End);
        self.mode = EditorMode::Insert;
    }

    /// Clear the buffer and reset the cursor.
    pub fn clear(&mut self) {
        self.replace_all("");
    }

    /// Replace the textarea's contents with `text` (split on `\n` into lines), preserving the
    /// cursor/line styles and the current mode's cursor color. The cursor lands at the start;
    /// callers reposition it as needed.
    fn replace_all(&mut self, text: &str) {
        let lines: Vec<String> = text.split('\n').map(str::to_owned).collect();
        let mut textarea = TextArea::new(lines);
        style_textarea(&mut textarea);
        textarea.set_cursor_style(cursor_style_for(self.mode));
        self.textarea = textarea;
    }

    /// Move the textarea cursor to `(row, col)` (char indices), clamped to the buffer.
    fn move_to_row_col(&mut self, row: usize, col: usize) {
        self.textarea
            .move_cursor(CursorMove::Jump(row as u16, col as u16));
    }
}

/// Apply ciq's centralized query-bar styling to a fresh textarea: the base text style, a visible
/// reverse-video cursor cell (Insert-mode color), and a disabled cursor-line highlight (so the
/// cursor line reads like any other — jiq does the same). All styles come from [`theme::app`].
fn style_textarea(textarea: &mut TextArea<'static>) {
    textarea.set_style(theme::app::query_text());
    textarea.set_cursor_line_style(Style::default());
    textarea.set_cursor_style(cursor_style_for(EditorMode::Insert));
}

/// The cursor cell style for a vim mode: Insert is the plain reverse-video block; every command
/// mode (Normal and the pending-key modes) uses the colored block so the mode reads at the cursor.
fn cursor_style_for(mode: EditorMode) -> Style {
    if mode.is_insert() {
        theme::app::cursor()
    } else {
        theme::app::cursor_normal()
    }
}

/// The byte offset of the `char_idx`-th character in `line` (clamped to `line.len()`). Always a
/// char boundary because it is read off `char_indices`.
fn byte_offset_of_char(line: &str, char_idx: usize) -> usize {
    line.char_indices()
        .nth(char_idx)
        .map(|(b, _)| b)
        .unwrap_or(line.len())
}

/// Convert a **byte** offset into the joined text into a `(row, col)` char-index pair (snapped to
/// the char boundary at or before the offset). The inverse of [`Editor::cursor_byte`].
fn row_col_of_byte(text: &str, byte_cursor: usize) -> (usize, usize) {
    // Snap the offset to the char boundary at or before it, so slicing below never splits a char.
    let mut byte_cursor = byte_cursor.min(text.len());
    while byte_cursor > 0 && !text.is_char_boundary(byte_cursor) {
        byte_cursor -= 1;
    }
    let mut row = 0;
    let mut line_start = 0; // byte index of the current line's first byte
    for (i, b) in text.bytes().enumerate() {
        if i >= byte_cursor {
            break;
        }
        if b == b'\n' {
            row += 1;
            line_start = i + 1;
        }
    }
    // Count chars from the line start up to (but not past) the byte cursor.
    let col = text[line_start..byte_cursor].chars().count();
    (row, col)
}

#[cfg(test)]
#[path = "editor_tests.rs"]
mod editor_tests;
