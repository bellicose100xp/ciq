//! Palette popup blit — `render_palette(state, frame, area)` over [`PaletteState`] (the SELECT-pane
//! column picker; user-locked redesign 2026-06-09).
//!
//! Each row is a checkbox (`[x]` checked / `[ ]` unchecked), the column name, and the column's
//! [`ColumnType`](crate::schema::ColumnType) badge right-aligned. The cursor row is reverse-video
//! with the popup's distinct accent so the popup reads as visually separate from the cyan-default
//! popups (autocomplete, history, AI, facet). The popup's BOTTOM border carries only the
//! non-obvious bulk-op shortcut hints (`Ctrl+A all • Ctrl+X none • Ctrl+I invert`), centered;
//! the intuitive toggle/nav/close keys are omitted.
//!
//! A **thin blit**: every layout decision (row text, checkbox glyph, badge, scrolled window) is a
//! pure helper testable directly; the paint itself is `TestBackend`-snapshot-tested. All colors come
//! from [`theme::palette`] — this file never names a `Color`.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use crate::theme;

use super::palette_state::{ColumnRef, PaletteState};

/// Max column rows shown at once (the visible window; a longer list scrolls with the cursor).
pub const MAX_VISIBLE_ROWS: u16 = 10;

/// Render the palette popup into `area`. No-op when the popup is closed (the caller checks
/// [`App::is_palette_open`](crate::app::App::is_palette_open)) or `area` is degenerate.
pub fn render_palette(state: &PaletteState, f: &mut Frame, area: Rect, hovered: Option<usize>) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    // Bottom-border hints, centered. Truncate trailing hints if the box is too narrow (drop whole
    // hints rather than overflowing the border). The `usable` width is the box width minus the two
    // corner glyphs.
    let usable = area.width.saturating_sub(2) as usize;
    let hint_line = Line::from(hint_spans(usable)).centered();

    // The popup overlays the results grid, whose cells may carry `Modifier::DIM` (a stale-error
    // grid) or per-span NULL dimming from `grid_render::style_body_line`. ratatui's text spans OR
    // their style into the underlying cell rather than overwriting, so without an explicit clear
    // those modifiers bleed through into the popup's text and gap cells (a row whose underlying
    // grid happened to have a NULL would render visibly dimmer than its neighbors). Clear the
    // popup's full area FIRST so every cell starts from a clean base before the Block + content
    // paint.
    f.render_widget(Clear, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme::palette::border())
        .style(theme::popup::surface())
        .title(Span::styled(" columns ", theme::palette::title()))
        .title_bottom(hint_line);
    let inner = block.inner(area);
    f.render_widget(block, area);

    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let lines = popup_lines(state, inner.width, inner.height, hovered);
    f.render_widget(Paragraph::new(lines), inner);
}

/// The bottom-border hints for the column-picker popup: ONLY the non-obvious bulk operations.
/// Space/Tab-toggle, arrow-nav, and Enter/Esc-close are intuitive and deliberately omitted — the
/// border legend exists to teach the chords a user wouldn't guess. The popup floors its width to
/// fit this whole line ([`hint_line_width`]) so all three always show.
const HINTS: &[(&str, &str)] = &[("Ctrl+A", "all"), ("Ctrl+X", "none"), ("Ctrl+I", "invert")];

/// The full rendered width (in cells) of the complete [`HINTS`] line — leading space + each
/// `key desc` joined by ` • `. The popup width floors to this (+ borders) so the bulk-op hints
/// (`Ctrl+A`/`Ctrl+X`/`Ctrl+I`) always fit rather than being dropped on a narrow CSV's popup.
pub(crate) fn hint_line_width() -> usize {
    HINTS.iter().enumerate().fold(0, |w, (i, (k, d))| {
        let sep = if i == 0 { 1 } else { 3 }; // " " or " • "
        w + sep + k.chars().count() + 1 + d.chars().count()
    })
}

/// The styled hint spans for the popup's bottom border. Drops trailing low-priority hints whole if
/// they wouldn't fit in `max_width` (the same narrow-width policy the main bottom-border hints use).
pub(crate) fn hint_spans(max_width: usize) -> Vec<Span<'static>> {
    let key_style = theme::help_line::key();
    let desc_style = theme::help_line::description();
    let sep_style = theme::help_line::separator();
    let hints = HINTS;

    let mut out: Vec<Span<'static>> = Vec::with_capacity(hints.len() * 4);
    let mut width = 0usize;
    for (k, d) in hints {
        let lead_first = out.is_empty();
        let sep = if lead_first { " " } else { " \u{2022} " };
        let chunk = sep.chars().count() + k.chars().count() + 1 + d.chars().count();
        if width + chunk > max_width {
            break;
        }
        out.push(Span::styled(sep.to_string(), sep_style));
        out.push(Span::styled(*k, key_style));
        out.push(Span::raw(" "));
        out.push(Span::styled(*d, desc_style));
        width += chunk;
    }
    out
}

/// Build the styled column rows for an inner width/height. Scrolled so the cursor is visible.
fn popup_lines(
    state: &PaletteState,
    width: u16,
    height: u16,
    hovered: Option<usize>,
) -> Vec<Line<'static>> {
    let cols = state.all_columns();
    if cols.is_empty() {
        return vec![Line::from(Span::styled(
            pad_or_truncate("(no columns)", width as usize),
            theme::palette::title(),
        ))];
    }
    let visible = (MAX_VISIBLE_ROWS.min(height)) as usize;
    let (start, end) = crate::scroll_window::visible_window(state.cursor(), cols.len(), visible);

    cols[start..end]
        .iter()
        .enumerate()
        .map(|(offset, column)| {
            let row = start + offset;
            row_line(
                column,
                state.is_checked(row),
                width,
                row == state.cursor(),
                hovered == Some(row),
            )
        })
        .collect()
}

/// One column row, padded to `width`: `<checkbox> <name>` left-aligned, the type badge
/// right-aligned, the gap filled with spaces. The cursor row is reverse-video; a hovered
/// (non-cursor) row carries the faint hover band; otherwise the checkbox carries the
/// checked/normal style and the badge the dimmed hint style.
fn row_line(
    column: &ColumnRef,
    checked: bool,
    width: u16,
    is_cursor: bool,
    hovered: bool,
) -> Line<'static> {
    // Every row reserves a 1-column left gutter (the accent bar on the cursor row, a blank space
    // otherwise) so checkboxes stay column-aligned; content lays out in the remaining `width-1`.
    let content_w = (width as usize).saturating_sub(1);
    let badge = column.ty.badge().to_string();
    let body = row_text(
        column,
        checked,
        content_w.saturating_sub(badge.len() + 1).max(1),
    );
    let used = body.chars().count() + badge.chars().count();
    let gap = content_w.saturating_sub(used);

    if is_cursor {
        let content = pad_or_truncate(&format!("{body}{}{badge}", " ".repeat(gap)), content_w);
        Line::from(vec![
            Span::styled(
                theme::popup::BAR,
                theme::popup::selected_bar(theme::palette::ACCENT),
            ),
            Span::styled(content, theme::palette::selected()),
        ])
    } else {
        // Spans set their own opaque bg (so the grid can't leak through), so a hovered row folds
        // the hover background into every span rather than rely on a line-level style.
        let row_bg = hovered.then(theme::popup::hover_bg);
        let patch = |style: ratatui::style::Style| match row_bg {
            Some(bg) => style.bg(bg),
            None => style,
        };
        let box_style = if checked {
            theme::palette::checked()
        } else {
            theme::palette::item()
        };
        Line::from(vec![
            Span::styled(" ", patch(theme::palette::item())), // gutter aligns with the bar
            Span::styled(checkbox(checked).to_string(), patch(box_style)),
            Span::styled(
                format!(" {body}", body = name_part(column, content_w)),
                patch(theme::palette::item()),
            ),
            Span::styled(" ".repeat(gap), patch(theme::palette::item())),
            Span::styled(badge, patch(theme::palette::type_hint())),
        ])
    }
}

/// The full left-side text of a row (`[x] name` / `[ ] name`), truncated to `max` chars.
pub fn row_text(column: &ColumnRef, checked: bool, max: usize) -> String {
    let full = format!("{} {}", checkbox(checked), column.name);
    truncate(&full, max)
}

fn name_part(column: &ColumnRef, width: usize) -> String {
    let avail = width.saturating_sub(4).max(1);
    truncate(&column.name, avail)
}

/// The checkbox glyph for a row: `[x]` checked, `[ ]` unchecked.
pub fn checkbox(checked: bool) -> &'static str {
    if checked { "[x]" } else { "[ ]" }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    if max == 0 {
        return String::new();
    }
    let keep = max.saturating_sub(1);
    let mut out: String = s.chars().take(keep).collect();
    out.push('\u{2026}');
    out
}

fn pad_or_truncate(s: &str, width: usize) -> String {
    let len = s.chars().count();
    if len > width {
        s.chars().take(width).collect()
    } else {
        format!("{s}{}", " ".repeat(width - len))
    }
}

#[cfg(test)]
#[path = "palette_render_tests.rs"]
mod palette_render_tests;
