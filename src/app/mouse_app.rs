//! Mouse App routing (`dev/PLAN.md` §3.1, ported from jiq's `app/mouse_*.rs`) — an `impl App` block
//! lifted out of `app.rs` to keep that file under the 1000-line cap, and because it is cohesive: it
//! resolves a synthetic [`MouseEvent`](crate::app::MouseEvent) to the surface under the pointer via
//! the recorded [`LayoutRegions`](crate::app::LayoutRegions) and scrolls / focuses / positions /
//! hovers that surface — the results grid, the query bar, or an open popup.
//!
//! All of it is headless: the coordinate mapping is the pure `LayoutRegions::target_at`, and the
//! routing here only mutates `App` state. Tests drive `on_mouse` directly with synthetic events
//! after a `TestBackend` render recorded the regions (see `app_tests/mouse_tests.rs`). Time enters
//! as `now_ms` (the debouncer seam) so the double-click threshold is deterministic under test.

use crate::app::double_click::{ClickSurface, Granularity};
use crate::app::layout_regions::{HoverTarget, MouseTarget, PopupKind};
use crate::app::{App, AppPhase, Focus, MouseEvent, QueryMode, SimplePane, app_render};

impl App {
    /// How many grid rows a single mouse-wheel notch scrolls (jiq's `RESULTS_SCROLL_LINES`).
    const WHEEL_ROWS: usize = 3;

    /// How many characters one trackpad horizontal-swipe notch slides the grid. Matches
    /// [`WHEEL_ROWS`](Self::WHEEL_ROWS)' grain on the vertical axis, so left/right and up/down
    /// felt scroll rates are consistent. Char-granular (not column-granular) so the trackpad
    /// glides across columns of varying widths without jerk.
    const CHAR_SCROLL_STEP: u16 = 3;

    /// Route one mouse event to the surface under the pointer (`dev/PLAN.md` §3.1; ported from jiq's
    /// `app/mouse_events.rs`). The event loop translates a real `crossterm::event::MouseEvent` into
    /// the neutral [`MouseEvent`](crate::app::MouseEvent) and calls this; tests drive it directly
    /// with synthetic events. `now_ms` feeds the double-click pairing (and any scheduled dispatch a
    /// click triggers) — the same time-as-parameter seam `on_key` uses.
    ///
    /// Routing, by kind:
    ///  - **scroll** resolves the cell to a surface and scrolls *that* surface — the results grid
    ///    (vertical wheel -> [`v_row_offset`](Self::v_row_offset), horizontal swipe ->
    ///    [`h_col_offset`](Self::h_col_offset)), an open list popup's selection, or nothing;
    ///  - **click / drag** focuses the surface under the pointer — the results pane (focus), the
    ///    query bar (focus + position the text cursor at the click column), or a popup row
    ///    (select; double-click accepts/toggles; a history row recalls). A click outside an open
    ///    modal popup (facet / history / AI) dismisses it, jiq's click-outside-dismiss.
    ///  - **move** updates the transient hover highlight (grid row / popup row under the pointer).
    ///
    /// While loading / on a load error the bar is frozen, so a query-bar click never positions the
    /// cursor (the [`on_key_query_bar`](Self::on_key_query_bar) freeze invariant).
    pub fn on_mouse(&mut self, ev: MouseEvent, now_ms: u64) {
        let (x, y) = ev.position();
        // The truncation banner row is gone — the cap signal lives on the results pane border now
        // (the row counter), so the grid header sits at the very top of the inner pane and no
        // banner-offset is needed when mapping a click to the drawn row.
        let banner_rows = 0u16;
        // The columns reserved left of the editable bar text: the `> ` prompt in Power mode, the
        // pane label column (`SELECT  ` / `WHERE   ` / …) in Simple mode — so a click on either
        // chrome clamps to text column 0 instead of landing the cursor offset into the text.
        let text_left = match self.query_form.mode() {
            QueryMode::Simple => app_render::SIMPLE_LABEL_WIDTH,
            QueryMode::Power => app_render::PROMPT_WIDTH,
        };
        let target =
            self.layout_regions
                .get()
                .target_at(x, y, text_left, self.v_row_offset, banner_rows);
        match ev {
            MouseEvent::ScrollUp { .. } => self.mouse_scroll_vertical(target, true),
            MouseEvent::ScrollDown { .. } => self.mouse_scroll_vertical(target, false),
            MouseEvent::ScrollLeft { .. } => self.mouse_scroll_horizontal(target, true),
            MouseEvent::ScrollRight { .. } => self.mouse_scroll_horizontal(target, false),
            MouseEvent::Click { .. } => self.mouse_click(target, x, y, now_ms, false),
            MouseEvent::Drag { .. } => self.mouse_click(target, x, y, now_ms, true),
            MouseEvent::Move { .. } => self.mouse_hover(target),
        }
    }

    /// Pointer motion with no button held: repaint the hover highlight for the row under the
    /// pointer, and invalidate a pending double-click pair (the pointer moved on — jiq resets the
    /// tracker on every hover/scroll). At most one hover exists at a time: resolving the new
    /// target replaces the old one wholesale.
    fn mouse_hover(&mut self, target: Option<MouseTarget>) {
        self.double_click.reset();
        self.hover = match target {
            Some(MouseTarget::Results {
                body_row: Some(row),
            }) if self.grid_row_exists(row) => Some(HoverTarget::GridRow(row)),
            Some(MouseTarget::Popup {
                kind,
                row: Some(row),
            }) => self
                .popup_abs_index(kind, row)
                .map(|abs| HoverTarget::PopupRow(kind, abs)),
            _ => None,
        };
    }

    /// Whether absolute grid body row `row` holds data in the displayed (possibly search-
    /// filtered) result — a hover on the blank area below a short result highlights nothing.
    fn grid_row_exists(&self, row: usize) -> bool {
        self.display_rows().is_some_and(|r| row < r.row_count())
    }

    /// Vertical wheel: scroll the grid body when over the results pane (or outside any surface, the
    /// jiq fallback), or move an open list popup's selection when over it. `up` scrolls toward
    /// earlier rows.
    fn mouse_scroll_vertical(&mut self, target: Option<MouseTarget>, up: bool) {
        self.double_click.reset();
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

    /// Horizontal trackpad swipe: smooth char-granular grid h-scroll when over the results pane
    /// (or outside any surface). `left` moves the viewport toward earlier columns. A swipe over
    /// a popup or the query bar is a no-op (those have no horizontal scroll axis ciq exposes).
    ///
    /// Each notch slides by [`CHAR_SCROLL_STEP`](Self::CHAR_SCROLL_STEP) chars (the same grain as
    /// the grid's wheel-rows-per-tick on the vertical axis), letting the user trackpad past wide
    /// columns smoothly. The slide updates `h_char_offset` (the render axis) and recomputes
    /// `h_col_offset` so keyboard ←/→ still lands on whole-column boundaries.
    fn mouse_scroll_horizontal(&mut self, target: Option<MouseTarget>, left: bool) {
        self.double_click.reset();
        if matches!(
            target,
            Some(MouseTarget::Popup { .. })
                | Some(MouseTarget::QueryBar { .. })
                | Some(MouseTarget::SearchBar)
        ) {
            return;
        }
        let delta = i32::from(Self::CHAR_SCROLL_STEP);
        let signed_delta = if left { -delta } else { delta };
        self.slide_h_chars(signed_delta);
    }

    /// A left click/drag: focus + position by the resolved target. `(x, y)` is the raw screen cell
    /// (the double-click tracker pairs on it); `now_ms` stamps the click for the pairing threshold.
    /// A drag positions/selects like a click but never *activates* — no double-click pairing, no
    /// history recall — so sweeping the pointer with the button held can't fire accept/recall
    /// repeatedly.
    fn mouse_click(
        &mut self,
        target: Option<MouseTarget>,
        x: u16,
        y: u16,
        now_ms: u64,
        drag: bool,
    ) {
        // Click-outside-dismiss for the modal popups (jiq's dismiss-overlay pattern): the facet is
        // informational (any click closes it, matching its any-key-dismisses contract); a click
        // outside the history/AI popup closes it without recalling/submitting. The dismissing
        // click is swallowed — the user clicked to get rid of the popup, not to act underneath it.
        if self.facet.is_some() {
            self.close_facet();
            return;
        }
        let on_popup = |t: &Option<MouseTarget>, k: PopupKind| matches!(t, Some(MouseTarget::Popup { kind, .. }) if *kind == k);
        if self.history_open && !on_popup(&target, PopupKind::History) {
            self.close_history();
            return;
        }
        if self.ai.is_open() && !on_popup(&target, PopupKind::Ai) {
            self.ai.close();
            return;
        }
        if self.save.is_open() && !on_popup(&target, PopupKind::Save) {
            self.close_save();
            return;
        }
        match target {
            Some(MouseTarget::Popup { kind, row }) => {
                self.popup_click(kind, row, x, y, now_ms, drag)
            }
            // A click on the open search bar re-enters needle editing on a confirmed search
            // (the Ctrl+F chord's unconfirm), and is a no-op while already editing — the bar
            // has no positionable cursor, so the whole box is one "give me the keyboard" target.
            Some(MouseTarget::SearchBar) if self.search.is_confirmed() => {
                self.search.unconfirm();
            }
            Some(MouseTarget::SearchBar) => {}
            // Move focus to the grid (matching the keyboard Down-handoff) only when there is a
            // result to navigate. A focused-cell concept does not exist yet, so the click row only
            // focuses the pane — the resolved `body_row` is available for a future row-cursor without
            // changing this seam.
            Some(MouseTarget::Results { .. }) if self.result.is_some() => {
                self.leave_search_editing();
                self.focus = Focus::Results;
            }
            Some(MouseTarget::QueryBar { row, col }) => {
                if matches!(self.phase, AppPhase::LoadError(_)) {
                    return; // bar frozen on load error (same invariant as on_key_query_bar)
                }
                // While the search bar is editing it captures the keyboard; a click on the query
                // bar is an explicit "type here now", so leave editing first (Enter parity:
                // confirm a non-empty needle, close an empty one) or the keys would still edit
                // the needle.
                self.leave_search_editing();
                // Focus the bar and land the cursor at the clicked (line, column), in Insert mode so
                // typing resumes immediately (jiq's click_input_field: focus, then position the
                // cursor). In Simple mode, the row indexes into the five-pane stack — clicking pane
                // N focuses it. In Power mode, the row indexes into the multiline textarea.
                self.focus = Focus::QueryBar;
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

    /// Scroll an open list popup's selection by one wheel notch. Each notch advances the
    /// popup's selection by [`WHEEL_ROWS`](Self::WHEEL_ROWS) — the same grain as the grid's
    /// wheel-rows-per-tick, so the felt scroll rate is consistent across surfaces. Only the list
    /// popups (autocomplete / palette / history) have a movable selection; facet/AI are not lists.
    /// The popup's own `select_*`/`cursor_*` methods are bounded (no-op past the end) and
    /// recompute the visible-window through the shared SCROLLOFF helper, so the selection stays
    /// inside the visible window with the jiq margin.
    fn popup_scroll(&mut self, kind: PopupKind, up: bool) {
        for _ in 0..Self::WHEEL_ROWS {
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
                PopupKind::Facet | PopupKind::Ai | PopupKind::Save => {}
            }
        }
    }

    /// Map a popup-inner row (visible-window-relative, what the hit-test yields) to the absolute
    /// list index the state machines speak — the same `scroll_window` math the renderer drew with,
    /// so a click/hover on a scrolled list lands on the row the user sees. `None` when the row is
    /// past the list's end (a blank popup row) or the popup has no list (facet/AI).
    fn popup_abs_index(&self, kind: PopupKind, row: usize) -> Option<usize> {
        // The visible-row count = the recorded popup box's inner height (border-stripped) — the same
        // window size the renderer used. 0 when no popup is recorded (then the offset is 0 anyway).
        let visible = self
            .layout_regions
            .get()
            .popup
            .map(|(_, rect)| rect.height.saturating_sub(2) as usize)
            .unwrap_or(0);
        let (start, len) = match kind {
            PopupKind::Autocomplete => (
                crate::scroll_window::scroll_offset(
                    self.autocomplete.selected(),
                    self.autocomplete.len(),
                    visible,
                ),
                self.autocomplete.len(),
            ),
            PopupKind::Palette => {
                let palette = self.palette.as_ref()?;
                (
                    crate::scroll_window::scroll_offset(
                        palette.cursor(),
                        palette.all_columns().len(),
                        visible,
                    ),
                    palette.all_columns().len(),
                )
            }
            // The history popup scrolls through its own state (`adjust_scroll_to_selection`), not
            // the shared scroll_window helper — read the offset it rendered with.
            PopupKind::History => (self.history.scroll_offset(), self.history.filtered_count()),
            PopupKind::Facet | PopupKind::Ai | PopupKind::Save => return None,
        };
        let abs = start + row;
        (abs < len).then_some(abs)
    }

    /// A click on a popup row (`row` is `None` on the border — ignored; a `drag` selects but never
    /// activates).
    ///
    ///  - **autocomplete**: select the clicked candidate; a double-click on the same cell accepts
    ///    it (jiq's double-click-accept), inserting it into the query.
    ///  - **palette**: move the cursor onto the clicked column; a double-click on the same row
    ///    toggles it (the Space analog), live-rewriting the SELECT pane.
    ///  - **history**: recall the clicked entry into the bar and run it (jiq's single-click
    ///    recall).
    ///  - **facet / AI**: no row action (the facet is dismissed upstream; the AI popup is a text
    ///    prompt, not a list).
    fn popup_click(
        &mut self,
        kind: PopupKind,
        row: Option<usize>,
        x: u16,
        y: u16,
        now_ms: u64,
        drag: bool,
    ) {
        let Some(row) = row else {
            return;
        };
        let Some(abs) = self.popup_abs_index(kind, row) else {
            return;
        };
        match kind {
            PopupKind::Autocomplete => {
                self.autocomplete.set_selected(abs);
                if !drag
                    && self.double_click.check_and_record(
                        now_ms,
                        x,
                        y,
                        ClickSurface::Popup(kind),
                        Granularity::SameCell,
                    )
                {
                    self.accept_suggestion(now_ms);
                }
            }
            PopupKind::Palette => {
                if let Some(palette) = self.palette.as_mut() {
                    palette.set_cursor(abs);
                }
                if !drag
                    && self.double_click.check_and_record(
                        now_ms,
                        x,
                        y,
                        ClickSurface::Popup(kind),
                        Granularity::SameRow,
                    )
                {
                    if let Some(palette) = self.palette.as_mut() {
                        palette.toggle_at_cursor();
                    }
                    self.write_palette_to_select_and_schedule(now_ms);
                }
            }
            PopupKind::History => {
                self.history.set_selected_index(abs);
                if !drag {
                    self.recall_selected_history(now_ms);
                }
            }
            PopupKind::Facet | PopupKind::Ai | PopupKind::Save => {}
        }
    }
}
