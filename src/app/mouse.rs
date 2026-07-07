//! Headless mouse-event model — the pointer vocabulary the core understands, decoupled from
//! crossterm (the sibling of [`key`](super::key)).
//!
//! `dev/PLAN.md` §3.1 / §4.7: the crossterm event loop ([`event_loop`](super::event_loop), the one
//! terminal edge) decodes a real `crossterm::event::MouseEvent` into one of these neutral
//! [`MouseEvent`]s and hands it to [`App::on_mouse`](super::App::on_mouse). Tests synthesize these
//! directly, so mouse routing — scroll, click-to-focus, click-to-position-cursor — stays in the
//! headless majority (North Star 2): no PTY, no real pointer.
//!
//! Ported from jiq's `app/mouse_events.rs` (`MouseEventKind::{ScrollUp/Down/Left/Right, Down(Left),
//! Drag(Left)}`) and re-justified on ciq's merits: ciq models only the kinds it acts on
//! (scroll + left click/drag) and carries the click cell as `(col, row)` so the pure
//! coordinate-mapping (`layout_regions`) can resolve it without re-reading crossterm.

/// A neutral mouse event (crossterm-free). Cell coordinates are 0-based screen columns/rows, as
/// crossterm reports them. Only the kinds ciq acts on are modeled; everything else the event loop
/// drops.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseEvent {
    /// Vertical wheel up (toward earlier rows). Carries the cell under the pointer so the handler
    /// can route to the surface there.
    ScrollUp { col: u16, row: u16 },
    /// Vertical wheel down (toward later rows).
    ScrollDown { col: u16, row: u16 },
    /// Horizontal trackpad swipe left (toward earlier columns).
    ScrollLeft { col: u16, row: u16 },
    /// Horizontal trackpad swipe right (toward later columns).
    ScrollRight { col: u16, row: u16 },
    /// Left button pressed at the cell — a click (focus / position-cursor / popup-select).
    Click { col: u16, row: u16 },
    /// Left button held and moved — a drag (treated like a click for cursor positioning).
    Drag { col: u16, row: u16 },
    /// Pointer moved with no button held — drives the transient hover highlight (grid row /
    /// popup row under the pointer). Any motion also invalidates a pending double-click pair.
    Move { col: u16, row: u16 },
}

impl MouseEvent {
    /// The cell `(col, row)` the event occurred at.
    pub fn position(&self) -> (u16, u16) {
        match *self {
            MouseEvent::ScrollUp { col, row }
            | MouseEvent::ScrollDown { col, row }
            | MouseEvent::ScrollLeft { col, row }
            | MouseEvent::ScrollRight { col, row }
            | MouseEvent::Click { col, row }
            | MouseEvent::Drag { col, row }
            | MouseEvent::Move { col, row } => (col, row),
        }
    }
}

#[cfg(test)]
#[path = "mouse_tests.rs"]
mod mouse_tests;
