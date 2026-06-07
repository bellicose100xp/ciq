//! Pure key -> [`HistoryAction`] mapping for the open history popup (`dev/PLAN.md` §7.6).
//!
//! ciq routes keys in the `App` (like the palette + facet), so — unlike jiq's App-coupled
//! `history_events.rs` — this file is the *pure* half: a total `fn(&KeyEvent) -> HistoryAction`
//! the App applies. Keeping the mapping here (not inline in `app.rs`) makes the chord set
//! unit-testable without an `App`. The App's `handle_history_key` is the thin applier.

use crate::app::{Key, KeyEvent};

/// What a key pressed while the history popup is open resolves to. The App applies it against its
/// [`HistoryState`](super::history_state::HistoryState) + query bar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HistoryAction {
    /// Ctrl-C: quit the app (the one chord that still quits from the popup).
    Quit,
    /// Move the cursor toward older entries.
    SelectNext,
    /// Move the cursor toward newer entries.
    SelectPrevious,
    /// Enter / Tab: recall the selected entry into the bar (fires the normal dispatch path) and
    /// close the popup.
    Accept,
    /// Esc: close the popup without recalling.
    Close,
    /// A printable char appended to the fuzzy needle.
    Push(char),
    /// Backspace: pop the last needle char.
    Pop,
    /// A key with no popup meaning — ignored (the popup stays open, unchanged).
    Ignore,
}

/// Map one [`KeyEvent`] to the popup action it triggers. Total + pure (no `App`, no state) so it is
/// unit-tested directly. Mirrors jiq's `handle_history_popup_key` chord set (Up/Down navigate,
/// Enter/Tab accept, Esc close, typing filters), minus jiq's Ctrl-D delete (not part of P5.2's
/// add/recall/dedupe/navigate/search scope).
pub fn map_key(ev: &KeyEvent) -> HistoryAction {
    if ev.mods.ctrl && matches!(ev.key, Key::Char('c') | Key::Char('C')) {
        return HistoryAction::Quit;
    }
    match ev.key {
        // jiq displays history newest-at-bottom and inverts Up/Down; ciq's popup lists
        // newest-first (top), so Down walks to older entries (select_next) and Up to newer.
        Key::Down => HistoryAction::SelectNext,
        Key::Up => HistoryAction::SelectPrevious,
        Key::Enter | Key::Tab => HistoryAction::Accept,
        Key::Esc => HistoryAction::Close,
        Key::Backspace => HistoryAction::Pop,
        // A bare printable char (no Ctrl/Alt) filters; modified chars are ignored.
        Key::Char(c) if !ev.mods.ctrl && !ev.mods.alt => HistoryAction::Push(c),
        _ => HistoryAction::Ignore,
    }
}

#[cfg(test)]
#[path = "history_events_tests.rs"]
mod history_events_tests;
