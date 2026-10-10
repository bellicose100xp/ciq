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
//! (`move_word_*`, `delete_whole_line`, `cut_col_range`, `yank_col_range`, `paste`, …). All of it
//! is headless and `App`-free. The register behind `y`/`p` is the textarea's yank buffer, exposed
//! through [`yank_text`](Editor::yank_text) / [`set_yank_text`](Editor::set_yank_text) so an owner
//! with several editors (the Simple-mode panes) can hand one register from pane to pane.
//!
//! ## Single-line editors
//!
//! The same type backs the five single-row Simple-mode clause panes. A [`single_line`] editor
//! keeps the whole vim layer but can never grow a second row: Enter and `o`/`O` do not open a
//! line, and any newline arriving through a paste, a wholesale set, or a register paste is
//! flattened to a space. Vertical vim motions have nowhere to go inside it, so the dispatch reports
//! them as a [`LineExit`] for the owning form to turn into a pane move.
//!
//! [`Insert`]: EditorMode::Insert
//! [`Normal`]: EditorMode::Normal
//! [`single_line`]: Editor::single_line

use std::borrow::Cow;

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
pub use vim::{LineExit, VimOutcome};

/// A text buffer + visible cursor, backed by [`tui_textarea::TextArea`], with a vim mode machine on
/// top (`Insert` by default; `Esc` -> `Normal`). Multiline by default; see [`Editor::single_line`].
pub struct Editor {
    textarea: TextArea<'static>,
    /// The current vim editing mode. `Insert` until `Esc`; see [`vim`].
    mode: EditorMode,
    /// The last `f`/`F`/`t`/`T` char-search, for `;` (repeat) and `,` (repeat reversed).
    last_char_search: Option<CharSearchState>,
    /// Whether the buffer is pinned to one row (a Simple-mode clause pane).
    single_line: bool,
}

impl Default for Editor {
    fn default() -> Self {
        Self::new()
    }
}

impl Editor {
    /// A multiline editor (the Power-mode SQL textarea).
    pub fn new() -> Self {
        let mut textarea = TextArea::default();
        style_textarea(&mut textarea);
        Self {
            textarea,
            mode: EditorMode::Insert,
            last_char_search: None,
            single_line: false,
        }
    }

    /// An editor pinned to a single row (a Simple-mode clause pane): newlines never enter the
    /// buffer, however they arrive.
    pub fn single_line() -> Self {
        Self {
            single_line: true,
            ..Self::new()
        }
    }

    /// Whether this editor is pinned to one row.
    pub fn is_single_line(&self) -> bool {
        self.single_line
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
        // Map the offset on the text that actually lands in the buffer (flattened in a single-line
        // editor, char for char, so the offset still means the same character); `replace_all`
        // then finds nothing left to flatten.
        let text = self.normalize(&text);
        let (row, col) = row_col_of_byte(&text, byte_cursor);
        self.replace_all(&text);
        self.move_to_row_col(row, col);
    }

    /// Insert a single character at the cursor, advancing it past the inserted char.
    pub fn insert_char(&mut self, c: char) {
        self.textarea.insert_char(c);
    }

    /// Insert a whole string at the cursor (the decoded-paste / multi-char path), advancing the
    /// cursor past all inserted characters. Embedded newlines split into multiple lines; in a
    /// single-line editor they become spaces instead.
    pub fn insert_str(&mut self, s: &str) {
        if s.is_empty() {
            return;
        }
        let s = self.normalize(s);
        self.textarea.insert_str(s);
    }

    /// Insert a newline at the cursor (the Enter / Shift+Enter key — newline universally, since
    /// queries run live on debounce and there is no submit key). A no-op in a single-line editor.
    pub fn insert_newline(&mut self) {
        if self.single_line {
            return;
        }
        self.textarea.insert_newline();
    }

    /// Delete the character before the cursor (Backspace / vim `X`). No-op at the very start of the
    /// buffer. At the head of a non-first line this joins it onto the previous line. Returns whether
    /// anything was removed.
    pub fn backspace(&mut self) -> bool {
        self.textarea.delete_char()
    }

    /// Delete the character at the cursor (the Insert-mode `Delete` key). No-op at the very end of
    /// the buffer. At the end of a non-last line this pulls the next line up (the conventional
    /// forward-delete). Returns whether anything was removed. (Vim `x` uses [`delete_char_in_line`],
    /// which never crosses the newline.)
    pub fn delete(&mut self) -> bool {
        self.textarea.delete_next_char()
    }

    /// Delete the character at the cursor but only **within the current line** (vim `x`). A no-op at
    /// the end of the line — vim's `x` never crosses the joining newline (unlike the `Delete` key's
    /// [`delete`](Self::delete), which merges the next line up there). Returns whether anything was
    /// removed.
    pub fn delete_char_in_line(&mut self) -> bool {
        let (_, col) = self.textarea.cursor();
        if col >= self.current_line().chars().count() {
            return false; // at line end: nothing on this line to delete (no newline merge)
        }
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
    /// key, plus `Esc` from Insert (to drop to Normal). The outcome says whether the text changed,
    /// whether the register was filled, and whether a vertical motion ran off the buffer.
    pub fn on_vim_key(&mut self, ev: &crate::app::key::KeyEvent) -> VimOutcome {
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

    /// Open a new empty line below the cursor and move onto it (`o`). A single-line editor has no
    /// row to open, so there `o` only parks the cursor at the line end (the `A` position). Returns
    /// whether the buffer changed.
    pub fn open_line_below(&mut self) -> bool {
        self.textarea.move_cursor(CursorMove::End);
        if self.single_line {
            return false;
        }
        self.textarea.insert_newline();
        true
    }

    /// Open a new empty line above the cursor and move onto it (`O`). In a single-line editor it
    /// only parks the cursor at the line start (the `I` position). Returns whether the buffer
    /// changed.
    pub fn open_line_above(&mut self) -> bool {
        self.textarea.move_cursor(CursorMove::Head);
        if self.single_line {
            return false;
        }
        self.textarea.insert_newline();
        self.textarea.move_cursor(CursorMove::Up);
        true
    }

    /// Delete from the cursor to the end of the **current** line (`D` / `d$` / `C`), never crossing
    /// the joining newline. Returns whether anything was removed. (tui-textarea's
    /// `delete_line_by_end` merges the next line up at end-of-line; line-bounding it via a selection
    /// to End keeps `D`/`C` a no-op at the end of a non-last line, as vim has it.)
    pub fn delete_to_line_end(&mut self) -> bool {
        let (_, col) = self.textarea.cursor();
        let line_len = self.current_line().chars().count();
        self.cut_col_range(col, line_len)
    }

    /// Delete the cursor's whole line (`dd`) — vim line-wise: the line's text *and* its joining
    /// newline, removing the row entirely. On a one-row buffer this clears the line (the row
    /// remains, now empty). The register receives the line's text without the newline, so a
    /// following `p` pastes it inline (ciq's register is text-only; see [`paste`](Self::paste)).
    /// Returns whether anything was removed.
    pub fn delete_whole_line(&mut self) -> bool {
        if self.textarea.lines().len() <= 1 {
            return self.clear_line();
        }
        let line = self.current_line();
        let removed = self.cut_row();
        if removed {
            self.textarea.set_yank_text(line);
        }
        removed
    }

    /// Clear the cursor's line (`cc`): the text goes, the row stays so Insert resumes on an empty
    /// line, as vim has it. The register receives the cleared text. Returns whether anything was
    /// removed.
    pub fn clear_line(&mut self) -> bool {
        let line = self.current_line();
        let removed = self.cut_col_range(0, line.chars().count());
        if removed {
            self.textarea.set_yank_text(line);
        }
        removed
    }

    /// Remove the cursor's row and its joining newline from a buffer with two or more rows.
    fn cut_row(&mut self) -> bool {
        let (row, _) = self.textarea.cursor();
        let last = self.textarea.lines().len() - 1;
        self.textarea.cancel_selection();
        if row == last {
            // Last line has no trailing newline: select from the end of the previous line across
            // the *leading* newline to the end of this line, removing the row + its join.
            self.textarea.move_cursor(CursorMove::Up);
            self.textarea.move_cursor(CursorMove::End);
            self.textarea.start_selection();
            self.textarea.move_cursor(CursorMove::Down);
            self.textarea.move_cursor(CursorMove::End);
        } else {
            // Non-last line: select from this line's head to the next line's head, removing this
            // line's text + its trailing newline while leaving the next line intact.
            self.textarea.move_cursor(CursorMove::Head);
            self.textarea.start_selection();
            self.textarea.move_cursor(CursorMove::Down);
            self.textarea.move_cursor(CursorMove::Head);
        }
        self.textarea.cut()
    }

    /// Copy the cursor's whole line into the register (`yy` / `Y`), leaving the buffer and cursor
    /// untouched. Returns whether anything was yanked (an empty line yields nothing).
    pub fn yank_whole_line(&mut self) -> bool {
        let line = self.current_line();
        if line.is_empty() {
            return false;
        }
        self.textarea.set_yank_text(line);
        true
    }

    /// The register: the text the last yank or cut left behind, which `p`/`P` paste. Empty until
    /// something is yanked or cut.
    pub fn yank_text(&self) -> String {
        self.textarea.yank_text()
    }

    /// Replace the register (used to hand one register across the Simple-mode panes).
    pub fn set_yank_text(&mut self, text: impl Into<String>) {
        self.textarea.set_yank_text(text);
    }

    /// Paste the register at the cursor (`P`, `after == false`) or after the cursor's character
    /// (`p`, `after == true`), as text: a line yanked with `yy` lands inline, not as a new row. The
    /// cursor ends on the last pasted character, as in vim. Returns whether anything was inserted
    /// (an empty register pastes nothing).
    pub fn paste(&mut self, after: bool) -> bool {
        let text = self.textarea.yank_text();
        if text.is_empty() {
            return false;
        }
        if after && self.cursor_col() < self.current_line().chars().count() {
            self.move_right();
        }
        self.insert_str(&text);
        self.move_left();
        true
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
    /// char-search). Both bounds are clamped to the current line's char count, so the cut never
    /// crosses the joining newline (`CursorMove::Forward` wraps to the next line at end-of-line —
    /// the clamp keeps the primitive line-safe regardless of caller). Leaves the cursor at `start`.
    /// Returns whether anything was removed.
    pub fn cut_col_range(&mut self, start: usize, end: usize) -> bool {
        self.select_col_range(start, end).is_some() && self.textarea.cut()
    }

    /// Copy the `[start, end)` char-column span on the current line into the register (`yw`, `y$`,
    /// `yiw`, …) without changing the text. Bounds are clamped to the line like
    /// [`cut_col_range`](Self::cut_col_range). Leaves the cursor at `start`, where vim parks it
    /// after a yank. Returns whether anything was yanked.
    pub fn yank_col_range(&mut self, start: usize, end: usize) -> bool {
        let Some(start) = self.select_col_range(start, end) else {
            return false;
        };
        self.textarea.copy();
        self.set_cursor_col(start);
        true
    }

    /// Select the `[start, end)` char-column span on the current line, both bounds clamped to the
    /// line's char count so the selection never crosses the joining newline (`CursorMove::Forward`
    /// wraps at end-of-line). Returns the clamped `start`, or `None` for an empty span (nothing
    /// selected). The cut and yank primitives share this so they can never disagree on the clamp.
    fn select_col_range(&mut self, start: usize, end: usize) -> Option<usize> {
        let line_len = self.current_line().chars().count();
        let start = start.min(line_len);
        let end = end.min(line_len);
        if start >= end {
            return None;
        }
        self.textarea.cancel_selection();
        self.set_cursor_col(start);
        self.textarea.start_selection();
        self.set_cursor_col(end);
        Some(start)
    }

    /// The `[start, end)` char-column span of the text object (`iw`, `i"`, `a(`, …) around the
    /// cursor on the current line, or `None` when there is none. The operator then cuts or yanks
    /// it through the column-range primitives.
    pub fn text_object_bounds(
        &self,
        target: TextObjectTarget,
        scope: TextObjectScope,
    ) -> Option<(usize, usize)> {
        let line = self.current_line();
        text_objects::find_text_object_bounds(&line, self.cursor_col(), target, scope)
    }

    /// Undo the last edit (`u`). Returns whether the buffer changed. (There is no `redo`: vim's
    /// `Ctrl-r` collides with ciq's global history chord, which the App intercepts before any key
    /// reaches vim dispatch — so a redo binding would be unreachable.)
    pub fn undo(&mut self) -> bool {
        self.textarea.undo()
    }

    /// Move the cursor to char column `col` within its current line (clamped by the textarea). A
    /// `Jump` keeps an open selection, so this also extends one.
    fn set_cursor_col(&mut self, col: usize) {
        let (row, _) = self.textarea.cursor();
        self.move_to_row_col(row, col);
    }

    /// Move the cursor to char column `col` on the **current line** (clamped to the line end by the
    /// textarea). The click-to-position-cursor path: a mouse click in the query bar maps the click
    /// column to a text column and lands the cursor there, mirroring jiq's `set_cursor_column`.
    pub fn set_cursor_column(&mut self, col: usize) {
        self.set_cursor_col(col);
    }

    /// Move the cursor to the clicked `(row, col)` of a **multiline** query bar (the click-to-position
    /// path for the multiline editor), clamped to the buffer's last line and that line's end. Without
    /// the row the cursor would land on whatever line it was already on, ignoring the clicked line.
    pub fn set_cursor_row_col(&mut self, row: usize, col: usize) {
        let last = self.textarea.lines().len().saturating_sub(1);
        self.move_to_row_col(row.min(last), col);
    }

    /// Replace the entire buffer with `text`, placing the cursor at the end. Used when the palette
    /// / history / AI sets the query wholesale; resets to Insert mode so the user lands in the
    /// typing mode they expect after a wholesale set.
    pub fn set_text(&mut self, text: impl Into<String>) {
        let text = text.into();
        self.replace_all(&text);
        self.textarea.move_cursor(CursorMove::Bottom);
        self.textarea.move_cursor(CursorMove::End);
        // Refresh the cursor cell style to match the mode it announces — a direct field write would
        // leave the cursor colored for the old mode (replace_all styled it from the prior mode).
        self.set_mode(EditorMode::Insert);
    }

    /// Clear the buffer and reset the cursor.
    pub fn clear(&mut self) {
        self.replace_all("");
    }

    /// Replace the textarea's contents with `text` (split on `\n` into lines, or flattened to one
    /// line in a single-line editor), preserving the cursor/line styles, the current mode's cursor
    /// color, and the register. The cursor lands at the start; callers reposition it as needed.
    fn replace_all(&mut self, text: &str) {
        let text = self.normalize(text);
        let lines: Vec<String> = text.split('\n').map(str::to_owned).collect();
        let mut textarea = TextArea::new(lines);
        style_textarea(&mut textarea);
        textarea.set_cursor_style(cursor_style_for(self.mode));
        textarea.set_yank_text(self.textarea.yank_text());
        self.textarea = textarea;
    }

    /// Text as it may enter this buffer: borrowed unchanged for a multiline editor (or when there
    /// is nothing to flatten), each newline character turned into a space for a single-line one.
    /// One char per char (a CRLF becomes two spaces), so a byte offset into the source still
    /// points at the same character afterwards.
    fn normalize<'a>(&self, text: &'a str) -> Cow<'a, str> {
        if self.single_line && text.contains(['\n', '\r']) {
            Cow::Owned(text.replace(['\n', '\r'], " "))
        } else {
            Cow::Borrowed(text)
        }
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
