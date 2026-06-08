//! Pure layout-region tracking + mouse hit-testing (`dev/PLAN.md` §3.1 / §4.7).
//!
//! Ported from jiq's `src/layout/` (`LayoutRegions` + `region_at`) and re-justified on ciq's merits:
//! ciq tracks only the surfaces it routes mouse events to (the results pane, the query bar, and the
//! one open popup), and the hit-test is the pure function the App's `on_mouse` leans on so the
//! coordinate mapping is exercised headlessly — no terminal.
//!
//! [`LayoutRegions`] is plain owned data the render layer fills with the on-screen [`Rect`] of each
//! visible surface every frame ([`app_render`](super::app_render)); [`MouseTarget`] is the result of
//! resolving a screen cell to "which surface, and where inside it". Both are pure: the App reads them
//! to scroll the right pane, focus the right surface, or position the text cursor — all without
//! re-reading crossterm.
//!
//! This module is on the pure-core hard floor (`dev/core-modules.txt`): a wrong hit-test silently
//! routes a click to the wrong surface, and every branch is a real behavior case
//! (cell-in-pane vs cell-outside, popup-overlay-wins-over-base, inner-vs-border).

use ratatui::layout::Rect;

/// The on-screen rectangle of each surface ciq routes mouse events to, recorded fresh every render
/// pass. A region is `None` when its surface is not visible (e.g. no popup open). The popup fields
/// are mutually exclusive in practice (opening one closes the others), so at most one is `Some`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LayoutRegions {
    /// The bordered results pane (the whole box, border included).
    pub results_pane: Option<Rect>,
    /// The query bar (the prompt + textarea row(s), no border).
    pub query_bar: Option<Rect>,
    /// The open popup overlay (autocomplete / palette / facet / history / AI), if any. Carries which
    /// popup it is so a click on a row routes to the right state machine.
    pub popup: Option<(PopupKind, Rect)>,
}

/// Which popup the [`LayoutRegions::popup`] rect belongs to — so a click resolves to the matching
/// state machine. The popups are mutually exclusive overlays (only one is open at a time).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PopupKind {
    Autocomplete,
    Palette,
    Facet,
    History,
    Ai,
}

/// The surface a screen cell resolves to, plus the local coordinate the handler needs. Overlays win
/// over the base layout (a click on an open popup hits the popup, not the pane behind it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseTarget {
    /// A cell inside the results pane. `body_row` is the 0-based row index **within the scrolled,
    /// header-stripped grid body** (`None` when the cell is on the border or the sticky header row,
    /// where there is no body row to select).
    Results { body_row: Option<usize> },
    /// A cell inside the query bar. `col` is the 0-based **character column within the editable text
    /// area** (past the `> ` prompt), already offset-corrected; the App maps it onto the editor.
    QueryBar { col: usize },
    /// A cell inside an open popup. `row` is the 0-based row index within the popup's inner area
    /// (past its border), or `None` when the cell is on the border itself.
    Popup { kind: PopupKind, row: Option<usize> },
}

impl LayoutRegions {
    /// Whether `(x, y)` lies inside `rect`.
    fn contains(rect: Rect, x: u16, y: u16) -> bool {
        x >= rect.x
            && x < rect.x.saturating_add(rect.width)
            && y >= rect.y
            && y < rect.y.saturating_add(rect.height)
    }

    /// Resolve a screen cell to the surface under it, in render order (overlays topmost). Returns
    /// `None` when the cell is outside every tracked surface.
    ///
    /// - `prompt_width` is the fixed `> ` column count reserved at the left of the query bar (so a
    ///   click maps onto the editable text, not the prompt).
    /// - `v_row_offset` is the grid's vertical scroll, added to the in-pane row so the resolved
    ///   `body_row` indexes the full result, not just the visible window.
    pub fn target_at(
        &self,
        x: u16,
        y: u16,
        prompt_width: u16,
        v_row_offset: usize,
    ) -> Option<MouseTarget> {
        // The popup overlays the pane, so it wins when the cell is inside it.
        if let Some((kind, rect)) = self.popup
            && Self::contains(rect, x, y)
        {
            return Some(MouseTarget::Popup {
                kind,
                row: Self::inner_row(rect, y),
            });
        }
        if let Some(rect) = self.query_bar
            && Self::contains(rect, x, y)
        {
            return Some(MouseTarget::QueryBar {
                col: Self::text_col(rect, x, prompt_width),
            });
        }
        if let Some(rect) = self.results_pane
            && Self::contains(rect, x, y)
        {
            return Some(MouseTarget::Results {
                body_row: Self::grid_body_row(rect, y, v_row_offset),
            });
        }
        None
    }

    /// The 0-based row inside a bordered box's inner area for screen row `y`, or `None` when `y` is
    /// on the top/bottom border row. A bordered box reserves one row top and one row bottom.
    fn inner_row(rect: Rect, y: u16) -> Option<usize> {
        let inner_top = rect.y.saturating_add(1);
        let inner_bottom = rect.y.saturating_add(rect.height.saturating_sub(1)); // exclusive
        if y < inner_top || y >= inner_bottom {
            return None;
        }
        Some((y - inner_top) as usize)
    }

    /// The grid **body** row index for screen row `y` inside the results pane, accounting for the
    /// pane's top border (1 row) + the grid's sticky header (1 row) + the vertical scroll offset.
    /// `None` when `y` falls on the border, the header, or the bottom border (no body row there).
    fn grid_body_row(rect: Rect, y: u16, v_row_offset: usize) -> Option<usize> {
        // inner area starts one row below the top border; the first inner row is the sticky header,
        // so the body begins two rows below the pane's top edge.
        let body_top = rect.y.saturating_add(2);
        let inner_bottom = rect.y.saturating_add(rect.height.saturating_sub(1)); // exclusive
        if y < body_top || y >= inner_bottom {
            return None;
        }
        Some((y - body_top) as usize + v_row_offset)
    }

    /// The 0-based character column within the query bar's editable text for screen column `x`,
    /// subtracting the fixed `> ` prompt. Clamped to 0 when the click lands on the prompt itself.
    fn text_col(rect: Rect, x: u16, prompt_width: u16) -> usize {
        let text_start = rect.x.saturating_add(prompt_width);
        (x.saturating_sub(text_start)) as usize
    }
}

#[cfg(test)]
#[path = "layout_regions_tests.rs"]
mod layout_regions_tests;
