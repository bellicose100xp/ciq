//! `App`-shell + end-to-end tests for the AI chat (P5.1): the `Ctrl+A` chord shows and hides the
//! popup and `Ctrl+G` moves the keyboard into and out of it, the popup's key routing (type / send /
//! pick / scroll / reset / close / quit), a request carrying the
//! system prompt and the whole conversation, a picked query firing through the **normal**
//! preprocess-validate + dispatch path, the read-only guard keeping a DML suggestion off the list,
//! the automatic turn a failing query adds, and the AI thread wiring exercised deterministically
//! with the mock (no network, no sleep).
//!
//! Split out of `app_tests.rs` to keep each file under the 1000-line limit; the shared App helpers
//! live in the parent (`super`).

use std::sync::mpsc::{Receiver, channel};

use crate::ai::ai_app::{AiJob, AiResult, spawn_ai_thread};
use crate::ai::ai_state::AiPhase;
use crate::ai::provider::{AiError, MockProvider};
use crate::app::{App, Key, KeyEvent, KeyMods};
use crate::engine::QueryOutcome;
use crate::harness::engine_harness::EngineHarness;
use crate::query::preprocess::prepare_interactive;
use crate::query::worker::types::QueryRequest;

use super::{loaded_app, type_str};

fn ctrl(c: char) -> KeyEvent {
    KeyEvent::new(Key::Char(c), KeyMods::CTRL)
}

fn alt(key: Key) -> KeyEvent {
    KeyEvent::new(key, KeyMods::ALT)
}

/// A loaded App with an AI request channel wired (the test keeps the receiver so it can read the
/// built request). Mirrors how the event loop wires `set_ai_channel`, but synchronous (no thread) so
/// the App-routing tests are deterministic. The query worker's receiver is returned too: dropping
/// it would make every dispatch fail with "worker unavailable" and silently weaken the tests.
fn ai_app() -> (App, Receiver<AiJob>, Receiver<QueryRequest>) {
    let (mut app, query_rx) = loaded_app();
    let (ai_tx, ai_rx) = channel::<AiJob>();
    app.set_ai_channel(ai_tx);
    (app, ai_rx, query_rx)
}

/// A model answer as the JSON contract describes it.
fn answer(seq: u64, json: &str) -> AiResult {
    AiResult {
        seq,
        outcome: Ok(json.to_string()),
    }
}

const TWO_SUGGESTIONS: &str = r#"{"reply":"filter on the region column","suggestions":[
    {"type":"fix","sql":"SELECT * FROM t WHERE region = 'EU'","details":"adds the filter"},
    {"type":"optimize","sql":"SELECT id FROM t","details":"projects less"}
]}"#;

/// Open the chat, send `question`, and answer it with `json`.
fn asked(question: &str, json: &str) -> (App, Receiver<AiJob>, Receiver<QueryRequest>) {
    let (mut app, ai_rx, query_rx) = ai_app();
    app.on_key(ctrl('a'), 0);
    type_str(&mut app, question, 0);
    app.on_key(KeyEvent::plain(Key::Enter), 0);
    let job = ai_rx.try_recv().expect("a request was sent");
    app.on_ai_result(answer(job.seq, json), 0);
    (app, ai_rx, query_rx)
}

// --- the chord + popup gating ---

#[test]
fn ctrl_a_opens_and_toggles_the_chat() {
    let (mut app, _ai_rx, _query_rx) = ai_app();
    assert!(!app.is_ai_open());
    app.on_key(ctrl('a'), 0);
    assert!(app.is_ai_open());
    assert!(
        app.ai().is_focused(),
        "opening by hand hands it the keyboard"
    );
    assert_eq!(app.ai().phase(), &AiPhase::Editing);
    app.on_key(ctrl('a'), 0);
    assert!(!app.is_ai_open(), "the same chord closes it");
}

#[test]
fn ctrl_a_hides_a_visible_chat_whether_or_not_it_has_the_keyboard() {
    let (mut app, _ai_rx, _query_rx) = asked("only EU rows", TWO_SUGGESTIONS);
    app.ai.blur();
    app.on_key(ctrl('a'), 0);
    assert!(
        !app.is_ai_open(),
        "Ctrl+A on an unfocused chat hides it, never focuses it"
    );
    app.on_key(ctrl('a'), 0);
    app.on_key(ctrl('a'), 0);
    assert!(!app.is_ai_open(), "and on a focused chat it hides it too");
}

#[test]
fn ctrl_g_moves_the_keyboard_between_the_bar_and_the_chat() {
    let (mut app, _ai_rx, _query_rx) = ai_app();
    app.on_key(ctrl('g'), 0);
    assert!(
        app.is_ai_open() && app.ai().is_focused(),
        "a hidden chat opens focused"
    );
    app.on_key(ctrl('g'), 0);
    assert!(app.is_ai_open(), "still visible");
    assert!(!app.ai().is_focused(), "the keyboard went back to the bar");
    type_str(&mut app, "x", 0);
    assert_eq!(app.ai().input(), "", "typing reaches the bar, not the chat");
    app.on_key(ctrl('g'), 0);
    assert!(app.ai().is_focused(), "and Ctrl+G brings it back");
}

#[test]
fn ctrl_g_is_a_no_op_without_a_provider() {
    let (mut app, _rx) = loaded_app();
    app.on_key(ctrl('g'), 0);
    assert!(!app.is_ai_open());
}

#[test]
fn the_chord_is_a_no_op_without_a_provider() {
    let (mut app, _rx) = loaded_app(); // no set_ai_channel -> feature unwired
    assert!(!app.ai_enabled());
    app.on_key(ctrl('a'), 0);
    assert!(!app.is_ai_open(), "no provider => the chord does nothing");
}

#[test]
fn typing_fills_the_chat_not_the_query_bar() {
    let (mut app, _ai_rx, _query_rx) = ai_app();
    app.on_key(ctrl('a'), 0);
    type_str(&mut app, "rows in EU", 0);
    assert_eq!(app.ai().input(), "rows in EU");
    assert_eq!(app.query(), "", "typing fills the popup, not the query bar");
}

#[test]
fn esc_leaves_the_chat_visible_and_ctrl_c_quits() {
    let (mut app, _ai_rx, _query_rx) = ai_app();
    app.on_key(ctrl('a'), 0);
    assert!(!app.on_key(KeyEvent::plain(Key::Esc), 0));
    assert!(app.is_ai_open(), "Esc keeps the box on screen");
    assert!(!app.ai().is_focused(), "but hands the keyboard back");
    app.on_key(ctrl('g'), 0);
    assert!(app.on_key(ctrl('c'), 0), "Ctrl-C quits even from the popup");
}

// --- sending carries the system prompt and the conversation ---

#[test]
fn enter_sends_a_schema_grounded_request() {
    let (mut app, ai_rx, _query_rx) = ai_app();
    app.on_key(ctrl('a'), 0);
    type_str(&mut app, "count rows by status", 0);
    app.on_key(KeyEvent::plain(Key::Enter), 0);

    assert_eq!(app.ai().phase(), &AiPhase::Pending, "send -> Pending");
    let job = ai_rx.try_recv().expect("a request was sent");
    assert!(
        job.system.contains("`t`"),
        "the system prompt names the table:\n{}",
        job.system
    );
    assert!(
        job.system.contains("status"),
        "the system prompt embeds the columns:\n{}",
        job.system
    );
    assert_eq!(job.turns.len(), 1, "the first request is one user turn");
    assert_eq!(job.turns[0].0, "user");
    assert!(
        job.turns[0].1.contains("count rows by status"),
        "the turn carries the question:\n{}",
        job.turns[0].1
    );
}

#[test]
fn a_blank_line_does_not_send() {
    let (mut app, ai_rx, _query_rx) = ai_app();
    app.on_key(ctrl('a'), 0);
    app.on_key(KeyEvent::plain(Key::Enter), 0);
    assert_eq!(app.ai().phase(), &AiPhase::Editing);
    assert!(ai_rx.try_recv().is_err(), "no request for an empty line");
}

#[test]
fn a_follow_up_sends_the_whole_conversation() {
    let (mut app, ai_rx, _query_rx) = asked("only EU rows", TWO_SUGGESTIONS);
    type_str(&mut app, "now group by status", 0);
    app.on_key(KeyEvent::plain(Key::Enter), 0);

    let job = ai_rx.try_recv().expect("the follow-up was sent");
    assert_eq!(
        job.turns.len(),
        3,
        "question, answer, follow-up — this is what makes refinement work"
    );
    assert_eq!(job.turns[0].0, "user");
    assert_eq!(job.turns[1].0, "assistant");
    assert_eq!(job.turns[2].0, "user");
    assert!(
        job.turns[1]
            .1
            .contains("SELECT * FROM t WHERE region = 'EU'"),
        "the model sees the SQL it already proposed:\n{}",
        job.turns[1].1
    );
    assert!(job.turns[2].1.contains("now group by status"));
}

#[test]
fn the_transcript_survives_closing_the_popup() {
    let (mut app, ai_rx, _query_rx) = asked("only EU rows", TWO_SUGGESTIONS);
    app.on_key(ctrl('a'), 0);
    assert!(!app.is_ai_open());
    app.on_key(ctrl('a'), 0);
    assert_eq!(app.ai().chat().len(), 2, "reopening resumes the thread");
    type_str(&mut app, "and sorted?", 0);
    app.on_key(KeyEvent::plain(Key::Enter), 0);
    let job = ai_rx.try_recv().expect("sent");
    assert_eq!(job.turns.len(), 3, "the prior turns are still attached");
}

#[test]
fn ctrl_l_starts_a_new_conversation() {
    let (mut app, ai_rx, _query_rx) = asked("only EU rows", TWO_SUGGESTIONS);
    app.on_key(ctrl('l'), 0);
    assert!(app.ai().chat().is_empty());
    assert!(app.is_ai_open(), "resetting does not close the popup");
    type_str(&mut app, "fresh start", 0);
    app.on_key(KeyEvent::plain(Key::Enter), 0);
    let job = ai_rx.try_recv().expect("sent");
    assert_eq!(job.turns.len(), 1, "no prior context is attached");
}

// --- answers land as turns ---

#[test]
fn an_answer_appends_prose_and_pickable_queries() {
    let (app, _rx, _query_rx) = asked("only EU rows", TWO_SUGGESTIONS);
    assert_eq!(app.ai().chat().len(), 2);
    let turn = &app.ai().chat().turns()[1];
    assert_eq!(turn.text, "filter on the region column");
    assert_eq!(turn.suggestions.len(), 2);
    assert_eq!(
        app.ai().phase(),
        &AiPhase::Editing,
        "ready for the next question"
    );
}

#[test]
fn a_prose_only_answer_is_a_normal_turn() {
    let (app, _rx, _query_rx) = asked(
        "what does region mean",
        r#"{"reply":"it is the sales territory the row belongs to","suggestions":[]}"#,
    );
    assert_eq!(app.ai().chat().len(), 2);
    assert!(app.ai().latest_suggestions().is_empty());
    assert_eq!(app.ai().phase(), &AiPhase::Editing);
}

#[test]
fn an_unparseable_answer_surfaces_an_error_without_losing_the_thread() {
    let (app, _rx, _query_rx) = asked("only EU rows", "I cannot help with that.");
    assert!(
        matches!(app.ai().phase(), AiPhase::Error(m) if m.contains("parse")),
        "phase: {:?}",
        app.ai().phase()
    );
    assert_eq!(
        app.ai().chat().len(),
        1,
        "the question stays in the transcript"
    );
}

#[test]
fn a_stale_answer_is_discarded() {
    let (mut app, ai_rx, _query_rx) = ai_app();
    app.on_key(ctrl('a'), 0);
    type_str(&mut app, "first", 0);
    app.on_key(KeyEvent::plain(Key::Enter), 0);
    let first = ai_rx.try_recv().expect("sent");
    // Answer a superseded sequence id.
    let changed = app.on_ai_result(answer(first.seq.wrapping_sub(1), TWO_SUGGESTIONS), 0);
    assert!(!changed, "an old reply is dropped");
    assert_eq!(app.ai().phase(), &AiPhase::Pending);
}

#[test]
fn suggestions_that_fail_the_read_only_guard_are_never_offered() {
    let (app, _rx, _query_rx) = asked(
        "drop everything",
        r#"{"reply":"careful","suggestions":[
            {"type":"fix","sql":"DROP TABLE t","details":"nope"},
            {"type":"fix","sql":"SELECT 1; SELECT 2","details":"two statements"},
            {"type":"optimize","sql":"SELECT * FROM t LIMIT 5","details":"fine"}
        ]}"#,
    );
    let sqls: Vec<&str> = app
        .ai()
        .latest_suggestions()
        .iter()
        .map(|s| s.sql.as_str())
        .collect();
    assert_eq!(
        sqls,
        vec!["SELECT * FROM t LIMIT 5"],
        "only the read-only single statement survives"
    );
}

// --- picking a query runs it through the normal path ---

#[test]
fn alt_digit_applies_that_suggestion_to_the_bar() {
    let (mut app, _rx, _query_rx) = asked("only EU rows", TWO_SUGGESTIONS);
    app.on_key(alt(Key::Char('2')), 0);
    assert_eq!(app.query(), "SELECT id FROM t", "row 2 landed in the bar");
    assert!(
        app.is_ai_open(),
        "the chat stays open so the conversation continues against the new result"
    );
}

#[test]
fn alt_arrows_move_the_cursor_and_enter_applies_it() {
    let (mut app, _rx, _query_rx) = asked("only EU rows", TWO_SUGGESTIONS);
    app.on_key(alt(Key::Up), 0); // lands on the newest row
    app.on_key(alt(Key::Up), 0); // steps back to the first
    assert_eq!(app.ai().selected(), Some(0));
    app.on_key(KeyEvent::plain(Key::Enter), 0);
    assert_eq!(app.query(), "SELECT * FROM t WHERE region = 'EU'");
}

#[test]
fn enter_sends_the_typed_line_even_when_a_row_is_selected() {
    let (mut app, ai_rx, _query_rx) = asked("only EU rows", TWO_SUGGESTIONS);
    app.on_key(alt(Key::Down), 0);
    assert!(app.ai().selected().is_some());
    type_str(&mut app, "and sorted?", 0);
    app.on_key(KeyEvent::plain(Key::Enter), 0);
    assert!(
        ai_rx.try_recv().is_ok(),
        "a typed line wins over the cursor — Enter should not silently apply a row"
    );
    assert_eq!(app.query(), "", "nothing was applied");
}

#[test]
fn enter_with_no_cursor_and_no_text_does_nothing() {
    let (mut app, ai_rx, _query_rx) = asked("only EU rows", TWO_SUGGESTIONS);
    app.on_key(KeyEvent::plain(Key::Enter), 0);
    assert!(ai_rx.try_recv().is_err());
    assert_eq!(app.query(), "");
    assert!(app.is_ai_open());
}

#[test]
fn an_applied_suggestion_passes_the_read_only_guard() {
    let (mut app, _rx, _query_rx) = asked("only EU rows", TWO_SUGGESTIONS);
    app.on_key(alt(Key::Char('1')), 0);
    let sql = app.query();
    assert!(
        prepare_interactive(&sql, None).is_ok(),
        "an applied suggestion is a valid read-only query: {sql}"
    );
}

#[test]
fn an_applied_suggestion_runs_against_a_real_engine() {
    let (mut app, _rx, _query_rx) = asked(
        "two rows please",
        r#"{"reply":"here","suggestions":[{"type":"optimize","sql":"SELECT * FROM t LIMIT 2","details":"two"}]}"#,
    );
    app.on_key(alt(Key::Char('1')), 0);
    let sql = prepare_interactive(&app.query(), None).expect("valid");
    let harness = EngineHarness::from_csv("id,region\n1,EU\n2,NA\n3,APAC\n").expect("fixture");
    match harness.query(&sql) {
        QueryOutcome::Rows(table) => assert_eq!(table.row_count(), 2),
        other => panic!("expected rows, got {other:?}"),
    }
}

// --- scrolling ---

#[test]
fn arrows_scroll_the_transcript() {
    let (mut app, _rx, _query_rx) = asked("only EU rows", TWO_SUGGESTIONS);
    let start = app.ai().scroll();
    app.on_key(KeyEvent::plain(Key::Up), 0);
    assert!(app.ai().scroll() <= start);
    app.on_key(KeyEvent::plain(Key::PageDown), 0);
    assert!(app.ai().scroll() >= app.ai().scroll().min(start));
}

// --- the automatic turn on a failing query ---

#[test]
fn a_failing_query_adds_a_turn_asking_for_fixes() {
    let (mut app, ai_rx, _query_rx) = ai_app();
    app.on_key(ctrl('a'), 0);
    app.ai_report_failure("Binder Error: Referenced column \"regionn\" not found", 0);

    assert_eq!(app.ai().phase(), &AiPhase::Pending);
    assert_eq!(app.ai().chat().len(), 1);
    assert!(
        app.ai().chat().turns()[0].text.contains("regionn"),
        "the transcript shows what failed: {}",
        app.ai().chat().turns()[0].text
    );
    let job = ai_rx.try_recv().expect("a request was sent");
    assert!(
        job.turns.last().is_some_and(|t| t.1.contains("regionn")),
        "the model is told the error"
    );
}

#[test]
fn a_failing_query_pops_the_chat_up_when_it_is_hidden() {
    let (mut app, ai_rx, _query_rx) = ai_app();
    assert!(!app.is_ai_open());
    app.ai_report_failure("some error", 0);
    assert!(
        app.is_ai_open(),
        "an error is the one thing that opens the chat on its own"
    );
    assert!(
        !app.ai().is_focused(),
        "without stealing the keyboard from the bar the user is editing"
    );
    assert_eq!(app.ai().chat().len(), 1);
    assert!(ai_rx.try_recv().is_ok(), "and the fixes are requested");
}

#[test]
fn a_failing_query_stays_quiet_when_the_feature_is_off() {
    let (mut app, _rx) = loaded_app();
    app.ai_report_failure("some error", 0);
    assert!(!app.is_ai_open());
    assert!(app.ai().chat().is_empty());
}

#[test]
fn a_burst_of_failures_does_not_stack_requests() {
    let (mut app, ai_rx, _query_rx) = ai_app();
    app.on_key(ctrl('a'), 0);
    app.ai_report_failure("first error", 0);
    app.ai_report_failure("second error", 0);
    assert_eq!(app.ai().chat().len(), 1);
    assert!(ai_rx.try_recv().is_ok());
    assert!(ai_rx.try_recv().is_err(), "only one request in flight");
}

// --- the AI thread wiring (deterministic, mock, no network, no sleep) ---

#[test]
fn ai_thread_round_trips_a_canned_reply() {
    // Spawn the real AI thread over a mock provider; send a job, read the reply over the channel
    // (a blocking recv, not a sleep). Proves the thread+channel wiring end to end.
    let provider = std::sync::Arc::new(MockProvider::returning(TWO_SUGGESTIONS));
    let bridge = spawn_ai_thread(provider);

    bridge
        .request_tx
        .send(AiJob {
            system: "sys".to_string(),
            turns: vec![("user".to_string(), "how many rows".to_string())],
            seq: 1,
        })
        .expect("send job");

    let result: AiResult = bridge.result_rx.recv().expect("a reply comes back");
    assert_eq!(result.seq, 1);
    assert_eq!(result.outcome.unwrap(), TWO_SUGGESTIONS);

    // Dropping the request sender ends the loop; the thread joins cleanly.
    drop(bridge.request_tx);
    bridge.handle.join().expect("AI thread joins");
}

#[test]
fn ai_thread_round_trips_an_error() {
    let provider = std::sync::Arc::new(MockProvider::failing(AiError::Request("boom".into())));
    let bridge = spawn_ai_thread(provider);
    bridge
        .request_tx
        .send(AiJob {
            system: String::new(),
            turns: vec![("user".to_string(), "x".to_string())],
            seq: 7,
        })
        .unwrap();
    let result = bridge.result_rx.recv().unwrap();
    assert_eq!(result.seq, 7);
    assert_eq!(result.outcome.unwrap_err(), AiError::Request("boom".into()));
    drop(bridge.request_tx);
    bridge.handle.join().unwrap();
}

// --- clicking a suggestion row ---

/// Record a layout with an AI popup of `height` rows at the origin, as the renderer would, so the
/// mouse hit-test and the row map agree.
fn with_ai_popup_layout(app: &App, height: u16) {
    use crate::app::{LayoutRegions, PopupKind};
    use ratatui::layout::Rect;
    app.layout_regions.set(LayoutRegions {
        popup: Some((PopupKind::Ai, Rect::new(0, 0, 60, height))),
        ..LayoutRegions::default()
    });
}

#[test]
fn clicking_a_suggestion_row_applies_it() {
    let (mut app, _rx, _query_rx) = asked("only EU rows", TWO_SUGGESTIONS);
    // Inner rows: 0 = you, 1 = ai, 2-3 = suggestion 1 (SQL + note), 4-5 = suggestion 2. The box
    // adds a border row, so inner row 4 is screen row 5.
    with_ai_popup_layout(&app, 10);
    app.on_mouse(crate::app::MouseEvent::Click { col: 5, row: 5 }, 0);
    assert_eq!(
        app.query(),
        "SELECT id FROM t",
        "clicking the second suggestion row applies it"
    );
    assert!(app.is_ai_open(), "the chat stays open, as with Alt+<n>");
}

#[test]
fn clicking_a_prose_row_applies_nothing() {
    let (mut app, _rx, _query_rx) = asked("only EU rows", TWO_SUGGESTIONS);
    with_ai_popup_layout(&app, 10);
    // Inner row 0 is the `you:` prose row (screen row 1, inside the border).
    app.on_mouse(crate::app::MouseEvent::Click { col: 5, row: 1 }, 0);
    assert_eq!(app.query(), "", "prose is not pickable");
    assert!(app.is_ai_open());
}

// --- the failure hook's lane and staleness gate ---

use crate::query::worker::types::{QueryResponse, RequestKind};

/// Type a query and let the debouncer dispatch it, returning the request id the worker would answer.
fn dispatched(app: &mut App, sql: &str) -> u64 {
    app.force_power_mode_for_tests(sql);
    app.schedule(0);
    app.tick(150);
    app.latest_request_id()
}

fn error_resp(id: u64, kind: RequestKind) -> QueryResponse {
    QueryResponse::Error {
        message: "Binder Error: Referenced column \"bogus\" not found".into(),
        request_id: id,
        kind,
    }
}

#[test]
fn an_accepted_main_lane_error_reaches_the_chat() {
    let (mut app, ai_rx, _query_rx) = ai_app();
    app.on_key(ctrl('a'), 0);
    let id = dispatched(&mut app, "SELECT bogus FROM t");
    assert!(app.on_response_with_ai(error_resp(id, RequestKind::Main), 0));
    assert_eq!(app.ai().chat().len(), 1, "the failure became a turn");
    assert!(ai_rx.try_recv().is_ok(), "and a request went out");
}

#[test]
fn a_value_fetch_error_never_reaches_the_chat() {
    let (mut app, ai_rx, _query_rx) = ai_app();
    app.on_key(ctrl('a'), 0);
    let id = dispatched(&mut app, "SELECT * FROM t");
    let resp = error_resp(
        id,
        RequestKind::Value {
            column: "region".into(),
        },
    );
    assert!(!app.on_response_with_ai(resp, 0));
    assert!(
        app.ai().chat().is_empty(),
        "an autocomplete value fetch failing is not the user's query failing"
    );
    assert!(ai_rx.try_recv().is_err());
}

#[test]
fn a_facet_error_never_reaches_the_chat() {
    let (mut app, ai_rx, _query_rx) = ai_app();
    app.on_key(ctrl('a'), 0);
    let id = dispatched(&mut app, "SELECT * FROM t");
    let resp = error_resp(
        id,
        RequestKind::Facet {
            column: "region".into(),
        },
    );
    assert!(!app.on_response_with_ai(resp, 0));
    assert!(app.ai().chat().is_empty());
    assert!(ai_rx.try_recv().is_err());
}

#[test]
fn a_stale_main_lane_error_never_reaches_the_chat() {
    let (mut app, ai_rx, _query_rx) = ai_app();
    app.on_key(ctrl('a'), 0);
    let stale = dispatched(&mut app, "SELECT bogus FROM t");
    // A newer query supersedes it before the error for the old one arrives.
    let _fresh = dispatched(&mut app, "SELECT * FROM t");
    assert!(!app.on_response_with_ai(error_resp(stale, RequestKind::Main), 0));
    assert!(
        app.ai().chat().is_empty(),
        "an error for a query the user already moved past is not worth a request"
    );
    assert!(ai_rx.try_recv().is_err());
}

#[test]
fn a_main_lane_success_stays_quiet() {
    let (mut app, ai_rx, _query_rx) = ai_app();
    app.on_key(ctrl('a'), 0);
    let id = dispatched(&mut app, "SELECT * FROM t");
    let ok = QueryResponse::ProcessedSuccess {
        result: super::two_row_result(),
        request_id: id,
        kind: RequestKind::Main,
    };
    assert!(app.on_response_with_ai(ok, 0));
    assert!(
        app.ai().chat().is_empty(),
        "a successful query triggers nothing"
    );
    assert!(ai_rx.try_recv().is_err());
}

#[test]
fn the_chat_reports_the_recorded_error_not_the_current_status_line() {
    let (mut app, ai_rx, _query_rx) = ai_app();
    // A failing query records its error...
    let id = dispatched(&mut app, "SELECT bogus FROM t");
    app.on_response_with_ai(error_resp(id, RequestKind::Main), 0);
    // ...then the status line moves on to something unrelated.
    app.set_status("some later notice");
    app.on_key(ctrl('a'), 0);
    type_str(&mut app, "why did that fail?", 0);
    app.on_key(KeyEvent::plain(Key::Enter), 0);
    let job = ai_rx.try_recv().expect("sent");
    let msg = &job.turns.last().unwrap().1;
    assert!(
        msg.contains("bogus"),
        "the real error is in the context:\n{msg}"
    );
    assert!(
        !msg.contains("some later notice"),
        "the status line is not mistaken for the error:\n{msg}"
    );
}

#[test]
fn a_success_clears_the_recorded_error() {
    let (mut app, ai_rx, _query_rx) = ai_app();
    let id = dispatched(&mut app, "SELECT bogus FROM t");
    app.on_response_with_ai(error_resp(id, RequestKind::Main), 0);
    // The error popped the chat up and asked for fixes; let that answer land first.
    let failure = ai_rx.try_recv().expect("the failure turn was sent");
    app.on_ai_result(
        answer(failure.seq, r#"{"reply":"typo","suggestions":[]}"#),
        0,
    );
    let id = dispatched(&mut app, "SELECT * FROM t");
    app.on_response_with_ai(
        QueryResponse::ProcessedSuccess {
            result: super::two_row_result(),
            request_id: id,
            kind: RequestKind::Main,
        },
        0,
    );
    app.on_key(ctrl('g'), 0);
    type_str(&mut app, "what now?", 0);
    app.on_key(KeyEvent::plain(Key::Enter), 0);
    let job = ai_rx.try_recv().expect("sent");
    let msg = &job.turns.last().unwrap().1;
    assert!(
        !msg.contains("## Error"),
        "no stale error after a success:\n{msg}"
    );
    assert!(msg.contains("## Result"), "{msg}");
}

#[test]
fn alt_up_scrolls_an_older_answer_into_view() {
    let (mut app, ai_rx, _query_rx) = ai_app();
    app.on_key(ctrl('a'), 0);
    for i in 0..6 {
        type_str(&mut app, &format!("q{i}"), 0);
        app.on_key(KeyEvent::plain(Key::Enter), 0);
        let job = ai_rx.try_recv().expect("sent");
        let json = format!(
            r#"{{"reply":"a{i}","suggestions":[{{"type":"optimize","sql":"SELECT {i}","details":"n"}}]}}"#
        );
        app.on_ai_result(answer(job.seq, &json), 0);
    }
    // A 6-row popup: 4 inner rows, 3 of transcript once the input line takes one.
    with_ai_popup_layout(&app, 6);
    let (width, height) = app.ai_transcript_size();
    let total = crate::ai::ai_render::row_count(app.ai(), width);
    for _ in 0..6 {
        app.on_key(alt(Key::Up), 0);
    }
    let row = crate::ai::ai_render::selected_row(app.ai(), width).expect("a cursor row");
    let start = app.ai().scroll().min(total.saturating_sub(height));
    assert!(
        row >= start && row < start + height,
        "the cursor row {row} sits inside the visible window {start}..{}",
        start + height
    );
    assert!(start < total - height, "the view scrolled up from the tail");
}

// --- the request budget covers the full message ---

#[test]
fn the_history_budget_shrinks_by_the_size_of_the_new_message() {
    let (mut app, ai_rx, _query_rx) = asked("only EU rows", TWO_SUGGESTIONS);
    // A budget the two prior turns fit inside on their own, but not once the new message (which
    // carries the query text, the result context, and the reply contract) is added.
    let prior: usize = app
        .ai()
        .chat()
        .turns()
        .iter()
        .map(|t| t.wire_text().chars().count())
        .sum();
    app.configure_ai(&crate::config::AiConfig {
        max_context_length: Some((prior + 40) as u32),
        ..crate::config::AiConfig::default()
    });
    type_str(&mut app, "and sorted?", 0);
    app.on_key(KeyEvent::plain(Key::Enter), 0);
    let job = ai_rx.try_recv().expect("sent");
    assert_eq!(
        job.turns.len(),
        1,
        "the new message alone exceeds what is left, so no history rides along"
    );
    assert!(job.turns[0].1.contains("and sorted?"));
}

#[test]
fn a_generous_budget_still_sends_the_whole_thread() {
    let (mut app, ai_rx, _query_rx) = asked("only EU rows", TWO_SUGGESTIONS);
    type_str(&mut app, "and sorted?", 0);
    app.on_key(KeyEvent::plain(Key::Enter), 0);
    let job = ai_rx.try_recv().expect("sent");
    assert_eq!(job.turns.len(), 3);
}

// --- the failure turn names the query that failed ---

#[test]
fn a_failure_turn_carries_the_dispatched_sql_not_the_edited_bar() {
    let (mut app, ai_rx, _query_rx) = ai_app();
    app.on_key(ctrl('a'), 0);
    let id = dispatched(&mut app, "SELECT bogus FROM t");
    // The user keeps typing before the error for the dispatched query lands.
    app.on_key(KeyEvent::plain(Key::Esc), 0); // leave the chat so keys reach the bar
    app.force_power_mode_for_tests("SELECT * FROM t WHERE region = 'EU'");
    app.on_key(ctrl('g'), 0);
    app.on_response_with_ai(error_resp(id, RequestKind::Main), 0);
    let job = ai_rx.try_recv().expect("a failure turn was sent");
    let msg = &job.turns.last().unwrap().1;
    assert!(
        msg.contains("SELECT bogus FROM t"),
        "the SQL that failed:\n{msg}"
    );
    assert!(
        !msg.contains("region = 'EU'"),
        "not the edit that has not run yet:\n{msg}"
    );
}

// --- picking a suggestion keeps the user's query mode ---

/// The test fixture starts in Power mode; switch to the five panes the way a user would (`Ctrl+Q`
/// on a simplifiable query), with the chat blurred so the chord reaches the bar.
fn into_simple_mode(app: &mut App) {
    app.ai.blur();
    app.force_power_mode_for_tests("SELECT * FROM t");
    app.on_key(ctrl('q'), 0);
    assert_eq!(app.query_form().mode(), crate::app::QueryMode::Simple);
}

#[test]
fn applying_a_simple_query_stays_in_simple_mode() {
    let (mut app, _ai_rx, _query_rx) = asked(
        "only EU rows",
        r#"{"reply":"filter","suggestions":[{"type":"fix","sql":"SELECT region, amount FROM t WHERE region = 'EU' ORDER BY amount DESC LIMIT 20","details":"n"}]}"#,
    );
    into_simple_mode(&mut app);
    assert_eq!(app.query_form().mode(), crate::app::QueryMode::Simple);
    app.on_key(alt(Key::Char('1')), 0);
    assert_eq!(
        app.query_form().mode(),
        crate::app::QueryMode::Simple,
        "a query the five panes can express stays in the simple panes"
    );
    assert_eq!(
        app.query_form().text(crate::app::SimplePane::Where),
        "region = 'EU'"
    );
    assert_eq!(
        app.query_form().text(crate::app::SimplePane::OrderBy),
        "amount DESC"
    );
    assert!(
        !app.status().contains("SQL mode"),
        "no mode-flip notice: {}",
        app.status()
    );
}

#[test]
fn applying_a_query_the_panes_cannot_hold_falls_back_to_sql_mode() {
    let (mut app, _ai_rx, _query_rx) = asked(
        "totals",
        r#"{"reply":"cte","suggestions":[{"type":"optimize","sql":"WITH s AS (SELECT region FROM t) SELECT * FROM s","details":"n"}]}"#,
    );
    into_simple_mode(&mut app);
    app.on_key(alt(Key::Char('1')), 0);
    assert_eq!(app.query_form().mode(), crate::app::QueryMode::Power);
    assert!(app.query().starts_with("WITH s AS"));
    assert!(
        app.status().contains("SQL mode"),
        "the fallback is announced: {}",
        app.status()
    );
}

#[test]
fn applying_in_sql_mode_stays_in_sql_mode() {
    let (mut app, _ai_rx, _query_rx) = asked("only EU rows", TWO_SUGGESTIONS);
    app.force_power_mode_for_tests("SELECT 1");
    app.on_key(alt(Key::Char('2')), 0);
    assert_eq!(app.query_form().mode(), crate::app::QueryMode::Power);
    assert_eq!(app.query(), "SELECT id FROM t");
}

// Focus, click, and open-on-start tests live in a sibling file to keep this one under the
// 1000-line cap; they share the fixtures above through `super`.
#[path = "ai_focus_tests.rs"]
mod ai_focus_tests;
