//! History popup blit — `render_history(state, frame, area)` over
//! [`HistoryState`](super::history_state::HistoryState) (`dev/PLAN.md` §7.6).
//!
//! Reuses the palette/autocomplete popup chrome (a bordered list, the cursor row reverse-video, a
//! search needle in the title) retargeted to the history's query rows: one line per visible
//! (filtered) prior query, newest first, the cursor row highlighted. The fuzzy needle filters which
//! rows show.
//!
//! A **thin blit**: every layout decision (which rows show, each row's text, the title) is a pure
//! function tested directly ([`popup_lines`], [`title`]), and the paint itself is `TestBackend`-
//! snapshot-tested (NOT shell-exempt — `TestBackend` is an in-memory cell grid an agent asserts;
//! only true-terminal glyphs / placement / color-polarity / the real chords are the §4.7 human
//! surface). All colors come from [`theme::history`] — this file never names a `Color`.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use crate::theme;

use super::history_state::{HistoryState, MAX_VISIBLE_HISTORY};

/// Render the history popup into `area`. No-op on a degenerate area (the caller checks
/// [`HistoryState::is_visible`]).
///
/// The popup draws a bordered box titled with the filtered/total count + the search needle, and
/// inside it one line per visible (filtered) prior query, the cursor row reverse-video.
pub fn render_history(state: &HistoryState, f: &mut Frame, area: Rect, hovered: Option<usize>) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    f.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme::history::border())
        .style(theme::popup::surface())
        .title(Span::styled(title(state), theme::history::hint()));
    let inner = block.inner(area);
    f.render_widget(block, area);

    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let lines = popup_lines(state, inner.width, inner.height, hovered);
    f.render_widget(Paragraph::new(lines), inner);
}

/// The popup title: the search needle (when non-empty) plus the filtered/total count, so the user
/// sees what they are filtering on and how many match.
pub fn title(state: &HistoryState) -> String {
    if state.needle().is_empty() {
        format!(" history ({}) ", state.total_count())
    } else {
        format!(
            " history: {} ({}/{}) ",
            state.needle(),
            state.filtered_count(),
            state.total_count()
        )
    }
}

/// Build the styled rows for an inner width/height: the [`MAX_VISIBLE_HISTORY`]-capped (and
/// height-capped) window of filtered entries the state exposes via
/// [`visible_entries`](HistoryState::visible_entries), each padded to `width`, the cursor row
/// reverse-video. An empty filtered list shows a dimmed "(no matches)" line.
pub fn popup_lines(
    state: &HistoryState,
    width: u16,
    height: u16,
    hovered: Option<usize>,
) -> Vec<Line<'static>> {
    if state.filtered_count() == 0 {
        return vec![Line::from(Span::styled(
            pad_or_truncate("(no matches)", width as usize),
            theme::history::hint(),
        ))];
    }
    let visible = (MAX_VISIBLE_HISTORY.min(height as usize)).max(1);
    state
        .visible_entries()
        .into_iter()
        .take(visible)
        .map(|(display_idx, entry)| {
            row_line(
                entry,
                width,
                display_idx == state.selected_index(),
                hovered == Some(display_idx),
            )
        })
        .collect()
}

/// One history row padded to `width`: the query text (truncated to fit), the cursor row
/// reverse-video, a hovered (non-cursor) row on the faint hover band, others normal.
fn row_line(entry: &str, width: u16, is_cursor: bool, hovered: bool) -> Line<'static> {
    if is_cursor {
        // Left accent bar (▌) + the row on the elevated selection band (bar takes column 0).
        let text = pad_or_truncate(entry, (width as usize).saturating_sub(1));
        return Line::from(vec![
            Span::styled(
                theme::popup::BAR,
                theme::popup::selected_bar(theme::history::ACCENT),
            ),
            Span::styled(text, theme::history::selected()),
        ]);
    }
    // A blank left gutter aligns the text with the cursor row's accent bar; content in `width-1`.
    let text = pad_or_truncate(entry, (width as usize).saturating_sub(1));
    let style = if hovered {
        theme::history::hovered_bg()
    } else {
        theme::history::item()
    };
    Line::from(vec![Span::styled(" ", style), Span::styled(text, style)])
}

/// Pad `s` with trailing spaces to exactly `width` chars, or truncate it to `width` (with a
/// trailing ellipsis when cut — the grid/popup ellipsis rule).
fn pad_or_truncate(s: &str, width: usize) -> String {
    let len = s.chars().count();
    if len > width {
        if width == 0 {
            return String::new();
        }
        let keep = width.saturating_sub(1);
        let mut out: String = s.chars().take(keep).collect();
        out.push('…');
        out
    } else {
        format!("{s}{}", " ".repeat(width - len))
    }
}

#[cfg(test)]
#[path = "history_render_tests.rs"]
mod history_render_tests;
