//! Tests for the neutral key-event model.

use super::{Key, KeyEvent, KeyMods};

#[test]
fn char_constructor_is_unmodified() {
    let ev = KeyEvent::char('x');
    assert_eq!(ev.key, Key::Char('x'));
    assert_eq!(ev.mods, KeyMods::NONE);
}

#[test]
fn esc_is_quit() {
    assert!(KeyEvent::plain(Key::Esc).is_quit());
}

#[test]
fn ctrl_c_is_quit() {
    assert!(KeyEvent::new(Key::Char('c'), KeyMods::CTRL).is_quit());
    assert!(KeyEvent::new(Key::Char('C'), KeyMods::CTRL).is_quit());
}

#[test]
fn plain_c_is_not_quit() {
    assert!(!KeyEvent::char('c').is_quit());
}

#[test]
fn enter_is_not_quit() {
    assert!(!KeyEvent::plain(Key::Enter).is_quit());
}

#[test]
fn paste_carries_payload() {
    let ev = KeyEvent::plain(Key::Paste("multi\nline".to_string()));
    match ev.key {
        Key::Paste(s) => assert_eq!(s, "multi\nline"),
        other => panic!("expected Paste, got {other:?}"),
    }
}
