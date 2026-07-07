//! Tests for `history_render` — the pure line/title helpers and the popup blit (`insta` +
//! `ratatui::TestBackend`, logical cells only).
//!
//! The snapshot proves the *logical* cell grid (which prior queries land where, the title, the
//! "(no matches)" line). True-terminal glyphs, popup placement against a real screen, the cursor
//! reverse-video color, and the real chords are the §4.7 human surface, NOT asserted here.

use super::*;
use crate::history::history_state::HistoryState;

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;

fn state() -> HistoryState {
    let mut h = HistoryState::with_entries(vec![
        "SELECT id FROM t".into(),
        "SELECT name FROM t WHERE id > 5".into(),
        "SELECT count(*) FROM t".into(),
    ]);
    h.open(None);
    h
}

fn render(state: &HistoryState, w: u16, h: u16, area: Rect) -> String {
    let mut t = Terminal::new(TestBackend::new(w, h)).expect("TestBackend");
    t.draw(|f| render_history(state, f, area, None))
        .expect("draw");
    t.backend().to_string()
}

// --- pure title helper ---

#[test]
fn title_shows_count_when_no_needle() {
    let h = state();
    assert_eq!(title(&h), " history (3) ");
}

#[test]
fn title_shows_needle_and_filtered_count() {
    let mut h = state();
    h.set_needle("count");
    assert_eq!(title(&h), " history: count (1/3) ");
}

// --- pure line builder ---

#[test]
fn popup_lines_pads_to_width() {
    let h = state();
    let lines = popup_lines(&h, 30, 10, None);
    assert_eq!(lines.len(), 3);
    for line in &lines {
        assert_eq!(line.width(), 30);
    }
}

#[test]
fn popup_lines_no_matches_hint() {
    let mut h = state();
    h.set_needle("zzz");
    let lines = popup_lines(&h, 30, 10, None);
    assert_eq!(lines.len(), 1);
    assert!(
        lines[0]
            .spans
            .iter()
            .any(|s| s.content.contains("(no matches)")),
        "expected the no-matches hint"
    );
}

#[test]
fn popup_lines_capped_by_height() {
    let entries: Vec<String> = (0..30).map(|i| format!("SELECT {i}")).collect();
    let mut h = HistoryState::with_entries(entries);
    h.open(None);
    // Height caps the window even below MAX_VISIBLE_HISTORY.
    let lines = popup_lines(&h, 30, 5, None);
    assert_eq!(lines.len(), 5);
}

// --- render ---

#[test]
fn popup_shows_entries_and_title() {
    let screen = render(&state(), 60, 10, Rect::new(0, 0, 50, 6));
    assert!(screen.contains("history"), "screen:\n{screen}");
    assert!(screen.contains("SELECT id FROM t"), "screen:\n{screen}");
    assert!(screen.contains("count(*)"), "screen:\n{screen}");
}

#[test]
fn popup_with_needle_filters() {
    let mut h = state();
    h.set_needle("count");
    let screen = render(&h, 60, 8, Rect::new(0, 0, 50, 5));
    assert!(screen.contains("count(*)"), "screen:\n{screen}");
    // The filtered-out entries are gone.
    assert!(!screen.contains("name"), "screen:\n{screen}");
}

#[test]
fn empty_filter_shows_no_matches() {
    let mut h = state();
    h.set_needle("zzz");
    let screen = render(&h, 60, 8, Rect::new(0, 0, 50, 5));
    assert!(screen.contains("(no matches)"), "screen:\n{screen}");
}

#[test]
fn snapshot_history_80x24() {
    let mut h = state();
    h.select_next(); // cursor on the second (older) entry
    let screen = render(&h, 80, 24, Rect::new(0, 1, 50, 6));
    insta::assert_snapshot!(screen);
}

#[test]
fn render_no_op_on_zero_area() {
    let h = state();
    let screen = render(&h, 10, 3, Rect::new(0, 0, 0, 0));
    assert!(
        screen.chars().all(|c| c == ' ' || c == '\n' || c == '"'),
        "zero area must paint nothing, got:\n{screen}"
    );
}

#[test]
fn render_does_not_panic_on_degenerate_area() {
    let h = state();
    for (w, h2) in [(1u16, 1u16), (2, 2), (3, 1), (1, 3)] {
        let _ = render(&h, w.max(1), h2.max(1), Rect::new(0, 0, w, h2));
    }
}

#[test]
fn hovered_row_carries_the_hover_band() {
    let h = state(); // cursor on display index 0
    let area = Rect::new(0, 0, 40, 6);
    let mut t = Terminal::new(TestBackend::new(50, 10)).expect("TestBackend");
    t.draw(|f| render_history(&h, f, area, Some(1)))
        .expect("draw");
    let buf = t.backend().buffer();
    assert_eq!(
        buf[(1, 2)].style().bg,
        crate::theme::history::hovered_bg().bg,
        "hovered (non-cursor) row carries the hover band"
    );
    assert_ne!(
        buf[(1, 3)].style().bg,
        crate::theme::history::hovered_bg().bg,
        "other rows keep the plain background"
    );
}
