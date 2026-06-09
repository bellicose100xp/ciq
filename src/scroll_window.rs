//! The shared scrolled-window math for the list popups (autocomplete + column palette + history).
//!
//! A neutral top-level leaf, lifted out so every popup, every renderer (which slices the candidate
//! list to the visible window), and every click handler (which maps a clicked visible row back to an
//! absolute list index) read **one** source of truth for the scroll offset. Pure `usize`-in/-out,
//! so it earns the pure-core hard floor (`dev/core-modules.txt`).
//!
//! ## SCROLLOFF (jiq parity, `vim`'s `'scrolloff'`)
//!
//! When the cursor walks toward an edge, the visible window slides BEFORE the cursor reaches the
//! literal edge — the cursor stays [`SCROLLOFF`] rows away from the top/bottom while the data is
//! free to scroll on either side. Sliding back toward the absolute top or bottom of the data shows
//! the cursor reach the edge (because `effective_scrolloff` is clamped to `viewport / 2` and the
//! data bounds clamp on the offset itself).
//!
//! Mirrors `jiq/src/results/cursor_state.rs:3` (`pub const SCROLLOFF: u16 = 4`) and the auto-scroll
//! formula at `jiq/src/results/cursor_state.rs:220`.

/// How many rows the cursor stays away from the top/bottom of the visible window before the window
/// slides. Clamped per-call to `viewport / 2` so a viewport of 3 rows still walks sensibly.
pub const SCROLLOFF: usize = 4;

/// The absolute list index of the **first** visible row for a list of `len` items showing `visible`
/// rows at a time with `selected` (the cursor) currently highlighted. Stateless: assumes the
/// scroll offset is the smallest one that keeps the cursor inside the SCROLLOFF margin (so the
/// answer is the same regardless of whether the caller scrolled there from above or below).
///
/// Most callers (autocomplete, palette) re-derive the offset every render from `selected`, so this
/// stateless version is what they want. Callers that store and update an offset across selection
/// moves (history) should call [`scroll_offset_for_cursor`] instead, which respects the existing
/// offset when the cursor is already inside the margin.
///
/// The renderer slices `items[start .. start + visible]`; the click handler resolves a clicked
/// visible row `r` to the absolute index `start + r`.
pub fn scroll_offset(selected: usize, len: usize, visible: usize) -> usize {
    if visible == 0 || len <= visible {
        return 0;
    }
    let off = effective_scrolloff(visible);
    // Stateless rule: the smallest offset that keeps `selected` inside `[off, visible - off)` —
    // i.e. the cursor is at most `off` rows from the top of the window (so unless the cursor is
    // near the start of the data, the window has slid down enough to leave `off` rows above the
    // cursor). Clamps to `len - visible` at the end so the last window isn't over-scrolled.
    let max = len - visible;
    selected.saturating_sub(off).min(max)
}

/// The `[start, end)` slice of indices to show. A convenience for the renderers, which need both
/// ends. Picks the smallest stateless offset (see [`scroll_offset`]).
pub fn visible_window(selected: usize, len: usize, visible: usize) -> (usize, usize) {
    if visible == 0 || len <= visible {
        return (0, len);
    }
    let start = scroll_offset(selected, len, visible);
    (start, start + visible)
}

/// Stateful auto-scroll: returns the new `scroll_offset` that keeps `selected` inside the
/// [`SCROLLOFF`] margin of the visible window, *minimally adjusting* `current_offset`. Mirrors
/// jiq's `adjust_scroll_to_selection` (`results/cursor_state.rs:220`):
///
/// * cursor in the **upper** margin band → slide the window up so the cursor sits exactly at the
///   margin (or 0 if the data top is closer);
/// * cursor in the **lower** margin band → slide the window down so the cursor sits exactly at the
///   bottom margin (or `len - visible` if the data end is closer);
/// * cursor inside the body → leave the offset untouched (the visible window doesn't shift just
///   because the cursor moved within it).
///
/// Use this from popups that store and update their own scroll offset (e.g. [`HistoryState`]).
pub fn scroll_offset_for_cursor(
    selected: usize,
    len: usize,
    visible: usize,
    current_offset: usize,
) -> usize {
    if visible == 0 || len <= visible {
        return 0;
    }
    let off = effective_scrolloff(visible);
    let max = len - visible;
    let mut new_offset = current_offset.min(max);

    let lower_threshold = new_offset + visible.saturating_sub(off);
    if selected >= lower_threshold {
        // Cursor stepped into (or past) the bottom margin. Slide down so the cursor is exactly
        // `off` rows from the bottom (i.e. at row `visible - off - 1` of the window).
        new_offset = selected.saturating_add(off + 1).saturating_sub(visible);
    } else if selected < new_offset.saturating_add(off) {
        // Cursor stepped into the top margin. Slide up so the cursor is exactly `off` rows from
        // the top (or 0 if the data top is closer).
        new_offset = selected.saturating_sub(off);
    }
    new_offset.min(max)
}

/// `SCROLLOFF` clamped to `viewport / 2` so a tiny window still walks sensibly. A viewport of 1-2
/// rows yields 0 (no margin); 3-9 rows yields 1-4; 10+ rows yields the full 4.
fn effective_scrolloff(viewport: usize) -> usize {
    SCROLLOFF.min(viewport / 2)
}

#[cfg(test)]
#[path = "scroll_window_tests.rs"]
mod scroll_window_tests;
