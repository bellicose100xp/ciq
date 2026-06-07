//! The AI popup state machine — pure owned data, pure transitions (`dev/PLAN.md` §7 P5.1).
//!
//! Ported from jiq's `ai/ai_state.rs` idea (a visible popup with input + a request lifecycle),
//! reshaped for ciq's single-SQL-out flow: jiq accumulates streaming chunks into a suggestion
//! list; ciq just collects the natural-language prompt the user types, then tracks one
//! pending->success/error request. No tokio, no `CancellationToken`, no chunk accumulation — the
//! whole result arrives at once over the AI thread's channel (see [`ai_app`](super::ai_app)).
//!
//! All transitions are `&mut self` over plain data and are unit-tested with plain asserts — no
//! terminal, no engine, no clock. The input is a `String` edited char-by-char (UTF-8 safe: pushes
//! whole `char`s and pops whole `char`s, never byte-slices).

/// Where the AI request is in its lifecycle.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum AiPhase {
    /// Open and accepting the natural-language prompt (the default while typing).
    #[default]
    Editing,
    /// The prompt was submitted; waiting for the provider's reply (the AI thread is working).
    Pending,
    /// The provider returned SQL successfully; carries the generated SQL the App dropped into the
    /// bar. The popup shows it briefly then is typically closed by the App once dispatched.
    Success(String),
    /// The request failed; carries the user-facing error message.
    Error(String),
}

/// The AI popup state: whether it's open, the natural-language input being typed, and the request
/// phase.
///
/// All fields private; transitions go through the methods so the invariants hold (closed implies
/// `Editing` with an empty input on the next open; the input only changes while `Editing`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AiState {
    /// Whether the AI popup is currently open.
    open: bool,
    /// The natural-language request the user is typing (single line).
    input: String,
    /// The request lifecycle phase.
    phase: AiPhase,
}

impl AiState {
    /// A closed, empty AI popup.
    pub fn new() -> Self {
        Self::default()
    }

    /// Open the popup fresh: empty input, `Editing` phase. Resets any prior request state so a
    /// reopened popup never shows a stale success/error.
    pub fn open(&mut self) {
        self.open = true;
        self.input.clear();
        self.phase = AiPhase::Editing;
    }

    /// Close the popup and clear its input + phase (so the next open starts clean).
    pub fn close(&mut self) {
        self.open = false;
        self.input.clear();
        self.phase = AiPhase::Editing;
    }

    /// Whether the popup is currently open.
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// The natural-language input typed so far.
    pub fn input(&self) -> &str {
        &self.input
    }

    /// The current request phase.
    pub fn phase(&self) -> &AiPhase {
        &self.phase
    }

    /// Whether a request is currently in flight (`Pending`) — the App gates re-submit on this so a
    /// single Enter never fires two overlapping requests.
    pub fn is_pending(&self) -> bool {
        matches!(self.phase, AiPhase::Pending)
    }

    /// Append a typed character to the input. Only mutates while `Editing` (a keystroke that
    /// arrives mid-request is ignored, not buffered into a stale prompt). UTF-8 safe (whole char).
    pub fn push_char(&mut self, c: char) {
        if matches!(self.phase, AiPhase::Editing) {
            self.input.push(c);
        }
    }

    /// Remove the last character of the input (Backspace). Only mutates while `Editing`. Pops a
    /// whole `char`, never a partial byte.
    pub fn backspace(&mut self) {
        if matches!(self.phase, AiPhase::Editing) {
            self.input.pop();
        }
    }

    /// Mark the request submitted: transition to `Pending`. No-op unless `Editing` with a
    /// non-blank input (an empty prompt is not submittable). Returns the trimmed prompt that was
    /// submitted, so the caller builds the model prompt from exactly what the popup committed.
    pub fn submit(&mut self) -> Option<String> {
        if !matches!(self.phase, AiPhase::Editing) {
            return None;
        }
        let prompt = self.input.trim().to_string();
        if prompt.is_empty() {
            return None;
        }
        self.phase = AiPhase::Pending;
        Some(prompt)
    }

    /// Record a successful provider reply (the generated SQL). Transition to `Success`. The App
    /// reads the SQL out and typically closes the popup after dispatching it.
    pub fn set_success(&mut self, sql: impl Into<String>) {
        self.phase = AiPhase::Success(sql.into());
    }

    /// Record a failed request with a user-facing message. Transition to `Error`; the popup stays
    /// open so the user sees the message (and can edit + retry).
    pub fn set_error(&mut self, message: impl Into<String>) {
        self.phase = AiPhase::Error(message.into());
        // Returning to an editable state lets the user fix the prompt without reopening.
        // The input is preserved so they can tweak rather than retype.
    }

    /// After an error, return to the `Editing` phase (so the next keystroke edits the preserved
    /// prompt). Called by the App when the user starts typing again after a failure.
    pub fn resume_editing(&mut self) {
        if matches!(self.phase, AiPhase::Error(_)) {
            self.phase = AiPhase::Editing;
        }
    }
}

#[cfg(test)]
#[path = "ai_state_tests.rs"]
mod ai_state_tests;
