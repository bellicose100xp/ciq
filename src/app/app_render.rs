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
//! re-querying — §3.1's "App re-lays-out from retained rows on resize"), then a one-row query
//! bar near the bottom, then a one-row status line at the very bottom. The query *input* sits at
//! the bottom of the screen; popups anchor just **above** it, over the results pane.

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::ai::ai_render::render_ai;
use crate::app::{App, AppPhase, Focus};
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

/// How tall the query bar may grow as the query gains lines (the multiline policy). A single-line
/// query keeps the bar one row (so the felt geometry is unchanged); each additional line adds a
/// row up to this cap, after which the textarea scrolls within the fixed window. Chosen small so a
/// long query never crowds out the results pane.
const MAX_BAR_ROWS: u16 = 5;

/// The query bar height for the current query: one row per line, clamped to [1, [`MAX_BAR_ROWS`]].
fn bar_height(app: &App) -> u16 {
    (app.editor().line_count() as u16).clamp(1, MAX_BAR_ROWS)
}

/// Render the whole app into `frame`.
pub fn render(app: &App, frame: &mut Frame) {
    let area = frame.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),                  // results pane (fills the space)
            Constraint::Length(bar_height(app)), // query bar (grows with line count, capped)
            Constraint::Length(1),               // status line (very bottom)
        ])
        .split(area);

    let results = chunks[0];
    let bar = chunks[1];

    render_results(app, frame, results);
    render_query_bar(app, frame, bar);
    render_status(app, frame, chunks[2]);
    // The popups overlay the results pane, anchored just ABOVE the bottom query bar (drawn last so
    // they sit on top). Headless: still just cells in the TestBackend buffer. They are mutually
    // exclusive (opening one closes the others), so painting them all is at most one visible box.
    render_autocomplete(app, frame, bar, results);
    render_palette_popup(app, frame, bar, results);
    render_facet_popup(app, frame, bar, results);
    render_history_popup(app, frame, bar, results);
    render_ai_popup(app, frame, bar, results);
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
    let rows = (palette.all_columns().len() as u16).clamp(1, PALETTE_MAX_ROWS);
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

/// Width of the leading `> ` prompt column reserved at the left of the query bar.
const PROMPT_WIDTH: u16 = 2;

/// The query bar: a `> ` prompt glyph in a fixed left column, then the multiline editing textarea
/// (which paints its own visible cursor cell into the buffer — headless-snapshotable). The prompt
/// pins to the top row so it reads as a single baseline marker even when the textarea spans rows.
fn render_query_bar(app: &App, frame: &mut Frame, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let prompt_w = PROMPT_WIDTH.min(area.width);
    let prompt_area = Rect {
        height: 1,
        width: prompt_w,
        ..area
    };
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled("> ", theme::app::prompt()))),
        prompt_area,
    );
    let text_area = Rect {
        x: area.x.saturating_add(prompt_w),
        width: area.width.saturating_sub(prompt_w),
        ..area
    };
    if text_area.width > 0 {
        frame.render_widget(app.editor().textarea(), text_area);
    }
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

/// The status line: the status text (error-styled on a load error, normal otherwise) at the left,
/// and the vim mode badge (`INSERT` / `NORMAL` / `d(` …) pinned to the right when the query bar has
/// focus — so the editing mode is always visible (the help bar will consume the same badge later).
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

    if app.focus() == Focus::QueryBar {
        let badge = app.editor_mode().display();
        let badge_w = (badge.chars().count() as u16).min(area.width);
        if badge_w > 0 {
            let badge_area = Rect {
                x: area.x + area.width - badge_w,
                width: badge_w,
                ..area
            };
            frame.render_widget(
                Paragraph::new(Span::styled(badge, theme::app::mode_indicator())),
                badge_area,
            );
        }
    }
}

#[cfg(test)]
#[path = "app_render_tests.rs"]
mod app_render_tests;
