//! The AI conversation — an ordered transcript of turns, pure owned data.
//!
//! ciq's AI surface is a back-and-forth: the user asks something, the model answers with prose plus
//! pickable SQL suggestions, and the next question is sent **with the prior turns attached** so the
//! model can refine ("now group by status", "only EU rows"). This module owns that transcript and
//! nothing else — no prompt text (see [`chat_prompt`](super::chat_prompt)), no I/O.
//!
//! ## The budget
//!
//! A conversation grows without bound, but a request cannot. [`Conversation::wire_turns`] returns
//! the newest turns that fit a character budget, always ending on a user turn boundary so the model
//! never sees a dangling assistant reply as the last thing before its own answer. The full
//! transcript stays on screen; only what is *sent* is trimmed.

use super::suggestion::Suggestion;

/// Who produced a turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// The user (a typed question, or the automatic "this query failed" turn).
    User,
    /// The model.
    Assistant,
}

impl Role {
    /// The wire value for the Messages API `role` field.
    pub fn as_wire(self) -> &'static str {
        match self {
            Role::User => "user",
            Role::Assistant => "assistant",
        }
    }

    /// The short transcript label the popup paints (`you:` / `ai:`).
    pub fn label(self) -> &'static str {
        match self {
            Role::User => "you",
            Role::Assistant => "ai",
        }
    }
}

/// One turn in the transcript.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Turn {
    pub role: Role,
    /// The prose. For a user turn this is what they typed (or the generated failure notice); for an
    /// assistant turn it is the model's explanation, with its SQL split out into `suggestions`.
    pub text: String,
    /// Pickable queries attached to an assistant turn. Always empty on a user turn.
    pub suggestions: Vec<Suggestion>,
}

impl Turn {
    /// A user turn.
    pub fn user(text: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            text: text.into(),
            suggestions: Vec::new(),
        }
    }

    /// An assistant turn carrying prose and its suggestions.
    pub fn assistant(text: impl Into<String>, suggestions: Vec<Suggestion>) -> Self {
        Self {
            role: Role::Assistant,
            text: text.into(),
            suggestions,
        }
    }

    /// The text as it goes on the wire: the prose, with each suggestion's SQL appended so the model
    /// can see what it previously proposed and build on it.
    pub fn wire_text(&self) -> String {
        if self.suggestions.is_empty() {
            return self.text.clone();
        }
        let mut s = self.text.clone();
        for sug in &self.suggestions {
            s.push('\n');
            s.push_str(&sug.sql);
        }
        s
    }
}

/// The transcript: every turn, oldest first.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Conversation {
    turns: Vec<Turn>,
}

impl Conversation {
    /// An empty conversation.
    pub fn new() -> Self {
        Self::default()
    }

    /// Every turn, oldest first.
    pub fn turns(&self) -> &[Turn] {
        &self.turns
    }

    /// Whether nothing has been said yet.
    pub fn is_empty(&self) -> bool {
        self.turns.is_empty()
    }

    /// How many turns the transcript holds.
    pub fn len(&self) -> usize {
        self.turns.len()
    }

    /// Append a turn.
    pub fn push(&mut self, turn: Turn) {
        self.turns.push(turn);
    }

    /// Drop every turn (the user asked to start over).
    pub fn clear(&mut self) {
        self.turns.clear();
    }

    /// The most recent assistant turn's suggestions — what the `Alt+<n>` shortcuts apply, so the
    /// digits always mean "from the answer I am looking at".
    pub fn latest_suggestions(&self) -> &[Suggestion] {
        self.turns
            .iter()
            .rev()
            .find(|t| t.role == Role::Assistant)
            .map(|t| t.suggestions.as_slice())
            .unwrap_or(&[])
    }

    /// Every suggestion in the transcript, oldest first, as `(turn index, index within the turn)`.
    /// The cursor navigates this flattened list, so `Enter` can apply an older suggestion the user
    /// scrolled back to.
    pub fn all_suggestions(&self) -> Vec<(usize, usize)> {
        self.turns
            .iter()
            .enumerate()
            .flat_map(|(ti, turn)| (0..turn.suggestions.len()).map(move |si| (ti, si)))
            .collect()
    }

    /// The suggestion at a `(turn, index)` coordinate.
    pub fn suggestion_at(&self, turn: usize, index: usize) -> Option<&Suggestion> {
        self.turns.get(turn)?.suggestions.get(index)
    }

    /// The newest turns whose combined `wire_text` fits `budget_chars`, oldest first.
    ///
    /// Walks backwards accumulating turns, then trims any leading assistant turn so the sent slice
    /// starts with a user turn (the Messages API wants the conversation to read as user-first, and a
    /// leading answer with no question is just wasted context). An empty budget sends nothing; a
    /// single turn longer than the whole budget is still sent, because dropping the user's actual
    /// question would be worse than exceeding the budget.
    pub fn wire_turns(&self, budget_chars: usize) -> Vec<&Turn> {
        let mut picked: Vec<&Turn> = Vec::new();
        let mut used = 0usize;
        for turn in self.turns.iter().rev() {
            let cost = turn.wire_text().chars().count();
            if !picked.is_empty() && used + cost > budget_chars {
                break;
            }
            used += cost;
            picked.push(turn);
        }
        picked.reverse();
        while picked.first().is_some_and(|t| t.role == Role::Assistant) {
            picked.remove(0);
        }
        picked
    }
}

#[cfg(test)]
#[path = "chat_tests.rs"]
mod chat_tests;
