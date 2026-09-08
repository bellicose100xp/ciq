use crate::app::editor::EditorMode;
use crate::theme::base as p;
use crate::theme::border::{ResultState, mode_color, query_box, result_color, results};
use ratatui::style::Modifier;

#[test]
fn mode_color_maps_each_vim_mode_to_its_galaxy_accent() {
    assert_eq!(mode_color(EditorMode::Insert), p::pal().cyan);
    assert_eq!(mode_color(EditorMode::Normal), p::pal().yellow);
    assert_eq!(mode_color(EditorMode::Operator('d')), p::pal().green);
    assert_eq!(
        mode_color(EditorMode::TextObject(
            'd',
            crate::app::editor::mode::TextObjectScope::Inner
        )),
        p::pal().green
    );
    assert_eq!(
        mode_color(EditorMode::CharSearch(
            crate::app::editor::char_search::SearchDirection::Forward,
            crate::app::editor::char_search::SearchType::Find
        )),
        p::pal().pink
    );
    assert_eq!(
        mode_color(EditorMode::OperatorCharSearch(
            'd',
            0,
            crate::app::editor::char_search::SearchDirection::Forward,
            crate::app::editor::char_search::SearchType::Find
        )),
        p::pal().pink
    );
}

#[test]
fn query_box_focused_takes_mode_color_and_error_overrides() {
    // Focused, no error → mode color.
    assert_eq!(
        query_box(EditorMode::Insert, false, true).fg,
        Some(p::pal().cyan)
    );
    assert_eq!(
        query_box(EditorMode::Normal, false, true).fg,
        Some(p::pal().yellow)
    );
    // Focused, error → red overrides mode.
    assert_eq!(
        query_box(EditorMode::Normal, true, true).fg,
        Some(p::pal().border_error)
    );
    assert_eq!(
        query_box(EditorMode::Insert, true, true).fg,
        Some(p::pal().border_error)
    );
}

#[test]
fn query_box_unfocused_recedes_in_muted_slate_regardless_of_mode_or_error() {
    for mode in [
        EditorMode::Insert,
        EditorMode::Normal,
        EditorMode::Operator('d'),
    ] {
        for has_error in [false, true] {
            assert_eq!(
                query_box(mode, has_error, false).fg,
                Some(p::pal().border_unfocused),
                "unfocused dominates: mode={mode:?} has_error={has_error}"
            );
        }
    }
}

#[test]
fn result_color_maps_each_state_to_its_galaxy_accent() {
    assert_eq!(result_color(ResultState::Ok), p::pal().green);
    assert_eq!(result_color(ResultState::Empty), p::pal().yellow);
    assert_eq!(result_color(ResultState::Error), p::pal().red);
    assert_eq!(result_color(ResultState::Pending), p::pal().cyan);
}

#[test]
fn results_focused_uses_state_color_unfocused_dims_same_hue() {
    // Focused: bright state color, no DIM.
    let focused_ok = results(ResultState::Ok, true);
    assert_eq!(focused_ok.fg, Some(p::pal().green));
    assert!(!focused_ok.add_modifier.contains(Modifier::DIM));
    // Unfocused: same hue, DIM applied.
    let unfocused_ok = results(ResultState::Ok, false);
    assert_eq!(unfocused_ok.fg, Some(p::pal().green));
    assert!(unfocused_ok.add_modifier.contains(Modifier::DIM));
    // Same pattern for the other states.
    for state in [ResultState::Empty, ResultState::Error, ResultState::Pending] {
        let focused = results(state, true);
        let unfocused = results(state, false);
        assert_eq!(focused.fg, unfocused.fg, "hue preserved across focus");
        assert!(!focused.add_modifier.contains(Modifier::DIM));
        assert!(unfocused.add_modifier.contains(Modifier::DIM));
    }
}
