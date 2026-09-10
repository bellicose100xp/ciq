//! Focus and pointer tests for the AI chat: the chat staying visible while the keyboard is in the
//! bar, `Ctrl+G` moving the keyboard, clicks deciding who owns it, `open_on_start`, and one end-to-end
//! pass through a real `TestBackend` render so the recorded regions are the ones clicks resolve
//! against. Split out of `ai_tests.rs` to keep each file under the 1000-line limit; the fixtures
//! (`ai_app`, `asked`, `ctrl`, `type_str`, …) come from the parent via `super`.

use super::*;

// --- the chat can stay visible while the keyboard is elsewhere ---

/// A layout with the AI popup at the bottom right and the query bar below it, as the renderer lays
/// them out, so clicks can be aimed at either.
fn with_chat_and_bar_layout(app: &App) {
    use crate::app::{LayoutRegions, PopupKind};
    use ratatui::layout::Rect;
    app.layout_regions.set(LayoutRegions {
        results_pane: Some(Rect::new(0, 1, 80, 15)),
        popup: Some((PopupKind::Ai, Rect::new(30, 4, 50, 12))),
        query_bar: Some(Rect::new(0, 16, 80, 5)),
        ..LayoutRegions::default()
    });
}

#[test]
fn typing_goes_to_the_bar_while_the_chat_is_visible_but_unfocused() {
    let (mut app, ai_rx, _query_rx) = asked("only EU rows", TWO_SUGGESTIONS);
    app.ai.blur();
    assert!(app.is_ai_open() && !app.ai().is_focused());
    type_str(&mut app, "SELECT 1", 0);
    assert_eq!(app.query(), "SELECT 1", "keys reach the query bar");
    assert_eq!(app.ai().input(), "", "and not the chat");
    app.on_key(KeyEvent::plain(Key::Enter), 0);
    assert!(
        ai_rx.try_recv().is_err(),
        "Enter did not send a chat message"
    );
}

#[test]
fn ctrl_g_focuses_a_visible_chat_and_the_bar_keys_stop_reaching_it() {
    let (mut app, _ai_rx, _query_rx) = asked("only EU rows", TWO_SUGGESTIONS);
    app.ai.blur();
    app.on_key(ctrl('g'), 0);
    assert!(app.is_ai_open(), "still open");
    assert!(
        app.ai().is_focused(),
        "Ctrl+G on an unfocused chat hands it the keyboard"
    );
    type_str(&mut app, "why", 0);
    assert_eq!(app.ai().input(), "why");
}

#[test]
fn alt_digit_applies_a_suggestion_even_when_the_chat_is_unfocused() {
    let (mut app, _ai_rx, _query_rx) = asked("only EU rows", TWO_SUGGESTIONS);
    app.ai.blur();
    app.on_key(alt(Key::Char('2')), 0);
    assert_eq!(app.query(), "SELECT id FROM t");
    assert!(app.is_ai_open());
}

#[test]
fn a_click_on_the_query_bar_blurs_the_chat_and_places_the_cursor() {
    let (mut app, _ai_rx, _query_rx) = asked("only EU rows", TWO_SUGGESTIONS);
    app.force_power_mode_for_tests("SELECT id FROM t");
    with_chat_and_bar_layout(&app);
    assert!(app.ai().is_focused());
    // Click inside the bar's text area (row 17 is the first inner bar row; col 5 is past `> `).
    app.on_mouse(crate::app::MouseEvent::Click { col: 5, row: 17 }, 0);
    assert!(app.is_ai_open(), "the chat stays visible");
    assert!(!app.ai().is_focused(), "but the keyboard moved to the bar");
    type_str(&mut app, "X", 0);
    assert!(app.query().contains('X'), "typing now edits the bar");
}

#[test]
fn a_click_inside_the_chat_gives_it_the_keyboard_back() {
    let (mut app, _ai_rx, _query_rx) = asked("only EU rows", TWO_SUGGESTIONS);
    with_chat_and_bar_layout(&app);
    app.ai.blur();
    // The input line is the last inner row of the 12-row popup at y=4: rows 5..=14, input at 14.
    app.on_mouse(crate::app::MouseEvent::Click { col: 40, row: 14 }, 0);
    assert!(
        app.ai().is_focused(),
        "clicking the input line focuses the chat"
    );
    type_str(&mut app, "hi", 0);
    assert_eq!(app.ai().input(), "hi");
}

#[test]
fn the_chat_opens_unfocused_on_load_by_default() {
    let (mut app, _query_rx) = super::super::app();
    let (ai_tx, _ai_rx) = channel::<AiJob>();
    app.set_ai_channel(ai_tx);
    app.configure_ai(&crate::config::AiConfig::default());
    app.set_schema(super::super::test_schema());
    app.on_loaded("ready");
    assert!(app.is_ai_open(), "open_on_start defaults to true");
    assert!(
        !app.ai().is_focused(),
        "but the keyboard stays in the bar so SQL typing is unaffected"
    );
}

#[test]
fn open_on_start_false_keeps_the_chat_closed() {
    let (mut app, _query_rx) = super::super::app();
    let (ai_tx, _ai_rx) = channel::<AiJob>();
    app.set_ai_channel(ai_tx);
    app.configure_ai(&crate::config::AiConfig {
        open_on_start: Some(false),
        ..crate::config::AiConfig::default()
    });
    app.set_schema(super::super::test_schema());
    app.on_loaded("ready");
    assert!(!app.is_ai_open());
}

#[test]
fn the_chat_does_not_auto_open_without_a_provider() {
    let (mut app, _query_rx) = super::super::app();
    app.configure_ai(&crate::config::AiConfig::default());
    app.set_schema(super::super::test_schema());
    app.on_loaded("ready");
    assert!(!app.is_ai_open(), "no provider wired, nothing to show");
}

/// End to end through a real `TestBackend` render: the regions the paint records are the ones the
/// click resolves against, so a click inside the drawn chat focuses it and one on the bar blurs it.
#[test]
fn a_real_render_lets_a_click_focus_the_chat_and_a_bar_click_leave_it() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    let (mut app, _ai_rx, _query_rx) = asked("only EU rows", TWO_SUGGESTIONS);
    app.ai.blur();
    let mut t = Terminal::new(TestBackend::new(100, 40)).unwrap();
    t.draw(|f| app.render(f)).unwrap();
    let (kind, popup) = app
        .layout_regions
        .get()
        .popup
        .expect("the chat was recorded");
    assert_eq!(kind, crate::app::PopupKind::Ai);
    let bar = app
        .layout_regions
        .get()
        .query_bar
        .expect("the bar was recorded");
    assert!(
        popup.bottom() <= bar.y,
        "the chat sits above the bar: {popup:?} vs {bar:?}"
    );
    // The input line is the last inner row of the box.
    let input_row = popup.y + popup.height - 2;
    app.on_mouse(
        crate::app::MouseEvent::Click {
            col: popup.x + 4,
            row: input_row,
        },
        0,
    );
    assert!(
        app.ai().is_focused(),
        "a click on the input line takes the keyboard"
    );
    type_str(&mut app, "why", 0);
    assert_eq!(app.ai().input(), "why");
    app.on_mouse(
        crate::app::MouseEvent::Click {
            col: bar.x + 12,
            row: bar.y,
        },
        0,
    );
    assert!(app.is_ai_open(), "the chat stays visible");
    assert!(!app.ai().is_focused(), "but the bar has the keyboard");
    type_str(&mut app, "1", 0);
    assert_eq!(app.ai().input(), "why", "the chat line is untouched");
    assert!(
        app.query().contains('1'),
        "the bar took the keystroke: {:?}",
        app.query()
    );
}
