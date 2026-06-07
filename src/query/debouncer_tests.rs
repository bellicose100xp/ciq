//! Tests for the debouncer — boundary behavior driven by explicit `u64` timestamps.

use crate::query::debouncer::{Debouncer, TEST_DEBOUNCE_MS};

#[test]
fn nothing_pending_never_fires() {
    let d = Debouncer::new();
    assert!(!d.has_pending());
    assert!(!d.should_execute_at(0));
    assert!(!d.should_execute_at(10_000));
}

#[test]
fn fires_exactly_at_window_boundary() {
    let mut d = Debouncer::new();
    d.schedule_execution_at(1000);
    assert!(d.has_pending());
    // before the window: no
    assert!(!d.should_execute_at(1000));
    assert!(!d.should_execute_at(1000 + TEST_DEBOUNCE_MS - 1));
    // exactly at the window: yes (>=)
    assert!(d.should_execute_at(1000 + TEST_DEBOUNCE_MS));
    // past the window: still yes
    assert!(d.should_execute_at(1000 + TEST_DEBOUNCE_MS + 500));
}

#[test]
fn rapid_keystrokes_coalesce_to_the_latest() {
    let mut d = Debouncer::new();
    d.schedule_execution_at(1000);
    d.schedule_execution_at(1050); // newer keystroke pushes the fire time out
    d.schedule_execution_at(1100);
    // window is measured from the LATEST schedule (1100), not the first
    assert!(!d.should_execute_at(1000 + TEST_DEBOUNCE_MS)); // 1150 < 1100+150=1250
    assert!(!d.should_execute_at(1100 + TEST_DEBOUNCE_MS - 1));
    assert!(d.should_execute_at(1100 + TEST_DEBOUNCE_MS));
}

#[test]
fn mark_executed_clears_pending() {
    let mut d = Debouncer::new();
    d.schedule_execution_at(1000);
    assert!(d.should_execute_at(1200));
    d.mark_executed();
    assert!(!d.has_pending());
    assert!(!d.should_execute_at(1200)); // nothing pending after execution
}

#[test]
fn reschedule_after_execution_works() {
    let mut d = Debouncer::new();
    d.schedule_execution_at(1000);
    d.mark_executed();
    d.schedule_execution_at(2000);
    assert!(d.has_pending());
    assert!(!d.should_execute_at(2000 + TEST_DEBOUNCE_MS - 1));
    assert!(d.should_execute_at(2000 + TEST_DEBOUNCE_MS));
}
