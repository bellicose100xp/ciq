//! `App`-shell + end-to-end tests for AI NL->SQL (P5.1): the `Ctrl+G` chord opens the popup, the
//! popup's key routing (type / submit / close / quit), a generated SQL reply dropping into the bar
//! and firing through the **normal** preprocess-validate + dispatch path, the read-only guard
//! rejecting a DML reply (the model can't smuggle a mutation), the canned SQL validated against a
//! real fixture `DuckdbEngine` yielding `Rows` with the expected count, and the AI thread wiring
//! exercised deterministically with the mock (no network, no sleep).
//!
//! Split out of `app_tests.rs` to keep each file under the 1000-line limit; the shared App helpers
//! live in the parent (`super`).

use std::sync::mpsc::channel;

use crate::ai::ai_app::{AiJob, AiResult, spawn_ai_thread};
use crate::ai::ai_state::AiPhase;
use crate::ai::provider::{AiError, MockProvider};
use crate::app::{App, Key, KeyEvent, KeyMods, VIEWPORT_ROW_LIMIT};
use crate::engine::QueryOutcome;
use crate::harness::engine_harness::EngineHarness;
use crate::query::preprocess::prepare_interactive;

use super::{loaded_app, type_str};

fn ctrl(c: char) -> KeyEvent {
    KeyEvent::new(Key::Char(c), KeyMods::CTRL)
}

/// A loaded App with an AI request channel wired (the test keeps the receiver so it can read the
/// built prompt). Mirrors how the event loop wires `set_ai_channel`, but synchronous (no thread) so
/// the App-routing tests are deterministic.
fn ai_app() -> (App, std::sync::mpsc::Receiver<AiJob>) {
    let (mut app, _rx) = loaded_app();
    let (ai_tx, ai_rx) = channel::<AiJob>();
    app.set_ai_channel(ai_tx);
    (app, ai_rx)
}

// --- the chord + popup gating ---

#[test]
fn ctrl_g_opens_the_ai_popup() {
    let (mut app, _ai_rx) = ai_app();
    assert!(!app.is_ai_open());
    app.on_key(ctrl('g'), 0);
    assert!(app.is_ai_open());
    assert_eq!(app.ai().phase(), &AiPhase::Editing);
}

#[test]
fn ctrl_g_is_a_no_op_without_a_provider() {
    let (mut app, _rx) = loaded_app(); // no set_ai_channel -> feature unwired
    assert!(!app.ai_enabled());
    app.on_key(ctrl('g'), 0);
    assert!(!app.is_ai_open(), "no provider => the chord does nothing");
}

#[test]
fn typing_builds_the_prompt_in_the_popup() {
    let (mut app, _ai_rx) = ai_app();
    app.on_key(ctrl('g'), 0);
    type_str(&mut app, "rows in EU", 0);
    assert_eq!(app.ai().input(), "rows in EU");
    // The query bar is untouched while the AI popup is open (typing fills the popup, not the bar).
    assert_eq!(app.query(), "", "typing fills the popup, not the query bar");
}

#[test]
fn esc_closes_the_popup_without_quitting() {
    let (mut app, _ai_rx) = ai_app();
    app.on_key(ctrl('g'), 0);
    let quit = app.on_key(KeyEvent::plain(Key::Esc), 0);
    assert!(!quit, "Esc closes the popup, it does not quit");
    assert!(!app.is_ai_open());
}

#[test]
fn ctrl_c_quits_from_the_popup() {
    let (mut app, _ai_rx) = ai_app();
    app.on_key(ctrl('g'), 0);
    assert!(app.on_key(ctrl('c'), 0), "Ctrl-C quits even from the popup");
}

// --- submit sends a schema-grounded prompt over the channel ---

#[test]
fn enter_submits_a_schema_grounded_prompt() {
    let (mut app, ai_rx) = ai_app();
    app.on_key(ctrl('g'), 0);
    type_str(&mut app, "count rows by status", 0);
    app.on_key(KeyEvent::plain(Key::Enter), 0);

    assert_eq!(app.ai().phase(), &AiPhase::Pending, "submit -> Pending");
    let job = ai_rx.try_recv().expect("a job was sent to the AI thread");
    // The prompt is schema-grounded (table + columns) and carries the NL request.
    assert!(
        job.prompt.contains("`t`"),
        "prompt names the table:\n{}",
        job.prompt
    );
    assert!(
        job.prompt.contains("status"),
        "prompt embeds a column:\n{}",
        job.prompt
    );
    assert!(
        job.prompt.contains("count rows by status"),
        "prompt carries the request:\n{}",
        job.prompt
    );
}

#[test]
fn empty_prompt_does_not_submit() {
    let (mut app, ai_rx) = ai_app();
    app.on_key(ctrl('g'), 0);
    app.on_key(KeyEvent::plain(Key::Enter), 0); // empty
    assert_eq!(app.ai().phase(), &AiPhase::Editing);
    assert!(ai_rx.try_recv().is_err(), "no job sent for an empty prompt");
}

// --- the generated SQL flows through the normal path ---

#[test]
fn generated_sql_drops_into_bar_and_dispatches() {
    let (mut app, ai_rx) = ai_app();
    app.on_key(ctrl('g'), 0);
    type_str(&mut app, "everything", 0);
    app.on_key(KeyEvent::plain(Key::Enter), 0);
    let job = ai_rx.try_recv().expect("job sent");

    // The AI thread (mocked) returns canned SQL; deliver it via on_ai_result.
    let result = AiResult {
        seq: job.seq,
        outcome: Ok("SELECT * FROM t WHERE status = 'active'".to_string()),
    };
    let changed = app.on_ai_result(result, 0);
    assert!(changed);
    assert!(
        !app.is_ai_open(),
        "popup closes after a successful generate"
    );
    assert_eq!(app.query(), "SELECT * FROM t WHERE status = 'active'");

    // The generated SQL passes the read-only single-statement guard (the same one a typed query
    // hits) and LIMIT-wraps — so firing the debounce would dispatch it through the normal path.
    let wrapped = prepare_interactive(&app.query(), VIEWPORT_ROW_LIMIT).expect("valid SELECT");
    assert!(wrapped.contains("status = 'active'"));
    assert!(wrapped.contains("LIMIT"), "viewport-wrapped: {wrapped}");
}

#[test]
fn fenced_select_reply_lands_as_runnable_sql() {
    // A very common model habit: wrap the SQL in a ```sql … ``` fence despite the prompt's
    // no-fences rule. Without unwrapping, the leading backtick/`sql` token makes preprocess reject
    // it as "read-only SELECT queries only"; the fence-strip must let the good SELECT through.
    let (mut app, ai_rx) = ai_app();
    app.on_key(ctrl('g'), 0);
    type_str(&mut app, "rows in EU", 0);
    app.on_key(KeyEvent::plain(Key::Enter), 0);
    let job = ai_rx.try_recv().expect("job sent");

    let result = AiResult {
        seq: job.seq,
        outcome: Ok("```sql\nSELECT * FROM t WHERE region = 'EU'\n```".to_string()),
    };
    app.on_ai_result(result, 0);

    // The fence noise is gone — the bar holds clean SQL that passes the read-only guard.
    assert_eq!(app.query(), "SELECT * FROM t WHERE region = 'EU'");
    let wrapped = prepare_interactive(&app.query(), VIEWPORT_ROW_LIMIT)
        .expect("a fenced SELECT must land as a runnable read-only query");
    assert!(wrapped.contains("region = 'EU'"));
}

#[test]
fn dml_reply_is_rejected_by_the_read_only_guard() {
    let (mut app, ai_rx) = ai_app();
    app.on_key(ctrl('g'), 0);
    type_str(&mut app, "drop the table", 0);
    app.on_key(KeyEvent::plain(Key::Enter), 0);
    let job = ai_rx.try_recv().expect("job sent");

    // A malicious/confused model reply: a DML statement.
    let result = AiResult {
        seq: job.seq,
        outcome: Ok("DROP TABLE t".to_string()),
    };
    app.on_ai_result(result, 0);
    // It landed in the bar (the AI layer is not the security boundary) but the existing preprocess
    // guard rejects it — it never reaches the engine.
    assert_eq!(app.query(), "DROP TABLE t");
    assert!(
        prepare_interactive(&app.query(), VIEWPORT_ROW_LIMIT).is_err(),
        "the read-only guard rejects DROP — the model cannot smuggle DML"
    );
    // Firing the debounce surfaces the rejection as a status-line error, not an engine call.
    app.tick(1000);
    assert!(
        app.status().to_lowercase().contains("read-only"),
        "status: {}",
        app.status()
    );
}

#[test]
fn multi_statement_reply_is_rejected() {
    let (mut app, ai_rx) = ai_app();
    app.on_key(ctrl('g'), 0);
    type_str(&mut app, "two things", 0);
    app.on_key(KeyEvent::plain(Key::Enter), 0);
    let job = ai_rx.try_recv().unwrap();
    app.on_ai_result(
        AiResult {
            seq: job.seq,
            outcome: Ok("SELECT 1; DROP TABLE t".to_string()),
        },
        0,
    );
    assert!(
        prepare_interactive(&app.query(), VIEWPORT_ROW_LIMIT).is_err(),
        "a multi-statement reply is rejected before any engine call"
    );
}

#[test]
fn error_reply_surfaces_in_the_popup() {
    let (mut app, ai_rx) = ai_app();
    app.on_key(ctrl('g'), 0);
    type_str(&mut app, "anything", 0);
    app.on_key(KeyEvent::plain(Key::Enter), 0);
    let job = ai_rx.try_recv().unwrap();
    app.on_ai_result(
        AiResult {
            seq: job.seq,
            outcome: Err(AiError::Request("network down".into())),
        },
        0,
    );
    assert!(app.is_ai_open(), "popup stays open to show the error");
    match app.ai().phase() {
        AiPhase::Error(msg) => assert!(msg.contains("network down"), "got: {msg}"),
        other => panic!("expected Error phase, got {other:?}"),
    }
}

#[test]
fn stale_ai_result_is_discarded() {
    let (mut app, ai_rx) = ai_app();
    app.on_key(ctrl('g'), 0);
    type_str(&mut app, "first", 0);
    app.on_key(KeyEvent::plain(Key::Enter), 0);
    let _first = ai_rx.try_recv().unwrap();
    // A reply for an old seq (e.g. seq 0, before the first submit bumped it) is dropped.
    let changed = app.on_ai_result(
        AiResult {
            seq: 0,
            outcome: Ok("SELECT 1".to_string()),
        },
        0,
    );
    assert!(!changed, "a stale (old seq) reply is discarded");
    assert_eq!(
        app.ai().phase(),
        &AiPhase::Pending,
        "still awaiting the live reply"
    );
}

// --- the canned SQL validates against a REAL fixture engine ---

#[test]
fn canned_sql_validates_and_runs_on_a_fixture_engine() {
    // The mock returns a canned SELECT; assert it parses, passes the read-only guard, and (run on
    // a real DuckdbEngine over a fixture) yields Rows with the expected count.
    let provider = MockProvider::returning("SELECT * FROM t WHERE region = 'EU'");
    let prompt = "rows in europe";

    // (1) The provider produces SQL deterministically (no network).
    use crate::ai::provider::Provider;
    let sql = provider.complete(prompt).unwrap();
    assert_eq!(provider.call_count(), 1);

    // (2) It passes the read-only single-statement guard (the gate every query crosses).
    let validated = prepare_interactive(&sql, VIEWPORT_ROW_LIMIT).expect("read-only SELECT");

    // (3) Run it on a real fixture engine -> Rows with the expected count (2 EU rows).
    let h = EngineHarness::from_csv("id,region\n1,EU\n2,NA\n3,EU\n").expect("load fixture");
    match h.query(&validated) {
        QueryOutcome::Rows(t) => assert_eq!(t.row_count(), 2, "two EU rows"),
        other => panic!("expected Rows, got {other:?}"),
    }
}

// --- the AI thread wiring (deterministic, mock, no network, no sleep) ---

#[test]
fn ai_thread_round_trips_a_canned_reply() {
    // Spawn the real AI thread over a mock provider; send a job, read the reply over the channel
    // (a blocking recv, not a sleep). Proves the thread+channel wiring end to end.
    let provider = Box::new(MockProvider::returning("SELECT count(*) FROM t"));
    let bridge = spawn_ai_thread(provider);

    bridge
        .request_tx
        .send(AiJob {
            prompt: "how many rows".to_string(),
            seq: 1,
        })
        .expect("send job");

    let result: AiResult = bridge.result_rx.recv().expect("a reply comes back");
    assert_eq!(result.seq, 1);
    assert_eq!(result.outcome.unwrap(), "SELECT count(*) FROM t");

    // Dropping the request sender ends the loop; the thread joins cleanly.
    drop(bridge.request_tx);
    bridge.handle.join().expect("AI thread joins");
}

#[test]
fn ai_thread_round_trips_an_error() {
    let provider = Box::new(MockProvider::failing(AiError::Request("boom".into())));
    let bridge = spawn_ai_thread(provider);
    bridge
        .request_tx
        .send(AiJob {
            prompt: "x".to_string(),
            seq: 7,
        })
        .unwrap();
    let result = bridge.result_rx.recv().unwrap();
    assert_eq!(result.seq, 7);
    assert_eq!(result.outcome.unwrap_err(), AiError::Request("boom".into()));
    drop(bridge.request_tx);
    bridge.handle.join().unwrap();
}
