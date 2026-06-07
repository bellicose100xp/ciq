//! Tests for `ai_render` — the pure title/prompt/status helpers and the popup blit (`insta` +
//! `ratatui::TestBackend`, logical cells only).
//!
//! The snapshot proves the *logical* cell grid (the prompt line, the status line). True-terminal
//! glyphs, popup placement against a real screen, the magenta border color, and the real `Ctrl+G`
//! chord are the §4.7 human surface, NOT asserted here.

use super::*;
use crate::ai::ai_state::AiState;

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;

fn render(state: &AiState, w: u16, h: u16, area: Rect) -> String {
    let mut t = Terminal::new(TestBackend::new(w, h)).expect("TestBackend");
    t.draw(|f| render_ai(state, f, area)).expect("draw");
    t.backend().to_string()
}

// --- pure helpers ---

#[test]
fn title_is_the_chord_hint() {
    assert!(title().contains("ask AI"));
    assert!(title().contains("Enter"));
    assert!(title().contains("Esc"));
}

#[test]
fn prompt_line_shows_the_input() {
    let mut s = AiState::new();
    s.open();
    for c in "rows in EU".chars() {
        s.push_char(c);
    }
    let line = prompt_line(&s);
    let text: String = line.spans.iter().map(|sp| sp.content.as_ref()).collect();
    assert!(text.contains("rows in EU"), "got: {text}");
}

#[test]
fn status_line_none_while_editing() {
    let mut s = AiState::new();
    s.open();
    assert!(status_line(&s).is_none());
}

#[test]
fn status_line_pending_shows_generating() {
    let mut s = AiState::new();
    s.open();
    s.push_char('x');
    s.submit();
    let line = status_line(&s).expect("pending status");
    let text: String = line.spans.iter().map(|sp| sp.content.as_ref()).collect();
    assert!(text.contains("generating"), "got: {text}");
}

#[test]
fn status_line_success_shows_sql() {
    let mut s = AiState::new();
    s.open();
    s.push_char('x');
    s.submit();
    s.set_success("SELECT * FROM t");
    let line = status_line(&s).expect("success status");
    let text: String = line.spans.iter().map(|sp| sp.content.as_ref()).collect();
    assert_eq!(text, "SELECT * FROM t");
}

#[test]
fn status_line_error_shows_message() {
    let mut s = AiState::new();
    s.open();
    s.push_char('x');
    s.submit();
    s.set_error("network down");
    let line = status_line(&s).expect("error status");
    let text: String = line.spans.iter().map(|sp| sp.content.as_ref()).collect();
    assert!(text.contains("network down"), "got: {text}");
}

// --- blit ---

#[test]
fn popup_shows_prompt_and_title() {
    let mut s = AiState::new();
    s.open();
    for c in "rows where status active".chars() {
        s.push_char(c);
    }
    let screen = render(&s, 60, 8, Rect::new(0, 0, 50, 4));
    assert!(screen.contains("ask AI"), "screen:\n{screen}");
    assert!(
        screen.contains("rows where status active"),
        "screen:\n{screen}"
    );
}

#[test]
fn snapshot_ai_pending_80x24() {
    let mut s = AiState::new();
    s.open();
    for c in "top 10 by amount".chars() {
        s.push_char(c);
    }
    s.submit(); // Pending -> shows "generating…"
    let screen = render(&s, 80, 24, Rect::new(0, 1, 48, 4));
    insta::assert_snapshot!(screen);
}

#[test]
fn snapshot_ai_success_80x24() {
    let mut s = AiState::new();
    s.open();
    for c in "all rows".chars() {
        s.push_char(c);
    }
    s.submit();
    s.set_success("SELECT * FROM t");
    let screen = render(&s, 80, 24, Rect::new(0, 1, 48, 4));
    insta::assert_snapshot!(screen);
}

#[test]
fn render_no_op_on_zero_area() {
    let mut s = AiState::new();
    s.open();
    let screen = render(&s, 10, 3, Rect::new(0, 0, 0, 0));
    assert!(
        screen.chars().all(|c| c == ' ' || c == '\n' || c == '"'),
        "zero area paints nothing, got:\n{screen}"
    );
}

#[test]
fn render_does_not_panic_on_degenerate_area() {
    let mut s = AiState::new();
    s.open();
    for (w, h) in [(1u16, 1u16), (2, 2), (3, 1), (1, 3)] {
        let _ = render(&s, w.max(1), h.max(1), Rect::new(0, 0, w, h));
    }
}
