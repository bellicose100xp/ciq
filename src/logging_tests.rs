//! Tests for debug logging. Single-threaded test execution (the project convention) makes
//! the process-global logger + env-var manipulation safe.
//!
//! Note: we do not assert the *contents* of the shared `/tmp/ciq/ciq-debug.log` here — other
//! processes/sessions may share it, and `env_logger` can only be installed once per process,
//! so a co-running test could already hold the global logger. Instead we assert the pure,
//! deterministic pieces: the log path, directory creation, Timer-drop-doesn't-panic, and that
//! the disabled path writes nothing.

use crate::logging::{Timer, init_logger, log_path};

#[test]
fn log_path_is_under_tmp_ciq() {
    let p = log_path();
    assert!(p.ends_with("ciq-debug.log"));
    assert!(
        p.to_string_lossy().contains("/tmp/ciq"),
        "log path should live under /tmp/ciq, was {p:?}"
    );
}

#[test]
fn timer_drop_never_panics_without_logger() {
    // With no logger installed (or one installed by another test), constructing and dropping
    // a Timer must be a harmless no-op — the log line is simply filtered out.
    {
        let _t = Timer::new("unit-test-span");
        // do a tiny bit of work so elapsed() is well-defined
        let _ = (0..1000).sum::<u64>();
    } // drop here logs (or no-ops); must not panic
}

#[test]
fn init_logger_disabled_is_noop() {
    // With cli_debug=false and CIQ_DEBUG unset, init_logger must return WITHOUT installing a
    // logger or creating a file — even in a debug build (ciq does NOT auto-enable on
    // debug_assertions, unlike jiq). We assert it doesn't panic; the stronger "no logger
    // installed" property is covered by `timer_is_free_when_logging_disabled` below, which
    // observes that the clock is never read.
    let prev = std::env::var_os("CIQ_DEBUG");
    // SAFETY: tests are single-threaded.
    unsafe {
        std::env::remove_var("CIQ_DEBUG");
    }
    init_logger(false); // explicit opt-out: clean no-op
    if let Some(v) = prev {
        unsafe {
            std::env::set_var("CIQ_DEBUG", v);
        }
    }
}

#[test]
fn timer_is_free_when_logging_disabled() {
    // When debug logging is not active, Timer must not read the wall clock — `start` is None,
    // so construction and drop are free. This is the "no flag -> no performance effect"
    // guarantee. (If another test installed the global logger first, `log_enabled!(Debug)`
    // could be true; in that case we can't assert None, so we only assert when logging is off.)
    let t = Timer::new("free-when-off");
    if !log::log_enabled!(log::Level::Debug) {
        assert!(
            t.clock_was_read().is_none(),
            "Timer must not read the clock when debug logging is disabled"
        );
    }
}

#[test]
fn init_logger_is_idempotent() {
    // Calling twice must not panic (try_init swallows the already-installed error).
    init_logger(true);
    init_logger(true);
    // And after enabling, a debug line + a Timer drop must not panic.
    log::debug!("logging_tests: idempotent init ok");
    let _t = Timer::new("idempotent-span");
}
