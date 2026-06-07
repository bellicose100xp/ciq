//! Tests for `palette_render` — the pure row helpers (`row_text`/`checkbox`) and the popup blit
//! (`insta` + `ratatui::TestBackend`, logical cells only).
//!
//! The snapshot proves the *logical* cell grid (which checkboxes / column names / right-aligned
//! type badges land where). True-terminal glyphs, popup placement against a real screen, the
//! checked-checkbox accent color, and the real `Space`/arrow chords are the §4.7 human surface, NOT
//! asserted here.

use super::*;
use crate::palette::palette_state::{ColumnRef, PaletteState};
use crate::schema::ColumnType;

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;

fn cols() -> Vec<ColumnRef> {
    vec![
        ColumnRef::new("id", ColumnType::Int),
        ColumnRef::new("status", ColumnType::Text),
        ColumnRef::new("amount", ColumnType::Float),
        ColumnRef::new("created_at", ColumnType::Date),
    ]
}

fn render(state: &PaletteState, w: u16, h: u16, area: Rect) -> String {
    let mut t = Terminal::new(TestBackend::new(w, h)).expect("TestBackend");
    t.draw(|f| render_palette(state, f, area)).expect("draw");
    t.backend().to_string()
}

// --- pure row helpers ---

#[test]
fn checkbox_glyphs() {
    assert_eq!(checkbox(true), "[x]");
    assert_eq!(checkbox(false), "[ ]");
}

#[test]
fn row_text_shows_checkbox_and_name() {
    let c = ColumnRef::new("status", ColumnType::Text);
    assert_eq!(row_text(&c, false, 20), "[ ] status");
    assert_eq!(row_text(&c, true, 20), "[x] status");
}

#[test]
fn row_text_truncates_long_names_with_ellipsis() {
    let c = ColumnRef::new("a_very_long_column_name", ColumnType::Text);
    let txt = row_text(&c, false, 10);
    assert_eq!(txt.chars().count(), 10);
    assert!(txt.ends_with('…'), "got: {txt}");
}

// --- render ---

#[test]
fn popup_shows_columns_checkboxes_and_badges() {
    let mut state = PaletteState::new(cols());
    state.toggle(0); // id checked
    state.toggle(2); // amount checked
    let screen = render(&state, 40, 12, Rect::new(0, 0, 30, 8));
    // Column names present.
    assert!(screen.contains("id"), "screen:\n{screen}");
    assert!(screen.contains("status"), "screen:\n{screen}");
    assert!(screen.contains("created_at"), "screen:\n{screen}");
    // Checkboxes present (both checked and unchecked rows).
    assert!(screen.contains("[x]"), "screen:\n{screen}");
    assert!(screen.contains("[ ]"), "screen:\n{screen}");
    // Type badges (right-aligned) present.
    assert!(screen.contains("int"), "screen:\n{screen}");
    assert!(screen.contains("txt"), "screen:\n{screen}");
    assert!(screen.contains("date"), "screen:\n{screen}");
}

#[test]
fn popup_title_reflects_the_needle() {
    let mut state = PaletteState::new(cols());
    state.set_needle("am");
    let screen = render(&state, 40, 8, Rect::new(0, 0, 30, 6));
    // The title carries the active fuzzy filter so the user sees what they're matching on.
    assert!(screen.contains("am"), "screen:\n{screen}");
    // Filtered to the columns containing "am" as a subsequence (status -> no; amount -> yes; name?
    // there's no name column). `created_at`? c-r-e-a-t-e-d-_-[a]-t -> a then m? no m. So just amount.
    assert!(screen.contains("amount"), "screen:\n{screen}");
}

#[test]
fn empty_filter_shows_no_match_hint() {
    let mut state = PaletteState::new(cols());
    state.set_needle("zzz");
    let screen = render(&state, 40, 8, Rect::new(0, 0, 30, 6));
    assert!(screen.contains("(no match)"), "screen:\n{screen}");
}

#[test]
fn snapshot_palette_80x24() {
    // The headless 80x24 snapshot the P4.5 exit criterion calls for: a populated palette with a
    // mixed selection, rendered at the canonical terminal size.
    let mut state = PaletteState::new(cols());
    state.toggle(1); // status checked
    state.toggle(3); // created_at checked
    state.cursor_down(); // cursor on `status`
    let screen = render(&state, 80, 24, Rect::new(0, 1, 30, 8));
    insta::assert_snapshot!(screen);
}

#[test]
fn render_does_not_panic_on_degenerate_area() {
    let state = PaletteState::new(cols());
    for (w, h) in [(1u16, 1u16), (2, 2), (3, 1), (1, 3)] {
        let _ = render(&state, w.max(1), h.max(1), Rect::new(0, 0, w, h));
    }
}

#[test]
fn render_no_op_on_zero_area() {
    let state = PaletteState::new(cols());
    // Zero width/height: paints nothing, no panic.
    let screen = render(&state, 10, 3, Rect::new(0, 0, 0, 0));
    assert!(
        screen.chars().all(|c| c == ' ' || c == '\n' || c == '"'),
        "zero area must paint nothing, got:\n{screen}"
    );
}
