//! App render layer — query bar (top) + results grid (middle) + status line (bottom).
//!
//! `dev/PLAN.md` §3 / §4.1: a **pure function of `App` state into a `Frame`**. The only
//! `Frame`-touching code in the shell besides the grid blit, and like every ciq render surface
//! it is `TestBackend`-snapshot-tested (NOT shell-exempt — `TestBackend` is an in-memory cell
//! grid an agent asserts). All colors come from [`theme::app`] / [`theme::grid`]; this file
//! never names a `Color`.
//!
//! Layout: a one-row query bar, a bordered results pane (which re-lays-out the retained result
//! `rows` against the *actual* inner viewport so a resize reflows without re-querying — §3.1's
//! "App re-lays-out from retained rows on resize"), and a one-row status line.

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::ai::ai_render::render_ai;
use crate::app::{App, AppPhase};
use crate::autocomplete::autocomplete_render::{MAX_VISIBLE_ROWS, render_popup};
use crate::facets::facet_render::render_facet;
use crate::grid::{GridFrame, GridView, grid_render, layout_grid};
use crate::history::history_render::render_history;
use crate::history::history_state::MAX_VISIBLE_HISTORY;
use crate::palette::palette_render::{MAX_VISIBLE_ROWS as PALETTE_MAX_ROWS, render_palette};
use crate::schema_bar;
use crate::theme;

/// Max content rows the facet popup reserves: the histogram is up to 2 stat lines + the top-K bars
/// ([`DEFAULT_TOP_K`](crate::facets::facet_query::DEFAULT_TOP_K) = 10), so 12 covers the largest
/// case; a summary (4 lines) fits comfortably inside it. The popup is height-clamped to the pane.
const FACET_POPUP_ROWS: u16 = 12;

/// Render the whole app into `frame`.
pub fn render(app: &App, frame: &mut Frame) {
    let area = frame.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // query bar
            Constraint::Min(1),    // results pane
            Constraint::Length(1), // status line
        ])
        .split(area);

    render_query_bar(app, frame, chunks[0]);
    render_results(app, frame, chunks[1]);
    render_status(app, frame, chunks[2]);
    // The autocomplete popup overlays the results pane, anchored under the query bar (drawn last
    // so it sits on top). Headless: still just cells in the TestBackend buffer.
    render_autocomplete(app, frame, chunks[0], chunks[1]);
    // The column palette overlays the results pane when open (it and the autocomplete popup are
    // mutually exclusive — opening the palette closes the popup). Drawn last so it sits on top.
    render_palette_popup(app, frame, chunks[0], chunks[1]);
    // The facet popup overlays the results pane when open. Drawn last so it sits on top.
    render_facet_popup(app, frame, chunks[0], chunks[1]);
    // The history popup overlays the results pane when open (mutually exclusive with the palette /
    // autocomplete popup — opening it closes them). Drawn last so it sits on top.
    render_history_popup(app, frame, chunks[0], chunks[1]);
    // The AI NL->SQL popup overlays the results pane when open (mutually exclusive with the other
    // popups — opening it closes them). Drawn last so it sits on top.
    render_ai_popup(app, frame, chunks[0], chunks[1]);
}

/// Overlay the AI NL->SQL popup below the query bar, over the results pane, when open (P5.1). A
/// short fixed-height box (the prompt line + a status line) anchored under the bar. No-op when the
/// popup is closed.
fn render_ai_popup(app: &App, frame: &mut Frame, bar: Rect, results: Rect) {
    if !app.is_ai_open() {
        return;
    }
    // The prompt row + a status row + the border (2) = 4; clamped to the available height.
    let height = 4u16.min(results.height.max(1));
    let width = popup_width(results.width);
    let area = Rect {
        x: bar.x,
        y: bar.y.saturating_add(1),
        width,
        height,
    };
    render_ai(app.ai(), frame, area);
}

/// Overlay the history popup below the query bar, over the results pane, when open (P5.2). Sized to
/// the filtered entry count (capped by the visible-row window and the available height) and to a
/// readable fraction of the width. No-op when the popup is closed.
fn render_history_popup(app: &App, frame: &mut Frame, bar: Rect, results: Rect) {
    if !app.is_history_open() {
        return;
    }
    let rows = (app.history().filtered_count().max(1) as u16).min(MAX_VISIBLE_HISTORY as u16);
    let height = (rows + 2).min(results.height.max(1)); // +2 for the popup border
    let width = popup_width(results.width);
    let area = Rect {
        x: bar.x,
        y: bar.y.saturating_add(1),
        width,
        height,
    };
    render_history(app.history(), frame, area);
}

/// Overlay the facet popup below the query bar, over the results pane, when one is open (P4.6).
/// Sized to the stat/histogram line count (capped by the available height) and to a readable
/// fraction of the width. No-op when no facet is open.
fn render_facet_popup(app: &App, frame: &mut Frame, bar: Rect, results: Rect) {
    let Some(facet) = app.facet() else {
        return;
    };
    let height = (FACET_POPUP_ROWS + 2).min(results.height.max(1)); // +2 for the popup border
    let width = popup_width(results.width);
    let area = Rect {
        x: bar.x,
        y: bar.y.saturating_add(1),
        width,
        height,
    };
    render_facet(facet, frame, area);
}

/// Overlay the column palette below the query bar, over the results pane, when it is open. Sized to
/// the column count (capped by the palette's visible-row window and the available height) and to a
/// readable fraction of the width. No-op when the palette is closed.
fn render_palette_popup(app: &App, frame: &mut Frame, bar: Rect, results: Rect) {
    if !app.is_palette_open() {
        return;
    }
    let Some(palette) = app.palette() else {
        return;
    };
    let rows = (palette.all_columns().len() as u16).clamp(1, PALETTE_MAX_ROWS);
    let height = (rows + 2).min(results.height.max(1)); // +2 for the popup border
    let width = popup_width(results.width);
    let area = Rect {
        x: bar.x,
        y: bar.y.saturating_add(1),
        width,
        height,
    };
    render_palette(palette, frame, area);
}

/// Overlay the autocomplete popup directly below the query bar, over the results pane. Sized to
/// the candidate count (capped by [`MAX_VISIBLE_ROWS`] and the available height) and to a readable
/// fraction of the width. No-op when the popup is closed (handled inside `render_popup`).
fn render_autocomplete(app: &App, frame: &mut Frame, bar: Rect, results: Rect) {
    let state = app.autocomplete();
    if !state.is_open() {
        return;
    }
    let rows = (state.len() as u16).min(MAX_VISIBLE_ROWS);
    let height = (rows + 2).min(results.height.max(1)); // +2 for the popup border
    let width = popup_width(results.width);
    let area = Rect {
        x: bar.x,
        y: bar.y.saturating_add(1),
        width,
        height,
    };
    render_popup(state, frame, area);
}

/// The popup width: a readable fraction of the pane width, clamped so it neither overflows nor
/// shrinks below a usable minimum.
fn popup_width(pane_width: u16) -> u16 {
    const MIN: u16 = 16;
    const MAX: u16 = 40;
    pane_width.clamp(MIN.min(pane_width.max(1)), MAX.min(pane_width.max(1)))
}

/// The query bar: a prompt glyph followed by the current query text.
fn render_query_bar(app: &App, frame: &mut Frame, area: Rect) {
    let line = Line::from(vec![
        Span::styled("> ", theme::app::prompt()),
        Span::styled(app.query().to_string(), theme::app::query_text()),
    ]);
    frame.render_widget(Paragraph::new(line), area);
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
            schema_bar::summary(delim, header),
            theme::schema_bar::summary(),
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
        // The truncation banner (when the grid is ciq-capped) pins the top inner row; the schema
        // bar pins the next row (above the grid's sticky header). Each reserved row shrinks the
        // grid's viewport by exactly that one row.
        let banner = app.truncation_banner();
        let (banner_area, below_banner) = split_off_banner(inner, banner.as_deref());
        if let (Some(area), Some(text)) = (banner_area, banner) {
            let p = Paragraph::new(Span::styled(text, theme::app::truncation_banner()));
            frame.render_widget(p, area);
        }
        let (bar_area, grid_area) = split_off_schema_bar(below_banner);

        // Re-lay-out from the retained rows against the grid's (post-bar) viewport so a resize
        // reflows without re-querying (§3.1). Column-granular h-scroll from the App's offset.
        let view = GridView {
            width: grid_area.width,
            height: grid_area.height,
            h_col_offset: app.h_col_offset(),
            v_row_offset: app.v_row_offset(),
        };
        let grid = layout_grid(&result.rows, &view);
        render_schema_bar(app, frame, bar_area, &grid);
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

/// Split the results pane inner area into the one-row schema bar (top) and the grid area (the
/// rows below). When the pane is only one row tall there is no room for both, so the grid keeps
/// the whole area and the bar is empty (degenerate; `render_schema_bar` no-ops on a 0-height area).
fn split_off_schema_bar(inner: Rect) -> (Rect, Rect) {
    if inner.height <= 1 {
        return (Rect { height: 0, ..inner }, inner);
    }
    let bar = Rect { height: 1, ..inner };
    let grid = Rect {
        y: inner.y.saturating_add(1),
        height: inner.height.saturating_sub(1),
        ..inner
    };
    (bar, grid)
}

/// Render the schema bar over the grid's computed geometry so names align dead-on with their data
/// columns and scroll in lockstep (shared `h_col_offset`). No-op without a loaded schema.
fn render_schema_bar(app: &App, frame: &mut Frame, area: Rect, grid: &GridFrame) {
    let Some(schema) = app.schema() else {
        return;
    };
    schema_bar::render_schema_bar(frame, area, schema, grid, app.h_col_offset(), None);
}

/// The status line: error-styled when the phase is a load error, normal otherwise.
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
