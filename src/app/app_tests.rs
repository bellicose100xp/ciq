//! Tests for the minimal P1.7 `App` (state + structured assertions). Render-to-buffer
//! assertions live in `harness/app_harness_tests.rs`.

use crate::app::{App, AppPhase};

#[test]
fn default_app_is_loading() {
    let app = App::new();
    assert_eq!(app.phase(), &AppPhase::Loading);
    assert_eq!(app.status(), "loading…");
}

#[test]
fn set_ready_transitions_phase_and_status() {
    let mut app = App::new();
    app.set_ready("3 rows");
    assert_eq!(app.phase(), &AppPhase::Ready);
    assert_eq!(app.status(), "3 rows");
}
