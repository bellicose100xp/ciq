//! The query-bar text editor — a pure buffer + cursor with character-granular edits.
//!
//! `dev/PLAN.md` §3.1 (the `editor: mutate query buffer + cursor` node). Headless and pure: it
//! takes edit operations and mutates an owned `String` + a cursor measured in **characters**
//! (not bytes), so multi-byte input (`'é'`, `'日'`) never splits a code point. Nothing here
//! touches a terminal, a clock, or the engine — the crossterm edge (`event_loop.rs`) decodes a
//! real key into one of these calls, and `app_events.rs` routes synthetic `KeyEvent`s to them in
//! tests. That split is what keeps editing in the headless majority (North Star 2).
//!
//! The cursor is a character index in `0..=chars().count()` (one past the last char is the
//! end-of-line position). All mutations clamp, so no operation can panic on an out-of-range
//! cursor.

/// A single-line text buffer with a character-indexed cursor.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Editor {
    text: String,
    /// Cursor position as a **character** index in `0..=char_count`.
    cursor: usize,
}

impl Editor {
    pub fn new() -> Self {
        Self::default()
    }

    /// An editor pre-filled with `text`, cursor at the end.
    pub fn with_text(text: impl Into<String>) -> Self {
        let text = text.into();
        let cursor = text.chars().count();
        Self { text, cursor }
    }

    /// The current buffer contents.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The cursor position as a character index.
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// Number of characters in the buffer (the max cursor position).
    pub fn char_count(&self) -> usize {
        self.text.chars().count()
    }

    /// Whether the buffer is empty (no characters).
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// The byte offset of the cursor's character index (for splicing). Always lands on a char
    /// boundary because it's derived from `char_indices`.
    fn byte_offset(&self, char_idx: usize) -> usize {
        self.text
            .char_indices()
            .nth(char_idx)
            .map(|(b, _)| b)
            .unwrap_or(self.text.len())
    }

    /// The cursor's **byte** offset into the text — the index the byte-oriented SQL lexer,
    /// clause-context detector, and autocomplete insertion all use. Always a char boundary.
    pub fn cursor_byte(&self) -> usize {
        self.byte_offset(self.cursor)
    }

    /// Set the buffer and place the cursor at the given **byte** offset (snapped to the nearest
    /// char boundary at or before it). Used by autocomplete insertion, which computes a new text +
    /// byte cursor; the editor stores the cursor as a char index, so convert here.
    pub fn set_text_with_byte_cursor(&mut self, text: impl Into<String>, byte_cursor: usize) {
        self.text = text.into();
        let byte_cursor = byte_cursor.min(self.text.len());
        // Count chars up to the byte offset; snap onto a boundary if it landed mid-char.
        self.cursor = self
            .text
            .char_indices()
            .take_while(|(b, _)| *b < byte_cursor)
            .count();
    }

    /// Insert a single character at the cursor, advancing it past the inserted char.
    pub fn insert_char(&mut self, c: char) {
        let at = self.byte_offset(self.cursor);
        self.text.insert(at, c);
        self.cursor += 1;
    }

    /// Insert a whole string at the cursor (the decoded-paste / multi-char path), advancing the
    /// cursor past all inserted characters.
    pub fn insert_str(&mut self, s: &str) {
        if s.is_empty() {
            return;
        }
        let at = self.byte_offset(self.cursor);
        self.text.insert_str(at, s);
        self.cursor += s.chars().count();
    }

    /// Delete the character before the cursor (Backspace). No-op at the start of the buffer.
    pub fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let start = self.byte_offset(self.cursor - 1);
        let end = self.byte_offset(self.cursor);
        self.text.replace_range(start..end, "");
        self.cursor -= 1;
    }

    /// Delete the character at the cursor (Delete). No-op at the end of the buffer.
    pub fn delete(&mut self) {
        if self.cursor >= self.char_count() {
            return;
        }
        let start = self.byte_offset(self.cursor);
        let end = self.byte_offset(self.cursor + 1);
        self.text.replace_range(start..end, "");
    }

    /// Move the cursor one character left. Clamped at the start.
    pub fn move_left(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    /// Move the cursor one character right. Clamped at the end.
    pub fn move_right(&mut self) {
        if self.cursor < self.char_count() {
            self.cursor += 1;
        }
    }

    /// Move the cursor to the start of the line.
    pub fn move_home(&mut self) {
        self.cursor = 0;
    }

    /// Move the cursor to the end of the line.
    pub fn move_end(&mut self) {
        self.cursor = self.char_count();
    }

    /// Replace the entire buffer with `text`, placing the cursor at the end. Used when the
    /// palette / history sets the query wholesale (later phases).
    pub fn set_text(&mut self, text: impl Into<String>) {
        self.text = text.into();
        self.cursor = self.char_count();
    }

    /// Clear the buffer and reset the cursor.
    pub fn clear(&mut self) {
        self.text.clear();
        self.cursor = 0;
    }
}

#[cfg(test)]
#[path = "editor_tests.rs"]
mod editor_tests;
