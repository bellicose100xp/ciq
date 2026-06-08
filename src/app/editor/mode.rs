//! The vim editing mode for the query bar (`dev/PLAN.md` §3.1 input UX; vim ported from jiq's
//! `src/editor/mode.rs`, re-justified on ciq's merits).
//!
//! ciq's query bar is modal like jiq's: a casual user types in **Insert** mode (the default on
//! focus, so typing "just works"); `Esc` drops to **Normal** mode for vim navigation/edits. The
//! intermediate machine states (`Operator`, `CharSearch`, `OperatorCharSearch`, `TextObject`) are
//! the pending-key states vim needs between, e.g., pressing `d` and the motion that completes it.
//!
//! This enum is pure data — a `Copy` state with a [`display`](EditorMode::display) string for the
//! status line / help bar. It carries no textarea and no `App`, so it sits on the pure-core hard
//! floor (`dev/core-modules.txt`); the key-driven transitions live in
//! [`vim`](crate::app::editor::vim) and are exercised through the [`Editor`](crate::app::Editor).

use crate::app::editor::char_search::{SearchDirection, SearchType};

/// Whether a text object selects the content *inside* delimiters (`ci"`, `di(`) or *around* them,
/// including the delimiters (`ca"`, `da(`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextObjectScope {
    /// Inner — the content between delimiters (`ci"`, `diw`).
    Inner,
    /// Around — the content plus the delimiters / trailing whitespace (`ca"`, `daw`).
    Around,
}

/// The vim editing mode for the query bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EditorMode {
    /// Insert mode — typed characters are inserted. The default on focus (casual typing works).
    #[default]
    Insert,
    /// Normal mode — vim navigation and commands; printable keys are motions/edits, not text.
    Normal,
    /// Operator-pending: an operator (`d`/`c`/`y`) was pressed and we await its motion/text object.
    Operator(char),
    /// Char-search-pending: `f`/`F`/`t`/`T` was pressed and we await the target character.
    CharSearch(SearchDirection, SearchType),
    /// Operator + char-search-pending: e.g. `df` — the operator, the cursor column at which the
    /// operator started, and the search direction/type; we await the target character.
    OperatorCharSearch(char, usize, SearchDirection, SearchType),
    /// Text-object-pending: an operator + `i`/`a` was pressed and we await the object char (`w`,
    /// `"`, `(`, …).
    TextObject(char, TextObjectScope),
}

impl EditorMode {
    fn char_search_display(dir: SearchDirection, st: SearchType) -> char {
        match (dir, st) {
            (SearchDirection::Forward, SearchType::Find) => 'f',
            (SearchDirection::Forward, SearchType::Till) => 't',
            (SearchDirection::Backward, SearchType::Find) => 'F',
            (SearchDirection::Backward, SearchType::Till) => 'T',
        }
    }

    /// The short label for the status line / help bar (`INSERT`, `NORMAL`, or the pending-key
    /// hint such as `d(` while waiting for a motion).
    pub fn display(&self) -> String {
        match self {
            EditorMode::Insert => "INSERT".to_string(),
            EditorMode::Normal => "NORMAL".to_string(),
            EditorMode::Operator(op) => format!("OPERATOR({op})"),
            EditorMode::CharSearch(dir, st) => {
                format!("CHAR({})", Self::char_search_display(*dir, *st))
            }
            EditorMode::OperatorCharSearch(op, _, dir, st) => {
                format!("{op}{}", Self::char_search_display(*dir, *st))
            }
            EditorMode::TextObject(op, scope) => {
                let scope_char = match scope {
                    TextObjectScope::Inner => 'i',
                    TextObjectScope::Around => 'a',
                };
                format!("{op}{scope_char}")
            }
        }
    }

    /// Whether typed text goes into the buffer (Insert) vs. is interpreted as commands (every
    /// other mode). Drives the query-bar render and the App's edit-vs-command routing.
    pub fn is_insert(&self) -> bool {
        matches!(self, EditorMode::Insert)
    }
}

#[cfg(test)]
#[path = "mode_tests.rs"]
mod mode_tests;
