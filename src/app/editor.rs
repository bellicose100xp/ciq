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
//! whitespace, so a multiline query flows through the debounced query loop unchanged.

use ratatui::style::Style;
use tui_textarea::{CursorMove, TextArea};

use crate::theme;

/// A multiline text buffer + visible cursor, backed by [`tui_textarea::TextArea`].
pub struct Editor {
    textarea: TextArea<'static>,
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
        Self { textarea }
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

    /// Delete the character before the cursor (Backspace). No-op at the very start of the buffer.
    /// At the head of a non-first line this joins it onto the previous line.
    pub fn backspace(&mut self) {
        self.textarea.delete_char();
    }

    /// Delete the character at the cursor (Delete). No-op at the very end of the buffer. At the end
    /// of a non-last line this pulls the next line up.
    pub fn delete(&mut self) {
        self.textarea.delete_next_char();
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

    /// Replace the entire buffer with `text`, placing the cursor at the end. Used when the palette
    /// / history / AI sets the query wholesale.
    pub fn set_text(&mut self, text: impl Into<String>) {
        let text = text.into();
        self.replace_all(&text);
        self.textarea.move_cursor(CursorMove::Bottom);
        self.textarea.move_cursor(CursorMove::End);
    }

    /// Clear the buffer and reset the cursor.
    pub fn clear(&mut self) {
        self.replace_all("");
    }

    /// Replace the textarea's contents with `text` (split on `\n` into lines), preserving the
    /// cursor/line styles. The cursor lands at the start; callers reposition it as needed.
    fn replace_all(&mut self, text: &str) {
        let lines: Vec<String> = text.split('\n').map(str::to_owned).collect();
        let mut textarea = TextArea::new(lines);
        style_textarea(&mut textarea);
        self.textarea = textarea;
    }

    /// Move the textarea cursor to `(row, col)` (char indices), clamped to the buffer.
    fn move_to_row_col(&mut self, row: usize, col: usize) {
        self.textarea
            .move_cursor(CursorMove::Jump(row as u16, col as u16));
    }
}

/// Apply ciq's centralized query-bar styling to a fresh textarea: the base text style, a visible
/// reverse-video cursor cell, and a disabled cursor-line highlight (so the cursor line reads like
/// any other — jiq does the same). All styles come from [`theme::app`].
fn style_textarea(textarea: &mut TextArea<'static>) {
    textarea.set_style(theme::app::query_text());
    textarea.set_cursor_line_style(Style::default());
    textarea.set_cursor_style(theme::app::cursor());
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
