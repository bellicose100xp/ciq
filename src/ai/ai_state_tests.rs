//! Tests for the AI popup state machine — pure transitions, plain asserts (no terminal/engine).

use super::*;

#[test]
fn new_is_closed_and_editing() {
    let s = AiState::new();
    assert!(!s.is_open());
    assert_eq!(s.phase(), &AiPhase::Editing);
    assert_eq!(s.input(), "");
}

#[test]
fn open_starts_clean() {
    let mut s = AiState::new();
    s.open();
    assert!(s.is_open());
    assert_eq!(s.phase(), &AiPhase::Editing);
    assert_eq!(s.input(), "");
}

#[test]
fn typing_builds_the_input() {
    let mut s = AiState::new();
    s.open();
    for c in "rows in EU".chars() {
        s.push_char(c);
    }
    assert_eq!(s.input(), "rows in EU");
}

#[test]
fn backspace_pops_a_char() {
    let mut s = AiState::new();
    s.open();
    s.push_char('a');
    s.push_char('b');
    s.backspace();
    assert_eq!(s.input(), "a");
}

#[test]
fn backspace_is_utf8_safe() {
    let mut s = AiState::new();
    s.open();
    s.push_char('é'); // multibyte char
    s.push_char('x');
    s.backspace();
    s.backspace();
    assert_eq!(s.input(), "", "popped whole chars, never a partial byte");
}

#[test]
fn submit_returns_trimmed_prompt_and_goes_pending() {
    let mut s = AiState::new();
    s.open();
    for c in "  count rows  ".chars() {
        s.push_char(c);
    }
    let prompt = s.submit().expect("non-blank prompt submits");
    assert_eq!(prompt, "count rows");
    assert!(s.is_pending());
    assert_eq!(s.phase(), &AiPhase::Pending);
}

#[test]
fn submit_blank_is_a_no_op() {
    let mut s = AiState::new();
    s.open();
    s.push_char(' ');
    assert!(s.submit().is_none(), "an empty prompt is not submittable");
    assert_eq!(s.phase(), &AiPhase::Editing);
}

#[test]
fn cannot_submit_twice_while_pending() {
    let mut s = AiState::new();
    s.open();
    s.push_char('x');
    assert!(s.submit().is_some());
    // A second submit while Pending is a no-op (no overlapping requests).
    assert!(s.submit().is_none());
}

#[test]
fn typing_while_pending_is_ignored() {
    let mut s = AiState::new();
    s.open();
    s.push_char('a');
    s.submit();
    s.push_char('b'); // ignored: not Editing
    assert_eq!(s.input(), "a");
}

#[test]
fn success_carries_the_generated_sql() {
    let mut s = AiState::new();
    s.open();
    s.push_char('x');
    s.submit();
    s.set_success("SELECT * FROM t");
    assert_eq!(s.phase(), &AiPhase::Success("SELECT * FROM t".to_string()));
    assert!(!s.is_pending());
}

#[test]
fn error_surfaces_the_message_and_preserves_input() {
    let mut s = AiState::new();
    s.open();
    for c in "bad request".chars() {
        s.push_char(c);
    }
    s.submit();
    s.set_error("network down");
    assert_eq!(s.phase(), &AiPhase::Error("network down".to_string()));
    assert_eq!(s.input(), "bad request", "input preserved for a retry");
}

#[test]
fn resume_editing_after_error_lets_typing_continue() {
    let mut s = AiState::new();
    s.open();
    s.push_char('a');
    s.submit();
    s.set_error("boom");
    s.resume_editing();
    assert_eq!(s.phase(), &AiPhase::Editing);
    s.push_char('b'); // now mutates again
    assert_eq!(s.input(), "ab");
}

#[test]
fn resume_editing_is_a_no_op_unless_error() {
    let mut s = AiState::new();
    s.open();
    s.push_char('a');
    s.submit(); // Pending
    s.resume_editing(); // no-op: not Error
    assert_eq!(s.phase(), &AiPhase::Pending);
}

#[test]
fn close_clears_everything() {
    let mut s = AiState::new();
    s.open();
    s.push_char('x');
    s.submit();
    s.set_success("SELECT 1");
    s.close();
    assert!(!s.is_open());
    assert_eq!(s.input(), "");
    assert_eq!(s.phase(), &AiPhase::Editing);
}
