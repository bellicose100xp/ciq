//! Save popup blit — `render_save(state, frame, area)` over [`SaveState`].
//!
//! Reuses the shared modern popup chrome ([`theme::popup`]: opaque surface, accent border) with
//! the save-specific green accent ([`theme::save`]): a filename prompt line with a visible block
//! cursor, a resolved-path preview line below it (with an overwrite warning when the destination
//! already exists), and an inline error line on a failed write. Key hints ride the bottom border,
//! like the search bar.
//!
//! A **thin blit**: the line builders ([`filename_line`], [`status_line`]) are pure and tested
//! directly; the paint is `TestBackend`-snapshot-tested (NOT shell-exempt). All colors come from
//! [`theme::save`] / [`theme::popup`] — this file never names a `Color`.

use ratatui::Frame;
use ratatui::layout::{Alignment, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use crate::theme;

use super::save_state::SaveState;

/// Content rows the popup needs: the filename prompt line + the preview/error status line.
pub const SAVE_POPUP_ROWS: u16 = 2;

/// Render the save popup into `area`. No-op on a degenerate area (the caller checks
/// [`SaveState::is_open`]).
pub fn render_save(state: &SaveState, f: &mut Frame, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    f.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme::save::border())
        .style(theme::popup::surface())
        .title(Span::styled(title(), theme::save::title()))
        .title_bottom(
            Line::from(Span::styled(
                " Enter save \u{2022} Esc cancel ",
                theme::save::hint(),
            ))
            .alignment(Alignment::Center),
        );
    let inner = block.inner(area);
    f.render_widget(block, area);

    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let mut lines = vec![filename_line(state)];
    if let Some(status) = status_line(state) {
        lines.push(status);
    }
    f.render_widget(Paragraph::new(lines), inner);
}

/// The popup title.
pub fn title() -> &'static str {
    " save result as CSV "
}

/// The first inner line: the filename being typed, prefixed with `> ` like the query bar, with a
/// visible block-cursor cell after it (the popup captures the keyboard while open).
pub fn filename_line(state: &SaveState) -> Line<'static> {
    Line::from(vec![
        Span::styled("> ", theme::save::prompt()),
        Span::styled(state.filename().to_string(), theme::save::input()),
        Span::styled(" ", theme::save::cursor()),
    ])
}

/// The status line below the filename: the inline error when the last Enter failed, else the
/// resolved-path preview (`→ /path/to/out.csv`, with an overwrite warning when the destination
/// exists), else `None` while the name is empty/unresolvable (the border hints suffice).
pub fn status_line(state: &SaveState) -> Option<Line<'static>> {
    if let Some(error) = state.error() {
        return Some(Line::from(Span::styled(
            error.to_string(),
            theme::save::error(),
        )));
    }
    let preview = state.preview()?;
    let mut spans = vec![Span::styled(
        format!("\u{2192} {}", preview.path.display()),
        theme::save::preview(),
    )];
    if preview.exists {
        spans.push(Span::styled(
            "  (overwrites existing file)".to_string(),
            theme::save::overwrite_warning(),
        ));
    }
    Some(Line::from(spans))
}

#[cfg(test)]
#[path = "save_render_tests.rs"]
mod save_render_tests;
