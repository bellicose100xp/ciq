//! TestBackend blit tests for the search bar: active vs confirmed chrome, the needle text + the
//! editing cursor cell, the shown/total badge (incl. the zero-match polarity), and the zero-area
//! guards.

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;

use super::render_search_bar;

fn draw(width: u16, needle: &str, confirmed: bool, shown: usize, total: usize) -> Vec<String> {
    let backend = TestBackend::new(width, 3);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|f| {
            render_search_bar(
                f,
                Rect::new(0, 0, width, 3),
                needle,
                confirmed,
                shown,
                total,
            );
        })
        .unwrap();
    let buffer = terminal.backend().buffer().clone();
    (0..3)
        .map(|y| {
            (0..width)
                .map(|x| buffer[(x, y)].symbol().to_string())
                .collect::<String>()
        })
        .collect()
}

#[test]
fn editing_bar_shows_title_needle_badge_and_hints() {
    let lines = draw(50, "eu", false, 3, 10);
    assert!(lines[0].contains("Search"), "title on top border");
    assert!(lines[0].contains("3/10 rows"), "row badge on top border");
    assert!(lines[1].contains("eu"), "needle text in the body");
    assert!(
        lines[2].contains("Enter confirm") && lines[2].contains("Esc close"),
        "hints on bottom border while editing: {}",
        lines[2]
    );
}

#[test]
fn confirmed_bar_drops_hints_and_cursor() {
    let lines = draw(50, "eu", true, 3, 10);
    assert!(lines[1].contains("eu"));
    assert!(
        !lines[2].contains("Enter"),
        "no hints once confirmed: {}",
        lines[2]
    );
}

#[test]
fn editing_cursor_cell_is_reversed() {
    let backend = TestBackend::new(30, 3);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|f| render_search_bar(f, Rect::new(0, 0, 30, 3), "ab", false, 1, 1))
        .unwrap();
    let buffer = terminal.backend().buffer();
    // Border col 0, needle at cols 1..3, cursor cell at col 3 of the text row.
    let cursor_cell = &buffer[(3, 1)];
    assert!(
        cursor_cell
            .style()
            .add_modifier
            .contains(ratatui::style::Modifier::REVERSED),
        "editing bar paints a reverse-video cursor cell"
    );
}

#[test]
fn zero_match_badge_renders_zero_over_total() {
    let lines = draw(50, "zzz", false, 0, 10);
    assert!(lines[0].contains("0/10 rows"), "{}", lines[0]);
}

#[test]
fn zero_area_is_a_noop() {
    let backend = TestBackend::new(10, 3);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|f| {
            render_search_bar(f, Rect::new(0, 0, 0, 0), "x", false, 0, 0);
        })
        .unwrap();
}
