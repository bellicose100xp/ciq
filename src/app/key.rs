//! Headless key-event model — the input vocabulary the core understands, decoupled from
//! crossterm.
//!
//! `dev/PLAN.md` §3.1: the crossterm event loop ([`event_loop`](super::event_loop)) is the only
//! terminal edge; it decodes a real `crossterm::event::KeyEvent` into one of these neutral
//! [`KeyEvent`]s and hands it to [`App::on_key`](super::App::on_key). Tests synthesize these
//! directly. Keeping the core's input type crossterm-free is what lets event routing and editor
//! mutations stay in the headless majority (North Star 2) — no PTY, no real keyboard.

/// Modifier keys held during a key press. Only the modifiers ciq acts on are modeled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct KeyMods {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
}

impl KeyMods {
    pub const NONE: KeyMods = KeyMods {
        ctrl: false,
        alt: false,
        shift: false,
    };
    pub const CTRL: KeyMods = KeyMods {
        ctrl: true,
        alt: false,
        shift: false,
    };
}

/// A neutral key (crossterm-free). `Paste` carries an already-decoded bracketed-paste payload —
/// the framing is produced by the real terminal (the §4.7 human surface), but the decoded string
/// is inserted by the headless [`Editor`](super::Editor).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Key {
    /// A printable character.
    Char(char),
    /// A decoded bracketed-paste payload (one or more chars, possibly multi-line).
    Paste(String),
    Backspace,
    Delete,
    Enter,
    Tab,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    PageUp,
    PageDown,
    Esc,
    /// Any key ciq doesn't act on.
    Other,
}

/// A key press with its modifiers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyEvent {
    pub key: Key,
    pub mods: KeyMods,
}

impl KeyEvent {
    pub fn new(key: Key, mods: KeyMods) -> Self {
        Self { key, mods }
    }

    /// A plain (unmodified) key press — the common test constructor.
    pub fn plain(key: Key) -> Self {
        Self {
            key,
            mods: KeyMods::NONE,
        }
    }

    /// A printable character key with no modifiers.
    pub fn char(c: char) -> Self {
        Self::plain(Key::Char(c))
    }

    /// Whether this event should quit the app: **`Ctrl-C` only**. With the vim query bar, `Esc`
    /// drops to Normal mode (or closes an open popup) rather than quitting, so `Esc` is no longer a
    /// quit key — `Ctrl-C` is the single quit from anywhere (matching jiq).
    pub fn is_quit(&self) -> bool {
        self.mods.ctrl && matches!(self.key, Key::Char('c') | Key::Char('C'))
    }
}

#[cfg(test)]
#[path = "key_tests.rs"]
mod key_tests;
