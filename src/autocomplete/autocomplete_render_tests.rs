//! Tests for `autocomplete_render` — the type-hint label (pure) and the popup blit
//! (`insta` + `ratatui::TestBackend`, logical cells only — §5.6).
//!
//! The snapshot proves the *logical* cell grid (which glyphs / which candidates / the right-aligned
//! hint column land where). True-terminal glyphs, popup placement against a real screen, and
//! type-hint color polarity are the §4.7 human surface, NOT asserted here.

use super::*;
use crate::autocomplete::autocomplete_state::{AutocompleteState, Suggestion, SuggestionType};
use crate::schema::ColumnType;

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;

fn render(state: &AutocompleteState, w: u16, h: u16, area: Rect) -> String {
    render_with(state, w, h, area, false)
}

fn render_with(
    state: &AutocompleteState,
    w: u16,
    h: u16,
    area: Rect,
    show_columns_hint: bool,
) -> String {
    let mut t = Terminal::new(TestBackend::new(w, h)).expect("TestBackend");
    t.draw(|f| render_popup(state, f, area, show_columns_hint))
        .expect("draw");
    t.backend().to_string()
}

// --- type_hint_label (pure) ---

#[test]
fn typed_field_label_is_column_badge() {
    let s = Suggestion::new_with_type("created_at", SuggestionType::Field, Some(ColumnType::Date));
    assert_eq!(type_hint_label(&s), "date");
    let n = Suggestion::new_with_type("id", SuggestionType::Field, Some(ColumnType::Int));
    assert_eq!(type_hint_label(&n), "int");
}

#[test]
fn typed_value_label_is_column_badge() {
    let s = Suggestion::new_with_type("active", SuggestionType::Value, Some(ColumnType::Text));
    assert_eq!(type_hint_label(&s), "txt");
}

#[test]
fn kind_labels_for_non_typed_suggestions() {
    assert_eq!(
        type_hint_label(&Suggestion::new("SELECT", SuggestionType::Keyword)),
        "kw"
    );
    assert_eq!(
        type_hint_label(&Suggestion::new("=", SuggestionType::Operator)),
        "op"
    );
    assert_eq!(
        type_hint_label(&Suggestion::new("lower", SuggestionType::Function)),
        "fn"
    );
    assert_eq!(
        type_hint_label(&Suggestion::new("COUNT", SuggestionType::Aggregate)),
        "agg"
    );
    // Field/Value without a type fall back to a generic tag.
    assert_eq!(
        type_hint_label(&Suggestion::new("t", SuggestionType::Field)),
        "fld"
    );
    assert_eq!(
        type_hint_label(&Suggestion::new("x", SuggestionType::Value)),
        "val"
    );
}

// --- render_popup ---

#[test]
fn closed_popup_renders_nothing() {
    let state = AutocompleteState::new();
    let screen = render(&state, 30, 6, Rect::new(0, 0, 24, 5));
    // A closed popup paints no border and no candidate glyphs — the buffer cells stay blank
    // (`TestBackend::to_string` wraps each row in quotes and joins with newlines, so allow those).
    assert!(
        screen.chars().all(|c| c == ' ' || c == '\n' || c == '"'),
        "closed popup must paint nothing, got:\n{screen}"
    );
}

#[test]
fn populated_popup_shows_candidates_and_typed_hints() {
    let mut state = AutocompleteState::new();
    state.open_with(vec![
        Suggestion::new_with_type("id", SuggestionType::Field, Some(ColumnType::Int)),
        Suggestion::new_with_type("status", SuggestionType::Field, Some(ColumnType::Text)),
        Suggestion::new_with_type("created_at", SuggestionType::Field, Some(ColumnType::Date)),
        Suggestion::new("COUNT", SuggestionType::Aggregate),
        Suggestion::new("WHERE", SuggestionType::Keyword),
    ]);
    let screen = render(&state, 40, 10, Rect::new(0, 0, 28, 8));
    // Candidate texts present.
    assert!(screen.contains("id"), "screen:\n{screen}");
    assert!(screen.contains("status"), "screen:\n{screen}");
    assert!(screen.contains("created_at"), "screen:\n{screen}");
    assert!(screen.contains("COUNT"), "screen:\n{screen}");
    // Typed hints (right-aligned) present.
    assert!(screen.contains("int"), "screen:\n{screen}");
    assert!(screen.contains("date"), "screen:\n{screen}");
    assert!(screen.contains("agg"), "screen:\n{screen}");
    assert!(screen.contains("kw"), "screen:\n{screen}");
}

#[test]
fn snapshot_populated_popup_with_typed_hints() {
    let mut state = AutocompleteState::new();
    state.open_with(vec![
        Suggestion::new_with_type("id", SuggestionType::Field, Some(ColumnType::Int)),
        Suggestion::new_with_type("status", SuggestionType::Field, Some(ColumnType::Text)),
        Suggestion::new_with_type("amount", SuggestionType::Field, Some(ColumnType::Float)),
        Suggestion::new_with_type("created_at", SuggestionType::Field, Some(ColumnType::Date)),
        Suggestion::new("COUNT", SuggestionType::Aggregate),
    ]);
    let screen = render(&state, 40, 10, Rect::new(0, 0, 28, 8));
    insta::assert_snapshot!(screen);
}

#[test]
fn snapshot_value_completion_popup() {
    // The `WHERE status = '|` value popup: distinct values typed as the column's type.
    let mut state = AutocompleteState::new();
    state.open_with(vec![
        Suggestion::new_with_type("active", SuggestionType::Value, Some(ColumnType::Text)),
        Suggestion::new_with_type("archived", SuggestionType::Value, Some(ColumnType::Text)),
        Suggestion::new_with_type("pending", SuggestionType::Value, Some(ColumnType::Text)),
    ]);
    let screen = render(&state, 40, 8, Rect::new(0, 0, 24, 6));
    insta::assert_snapshot!(screen);
}

#[test]
fn selection_moves_with_next_prev() {
    let mut state = AutocompleteState::new();
    state.open_with(vec![
        Suggestion::new("a", SuggestionType::Field),
        Suggestion::new("b", SuggestionType::Field),
    ]);
    assert_eq!(state.selected(), 0);
    state.select_next();
    assert_eq!(state.selected(), 1);
    state.select_next(); // bounded at the last entry; no wrap
    assert_eq!(state.selected(), 1);
    state.select_prev();
    assert_eq!(state.selected(), 0);
    state.select_prev(); // bounded at 0; no wrap
    assert_eq!(state.selected(), 0);
}

#[test]
fn render_does_not_panic_on_degenerate_area() {
    let mut state = AutocompleteState::new();
    state.open_with(vec![Suggestion::new("col", SuggestionType::Field)]);
    for (w, h) in [(1u16, 1u16), (2, 2), (3, 1), (1, 3)] {
        let _ = render(&state, w.max(1), h.max(1), Rect::new(0, 0, w, h));
    }
}

// --- bottom-border hints ---

#[test]
fn bottom_border_is_clean_off_select_pane() {
    // Tab-accept, ↑↓-select, and Esc-close are universal autocomplete idioms — none are spelled
    // out on the popup's bottom border. The ONLY hint is the contextual `Ctrl+P multi-select`,
    // shown only when the SELECT pane is focused. Off SELECT (the default render), the bottom
    // border carries no hint text at all. See `hint_spans` for the rationale.
    let mut state = AutocompleteState::new();
    state.open_with(vec![Suggestion::new("id", SuggestionType::Field)]);
    let screen = render(&state, 80, 8, Rect::new(0, 0, 80, 6));
    assert!(
        !screen.contains("accept") && !screen.contains("close") && !screen.contains("Ctrl+P"),
        "off-SELECT popup border carries no hint text: {screen}"
    );
}

#[test]
fn bottom_border_shows_multi_select_hint_when_select_pane_focused() {
    let mut state = AutocompleteState::new();
    state.open_with(vec![Suggestion::new("id", SuggestionType::Field)]);
    let screen = render_with(&state, 80, 8, Rect::new(0, 0, 80, 6), true);
    assert!(
        screen.contains("Ctrl+P") && screen.contains("multi-select"),
        "Ctrl+P multi-select hint surfaces when focus is on the SELECT pane: {screen}"
    );
}

#[test]
fn bottom_border_omits_multi_select_hint_off_select_pane() {
    let mut state = AutocompleteState::new();
    state.open_with(vec![Suggestion::new("id", SuggestionType::Field)]);
    let screen = render_with(&state, 80, 8, Rect::new(0, 0, 80, 6), false);
    assert!(
        !screen.contains("Ctrl+P"),
        "Ctrl+P multi-select hint must NOT appear off the SELECT pane: {screen}"
    );
}
