//! AI popup blit — `render_ai(state, frame, area)` over [`AiState`] (`dev/PLAN.md` §7 P5.1).
//!
//! Reuses the autocomplete/palette/history popup chrome (a bordered box, a dimmed title) retargeted
//! to the AI flow: a prompt line where the user types the natural-language request, and a status
//! line that reflects the [`AiPhase`] (typing / generating… / the generated SQL / an error).
//!
//! A **thin blit**: every layout decision (the title, the prompt line, the status line) is a pure
//! helper tested directly ([`title`], [`status_line`]), and the paint itself is `TestBackend`-
//! snapshot-tested (NOT shell-exempt — `TestBackend` is an in-memory cell grid an agent asserts;
//! only true-terminal glyphs / placement / color-polarity / the real chord are the §4.7 human
//! surface). All colors come from [`theme::ai`] — this file never names a `Color`.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::theme;

use super::ai_state::{AiPhase, AiState};

/// Render the AI popup into `area`. No-op on a degenerate area (the caller checks
/// [`AiState::is_open`]).
///
/// The box is titled (the chord hint), with the natural-language prompt on the first inner row and
/// a phase-dependent status line below it (the generating/SQL/error feedback).
pub fn render_ai(state: &AiState, f: &mut Frame, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme::ai::border())
        .title(Span::styled(title(), theme::ai::hint()));
    let inner = block.inner(area);
    f.render_widget(block, area);

    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let mut lines = vec![prompt_line(state)];
    if let Some(status) = status_line(state) {
        lines.push(status);
    }
    f.render_widget(Paragraph::new(lines), inner);
}

/// The popup title — the chord hint legend.
pub fn title() -> &'static str {
    " ask AI (Enter to generate, Esc to close) "
}

/// The first inner line: the prompt the user is typing, prefixed with `> ` like the query bar.
pub fn prompt_line(state: &AiState) -> Line<'static> {
    Line::from(vec![
        Span::styled("> ", theme::ai::hint()),
        Span::styled(state.input().to_string(), theme::ai::input()),
    ])
}

/// The status line below the prompt, reflecting the request phase: `None` while plainly editing
/// (the prompt line is enough), a dimmed "generating…" while pending, the generated SQL on
/// success, or the error message on failure.
pub fn status_line(state: &AiState) -> Option<Line<'static>> {
    match state.phase() {
        AiPhase::Editing => None,
        AiPhase::Pending => Some(Line::from(Span::styled(
            "generating…".to_string(),
            theme::ai::pending(),
        ))),
        AiPhase::Success(sql) => Some(Line::from(Span::styled(sql.clone(), theme::ai::success()))),
        AiPhase::Error(msg) => Some(Line::from(Span::styled(
            format!("error: {msg}"),
            theme::ai::error(),
        ))),
    }
}

#[cfg(test)]
#[path = "ai_render_tests.rs"]
mod ai_render_tests;
