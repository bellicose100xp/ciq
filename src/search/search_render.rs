//! Thin blit for the search bar (`Ctrl+F`) — a one-row bordered box between the results pane and
//! the query box.
//!
//! Pure function of plain data into a `Frame` (TestBackend-snapshot-tested, NOT shell-exempt).
//! All colors come from [`theme::search`]; this file never names a `Color`. jiq's bar shape
//! (bordered, active-vs-confirmed border polarity, a match badge on the border) with ciq's badge
//! semantics: `shown/total` filtered ROWS, not match-occurrence count — the filter is the verb.

use ratatui::Frame;
use ratatui::layout::{Alignment, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph};

use crate::theme;

/// Total height of the search bar box (one text row + top/bottom border).
pub const SEARCH_BAR_HEIGHT: u16 = 3;

/// Render the search bar into `area`: the needle text (with a trailing block cursor while
/// editing), the `shown/total` row badge on the top-right border, and the key hints on the
/// bottom border while editing. `confirmed` flips the chrome to the quiet polarity (the filter
/// is frozen; keys navigate the grid again).
pub fn render_search_bar(
    f: &mut Frame,
    area: Rect,
    needle: &str,
    confirmed: bool,
    shown: usize,
    total: usize,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let border = if confirmed {
        theme::search::border_inactive()
    } else {
        theme::search::border_active()
    };
    let mut block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(Span::styled(" Search ", border))
        .border_style(border);

    // The row badge: how many rows survive the filter. Red-tinted when the needle matched
    // nothing (the user typed themselves into an empty grid), quiet otherwise.
    let badge_style = if shown == 0 && !needle.is_empty() {
        theme::search::badge_no_matches()
    } else {
        theme::search::badge()
    };
    block = block.title_top(
        Line::from(Span::styled(format!(" {shown}/{total} rows "), badge_style))
            .alignment(Alignment::Right),
    );
    if !confirmed {
        block = block.title_bottom(
            Line::from(Span::styled(
                " Enter confirm  Esc close ",
                theme::search::hint(),
            ))
            .alignment(Alignment::Center),
        );
    }

    let inner = block.inner(area);
    f.render_widget(block, area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let text_style = if confirmed {
        theme::search::text_inactive()
    } else {
        theme::search::text_active()
    };
    let mut spans = vec![Span::styled(needle.to_string(), text_style)];
    if !confirmed {
        // A visible block-cursor cell after the needle while editing (reverse-video, like the
        // query bar's Insert cursor) so "typing goes here" reads at a glance.
        spans.push(Span::styled(" ", theme::app::cursor()));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), inner);
}

#[cfg(test)]
#[path = "search_render_tests.rs"]
mod search_render_tests;
