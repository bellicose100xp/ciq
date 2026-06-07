//! Palette popup blit — `render_palette(state, frame, area)` over [`PaletteState`] (`dev/PLAN.md`
//! §6.2, `dev/DECISIONS.md` D3).
//!
//! Reuses the autocomplete popup chrome (a bordered list, the cursor row reverse-video, a dimmed
//! right-aligned type badge) retargeted to the palette's column rows. Each visible row is a
//! checkbox (`[x]` checked / `[ ]` unchecked), the column name, and the column's
//! [`ColumnType`](crate::schema::ColumnType) badge right-aligned; the fuzzy needle filters which
//! columns show, the cursor highlights one of them.
//!
//! A **thin blit**: every layout decision (which rows show, each row's text, the checkbox glyph,
//! the badge) is a pure function tested directly ([`row_text`], [`checkbox`]), and the paint itself
//! is `TestBackend`-snapshot-tested (NOT shell-exempt — `TestBackend` is an in-memory cell grid an
//! agent asserts; only true-terminal glyphs / placement / color-polarity / the real `Space`+arrow
//! chords are the §4.7 human surface). All colors come from [`theme::palette`] — this file never
//! names a `Color`.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::theme;

use super::palette_state::{ColumnRef, PaletteState};

/// Max column rows shown at once (the visible window; a longer list scrolls with the cursor).
pub const MAX_VISIBLE_ROWS: u16 = 10;

/// Render the palette popup into `area`. No-op when the palette is not open (the caller checks
/// [`App::is_palette_open`](crate::app::App::is_palette_open)) or `area` is degenerate.
///
/// The popup draws a bordered box titled with the fuzzy needle (so the user sees what they are
/// filtering on), and inside it one line per visible (filtered) column — checkbox + name + badge —
/// the cursor row reverse-video.
pub fn render_palette(state: &PaletteState, f: &mut Frame, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let title = if state.needle().is_empty() {
        " columns ".to_string()
    } else {
        format!(" columns: {} ", state.needle())
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme::palette::border())
        .title(Span::styled(title, theme::palette::hint()));
    let inner = block.inner(area);
    f.render_widget(block, area);

    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let lines = popup_lines(state, inner.width, inner.height);
    f.render_widget(Paragraph::new(lines), inner);
}

/// Build the styled column rows for an inner width/height: a window of [`MAX_VISIBLE_ROWS`] (capped
/// by `height`) filtered columns scrolled so the cursor row is always visible, each `checkbox name
/// … badge` and styled (cursor row reverse-video, badge dimmed, checked checkbox accented).
fn popup_lines(state: &PaletteState, width: u16, height: u16) -> Vec<Line<'static>> {
    let filtered = state.filtered_indices();
    if filtered.is_empty() {
        return vec![Line::from(Span::styled(
            pad_or_truncate("(no match)", width as usize),
            theme::palette::hint(),
        ))];
    }
    let visible = (MAX_VISIBLE_ROWS.min(height)) as usize;
    let (start, end) = visible_window(state.cursor(), filtered.len(), visible);

    filtered[start..end]
        .iter()
        .enumerate()
        .map(|(offset, &col_idx)| {
            let row = start + offset;
            let column = &state.all_columns()[col_idx];
            row_line(
                column,
                state.is_checked(col_idx),
                width,
                row == state.cursor(),
            )
        })
        .collect()
}

/// The `[start, end)` slice of filtered-row indices to show: a window of `visible` rows that keeps
/// `cursor` in view (scrolls only when the cursor would fall outside the top window). Mirrors the
/// autocomplete popup's `visible_window`.
fn visible_window(cursor: usize, len: usize, visible: usize) -> (usize, usize) {
    if len <= visible {
        return (0, len);
    }
    let start = if cursor < visible {
        0
    } else {
        (cursor + 1).saturating_sub(visible).min(len - visible)
    };
    (start, start + visible)
}

/// One column row, padded to `width`: `<checkbox> <name>` left-aligned, the type badge
/// right-aligned, the gap filled with spaces. The cursor row is reverse-video (the whole line);
/// otherwise the checkbox carries the checked/normal style and the badge the dimmed hint style.
fn row_line(column: &ColumnRef, checked: bool, width: u16, is_cursor: bool) -> Line<'static> {
    let width = width as usize;
    let badge = column.ty.badge().to_string();
    let body = row_text(
        column,
        checked,
        width.saturating_sub(badge.len() + 1).max(1),
    );
    let used = body.chars().count() + badge.chars().count();
    let gap = width.saturating_sub(used);

    if is_cursor {
        let content = format!("{body}{}{badge}", " ".repeat(gap));
        let content = pad_or_truncate(&content, width);
        Line::from(Span::styled(content, theme::palette::selected()))
    } else {
        let box_style = if checked {
            theme::palette::checked()
        } else {
            theme::palette::item()
        };
        Line::from(vec![
            Span::styled(checkbox(checked).to_string(), box_style),
            Span::styled(
                format!(" {body}", body = name_part(column, width)),
                theme::palette::item(),
            ),
            Span::styled(" ".repeat(gap), theme::palette::item()),
            Span::styled(badge, theme::palette::type_hint()),
        ])
    }
}

/// The full left-side text of a row (`[x] name` / `[ ] name`), truncated to `max` chars — used for
/// the reverse-video cursor row (one span) and asserted directly. The unselected row paints the
/// checkbox and name as separate spans (so the checked checkbox can be accented), but the text is
/// identical.
pub fn row_text(column: &ColumnRef, checked: bool, max: usize) -> String {
    let full = format!("{} {}", checkbox(checked), column.name);
    truncate(&full, max)
}

/// Just the name portion (after the checkbox), truncated to fit the remaining width — the second
/// span of an unselected row.
fn name_part(column: &ColumnRef, width: usize) -> String {
    // 4 chars for "[x] " prefix; leave room for it plus the badge handled by the caller's gap math.
    let avail = width.saturating_sub(4).max(1);
    truncate(&column.name, avail)
}

/// The checkbox glyph for a row: `[x]` checked, `[ ]` unchecked. ASCII only (no emoji), per the
/// theme conventions.
pub fn checkbox(checked: bool) -> &'static str {
    if checked { "[x]" } else { "[ ]" }
}

/// Truncate `s` to at most `max` characters, appending `…` when cut (matching the grid / popup
/// ellipsis rule). Returns `s` unchanged when it already fits.
fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    if max == 0 {
        return String::new();
    }
    let keep = max.saturating_sub(1);
    let mut out: String = s.chars().take(keep).collect();
    out.push('…');
    out
}

/// Pad `s` with trailing spaces to exactly `width` chars, or truncate it to `width`.
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
