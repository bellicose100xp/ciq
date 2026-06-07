//! Tests for the pure key -> [`HistoryAction`] mapping (`history_events.rs`).

use super::{HistoryAction, map_key};
use crate::app::{Key, KeyEvent, KeyMods};

fn ctrl(c: char) -> KeyEvent {
    KeyEvent::new(Key::Char(c), KeyMods::CTRL)
}

#[test]
fn ctrl_c_quits() {
    assert_eq!(map_key(&ctrl('c')), HistoryAction::Quit);
    assert_eq!(map_key(&ctrl('C')), HistoryAction::Quit);
}

#[test]
fn arrows_navigate() {
    // newest-first list: Down -> older (select_next), Up -> newer (select_previous).
    assert_eq!(
        map_key(&KeyEvent::plain(Key::Down)),
        HistoryAction::SelectNext
    );
    assert_eq!(
        map_key(&KeyEvent::plain(Key::Up)),
        HistoryAction::SelectPrevious
    );
}

#[test]
fn enter_and_tab_accept() {
    assert_eq!(map_key(&KeyEvent::plain(Key::Enter)), HistoryAction::Accept);
    assert_eq!(map_key(&KeyEvent::plain(Key::Tab)), HistoryAction::Accept);
}

#[test]
fn esc_closes() {
    assert_eq!(map_key(&KeyEvent::plain(Key::Esc)), HistoryAction::Close);
}

#[test]
fn printable_char_pushes_needle() {
    assert_eq!(map_key(&KeyEvent::char('s')), HistoryAction::Push('s'));
}

#[test]
fn backspace_pops_needle() {
    assert_eq!(
        map_key(&KeyEvent::plain(Key::Backspace)),
        HistoryAction::Pop
    );
}

#[test]
fn ctrl_char_other_than_c_is_ignored() {
    // Modified chars (other than Ctrl-C) don't filter — they're ignored.
    assert_eq!(map_key(&ctrl('x')), HistoryAction::Ignore);
}

#[test]
fn unmodeled_key_is_ignored() {
    assert_eq!(map_key(&KeyEvent::plain(Key::Home)), HistoryAction::Ignore);
}
