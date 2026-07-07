//! Thin blit shim: paint a [`GridFrame`] into a ratatui `Frame` (`dev/PLAN.md` §6.4).
//!
//! This is the ONLY grid code that sees a `Frame`. It does exactly: reserve the top inner row
//! for the **sticky header** (rendered with its own widget, no scroll, so body scrolling never
//! moves it), then render the body as a scrolled `Paragraph` in the area below. The body
//! viewport height is `inner_height - 1` (one row given to the header) — the single arithmetic
//! delta from jiq's reused vertical-slice model.
//!
//! It is headless-snapshot-tested via `ratatui::TestBackend` (NOT shell-exempt): `TestBackend`
//! is an in-memory cell grid, so an agent asserts the rendered buffer. All colors come from
//! `theme::grid::*` — this file never names a `Color`.

use std::ops::Range;

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::search::matcher;
use crate::theme;

use super::grid_layout::{BodyRow, GridFrame};

/// The body viewport height for a pane of `inner_height` rows: one row is reserved for the
/// sticky header. Pure; unit-tested.
pub fn body_viewport_height(inner_height: u16) -> u16 {
    inner_height.saturating_sub(1)
}

/// The presentation state for one grid paint — everything [`render_grid`] needs beyond the
/// laid-out [`GridFrame`] itself. Grouped so the blit's signature stays readable as options
/// accrue (scroll, stale-dim, hover, accent, search highlight).
#[derive(Debug, Clone, Copy, Default)]
pub struct GridPaint<'a> {
    /// Number of body rows scrolled off the top (the vertical slice offset).
    pub v_row_offset: usize,
    /// Dim the header + body ([`theme::grid::stale_modifier`]) — jiq's
    /// error-keeps-last-result-dimmed behavior.
    pub stale: bool,
    /// The absolute body-row index under the mouse pointer, if any — painted with the
    /// [`theme::grid::hovered_bg`] band plus a bright left accent bar (`▌`) that rides column 0
    /// of that row and follows the pointer.
    pub hovered_row: Option<usize>,
    /// The pane's state color; colors the hover bar so it harmonizes with the border chrome.
    pub accent: Color,
    /// The `Ctrl+F` needle: when non-empty, every case-insensitive occurrence within the
    /// visible body lines is highlighted (the filter's in-place match marking).
    pub search_needle: &'a str,
}

/// Render `frame` (the laid-out grid) into `area` with the given [`GridPaint`] presentation.
///
/// `area` is the inner pane (already inside any border). Header occupies `area`'s top row;
/// the body is rendered in the rows below it, sliced to the visible window.
pub fn render_grid(f: &mut Frame, area: Rect, grid: &GridFrame, paint: GridPaint<'_>) {
    if area.height == 0 {
        return;
    }
    let GridPaint {
        v_row_offset,
        stale,
        hovered_row,
        accent,
        search_needle,
    } = paint;

    let extra = if stale {
        theme::grid::stale_modifier()
    } else {
        Modifier::empty()
    };

    let header_area = Rect { height: 1, ..area };
    // Both header and body slide horizontally by `grid.body_scroll_chars` (char-grain) so they
    // stay in lockstep — the trackpad smooth scroll. ratatui's `Paragraph::scroll((rows, cols))`
    // is exactly the right primitive: cols is char-offset into the laid-out line.
    let h = grid.body_scroll_chars;
    let header = Paragraph::new(Line::from(Span::styled(
        grid.header.clone(),
        theme::grid::header().add_modifier(extra),
    )))
    .scroll((0, h));
    f.render_widget(header, header_area);

    let body_height = body_viewport_height(area.height);
    if body_height == 0 {
        return;
    }
    let body_area = Rect {
        y: area.y.saturating_add(1),
        height: body_height,
        ..area
    };

    let end = (v_row_offset + body_height as usize).min(grid.body.len());
    let visible = if v_row_offset < grid.body.len() {
        &grid.body[v_row_offset..end]
    } else {
        &[]
    };
    let hovered_offset = hovered_row
        .and_then(|abs| abs.checked_sub(v_row_offset))
        .filter(|&off| off < visible.len());
    let lines: Vec<Line> = visible
        .iter()
        .enumerate()
        .map(|(offset, row)| {
            let line = style_body_line(row, extra, search_needle);
            if Some(offset) == hovered_offset {
                line.style(theme::grid::hovered_bg())
            } else {
                line
            }
        })
        .collect();
    f.render_widget(Paragraph::new(lines).scroll((0, h)), body_area);

    // The bright left accent bar rides column 0 of the hovered row, painted AFTER the body so it
    // overlays the first cell (not scrolled with the body — it always marks the pane's left edge,
    // lazygit-style). The band already tints the whole row; this just adds the following bar.
    if let Some(offset) = hovered_offset {
        let bar_area = Rect {
            x: body_area.x,
            y: body_area.y.saturating_add(offset as u16),
            width: 1,
            height: 1,
        };
        f.render_widget(
            Paragraph::new(Span::styled("\u{258c}", theme::grid::hover_bar(accent))),
            bar_area,
        );
    }
}

/// Style one body line: dim the byte ranges `layout_grid` flagged as genuine SQL nulls so a
/// `NULL` reads as absent, leaving everything else (including a present `Cell::Text("NULL")`) in
/// the normal cell style; then paint the search needle's case-insensitive occurrences with the
/// match highlight. Null-ness comes from the layout mask, not from scanning the text — the text
/// alone cannot distinguish an absent null from data that happens to read "NULL". A match that
/// overlaps a null span keeps the null styling (the filter says NULL never matches; the render
/// agrees — a needle like "null" must not light up absent values). `extra` is OR'd into every
/// span's modifier so a stale-dimmed render adds DIM uniformly without losing the per-span
/// colors.
fn style_body_line(row: &BodyRow, extra: Modifier, needle: &str) -> Line<'static> {
    let matches = if needle.is_empty() {
        Vec::new()
    } else {
        matcher::find_matches(&row.text, needle)
    };
    if row.null_spans.is_empty() && matches.is_empty() {
        return Line::from(Span::styled(
            row.text.clone(),
            theme::grid::cell().add_modifier(extra),
        ));
    }
    // Walk the line as boundary-delimited segments; each takes the highest-precedence styling of
    // the range sets covering it (null > match > plain).
    let mut boundaries: Vec<usize> = vec![0, row.text.len()];
    for r in row.null_spans.iter().chain(matches.iter()) {
        boundaries.push(r.start);
        boundaries.push(r.end);
    }
    boundaries.sort_unstable();
    boundaries.dedup();
    let covered = |ranges: &[Range<usize>], start: usize| {
        ranges.iter().any(|r| r.start <= start && start < r.end)
    };
    let mut spans: Vec<Span<'static>> = Vec::new();
    for pair in boundaries.windows(2) {
        let (start, end) = (pair[0], pair[1]);
        if start == end {
            continue;
        }
        let style = if covered(&row.null_spans, start) {
            theme::grid::null()
        } else if covered(&matches, start) {
            theme::grid::search_match()
        } else {
            theme::grid::cell()
        };
        spans.push(Span::styled(
            row.text[start..end].to_string(),
            style.add_modifier(extra),
        ));
    }
    Line::from(spans)
}

#[cfg(test)]
#[path = "grid_render_tests.rs"]
mod grid_render_tests;
