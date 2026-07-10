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
    /// The absolute body-row index of the **current** search match, if any. Its matched runs are
    /// painted with [`theme::grid::current_match`] (bright, distinct) instead of the dim
    /// [`theme::grid::search_match`] every other match gets — jiq's current-vs-other match colors.
    pub current_match_row: Option<usize>,
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
        current_match_row,
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
    let header = Paragraph::new(style_header_line(grid, extra)).scroll((0, h));
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

    // `grid.body` is already the visible page starting at `grid.body_row_offset` (the layout
    // windowed it), so rebase the absolute `v_row_offset` / hover / current-match indices onto the
    // page before slicing. In the normal path `v_row_offset == body_row_offset`, so `local` is 0
    // and the whole page is taken; the subtraction only guards a caller that slices further.
    let base = grid.body_row_offset;
    let local = v_row_offset.saturating_sub(base);
    let end = (local + body_height as usize).min(grid.body.len());
    let visible = if local < grid.body.len() {
        &grid.body[local..end]
    } else {
        &[]
    };
    let hovered_offset = hovered_row
        .and_then(|abs| abs.checked_sub(v_row_offset))
        .filter(|&off| off < visible.len());
    let current_offset = current_match_row.and_then(|abs| abs.checked_sub(v_row_offset));
    let lines: Vec<Line> = visible
        .iter()
        .enumerate()
        .map(|(offset, row)| {
            let is_current = Some(offset) == current_offset;
            let line = style_body_line(row, &grid.col_indices, extra, search_needle, is_current);
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

/// Style the sticky header line: paint each column's label in its column's pastel hue (keyed on
/// the absolute column index via [`theme::grid::header_column`]), so every header ties to the
/// color of the data below it. Any gap between labels (the gutter, or the empty tail of a
/// header-only frame) takes the neutral header style. `extra` rides every span so a stale render
/// dims uniformly. Falls back to a single neutral span when the frame carries no header spans (an
/// empty grid).
fn style_header_line(grid: &GridFrame, extra: Modifier) -> Line<'static> {
    if grid.header_spans.is_empty() {
        return Line::from(Span::styled(
            grid.header.clone(),
            theme::grid::header().add_modifier(extra),
        ));
    }
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut cursor = 0usize;
    for (span, &col) in grid.header_spans.iter().zip(&grid.col_indices) {
        if cursor < span.start {
            // Gutter between two labels — neutral.
            spans.push(Span::styled(
                grid.header[cursor..span.start].to_string(),
                theme::grid::header().add_modifier(extra),
            ));
        }
        spans.push(Span::styled(
            grid.header[span.start..span.end].to_string(),
            theme::grid::header_column(col).add_modifier(extra),
        ));
        cursor = span.end;
    }
    if cursor < grid.header.len() {
        spans.push(Span::styled(
            grid.header[cursor..].to_string(),
            theme::grid::header().add_modifier(extra),
        ));
    }
    Line::from(spans)
}

/// Style one body line. Each visible cell is painted in its column's pastel hue (keyed on the
/// absolute column index in `col_indices`, so a column keeps its color under horizontal scroll),
/// with two overrides layered on top:
/// - genuine SQL nulls (the byte ranges `layout_grid` flagged) render dim so a `NULL` reads as
///   absent — the text alone can't distinguish an absent null from data that reads "NULL", so
///   null-ness comes from the layout mask;
/// - the search needle's case-insensitive occurrences take the match highlight (bright band).
///
/// Precedence is null > match > column-hue: a match overlapping a null keeps the null styling (the
/// filter says NULL never matches; the render agrees). `extra` is OR'd into every span so a
/// stale-dimmed render adds DIM uniformly without losing the per-span colors. When `is_current`
/// the row is the current search match, so its matched runs use the bright
/// [`theme::grid::current_match`] style instead of the dim [`theme::grid::search_match`].
fn style_body_line(
    row: &BodyRow,
    col_indices: &[usize],
    extra: Modifier,
    needle: &str,
    is_current: bool,
) -> Line<'static> {
    let matches = if needle.is_empty() {
        Vec::new()
    } else {
        // Match within each cell (never across the gutter), so a needle can't spuriously
        // "match" a run that straddles two columns' padding.
        cell_scoped_matches(row, needle)
    };
    let match_style = if is_current {
        theme::grid::current_match()
    } else {
        theme::grid::search_match()
    };
    // Walk the line as boundary-delimited segments; each takes the highest-precedence styling of
    // the range sets covering it (null > match > column-hue). Cell boundaries are always included
    // so each segment lies within exactly one column and picks up that column's pastel hue.
    let mut boundaries: Vec<usize> = vec![0, row.text.len()];
    for r in row
        .cell_spans
        .iter()
        .chain(row.null_spans.iter())
        .chain(matches.iter())
    {
        boundaries.push(r.start);
        boundaries.push(r.end);
    }
    boundaries.sort_unstable();
    boundaries.dedup();
    let covered = |ranges: &[Range<usize>], start: usize| {
        ranges.iter().any(|r| r.start <= start && start < r.end)
    };
    // The absolute column index of the cell containing byte `start`, if any — its position among
    // `cell_spans` maps 1:1 onto `col_indices` (both in visible-column order).
    let column_at = |start: usize| {
        row.cell_spans
            .iter()
            .position(|c| c.start <= start && start < c.end)
            .and_then(|vis| col_indices.get(vis).copied())
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
            match_style
        } else {
            match column_at(start) {
                Some(col) => theme::grid::column(col),
                None => theme::grid::cell(),
            }
        };
        spans.push(Span::styled(
            row.text[start..end].to_string(),
            style.add_modifier(extra),
        ));
    }
    if spans.is_empty() {
        // No visible columns (empty row) — emit one neutral span so the line still renders.
        return Line::from(Span::styled(
            row.text.clone(),
            theme::grid::cell().add_modifier(extra),
        ));
    }
    Line::from(spans)
}

/// Needle matches found **within each cell** of the row, offset back into the row's byte space.
/// Scoping to cells (via [`BodyRow::cell_spans`]) keeps a match from straddling the two-space
/// gutter between columns — a run of gutter+cell-edge text is never a real cell value.
fn cell_scoped_matches(row: &BodyRow, needle: &str) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    for cell in &row.cell_spans {
        let text = &row.text[cell.start..cell.end];
        for m in matcher::find_matches(text, needle) {
            out.push((cell.start + m.start)..(cell.start + m.end));
        }
    }
    out
}

#[cfg(test)]
#[path = "grid_render_tests.rs"]
mod grid_render_tests;
