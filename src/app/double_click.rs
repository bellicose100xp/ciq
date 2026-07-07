//! Deterministic double-click detection (ported from jiq's `app/double_click.rs`, re-justified on
//! ciq's merits): crossterm has no native double-click on Unix, so the App pairs two left clicks
//! itself. jiq's tracker reads `Instant::now()` internally; ciq diverges — time enters as a
//! `now_ms: u64` parameter (the same time-as-parameter seam the debouncer uses), so the threshold
//! branch is testable with explicit timestamps and the determinism `disallowed-methods` gate holds.
//!
//! Pure data-in/data-out (a wrong pair silently double-fires or swallows an accept), so it earns
//! the hard coverage floor (`dev/core-modules.txt`).

use crate::app::layout_regions::PopupKind;

/// Two clicks pair into a double only when they land within this many milliseconds. Matches jiq
/// (which matches zellij/alacritty).
pub const DOUBLE_CLICK_THRESHOLD_MS: u64 = 400;

/// What the first click must share with the second for the pair to count.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Granularity {
    /// Exact same screen cell (col + row) — for dense lists where a row shift means a different
    /// intent (jiq's autocomplete granularity).
    SameCell,
    /// Same screen row, any column — for row-shaped targets (list rows) where horizontal jitter
    /// between the two clicks is expected.
    SameRow,
}

/// The surface identity a click pair must share. Coarser than [`MouseTarget`]
/// (crate::app::layout_regions::MouseTarget) on purpose: the pair must be on the *same surface*,
/// and the cell/row match is the [`Granularity`]'s job.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClickSurface {
    Results,
    QueryBar,
    Popup(PopupKind),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LastClick {
    at_ms: u64,
    col: u16,
    row: u16,
    surface: ClickSurface,
}

/// Buffers the previous left click so the next one can be classified as the second half of a
/// double. One shared tracker on the App (jiq's shape): the surface key keeps pairs from bleeding
/// across surfaces, and a successful pair clears the buffer so a triple click never chains.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DoubleClickTracker {
    last: Option<LastClick>,
}

impl DoubleClickTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a click at (`col`, `row`) on `surface` at `now_ms`, and report whether it completes
    /// a double: same surface, position match per `granularity`, and within
    /// [`DOUBLE_CLICK_THRESHOLD_MS`] of the previous click. A completed double clears the buffer
    /// (a third click starts a fresh pair); a non-matching click becomes the new first half.
    pub fn check_and_record(
        &mut self,
        now_ms: u64,
        col: u16,
        row: u16,
        surface: ClickSurface,
        granularity: Granularity,
    ) -> bool {
        let is_double = self.last.is_some_and(|prev| {
            let close_in_time = now_ms.saturating_sub(prev.at_ms) <= DOUBLE_CLICK_THRESHOLD_MS;
            let same_place = match granularity {
                Granularity::SameCell => prev.col == col && prev.row == row,
                Granularity::SameRow => prev.row == row,
            };
            prev.surface == surface && close_in_time && same_place
        });
        if is_double {
            self.last = None;
        } else {
            self.last = Some(LastClick {
                at_ms: now_ms,
                col,
                row,
                surface,
            });
        }
        is_double
    }

    /// Invalidate a pending pair — any scroll or pointer motion between two clicks means the user
    /// moved on (jiq resets on hover + scroll).
    pub fn reset(&mut self) {
        self.last = None;
    }
}

#[cfg(test)]
#[path = "double_click_tests.rs"]
mod double_click_tests;
