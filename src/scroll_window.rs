//! The shared scrolled-window math for the list popups (autocomplete + column palette).
//!
//! A neutral top-level leaf, lifted out so the renderer (which slices the candidate list to the
//! visible window) and the click handler (which maps a clicked visible row back to an absolute list
//! index) read **one** source of truth for the scroll offset. Previously each `*_render` module
//! kept its own `visible_window`, and the click path ignored the offset entirely — so a click after
//! the list had scrolled selected the wrong (off-screen) item. Pure `usize`-in/`usize`-out, so it
//! earns the pure-core hard floor (`dev/core-modules.txt`).

/// The absolute list index of the **first** visible row for a list of `len` items showing `visible`
/// rows at a time with `selected` (the cursor) currently highlighted. Anchors the window so the
/// selection stays in view: `0` until the selection scrolls past the first window, then it tracks
/// the selection (keeping it as the last visible row) without ever scrolling past the end.
///
/// The renderer slices `items[start .. start + visible]`; the click handler resolves a clicked
/// visible row `r` to the absolute index `start + r`.
pub fn scroll_offset(selected: usize, len: usize, visible: usize) -> usize {
    if visible == 0 || len <= visible {
        return 0;
    }
    if selected < visible {
        0
    } else {
        (selected + 1).saturating_sub(visible).min(len - visible)
    }
}

/// The `[start, end)` slice of indices to show (the window `scroll_offset` anchors). A convenience
/// for the renderers, which need both ends.
pub fn visible_window(selected: usize, len: usize, visible: usize) -> (usize, usize) {
    if visible == 0 || len <= visible {
        return (0, len);
    }
    let start = scroll_offset(selected, len, visible);
    (start, start + visible)
}

#[cfg(test)]
#[path = "scroll_window_tests.rs"]
mod scroll_window_tests;
