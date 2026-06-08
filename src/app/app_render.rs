//! App render layer — results grid (top) + query bar + status line (both bottom).
//!
//! `dev/PLAN.md` §3 / §4.1: a **pure function of `App` state into a `Frame`**. The only
//! `Frame`-touching code in the shell besides the grid blit, and like every ciq render surface
//! it is `TestBackend`-snapshot-tested (NOT shell-exempt — `TestBackend` is an in-memory cell
//! grid an agent asserts). All colors come from [`theme::app`] / [`theme::grid`]; this file
//! never names a `Color`.
//!
//! Layout (top -> bottom): a bordered results pane filling the space (which re-lays-out the
//! retained result `rows` against the *actual* inner viewport so a resize reflows without
//! re-querying — §3.1's "App re-lays-out from retained rows on resize"), then a **bordered query
//! box** near the bottom whose **bottom border carries the context-sensitive keyboard help hints**
//! (§4.1, jiq-style — the hints live on the box border, not a standalone row), then a one-row
//! status line at the very bottom. The query *input* sits near the bottom of the screen; popups
//! anchor just **above** the query box, over the results pane.

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::ai::ai_render::render_ai;
use crate::app::help_line;
use crate::app::layout_regions::{LayoutRegions, PopupKind};
use crate::app::{App, AppPhase};
use crate::autocomplete::autocomplete_render::{MAX_VISIBLE_ROWS, render_popup};
use crate::facets::facet_render::render_facet;
use crate::grid::{GridView, grid_render, layout_grid};
use crate::history::history_render::render_history;
use crate::history::history_state::MAX_VISIBLE_HISTORY;
use crate::ingest::dialect_summary;
use crate::palette::palette_render::{MAX_VISIBLE_ROWS as PALETTE_MAX_ROWS, render_palette};
use crate::theme;

/// Max content rows the facet popup reserves: the histogram is up to 2 stat lines + the top-K bars
/// ([`DEFAULT_TOP_K`](crate::facets::facet_query::DEFAULT_TOP_K) = 10), so 12 covers the largest
/// case; a summary (4 lines) fits comfortably inside it. The popup is height-clamped to the pane.
const FACET_POPUP_ROWS: u16 = 12;

/// How many *text* rows the query box may grow to as the query gains lines (the multiline policy).
/// A single-line query keeps one text row; each additional line adds a row up to this cap, after
/// which the textarea scrolls within the fixed window. Chosen small so a long query never crowds
/// out the results pane. The box itself is 2 rows taller (top + bottom border) — see [`box_height`].
const MAX_BAR_ROWS: u16 = 5;

/// The number of editable text rows for the current query: one per line, clamped to
/// [1, [`MAX_BAR_ROWS`]].
fn bar_text_rows(app: &App) -> u16 {
    (app.editor().line_count() as u16).clamp(1, MAX_BAR_ROWS)
}

/// The total height of the bordered query box: the text rows plus the top + bottom border rows.
/// The bottom border carries the keyboard help hints (§4.1).
fn box_height(app: &App) -> u16 {
    bar_text_rows(app).saturating_add(2)
}

/// Render the whole app into `frame`.
pub fn render(app: &App, frame: &mut Frame) {
    let area = frame.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),                  // results pane (fills the space)
            Constraint::Length(box_height(app)), // bordered query box (text rows + 2 border rows)
            Constraint::Length(1),               // status line (very bottom)
        ])
        .split(area);

    let results = chunks[0];
    let query_box = chunks[1];

    render_results(app, frame, results);
    // The query box is bordered; its bottom border carries the help hints. `render_query_box`
    // returns the inner text rect — what the editor occupies and what a mouse click maps onto.
    let text_area = render_query_box(app, frame, query_box);
    render_status(app, frame, chunks[2]);
    // The popups overlay the results pane, anchored just ABOVE the bottom query box (drawn last so
    // they sit on top). Headless: still just cells in the TestBackend buffer. They are mutually
    // exclusive (opening one closes the others), so painting them all is at most one visible box.
    render_autocomplete(app, frame, query_box, results);
    render_palette_popup(app, frame, query_box, results);
    render_facet_popup(app, frame, query_box, results);
    render_history_popup(app, frame, query_box, results);
    render_ai_popup(app, frame, query_box, results);

    // Record the on-screen regions so the next mouse event resolves against the geometry the user
    // actually sees (the click-to-focus / click-to-position / scroll-routing seam). The query-bar
    // region is the box's INNER text area (border-stripped), so the existing prompt/text-col mouse
    // math is unchanged. At most one popup is open (mutually exclusive overlays), recomputed with
    // the same `popup_above_bar` anchoring the render used above.
    app.set_layout_regions(LayoutRegions {
        results_pane: Some(results),
        query_bar: Some(text_area),
        popup: active_popup_region(app, query_box, results),
    });
}

/// The kind + on-screen rect of the single open popup (mutually exclusive overlays), or `None` when
/// none is open. Mirrors each `render_*_popup` sizing so the recorded region matches what was drawn.
fn active_popup_region(app: &App, bar: Rect, results: Rect) -> Option<(PopupKind, Rect)> {
    if app.is_ai_open() {
        return Some((PopupKind::Ai, popup_above_bar(bar, results, 2)));
    }
    if app.is_history_open() {
        let rows = (app.history().filtered_count().max(1) as u16).min(MAX_VISIBLE_HISTORY as u16);
        return Some((PopupKind::History, popup_above_bar(bar, results, rows)));
    }
    if app.is_palette_open()
        && let Some(palette) = app.palette()
    {
        // Size from the FILTERED count (what render_palette actually draws), not the full column
        // count — otherwise a needle that narrows the list leaves the recorded box taller than the
        // drawn one, and a click in the blank lower band mis-resolves to a Popup row (finding).
        let rows = (palette.filtered_indices().len().max(1) as u16).min(PALETTE_MAX_ROWS);
        return Some((PopupKind::Palette, popup_above_bar(bar, results, rows)));
    }
    if app.facet().is_some() {
        return Some((
            PopupKind::Facet,
            popup_above_bar(bar, results, FACET_POPUP_ROWS),
        ));
    }
    if app.autocomplete().is_open() {
        let rows = (app.autocomplete().len() as u16).min(MAX_VISIBLE_ROWS);
        return Some((PopupKind::Autocomplete, popup_above_bar(bar, results, rows)));
    }
    None
}

/// The screen rectangle for a popup of `content_rows` content rows + a border, anchored so its
/// **bottom** edge sits just above the query `bar` and it grows upward into the `results` pane.
/// Height is clamped to the available results height; the box never overflows the top of the pane.
fn popup_above_bar(bar: Rect, results: Rect, content_rows: u16) -> Rect {
    let height = (content_rows + 2).min(results.height.max(1)); // +2 for the popup border
    // Anchor the box's bottom edge on the row directly above the bar, growing upward.
    let bottom = bar.y; // exclusive bottom (the bar row itself stays visible)
    let y = bottom.saturating_sub(height).max(results.y);
    Rect {
        x: bar.x,
        y,
        width: popup_width(results.width),
        height,
    }
}

/// Overlay the AI NL->SQL popup just above the query bar, over the results pane, when open (P5.1).
/// A short fixed-height box (the prompt line + a status line). No-op when the popup is closed.
fn render_ai_popup(app: &App, frame: &mut Frame, bar: Rect, results: Rect) {
    if !app.is_ai_open() {
        return;
    }
    // The prompt row + a status row = 2 content rows (the border adds the other 2 of the box).
    let area = popup_above_bar(bar, results, 2);
    render_ai(app.ai(), frame, area);
}

/// Overlay the history popup just above the query bar, over the results pane, when open (P5.2).
/// Sized to the filtered entry count (capped by the visible-row window and the available height)
/// and to a readable fraction of the width. No-op when the popup is closed.
fn render_history_popup(app: &App, frame: &mut Frame, bar: Rect, results: Rect) {
    if !app.is_history_open() {
        return;
    }
    let rows = (app.history().filtered_count().max(1) as u16).min(MAX_VISIBLE_HISTORY as u16);
    let area = popup_above_bar(bar, results, rows);
    render_history(app.history(), frame, area);
}

/// Overlay the facet popup just above the query bar, over the results pane, when one is open
/// (P4.6). Sized to the stat/histogram line count (capped by the available height) and to a
/// readable fraction of the width. No-op when no facet is open.
fn render_facet_popup(app: &App, frame: &mut Frame, bar: Rect, results: Rect) {
    let Some(facet) = app.facet() else {
        return;
    };
    let area = popup_above_bar(bar, results, FACET_POPUP_ROWS);
    render_facet(facet, frame, area);
}

/// Overlay the column palette just above the query bar, over the results pane, when it is open.
/// Sized to the column count (capped by the palette's visible-row window and the available height)
/// and to a readable fraction of the width. No-op when the palette is closed.
fn render_palette_popup(app: &App, frame: &mut Frame, bar: Rect, results: Rect) {
    if !app.is_palette_open() {
        return;
    }
    let Some(palette) = app.palette() else {
        return;
    };
    // Mirror active_popup_region: size from the filtered count so the recorded region matches the
    // drawn box (the "(no match)" line still reserves one row via the `.max(1)`).
    let rows = (palette.filtered_indices().len().max(1) as u16).min(PALETTE_MAX_ROWS);
    let area = popup_above_bar(bar, results, rows);
    render_palette(palette, frame, area);
}

/// Overlay the autocomplete popup just above the query bar, over the results pane. Sized to the
/// candidate count (capped by [`MAX_VISIBLE_ROWS`] and the available height) and to a readable
/// fraction of the width. No-op when the popup is closed (handled inside `render_popup`).
fn render_autocomplete(app: &App, frame: &mut Frame, bar: Rect, results: Rect) {
    let state = app.autocomplete();
    if !state.is_open() {
        return;
    }
    let rows = (state.len() as u16).min(MAX_VISIBLE_ROWS);
    let area = popup_above_bar(bar, results, rows);
    render_popup(state, frame, area);
}

/// The popup width: a readable fraction of the pane width, clamped so it neither overflows nor
/// shrinks below a usable minimum.
fn popup_width(pane_width: u16) -> u16 {
    const MIN: u16 = 16;
    const MAX: u16 = 40;
    pane_width.clamp(MIN.min(pane_width.max(1)), MAX.min(pane_width.max(1)))
}

/// Width of the leading `> ` prompt column reserved at the left of the query bar. The single source
/// of truth for the prompt offset: the render uses it to place the textarea, and
/// [`App::on_mouse`](crate::app::App::on_mouse) uses it to map a query-bar click column onto the
/// editable text (subtracting the prompt).
pub(crate) const PROMPT_WIDTH: u16 = 2;

/// The query box: a bordered [`Block`] whose **bottom border carries the context-sensitive keyboard
/// help hints** (§4.1, jiq-style), wrapping a `> ` prompt glyph in a fixed left column + the
/// multiline editing textarea (which paints its own visible cursor cell into the buffer —
/// headless-snapshotable). The prompt pins to the top inner row so it reads as a single baseline
/// marker even when the textarea spans rows. Returns the box's **inner rect** (border-stripped,
/// before the prompt) so the caller records it as the mouse-click target — [`text_col`] subtracts
/// the `> ` prompt from it, so a click on the prompt clamps to column 0 and a click on the text
/// maps onto the right character (the click math is unchanged from the borderless bar, just shifted
/// in by the border). Returns a zero-size rect on a degenerate area.
fn render_query_box(app: &App, frame: &mut Frame, area: Rect) -> Rect {
    if area.width == 0 || area.height == 0 {
        return Rect::new(area.x, area.y, 0, 0);
    }
    // The help hints render on the bottom border as a left-aligned title. Build them width-aware so
    // a narrow box drops the lowest-priority trailing hints rather than overflowing the border (the
    // usable title width is the box width minus the two corner glyphs).
    let hint_width = area.width.saturating_sub(2) as usize;
    let hint_spans = help_line::hint_spans(app, hint_width);
    // `title_bottom` places the hints on the box's bottom border, left-aligned by default — jiq's
    // look, via ratatui 0.29's non-deprecated title API.
    let block = Block::default()
        .borders(Borders::ALL)
        .title_bottom(Line::from(hint_spans));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.width == 0 || inner.height == 0 {
        return Rect::new(inner.x, inner.y, 0, 0);
    }

    let prompt_w = PROMPT_WIDTH.min(inner.width);
    let prompt_area = Rect {
        height: 1,
        width: prompt_w,
        ..inner
    };
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled("> ", theme::app::prompt()))),
        prompt_area,
    );
    let text_area = Rect {
        x: inner.x.saturating_add(prompt_w),
        width: inner.width.saturating_sub(prompt_w),
        ..inner
    };
    if text_area.width > 0 {
        frame.render_widget(app.editor().textarea(), text_area);
    }
    // Record the INNER rect (before the prompt) as the click target: `text_col` subtracts the
    // prompt from it, matching the borderless-bar contract the mouse mapping was written against.
    inner
}

/// The results pane: a bordered box containing the aligned grid, a loading indicator, or an
/// empty hint. The grid is re-laid-out against the actual inner viewport.
fn render_results(app: &App, frame: &mut Frame, area: Rect) {
    let mut block = Block::default().borders(Borders::ALL);
    // Once loaded, surface the CSV dialect (`delim , | header on`) as the pane's border title —
    // the global indicator §6.3 calls for, distinct from the per-column schema bar below.
    if app.schema().is_some() {
        let (delim, header) = app.csv_summary();
        block = block.title(Span::styled(
            dialect_summary(delim, header),
            theme::app::dialect_summary(),
        ));
    }
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.width == 0 || inner.height == 0 {
        return;
    }

    match app.phase() {
        AppPhase::Loading => {
            let p = Paragraph::new(Span::styled("loading CSV…", theme::app::loading()));
            frame.render_widget(p, inner);
            return;
        }
        AppPhase::LoadError(msg) => {
            let p = Paragraph::new(Span::styled(
                format!("could not load CSV: {msg}"),
                theme::app::status_error(),
            ));
            frame.render_widget(p, inner);
            return;
        }
        AppPhase::Ready | AppPhase::Querying => {}
    }

    // An empty result (zero rows) or no result yet shows the empty-state line, never the grid.
    if let Some(message) = app.empty_state() {
        let p = Paragraph::new(Span::styled(message, theme::app::empty_state()));
        frame.render_widget(p, inner);
        return;
    }

    if let Some(result) = app.result() {
        // The truncation banner (when the grid is ciq-capped) pins the top inner row; each
        // reserved row shrinks the grid's viewport by exactly that one row. The grid's own sticky
        // header now carries each column's `name (badge)` type label, so there is no separate
        // schema-bar row (the dialect summary lives in the pane border title above).
        let banner = app.truncation_banner();
        let (banner_area, grid_area) = split_off_banner(inner, banner.as_deref());
        if let (Some(area), Some(text)) = (banner_area, banner) {
            let p = Paragraph::new(Span::styled(text, theme::app::truncation_banner()));
            frame.render_widget(p, area);
        }

        // Re-lay-out from the retained rows against the grid's viewport so a resize reflows
        // without re-querying (§3.1). Column-granular h-scroll from the App's offset.
        let view = GridView {
            width: grid_area.width,
            height: grid_area.height,
            h_col_offset: app.h_col_offset(),
            v_row_offset: app.v_row_offset(),
        };
        let grid = layout_grid(&result.rows, &view);
        grid_render::render_grid(frame, grid_area, &grid, app.v_row_offset());
    }
}

/// Reserve the top inner row for the truncation banner when one is present and there is room for
/// both it and at least one grid row. Returns `(banner_area, remaining_area)`; the banner area is
/// `None` when there is no banner or the pane is too short to spare a row.
fn split_off_banner(inner: Rect, banner: Option<&str>) -> (Option<Rect>, Rect) {
    if banner.is_none() || inner.height <= 1 {
        return (None, inner);
    }
    let banner_area = Rect { height: 1, ..inner };
    let rest = Rect {
        y: inner.y.saturating_add(1),
        height: inner.height.saturating_sub(1),
        ..inner
    };
    (Some(banner_area), rest)
}

/// The status line: the status text (error-styled on a load error, normal otherwise) at the left.
/// The vim mode badge (`INSERT` / `NORMAL` / `d(` …) leads the help hints on the query box's bottom
/// border, so the mode is always visible without duplicating it here.
fn render_status(app: &App, frame: &mut Frame, area: Rect) {
    let style = if matches!(app.phase(), AppPhase::LoadError(_)) {
        theme::app::status_error()
    } else {
        theme::app::status()
    };
    frame.render_widget(
        Paragraph::new(Span::styled(app.status().to_string(), style)),
        area,
    );
}

#[cfg(test)]
#[path = "app_render_tests.rs"]
mod app_render_tests;
