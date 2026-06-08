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
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::theme;

use super::grid_layout::{BodyRow, GridFrame};

/// The body viewport height for a pane of `inner_height` rows: one row is reserved for the
/// sticky header. Pure; unit-tested.
pub fn body_viewport_height(inner_height: u16) -> u16 {
    inner_height.saturating_sub(1)
}

/// Render `frame` (the laid-out grid) into `area`, scrolling the body by `v_row_offset` rows.
///
/// `area` is the inner pane (already inside any border). Header occupies `area`'s top row;
/// the body is rendered in the rows below it, sliced to the visible window. When `stale` is
/// `true`, the header + body cells carry [`theme::grid::stale_modifier`] (dim) so the user sees
/// the last-good grid kept under the error message in the status line (jiq's
/// error-keeps-last-result-dimmed behavior).
pub fn render_grid(f: &mut Frame, area: Rect, grid: &GridFrame, v_row_offset: usize, stale: bool) {
    if area.height == 0 {
        return;
    }

    let extra = if stale {
        theme::grid::stale_modifier()
    } else {
        Modifier::empty()
    };

    let header_area = Rect { height: 1, ..area };
    let header = Paragraph::new(Line::from(Span::styled(
        grid.header.clone(),
        theme::grid::header().add_modifier(extra),
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
    let lines: Vec<Line> = visible
        .iter()
        .map(|row| style_body_line(row, extra))
        .collect();
    f.render_widget(Paragraph::new(lines), body_area);
}

/// Style one body line: dim the byte ranges `layout_grid` flagged as genuine SQL nulls so a
/// `NULL` reads as absent, leaving everything else (including a present `Cell::Text("NULL")`) in
/// the normal cell style. Null-ness comes from the layout mask, not from scanning the text — the
/// text alone cannot distinguish an absent null from data that happens to read "NULL". `extra` is
/// OR'd into every span's modifier so a stale-dimmed render adds DIM uniformly without losing the
/// per-span colors.
fn style_body_line(row: &BodyRow, extra: Modifier) -> Line<'static> {
    if row.null_spans.is_empty() {
        return Line::from(Span::styled(
            row.text.clone(),
            theme::grid::cell().add_modifier(extra),
        ));
    }
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut cursor = 0usize;
    for span in &row.null_spans {
        if span.start > cursor {
            spans.push(Span::styled(
                row.text[cursor..span.start].to_string(),
                theme::grid::cell().add_modifier(extra),
            ));
        }
        spans.push(Span::styled(
            row.text[span.start..span.end].to_string(),
            theme::grid::null().add_modifier(extra),
        ));
        cursor = span.end;
    }
    if cursor < row.text.len() {
        spans.push(Span::styled(
            row.text[cursor..].to_string(),
            theme::grid::cell().add_modifier(extra),
        ));
    }
    Line::from(spans)
}

#[cfg(test)]
#[path = "grid_render_tests.rs"]
mod grid_render_tests;
