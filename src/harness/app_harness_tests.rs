//! Tests for `AppHarness` — the headless render seam (P1.7), including a no-TTY self-test.

use crate::app::App;
use crate::harness::app_harness::AppHarness;

#[test]
fn renders_loading_frame_to_buffer() {
    let mut h = AppHarness::loading(40, 6);
    let screen = h.screen();
    // The in-memory buffer contains the placeholder body and status text.
    assert!(screen.contains("loading"), "screen was:\n{screen}");
    // A bordered body box is drawn (border glyphs present).
    assert!(
        screen.contains('┌') || screen.contains('│'),
        "expected a border, screen was:\n{screen}"
    );
}

#[test]
fn renders_ready_frame_after_state_change() {
    let mut app = App::new();
    app.set_ready("ready: t loaded");
    let mut h = AppHarness::new(app, 40, 6);
    let screen = h.screen();
    assert!(screen.contains("ready"), "screen was:\n{screen}");
}

#[test]
fn render_is_deterministic() {
    // Same state -> byte-identical buffer (prerequisite for insta snapshots).
    let mut a = AppHarness::loading(40, 6);
    let mut b = AppHarness::loading(40, 6);
    assert_eq!(a.screen(), b.screen());
}

#[test]
fn advance_moves_synthetic_clock_only() {
    let mut h = AppHarness::loading(20, 4);
    assert_eq!(h.now_ms(), 0);
    h.advance(150);
    assert_eq!(h.now_ms(), 150);
    // advancing time does not change what renders in P1.7 (no debouncer yet)
    let before = AppHarness::loading(20, 4).screen();
    assert_eq!(h.screen(), before);
}

/// P1.7 exit criterion: the App render path runs with no controlling terminal. `TestBackend`
/// is purely in-memory, so rendering must succeed with `TERM` unset.
#[test]
fn renders_with_term_unset() {
    let prev = std::env::var_os("TERM");
    // SAFETY: tests run single-threaded (`--test-threads=1`); no other thread sees this.
    unsafe {
        std::env::remove_var("TERM");
    }

    let mut h = AppHarness::loading(30, 5);
    let screen = h.screen();
    let ok = screen.contains("loading");

    if let Some(v) = prev {
        unsafe {
            std::env::set_var("TERM", v);
        }
    }
    assert!(ok, "render must work with no controlling terminal");
}
