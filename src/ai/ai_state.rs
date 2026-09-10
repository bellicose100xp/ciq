//! The AI chat popup state machine — pure owned data, pure transitions (`dev/PLAN.md` §7 P5.1).
//!
//! One surface, opened with `Ctrl+A`: a conversation. The user types a question, the model answers
//! with prose plus pickable SQL, and the next question carries the prior turns so it can refine
//! ("now group by status"). The transcript lives in [`Conversation`] and survives closing the popup,
//! so reopening resumes the same thread; `Ctrl+L` starts over.
//!
//! ciq is synchronous, so there is no chunk accumulation: a whole answer arrives at once over the
//! AI thread's channel (see [`ai_app`](super::ai_app)). All transitions are `&mut self` over plain
//! data and are unit-tested with plain asserts — no terminal, no engine, no clock. The input is
//! edited char-by-char (UTF-8 safe: whole `char`s in and out, never byte slices).

use super::chat::{Conversation, Turn};
use super::suggestion::{ChatReply, Suggestion};

/// Where the current request is in its lifecycle. The transcript is separate — these are only about
/// what is in flight.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum AiPhase {
    /// Idle: accepting input (the default).
    #[default]
    Editing,
    /// A message was sent; waiting for the model.
    Pending,
    /// The last request failed; carries the user-facing message. The transcript is untouched, so the
    /// conversation continues from where it was once the user retries.
    Error(String),
}

/// The AI chat popup: whether it is open, the transcript, the line being typed, the request phase,
/// the cursor into the transcript's suggestions, and the scroll offset.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AiState {
    open: bool,
    /// Whether the chat owns the keyboard. The popup can stay visible while the user types in the
    /// query bar or scrolls the grid; a click in the chat or `Ctrl+A` hands the keyboard back.
    focused: bool,
    /// The conversation so far. Outlives the popup being closed.
    chat: Conversation,
    /// The line the user is typing.
    input: String,
    /// The request lifecycle phase.
    phase: AiPhase,
    /// Cursor into [`Conversation::all_suggestions`], once the user navigates. `None` means no row
    /// is selected, so `Enter` sends the typed line instead of applying a suggestion.
    selected: Option<usize>,
    /// First visible transcript row, for scrolling back through a long conversation. `usize::MAX`
    /// means "follow the tail"; the renderer clamps it into the real window.
    scroll: usize,
}

impl AiState {
    /// A closed, empty chat.
    pub fn new() -> Self {
        Self::default()
    }

    /// Open the popup. The transcript is **kept** — reopening resumes the conversation. Clears the
    /// half-typed line and any stale error so the popup opens ready to type.
    pub fn open(&mut self) {
        self.open_unfocused();
        self.focused = true;
    }

    /// Show the popup without taking the keyboard — the startup case, where the user expects to
    /// start typing SQL, and the assistant should be visible but not in the way.
    pub fn open_unfocused(&mut self) {
        self.open = true;
        self.focused = false;
        self.input.clear();
        self.selected = None;
        if matches!(self.phase, AiPhase::Error(_)) {
            self.phase = AiPhase::Editing;
        }
    }

    /// Close the popup, keeping the transcript for the next open.
    pub fn close(&mut self) {
        self.open = false;
        self.focused = false;
        self.input.clear();
        self.selected = None;
    }

    /// Whether the chat owns the keyboard (always false while closed).
    pub fn is_focused(&self) -> bool {
        self.open && self.focused
    }

    /// Give the chat the keyboard (a click inside it, or `Ctrl+A` while it is visible).
    pub fn focus(&mut self) {
        if self.open {
            self.focused = true;
        }
    }

    /// Hand the keyboard back to the rest of the app, leaving the popup visible.
    pub fn blur(&mut self) {
        self.focused = false;
        self.selected = None;
    }

    /// Whether the popup is currently open.
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// The conversation so far.
    pub fn chat(&self) -> &Conversation {
        &self.chat
    }

    /// The line being typed.
    pub fn input(&self) -> &str {
        &self.input
    }

    /// The current request phase.
    pub fn phase(&self) -> &AiPhase {
        &self.phase
    }

    /// Whether a request is in flight — the App gates sending on this so one Enter never fires two
    /// overlapping requests.
    pub fn is_pending(&self) -> bool {
        matches!(self.phase, AiPhase::Pending)
    }

    /// The cursor into the flattened suggestion list, if the user has navigated.
    pub fn selected(&self) -> Option<usize> {
        self.selected
    }

    /// The first visible transcript row.
    pub fn scroll(&self) -> usize {
        self.scroll
    }

    /// Whether anything has been said yet (drives the popup's empty-state hint).
    pub fn is_fresh(&self) -> bool {
        self.chat.is_empty()
    }

    /// Append a typed character. Ignored while a request is in flight, so a keystroke mid-request is
    /// never folded into the next message. UTF-8 safe (whole char).
    pub fn push_char(&mut self, c: char) {
        if !self.is_pending() {
            self.clear_error();
            self.input.push(c);
        }
    }

    /// Remove the last character (Backspace). Same gating as [`push_char`](Self::push_char).
    pub fn backspace(&mut self) {
        if !self.is_pending() {
            self.clear_error();
            self.input.pop();
        }
    }

    /// Insert a pasted payload, flattening newlines to spaces (the input is one line).
    pub fn paste(&mut self, text: &str) {
        if self.is_pending() {
            return;
        }
        self.clear_error();
        for c in text.chars() {
            self.input
                .push(if c == '\n' || c == '\r' { ' ' } else { c });
        }
    }

    /// A typed edit clears a stale error banner so the popup stops showing a failure the user has
    /// moved on from.
    fn clear_error(&mut self) {
        if matches!(self.phase, AiPhase::Error(_)) {
            self.phase = AiPhase::Editing;
        }
    }

    /// Commit the typed line as a user turn and go `Pending`. Returns the trimmed question so the
    /// caller builds the model message from exactly what was committed. `None` when a request is
    /// already in flight or the line is blank.
    pub fn send(&mut self) -> Option<String> {
        if self.is_pending() {
            return None;
        }
        let question = self.input.trim().to_string();
        if question.is_empty() {
            return None;
        }
        self.input.clear();
        self.chat.push(Turn::user(&question));
        self.phase = AiPhase::Pending;
        self.selected = None;
        self.scroll_to_end();
        Some(question)
    }

    /// Record the automatic "this query failed" user turn and go `Pending`. `notice` is what the
    /// transcript shows; the caller sends the full generated message. `None` when a request is
    /// already in flight, so a burst of failures never stacks requests.
    pub fn begin_failure_turn(&mut self, notice: impl Into<String>) -> bool {
        if self.is_pending() {
            return false;
        }
        self.chat.push(Turn::user(notice));
        self.phase = AiPhase::Pending;
        self.selected = None;
        self.scroll_to_end();
        true
    }

    /// Record the model's answer as an assistant turn and return to `Editing`.
    pub fn set_reply(&mut self, reply: ChatReply) {
        self.chat
            .push(Turn::assistant(reply.reply, reply.suggestions));
        self.phase = AiPhase::Editing;
        self.selected = None;
        self.scroll_to_end();
    }

    /// Record a failed request. The transcript keeps the pending question, so the user can retype or
    /// ask again without losing context.
    pub fn set_error(&mut self, message: impl Into<String>) {
        self.phase = AiPhase::Error(message.into());
    }

    /// Start a new conversation (`Ctrl+L`): drop every turn and the cursor.
    pub fn reset(&mut self) {
        self.chat.clear();
        self.input.clear();
        self.phase = AiPhase::Editing;
        self.selected = None;
        self.scroll = 0;
    }

    // --- suggestions ---

    /// The suggestions of the most recent answer — what `Alt+<n>` applies, so the digits always mean
    /// "from the answer I am looking at".
    pub fn latest_suggestions(&self) -> &[Suggestion] {
        self.chat.latest_suggestions()
    }

    /// The SQL of the `idx`-th suggestion of the latest answer (0-based).
    pub fn take_latest(&self, idx: usize) -> Option<String> {
        self.latest_suggestions().get(idx).map(|s| s.sql.clone())
    }

    /// The SQL under the navigation cursor, which may be in an older turn the user scrolled back to.
    pub fn take_selected(&self) -> Option<String> {
        let coords = self.chat.all_suggestions();
        let (turn, index) = *coords.get(self.selected?)?;
        self.chat.suggestion_at(turn, index).map(|s| s.sql.clone())
    }

    /// Move the suggestion cursor one row later (first press lands on the newest suggestion, the one
    /// the user is looking at). Clamps at the last row — no wraparound. No-op when the transcript
    /// holds no suggestions.
    pub fn select_next(&mut self) {
        let len = self.chat.all_suggestions().len();
        if len == 0 {
            return;
        }
        self.selected = Some(match self.selected {
            None => len - 1, // the newest answer is what the user is looking at
            Some(i) => (i + 1).min(len - 1),
        });
    }

    /// Move the suggestion cursor one row earlier. Clamps at the first row.
    pub fn select_prev(&mut self) {
        let len = self.chat.all_suggestions().len();
        if len == 0 {
            return;
        }
        self.selected = Some(match self.selected {
            None => len.saturating_sub(1),
            Some(i) => i.saturating_sub(1),
        });
    }

    /// Point the cursor at a flattened suggestion index (a mouse click). Out of range is ignored.
    pub fn select(&mut self, idx: usize) {
        if idx < self.chat.all_suggestions().len() {
            self.selected = Some(idx);
        }
    }

    /// Scroll just enough that `row` (the cursor's first row, as the renderer lays it out) is inside
    /// a `height`-row window over `total` rows. Called after every cursor move so `Alt+Up` into an
    /// older answer brings that answer on screen rather than moving a cursor the user cannot see.
    pub fn reveal_selected(&mut self, row: usize, height: usize, total: usize) {
        if height == 0 {
            return;
        }
        let start = self.clamped_scroll(height, total);
        if row < start {
            self.scroll = row;
        } else if row >= start + height {
            self.scroll = row + 1 - height;
        } else {
            self.scroll = start;
        }
    }

    // --- scrolling ---
    //
    // The state does not know the popup's size, and with wrapped rows it cannot know the row count
    // either; the renderer's pure row model supplies both. Every scroll first clamps the stored
    // offset into the real window, so a press right after `scroll_to_end` moves the view at once.

    /// Scroll the transcript up by `rows` (toward older turns) inside a `height`-row window over
    /// `total` rows.
    pub fn scroll_up(&mut self, rows: usize, height: usize, total: usize) {
        self.scroll = self.clamped_scroll(height, total).saturating_sub(rows);
    }

    /// Scroll the transcript down by `rows`, never past the last window position.
    pub fn scroll_down(&mut self, rows: usize, height: usize, total: usize) {
        let max_start = total.saturating_sub(height.max(1));
        self.scroll = (self.clamped_scroll(height, total) + rows).min(max_start);
    }

    /// Follow the tail: park the offset past any possible end so the renderer, which knows the
    /// window, shows the last rows. Sending a message and receiving an answer both do this.
    pub fn scroll_to_end(&mut self) {
        self.scroll = usize::MAX;
    }

    /// The stored offset clamped into a `height`-row window over `total` rows, matching the
    /// renderer's clamp.
    fn clamped_scroll(&self, height: usize, total: usize) -> usize {
        self.scroll.min(total.saturating_sub(height.max(1)))
    }
}

#[cfg(test)]
#[path = "ai_state_tests.rs"]
mod ai_state_tests;
