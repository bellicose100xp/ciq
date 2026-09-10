//! Tests for the conversation transcript: turn construction, the wire text a turn contributes, the
//! suggestion coordinate maps the popup navigates, and the send-budget trimming.

use super::*;
use crate::ai::suggestion::{Suggestion, SuggestionKind};

fn sug(sql: &str) -> Suggestion {
    Suggestion {
        kind: SuggestionKind::Optimize,
        sql: sql.to_string(),
        details: "note".to_string(),
    }
}

fn convo() -> Conversation {
    let mut c = Conversation::new();
    c.push(Turn::user("only EU rows"));
    c.push(Turn::assistant(
        "filter on region",
        vec![sug("SELECT * FROM t WHERE region = 'EU'")],
    ));
    c.push(Turn::user("now group by status"));
    c.push(Turn::assistant(
        "added a GROUP BY",
        vec![
            sug("SELECT status, count(*) FROM t GROUP BY status"),
            sug("SELECT status, sum(amount) FROM t GROUP BY status"),
        ],
    ));
    c
}

#[test]
fn roles_carry_a_wire_value_and_a_display_label() {
    assert_eq!(Role::User.as_wire(), "user");
    assert_eq!(Role::Assistant.as_wire(), "assistant");
    assert_eq!(Role::User.label(), "you");
    assert_eq!(Role::Assistant.label(), "ai");
}

#[test]
fn a_user_turn_carries_no_suggestions() {
    let t = Turn::user("hello");
    assert_eq!(t.role, Role::User);
    assert_eq!(t.text, "hello");
    assert!(t.suggestions.is_empty());
    assert_eq!(t.wire_text(), "hello");
}

#[test]
fn an_assistant_turns_wire_text_appends_its_sql() {
    let t = Turn::assistant("here you go", vec![sug("SELECT 1"), sug("SELECT 2")]);
    let wire = t.wire_text();
    assert!(wire.starts_with("here you go"), "{wire}");
    assert!(
        wire.contains("SELECT 1") && wire.contains("SELECT 2"),
        "the model must see what it previously proposed: {wire}"
    );
}

#[test]
fn an_empty_conversation_is_empty() {
    let c = Conversation::new();
    assert!(c.is_empty());
    assert_eq!(c.len(), 0);
    assert!(c.turns().is_empty());
    assert!(c.latest_suggestions().is_empty());
    assert!(c.all_suggestions().is_empty());
    assert_eq!(c.suggestion_at(0, 0), None);
}

#[test]
fn push_appends_in_order_and_clear_empties() {
    let mut c = convo();
    assert_eq!(c.len(), 4);
    assert_eq!(c.turns()[0].text, "only EU rows");
    assert_eq!(c.turns()[3].role, Role::Assistant);
    c.clear();
    assert!(c.is_empty());
}

#[test]
fn latest_suggestions_come_from_the_newest_answer() {
    let c = convo();
    let latest = c.latest_suggestions();
    assert_eq!(latest.len(), 2, "the second answer had two");
    assert_eq!(
        latest[0].sql,
        "SELECT status, count(*) FROM t GROUP BY status"
    );
}

#[test]
fn latest_suggestions_is_empty_when_the_newest_answer_had_none() {
    let mut c = Conversation::new();
    c.push(Turn::user("what does region mean"));
    c.push(Turn::assistant("it is the sales territory", Vec::new()));
    assert!(
        c.latest_suggestions().is_empty(),
        "a prose-only answer offers nothing to pick"
    );
}

#[test]
fn latest_suggestions_ignores_a_trailing_user_turn() {
    let mut c = convo();
    c.push(Turn::user("and sorted?"));
    assert_eq!(
        c.latest_suggestions().len(),
        2,
        "a pending question does not clear the last answer's picks"
    );
}

#[test]
fn all_suggestions_flattens_every_turn_oldest_first() {
    let c = convo();
    assert_eq!(c.all_suggestions(), vec![(1, 0), (3, 0), (3, 1)]);
    assert_eq!(
        c.suggestion_at(1, 0).map(|s| s.sql.as_str()),
        Some("SELECT * FROM t WHERE region = 'EU'")
    );
    assert_eq!(
        c.suggestion_at(3, 1).map(|s| s.sql.as_str()),
        Some("SELECT status, sum(amount) FROM t GROUP BY status")
    );
    assert_eq!(c.suggestion_at(3, 2), None);
    assert_eq!(c.suggestion_at(9, 0), None);
}

// --- the send budget ---

#[test]
fn wire_turns_sends_everything_that_fits() {
    let c = convo();
    let sent = c.wire_turns(100_000);
    assert_eq!(sent.len(), 4, "a generous budget sends the whole thread");
    assert_eq!(sent[0].text, "only EU rows");
}

#[test]
fn wire_turns_keeps_the_newest_turns_when_the_budget_bites() {
    let c = convo();
    // Enough for the last user turn plus the last answer, not the whole thread.
    let tail_cost =
        c.turns()[2].wire_text().chars().count() + c.turns()[3].wire_text().chars().count();
    let sent = c.wire_turns(tail_cost);
    assert!(sent.len() < 4, "the oldest turns were dropped");
    assert_eq!(
        sent.last().map(|t| t.text.as_str()),
        Some("added a GROUP BY"),
        "the newest turn is always kept"
    );
}

#[test]
fn wire_turns_starts_on_a_user_turn() {
    let c = convo();
    // A budget that would otherwise start mid-answer.
    for budget in 1..400 {
        let sent = c.wire_turns(budget);
        if let Some(first) = sent.first() {
            assert_eq!(
                first.role,
                Role::User,
                "budget {budget} produced a slice starting with an answer"
            );
        }
    }
}

#[test]
fn wire_turns_sends_the_last_turn_even_when_it_alone_exceeds_the_budget() {
    let mut c = Conversation::new();
    c.push(Turn::user("x".repeat(500)));
    let sent = c.wire_turns(10);
    assert_eq!(
        sent.len(),
        1,
        "dropping the actual question would be worse than exceeding the budget"
    );
}

#[test]
fn wire_turns_on_an_empty_conversation_sends_nothing() {
    assert!(Conversation::new().wire_turns(1000).is_empty());
}

#[test]
fn wire_turns_drops_a_lone_trailing_answer() {
    // An assistant-only transcript has no user turn to anchor on, so nothing is sent.
    let mut c = Conversation::new();
    c.push(Turn::assistant("unprompted", Vec::new()));
    assert!(c.wire_turns(1000).is_empty());
}
