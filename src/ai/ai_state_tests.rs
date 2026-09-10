//! Tests for the AI chat state machine — pure transitions over owned data (no terminal, no engine,
//! no clock). Covers the input line, the send/answer/error lifecycle, transcript persistence across
//! close, the suggestion cursor over the whole transcript, and scrolling.

use super::*;
use crate::ai::suggestion::{ChatReply, Suggestion, SuggestionKind};

fn sug(kind: SuggestionKind, sql: &str) -> Suggestion {
    Suggestion {
        kind,
        sql: sql.to_string(),
        details: format!("note for {sql}"),
    }
}

fn reply(text: &str, sqls: &[&str]) -> ChatReply {
    ChatReply {
        reply: text.to_string(),
        suggestions: sqls
            .iter()
            .map(|s| sug(SuggestionKind::Optimize, s))
            .collect(),
    }
}

fn typed(s: &mut AiState, text: &str) {
    for c in text.chars() {
        s.push_char(c);
    }
}

/// An open chat with one question already answered by two suggestions.
fn answered() -> AiState {
    let mut s = AiState::new();
    s.open();
    typed(&mut s, "only EU rows");
    s.send();
    s.set_reply(reply("filter on region", &["SELECT 1", "SELECT 2"]));
    s
}

// --- opening and closing ---

#[test]
fn new_is_closed_and_empty() {
    let s = AiState::new();
    assert!(!s.is_open());
    assert_eq!(s.input(), "");
    assert_eq!(s.phase(), &AiPhase::Editing);
    assert!(s.is_fresh());
    assert_eq!(s.selected(), None);
    assert_eq!(s.scroll(), 0);
}

#[test]
fn open_clears_the_input_and_any_stale_error() {
    let mut s = AiState::new();
    s.open();
    typed(&mut s, "half typed");
    s.send();
    s.set_error("boom");
    s.close();
    s.open();
    assert_eq!(s.input(), "");
    assert_eq!(s.phase(), &AiPhase::Editing, "a stale error is cleared");
}

#[test]
fn closing_keeps_the_transcript_so_reopening_resumes_the_conversation() {
    let mut s = answered();
    assert_eq!(s.chat().len(), 2);
    s.close();
    assert!(!s.is_open());
    s.open();
    assert!(s.is_open());
    assert_eq!(
        s.chat().len(),
        2,
        "the conversation survives a close/reopen — that is the whole point of the memory"
    );
    assert!(!s.is_fresh());
}

#[test]
fn reset_starts_a_new_conversation() {
    let mut s = answered();
    s.select_next();
    s.reset();
    assert!(s.chat().is_empty());
    assert!(s.is_fresh());
    assert_eq!(s.input(), "");
    assert_eq!(s.selected(), None);
    assert_eq!(s.scroll(), 0);
    assert_eq!(s.phase(), &AiPhase::Editing);
}

// --- the input line ---

#[test]
fn typing_appends_and_backspace_pops_whole_chars() {
    let mut s = AiState::new();
    s.open();
    typed(&mut s, "héllo\u{1F600}");
    assert_eq!(s.input(), "héllo\u{1F600}");
    s.backspace();
    assert_eq!(s.input(), "héllo", "pops the whole emoji, not a byte");
}

#[test]
fn backspace_on_empty_input_is_a_no_op() {
    let mut s = AiState::new();
    s.open();
    s.backspace();
    assert_eq!(s.input(), "");
}

#[test]
fn paste_flattens_newlines_into_one_line() {
    let mut s = AiState::new();
    s.open();
    s.paste("group by\nstatus\r\nplease");
    assert_eq!(s.input(), "group by status  please");
}

#[test]
fn editing_clears_a_stale_error_banner() {
    let mut s = AiState::new();
    s.open();
    typed(&mut s, "q");
    s.send();
    s.set_error("network down");
    typed(&mut s, "r");
    assert_eq!(
        s.phase(),
        &AiPhase::Editing,
        "typing means the user moved on from the failure"
    );
    assert_eq!(s.input(), "r");
}

#[test]
fn input_is_ignored_while_a_request_is_in_flight() {
    let mut s = AiState::new();
    s.open();
    typed(&mut s, "q");
    s.send();
    typed(&mut s, "more");
    s.backspace();
    s.paste("x");
    assert_eq!(s.input(), "", "the next message is not built mid-request");
}

// --- send / answer / error ---

#[test]
fn send_commits_a_user_turn_and_goes_pending() {
    let mut s = AiState::new();
    s.open();
    typed(&mut s, "  top 5 regions  ");
    assert_eq!(s.send(), Some("top 5 regions".to_string()));
    assert!(s.is_pending());
    assert_eq!(s.input(), "", "the line is consumed");
    assert_eq!(s.chat().len(), 1);
    assert_eq!(s.chat().turns()[0].text, "top 5 regions");
}

#[test]
fn send_is_a_no_op_on_a_blank_line_or_while_pending() {
    let mut s = AiState::new();
    s.open();
    assert_eq!(s.send(), None);
    typed(&mut s, "   ");
    assert_eq!(s.send(), None);
    assert!(s.chat().is_empty());
    typed(&mut s, "q");
    assert!(s.send().is_some());
    typed(&mut s, "again");
    assert_eq!(s.send(), None, "one request at a time");
}

#[test]
fn an_answer_appends_an_assistant_turn_and_returns_to_editing() {
    let s = answered();
    assert_eq!(s.phase(), &AiPhase::Editing);
    assert_eq!(s.chat().len(), 2);
    let turn = &s.chat().turns()[1];
    assert_eq!(turn.text, "filter on region");
    assert_eq!(turn.suggestions.len(), 2);
    assert_eq!(s.selected(), None, "a fresh answer starts with no cursor");
}

#[test]
fn an_answer_with_no_suggestions_is_still_a_turn() {
    let mut s = AiState::new();
    s.open();
    typed(&mut s, "what does region mean");
    s.send();
    s.set_reply(reply("it is the sales territory", &[]));
    assert_eq!(s.chat().len(), 2);
    assert!(s.latest_suggestions().is_empty());
    assert_eq!(s.phase(), &AiPhase::Editing);
}

#[test]
fn an_error_keeps_the_transcript_intact() {
    let mut s = answered();
    typed(&mut s, "and sorted?");
    s.send();
    s.set_error("HTTP 500");
    assert_eq!(s.phase(), &AiPhase::Error("HTTP 500".to_string()));
    assert_eq!(
        s.chat().len(),
        3,
        "the question stays in the transcript so context is not lost"
    );
}

#[test]
fn a_failure_turn_is_recorded_only_when_nothing_is_in_flight() {
    let mut s = AiState::new();
    s.open();
    assert!(s.begin_failure_turn("the query failed: no such column"));
    assert!(s.is_pending());
    assert_eq!(s.chat().len(), 1);
    assert!(
        !s.begin_failure_turn("another failure"),
        "a burst of failures must not stack requests"
    );
    assert_eq!(s.chat().len(), 1);
}

#[test]
fn a_conversation_alternates_and_remembers_every_turn() {
    let mut s = answered();
    typed(&mut s, "now group by status");
    s.send();
    s.set_reply(reply("added a GROUP BY", &["SELECT 3"]));
    let roles: Vec<_> = s.chat().turns().iter().map(|t| t.role).collect();
    assert_eq!(
        roles,
        vec![
            crate::ai::chat::Role::User,
            crate::ai::chat::Role::Assistant,
            crate::ai::chat::Role::User,
            crate::ai::chat::Role::Assistant
        ]
    );
    assert_eq!(s.chat().len(), 4);
}

// --- picking a suggestion ---

#[test]
fn alt_digit_reads_the_latest_answers_suggestions() {
    let s = answered();
    assert_eq!(s.take_latest(0), Some("SELECT 1".to_string()));
    assert_eq!(s.take_latest(1), Some("SELECT 2".to_string()));
    assert_eq!(s.take_latest(5), None);
}

#[test]
fn alt_digit_follows_the_newest_answer_as_the_conversation_moves_on() {
    let mut s = answered();
    typed(&mut s, "now group by status");
    s.send();
    s.set_reply(reply("added a GROUP BY", &["SELECT 99"]));
    assert_eq!(
        s.take_latest(0),
        Some("SELECT 99".to_string()),
        "the digits always mean the answer on screen"
    );
    assert_eq!(s.take_latest(1), None);
}

#[test]
fn the_cursor_starts_on_the_newest_suggestion_and_clamps() {
    let mut s = answered();
    assert_eq!(s.take_selected(), None, "no cursor yet");
    s.select_next();
    assert_eq!(
        s.selected(),
        Some(1),
        "the first press lands on the newest row, which is what the user is looking at"
    );
    s.select_next();
    assert_eq!(s.selected(), Some(1), "clamps at the last row");
    s.select_prev();
    assert_eq!(s.selected(), Some(0));
    s.select_prev();
    assert_eq!(s.selected(), Some(0), "clamps at the first row");
}

#[test]
fn the_cursor_can_reach_an_older_answers_suggestion() {
    let mut s = answered();
    typed(&mut s, "now group by status");
    s.send();
    s.set_reply(reply("added a GROUP BY", &["SELECT 3"]));
    // Flattened: [SELECT 1, SELECT 2, SELECT 3]. Walk back to the first answer's row.
    s.select_next();
    assert_eq!(s.take_selected(), Some("SELECT 3".to_string()));
    s.select_prev();
    s.select_prev();
    assert_eq!(
        s.take_selected(),
        Some("SELECT 1".to_string()),
        "scrolling back and applying an older suggestion works"
    );
}

#[test]
fn cursor_navigation_is_a_no_op_without_suggestions() {
    let mut s = AiState::new();
    s.open();
    s.select_next();
    s.select_prev();
    assert_eq!(s.selected(), None);
    assert_eq!(s.take_selected(), None);
}

#[test]
fn select_ignores_an_out_of_range_index() {
    let mut s = answered();
    s.select(1);
    assert_eq!(s.selected(), Some(1));
    s.select(7);
    assert_eq!(
        s.selected(),
        Some(1),
        "an out-of-range click keeps the cursor"
    );
}

#[test]
fn a_new_answer_clears_a_stale_cursor() {
    let mut s = answered();
    s.select(1);
    typed(&mut s, "again");
    s.send();
    s.set_reply(reply("here", &["SELECT 9"]));
    assert_eq!(
        s.selected(),
        None,
        "a stale index must not point into the new list"
    );
}

// --- keeping the cursor on screen (the renderer supplies the row and the total) ---

#[test]
fn reveal_selected_scrolls_an_offscreen_row_into_a_window() {
    let mut s = answered();
    // 20 rows total, 4-row window; the cursor's block starts at row 17.
    s.scroll_up(100, 4, 20);
    assert_eq!(s.scroll(), 0);
    s.reveal_selected(17, 4, 20);
    assert_eq!(s.scroll(), 14, "row 17 lands on the last of 4 visible rows");
    // Back up to row 2 from a window starting at 14.
    s.reveal_selected(2, 4, 20);
    assert_eq!(s.scroll(), 2, "scrolled up so the row is the first visible");
    // Already visible: the window does not move.
    s.reveal_selected(4, 4, 20);
    assert_eq!(s.scroll(), 2);
}

#[test]
fn reveal_selected_is_a_no_op_for_a_zero_window() {
    let mut s = answered();
    s.scroll_up(100, 4, 20);
    s.reveal_selected(17, 0, 20);
    assert_eq!(s.scroll(), 0);
}

// --- scrolling ---

#[test]
fn sending_and_answering_follow_the_tail() {
    let mut s = answered();
    s.scroll_up(10, 2, 6);
    assert_eq!(s.scroll(), 0);
    typed(&mut s, "again");
    s.send();
    assert_eq!(
        s.scroll(),
        usize::MAX,
        "sending parks past the end (follow the tail)"
    );
    s.scroll_up(10, 2, 8);
    s.set_reply(reply("ok", &[]));
    assert_eq!(s.scroll(), usize::MAX, "an answer parks past the end too");
}

#[test]
fn scrolling_clamps_at_both_ends() {
    let mut s = answered();
    s.scroll_up(100, 2, 6);
    assert_eq!(s.scroll(), 0, "never above the first row");
    s.scroll_down(1000, 2, 6);
    assert_eq!(
        s.scroll(),
        4,
        "never past the last full window (6 rows, 2 visible)"
    );
}

#[test]
fn the_first_scroll_up_after_an_answer_moves_the_view() {
    // Parked past the end over 18 rows with a 4-row window, the renderer shows rows 14..18. One
    // press must show 13..17, not eat presses until a raw offset catches up.
    let mut s = answered();
    s.scroll_to_end();
    s.scroll_up(1, 4, 18);
    assert_eq!(s.scroll(), 13);
    // A transcript shorter than the window is already fully shown: one press lands on 0.
    s.scroll_to_end();
    s.scroll_up(1, 10, 6);
    assert_eq!(s.scroll(), 0);
}

#[test]
fn scroll_down_from_the_tail_stays_at_the_tail() {
    let mut s = answered();
    s.scroll_to_end();
    s.scroll_down(3, 4, 18);
    assert_eq!(s.scroll(), 14);
}
