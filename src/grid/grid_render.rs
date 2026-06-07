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

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::theme;

use super::col_width::NULL_GLYPH;
use super::grid_layout::GridFrame;

/// The body viewport height for a pane of `inner_height` rows: one row is reserved for the
/// sticky header. Pure; unit-tested.
pub fn body_viewport_height(inner_height: u16) -> u16 {
    inner_height.saturating_sub(1)
}

/// Render `frame` (the laid-out grid) into `area`, scrolling the body by `v_row_offset` rows.
///
/// `area` is the inner pane (already inside any border). Header occupies `area`'s top row;
/// the body is rendered in the rows below it, sliced to the visible window.
pub fn render_grid(f: &mut Frame, area: Rect, grid: &GridFrame, v_row_offset: usize) {
    if area.height == 0 {
        return;
    }

    let header_area = Rect { height: 1, ..area };
    let header = Paragraph::new(Line::from(Span::styled(
        grid.header.clone(),
        theme::grid::header(),
    )));
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
    let lines: Vec<Line> = visible.iter().map(|row| style_body_line(row)).collect();
    f.render_widget(Paragraph::new(lines), body_area);
}

/// Style one body line: dim any run of the null glyph so a `NULL` reads as absent. The line
/// text is already laid out (aligned + padded) by `layout_grid`; here we only colorize.
fn style_body_line(row: &str) -> Line<'static> {
    if !row.contains(NULL_GLYPH) {
        return Line::from(Span::styled(row.to_string(), theme::grid::cell()));
    }
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut rest = row;
    while let Some(pos) = rest.find(NULL_GLYPH) {
        if pos > 0 {
            spans.push(Span::styled(rest[..pos].to_string(), theme::grid::cell()));
        }
        spans.push(Span::styled(NULL_GLYPH.to_string(), theme::grid::null()));
        rest = &rest[pos + NULL_GLYPH.len()..];
    }
    if !rest.is_empty() {
        spans.push(Span::styled(rest.to_string(), theme::grid::cell()));
    }
    Line::from(spans)
}

#[cfg(test)]
#[path = "grid_render_tests.rs"]
mod grid_render_tests;
