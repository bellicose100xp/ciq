//! Tests for `query_state` — exhaustive boundary checks on the pure stale-discard decision.

use crate::query::query_state::{QueryState, is_stale};

#[test]
fn is_stale_boundaries() {
    assert!(is_stale(0, 1)); // older
    assert!(is_stale(4, 5));
    assert!(!is_stale(5, 5)); // current — not stale
    assert!(!is_stale(6, 5)); // newer than latest (shouldn't happen, but not "stale")
    assert!(!is_stale(0, 0));
    assert!(is_stale(u64::MIN, u64::MAX));
    assert!(!is_stale(u64::MAX, u64::MAX));
}

#[test]
fn issue_increments_monotonically() {
    let mut s = QueryState::new();
    assert_eq!(s.latest_id(), 0);
    assert!(!s.in_flight());
    assert_eq!(s.issue(), 1);
    assert_eq!(s.issue(), 2);
    assert_eq!(s.issue(), 3);
    assert_eq!(s.latest_id(), 3);
    assert!(s.in_flight());
}

#[test]
fn accept_latest_clears_in_flight() {
    let mut s = QueryState::new();
    let id = s.issue();
    assert!(s.in_flight());
    assert!(s.accept(id));
    assert!(!s.in_flight());
}

#[test]
fn reject_stale_response_keeps_state() {
    let mut s = QueryState::new();
    let id1 = s.issue(); // 1
    let id2 = s.issue(); // 2 supersedes 1
    // the late response for id1 is stale and rejected
    assert!(!s.accept(id1));
    assert!(s.in_flight()); // still waiting for id2
    // id2's response is accepted
    assert!(s.accept(id2));
    assert!(!s.in_flight());
}

#[test]
fn many_rapid_issues_only_latest_accepted() {
    let mut s = QueryState::new();
    let mut last = 0;
    for _ in 0..100 {
        last = s.issue();
    }
    // every id before the last is stale
    for stale in 1..last {
        assert!(!s.accept(stale), "id {stale} should be stale vs {last}");
    }
    assert!(s.accept(last));
}
