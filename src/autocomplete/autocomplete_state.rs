//! Suggestion model — the reused popup-item type (`dev/PLAN.md` §5.1, `dev/DECISIONS.md` S5).
//!
//! Ported from jiq's `autocomplete_state.rs` `Suggestion` / `SuggestionType`, kept deliberately
//! minimal: this module owns only the *item* shape the candidate generator (P3.5) returns. The full
//! popup state machine, render, and insertion (jiq's `AutocompleteState` + `autocomplete_render` +
//! `insertion`) are P3.6 — they layer on top of this type without changing it.
//!
//! ciq-specific shape, re-justified on ciq's merits rather than copied wholesale:
//!  - `field_type` is `Option<ColumnType>` (jiq's `Option<JsonFieldType>`), so the popup shows a
//!    DuckDB type (`int`/`date`/…) inline against a column — the headline schema-aware win.
//!  - `SuggestionType` **reuses** `Field`/`Function`/`Operator`/`Value` and **adds** `Keyword` and
//!    `Aggregate` (which do not exist in jiq — §5.1); jiq's `Pattern`/`Variable` are **dropped**
//!    (jq-iterator and `$var` have no SQL analog).
//!  - `with_signature` / `with_description` carry the optional function signature + one-line hint
//!    that `sql_keywords::FunctionEntry` supplies. `needs_parens` is dropped (jiq inserted `(` for
//!    its builtins; SQL function insertion is handled by the signature, not a flag).
//!
//! This file also owns the **popup state machine** [`AutocompleteState`] (P3.6) — the open/closed
//! flag, the candidate list, and the selected index — ported from jiq's `AutocompleteState`. It is
//! pure owned data: the App drives it with synthetic keys (open/close/move-selection) and the
//! render layer reads it; nothing here touches a terminal or the engine.

use std::fmt;

use crate::schema::ColumnType;

/// What kind of thing a [`Suggestion`] is — selects its type label and its insertion rules
/// (P3.6). Reuses jiq's `Field`/`Function`/`Operator`/`Value`; adds `Keyword`/`Aggregate` for SQL;
/// drops jiq's `Pattern`/`Variable` (no SQL analog).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuggestionType {
    /// A schema column name.
    Field,
    /// A scalar SQL function (`lower`, `date_trunc`, …).
    Function,
    /// An aggregate function (`COUNT`, `SUM`, …) — legal only in `SelectList`/`HAVING` (§5.7).
    Aggregate,
    /// A comparison/membership operator (`=`, `LIKE`, `IS NOT NULL`, …).
    Operator,
    /// A SQL clause keyword (`SELECT`, `WHERE`, `ASC`, …).
    Keyword,
    /// A distinct value of a column, for value-completion (`WHERE status = 'active'`).
    Value,
}

impl fmt::Display for SuggestionType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            SuggestionType::Field => "field",
            SuggestionType::Function => "function",
            SuggestionType::Aggregate => "aggregate",
            SuggestionType::Operator => "operator",
            SuggestionType::Keyword => "keyword",
            SuggestionType::Value => "value",
        };
        f.write_str(s)
    }
}

/// One popup candidate: the text to insert, its kind, and optional decorations the popup shows.
/// Built via [`Suggestion::new`] / [`Suggestion::new_with_type`] and the `with_*` chainers, exactly
/// like jiq's constructors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suggestion {
    /// The candidate text — what gets inserted at the cursor (before any SQL identifier quoting,
    /// which insertion applies in P3.6).
    pub text: String,
    /// The candidate kind.
    pub suggestion_type: SuggestionType,
    /// Optional one-line description for the popup hint (functions/operators).
    pub description: Option<String>,
    /// Optional column type, shown right-aligned for `Field`/`Value` candidates — the schema-aware
    /// type hint. `None` for keywords/operators/functions.
    pub field_type: Option<ColumnType>,
    /// Optional call signature for function/aggregate candidates (e.g. `COUNT(expr)`).
    pub signature: Option<String>,
}

impl Suggestion {
    /// A bare suggestion with no decorations.
    pub fn new(text: impl Into<String>, suggestion_type: SuggestionType) -> Self {
        Self {
            text: text.into(),
            suggestion_type,
            description: None,
            field_type: None,
            signature: None,
        }
    }

    /// A suggestion carrying a typed hint (a column + its [`ColumnType`]).
    pub fn new_with_type(
        text: impl Into<String>,
        suggestion_type: SuggestionType,
        field_type: Option<ColumnType>,
    ) -> Self {
        Self {
            text: text.into(),
            suggestion_type,
            description: None,
            field_type,
            signature: None,
        }
    }

    /// Attach a one-line description (popup hint).
    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }

    /// Attach a call signature (function/aggregate candidates).
    pub fn with_signature(mut self, sig: impl Into<String>) -> Self {
        self.signature = Some(sig.into());
        self
    }
}

/// The autocomplete popup state machine (`dev/PLAN.md` §5.1/§5.6) — ported from jiq's
/// `AutocompleteState`. Owned, pure data: whether the popup is open, the ranked candidate list it
/// is showing, and which entry is selected. The App recomputes the list on each query-bar edit and
/// drives selection/dismissal through synthetic keys; the render layer reads it.
///
/// Closed is the default. The popup is open iff it was opened with a **non-empty** candidate list —
/// [`open_with`] closes itself on an empty list, so "open but showing nothing" is never a state.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AutocompleteState {
    open: bool,
    suggestions: Vec<Suggestion>,
    /// Selected index into `suggestions`; always `< suggestions.len()` while `open`.
    selected: usize,
}

impl AutocompleteState {
    /// A closed, empty popup.
    pub fn new() -> Self {
        Self::default()
    }

    /// (Re)open the popup with a fresh candidate list, selecting the first entry. An **empty**
    /// list closes the popup instead (there is nothing to show). Called after each edit recomputes
    /// suggestions — so the popup tracks the current cursor context.
    pub fn open_with(&mut self, suggestions: Vec<Suggestion>) {
        if suggestions.is_empty() {
            self.close();
            return;
        }
        self.suggestions = suggestions;
        self.selected = 0;
        self.open = true;
    }

    /// Close the popup and drop its candidate list.
    pub fn close(&mut self) {
        self.open = false;
        self.suggestions.clear();
        self.selected = 0;
    }

    /// Whether the popup is currently open (and therefore showing at least one candidate).
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// The candidates currently shown (empty when closed).
    pub fn suggestions(&self) -> &[Suggestion] {
        &self.suggestions
    }

    /// The selected index into [`suggestions`](Self::suggestions).
    pub fn selected(&self) -> usize {
        self.selected
    }

    /// Select the entry at `index` (the mouse click-to-select path). Out-of-range or closed is a
    /// no-op, so a click on a blank popup row never desyncs the selection.
    pub fn set_selected(&mut self, index: usize) {
        if self.open && index < self.suggestions.len() {
            self.selected = index;
        }
    }

    /// The currently-selected suggestion, or `None` when the popup is closed.
    pub fn selected_suggestion(&self) -> Option<&Suggestion> {
        if self.open {
            self.suggestions.get(self.selected)
        } else {
            None
        }
    }

    /// Move the selection down one (the next candidate), wrapping from the last back to the first.
    /// No-op when closed.
    pub fn select_next(&mut self) {
        if !self.open || self.suggestions.is_empty() {
            return;
        }
        self.selected = (self.selected + 1) % self.suggestions.len();
    }

    /// Move the selection up one (the previous candidate), wrapping from the first to the last.
    /// No-op when closed.
    pub fn select_prev(&mut self) {
        if !self.open || self.suggestions.is_empty() {
            return;
        }
        self.selected = if self.selected == 0 {
            self.suggestions.len() - 1
        } else {
            self.selected - 1
        };
    }

    /// Number of candidates currently shown.
    pub fn len(&self) -> usize {
        self.suggestions.len()
    }

    /// Whether the popup shows no candidates (always true when closed).
    pub fn is_empty(&self) -> bool {
        self.suggestions.is_empty()
    }
}

#[cfg(test)]
#[path = "autocomplete_state_tests.rs"]
mod autocomplete_state_tests;
