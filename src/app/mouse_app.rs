//! Mouse App routing (`dev/PLAN.md` §3.1, ported from jiq's `app/mouse_*.rs`) — an `impl App` block
//! lifted out of `app.rs` to keep that file under the 1000-line cap, and because it is cohesive: it
//! resolves a synthetic [`MouseEvent`](crate::app::MouseEvent) to the surface under the pointer via
//! the recorded [`LayoutRegions`](crate::app::LayoutRegions) and scrolls / focuses / positions that
//! surface — the results grid, the query bar, or an open list popup.
//!
//! All of it is headless: the coordinate mapping is the pure `LayoutRegions::target_at`, and the
//! routing here only mutates `App` state. Tests drive `on_mouse` directly with synthetic events
//! after a `TestBackend` render recorded the regions (see `app_tests/mouse_tests.rs`).

use crate::app::layout_regions::{MouseTarget, PopupKind};
use crate::app::{App, AppPhase, Focus, MouseEvent, SimplePane, app_render};

impl App {
    /// How many grid rows a single mouse-wheel notch scrolls (jiq's `RESULTS_SCROLL_LINES`).
    const WHEEL_ROWS: usize = 3;

    /// Route one mouse event to the surface under the pointer (`dev/PLAN.md` §3.1; ported from jiq's
    /// `app/mouse_events.rs`). The event loop translates a real `crossterm::event::MouseEvent` into
    /// the neutral [`MouseEvent`](crate::app::MouseEvent) and calls this; tests drive it directly
    /// with synthetic events.
    ///
    /// Routing, by kind:
    ///  - **scroll** resolves the cell to a surface and scrolls *that* surface — the results grid
    ///    (vertical wheel -> [`v_row_offset`](Self::v_row_offset), horizontal swipe ->
    ///    [`h_col_offset`](Self::h_col_offset)), an open list popup's selection, or nothing;
    ///  - **click / drag** focuses the surface under the pointer — the results pane (focus +
    ///    optional row select) or the query bar (focus + position the text cursor at the click
    ///    column), or selects a row in an open popup.
    ///
    /// While loading / on a load error the bar is frozen, so a query-bar click never positions the
    /// cursor (the [`on_key_query_bar`](Self::on_key_query_bar) freeze invariant).
    pub fn on_mouse(&mut self, ev: MouseEvent) {
        let (x, y) = ev.position();
        // The truncation banner row is gone — the cap signal lives on the results pane border now
        // (the row counter), so the grid header sits at the very top of the inner pane and no
        // banner-offset is needed when mapping a click to the drawn row.
        let banner_rows = 0u16;
        let target = self.layout_regions.get().target_at(
            x,
            y,
            app_render::PROMPT_WIDTH,
            self.v_row_offset,
            banner_rows,
        );
        match ev {
            MouseEvent::ScrollUp { .. } => self.mouse_scroll_vertical(target, true),
            MouseEvent::ScrollDown { .. } => self.mouse_scroll_vertical(target, false),
            MouseEvent::ScrollLeft { .. } => self.mouse_scroll_horizontal(target, true),
            MouseEvent::ScrollRight { .. } => self.mouse_scroll_horizontal(target, false),
            MouseEvent::Click { .. } | MouseEvent::Drag { .. } => self.mouse_click(target),
        }
    }

    /// Vertical wheel: scroll the grid body when over the results pane (or outside any surface, the
    /// jiq fallback), or move an open list popup's selection when over it. `up` scrolls toward
    /// earlier rows.
    fn mouse_scroll_vertical(&mut self, target: Option<MouseTarget>, up: bool) {
        match target {
            Some(MouseTarget::Popup { kind, .. }) => self.popup_scroll(kind, up),
            // Over the results pane, the query bar, or nowhere: the grid scrolls (the felt default —
            // the wheel always pages the result unless it is explicitly over a list popup).
            _ => {
                if up {
                    self.v_row_offset = self.v_row_offset.saturating_sub(Self::WHEEL_ROWS);
                } else {
                    self.scroll_down(Self::WHEEL_ROWS);
                }
            }
        }
    }

    /// Horizontal trackpad swipe: column-granular grid h-scroll when over the results pane (or
    /// outside any surface). `left` moves the viewport toward earlier columns. A swipe over a popup
    /// or the query bar is a no-op (those have no horizontal scroll axis ciq exposes).
    fn mouse_scroll_horizontal(&mut self, target: Option<MouseTarget>, left: bool) {
        if matches!(
            target,
            Some(MouseTarget::Popup { .. }) | Some(MouseTarget::QueryBar { .. })
        ) {
            return;
        }
        if left {
            self.h_col_offset = self.h_col_offset.saturating_sub(1);
        } else {
            self.scroll_right();
        }
    }

    /// A left click/drag: focus + position by the resolved target.
    fn mouse_click(&mut self, target: Option<MouseTarget>) {
        match target {
            Some(MouseTarget::Popup { kind, row }) => self.popup_click(kind, row),
            // Move focus to the grid (matching the keyboard Down-handoff) only when there is a
            // result to navigate. A focused-cell concept does not exist yet, so the click row only
            // focuses the pane — the resolved `body_row` is available for a future row-cursor without
            // changing this seam.
            Some(MouseTarget::Results { .. }) if self.result.is_some() => {
                self.focus = Focus::Results;
            }
            Some(MouseTarget::QueryBar { row, col }) => {
                if matches!(self.phase, AppPhase::LoadError(_)) {
                    return; // bar frozen on load error (same invariant as on_key_query_bar)
                }
                // Focus the bar and land the cursor at the clicked (line, column), in Insert mode so
                // typing resumes immediately (jiq's click_input_field: focus, then position the
                // cursor). In Simple mode, the row indexes into the five-pane stack — clicking pane
                // N focuses it. In Power mode, the row indexes into the multiline textarea.
                self.focus = Focus::QueryBar;
                use crate::app::QueryMode;
                match self.query_form.mode() {
                    QueryMode::Simple => {
                        if let Some(pane) = SimplePane::from_index(row) {
                            self.query_form.focus(pane);
                            // The pane is single-line; place the cursor at the clicked column.
                            self.input_editor_mut().reset_to_insert();
                            self.input_editor_mut().set_cursor_row_col(0, col);
                            self.refresh_autocomplete();
                        }
                    }
                    QueryMode::Power => {
                        self.input_editor_mut().reset_to_insert();
                        self.input_editor_mut().set_cursor_row_col(row, col);
                        self.refresh_autocomplete();
                    }
                }
            }
            // Outside any surface, or a results click with no result to focus — nothing to do.
            _ => {}
        }
    }

    /// Scroll an open list popup's selection by one (mouse wheel over the popup). Only the list
    /// popups (autocomplete / palette / history) have a movable selection; facet/AI are not lists.
    fn popup_scroll(&mut self, kind: PopupKind, up: bool) {
        match kind {
            PopupKind::Autocomplete => {
                if up {
                    self.autocomplete.select_prev();
                } else {
                    self.autocomplete.select_next();
                }
            }
            PopupKind::Palette => {
                if let Some(palette) = self.palette.as_mut() {
                    if up {
                        palette.cursor_up();
                    } else {
                        palette.cursor_down();
                    }
                }
            }
            PopupKind::History => {
                if up {
                    self.history.select_previous();
                } else {
                    self.history.select_next();
                }
            }
            PopupKind::Facet | PopupKind::Ai => {}
        }
    }

    /// A click on a popup row. For the autocomplete popup this selects the clicked candidate (so a
    /// subsequent Tab/Enter accepts it); a click on the palette moves the cursor onto the clicked
    /// column. The facet/history/AI popups do not act on a row click here (history recall + facet
    /// dismissal stay keyboard-driven — noted as a clean deferral). `row` is `None` on the border.
    ///
    /// The clicked `row` is **relative to the visible window**, but the popup renders a scrolled
    /// slice — so the absolute list index is `scroll_offset(selected, len, visible) + row`, using the
    /// same `scroll_window` math the renderer drew with. Ignoring the offset would select an
    /// off-screen item once the list has scrolled.
    fn popup_click(&mut self, kind: PopupKind, row: Option<usize>) {
        let Some(row) = row else {
            return;
        };
        // The visible-row count = the recorded popup box's inner height (border-stripped) — the same
        // window size the renderer used. 0 when no popup is recorded (then the offset is 0 anyway).
        let visible = self
            .layout_regions
            .get()
            .popup
            .map(|(_, rect)| rect.height.saturating_sub(2) as usize)
            .unwrap_or(0);
        match kind {
            PopupKind::Autocomplete => {
                let start = crate::scroll_window::scroll_offset(
                    self.autocomplete.selected(),
                    self.autocomplete.len(),
                    visible,
                );
                self.autocomplete.set_selected(start + row);
            }
            PopupKind::Palette => {
                if let Some(palette) = self.palette.as_mut() {
                    let start = crate::scroll_window::scroll_offset(
                        palette.cursor(),
                        palette.all_columns().len(),
                        visible,
                    );
                    palette.set_cursor(start + row);
                }
            }
            PopupKind::Facet | PopupKind::History | PopupKind::Ai => {}
        }
    }
}
