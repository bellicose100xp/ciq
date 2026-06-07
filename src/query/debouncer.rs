//! Debouncer — coalesces rapid keystrokes into one query per quiet window.
//!
//! Reused ~verbatim from jiq (`dev/PLAN.md` §3.2): **time-as-`u64`-parameter**, not a `Clock`
//! trait. Logic methods take `current_time_ms: u64` so tests (and the `AppHarness`) drive time
//! deterministically by passing synthetic values; the convenience methods read a process-start
//! monotonic clock via the `system_time_ms()` seam.
//!
//! Determinism: `system_time_ms()` is the one wall-clock read here, confined behind the
//! documented `clippy.toml` seam allow (like `logging::Timer`). All *logic* uses the `_at(u64)`
//! variants — no ambient time enters the tested decision.

/// The fixed debounce window: a query fires once typing has been quiet this long.
const DEBOUNCE_MS: u64 = 150;

#[cfg(test)]
pub const TEST_DEBOUNCE_MS: u64 = DEBOUNCE_MS;

/// Milliseconds since process start (monotonic). Wall-clock seam — see module docs.
#[allow(clippy::disallowed_methods)]
fn system_time_ms() -> u64 {
    use std::sync::OnceLock;
    use std::time::Instant;
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_millis() as u64
}

/// Tracks whether a query is pending and when it was last scheduled, so `should_execute_at`
/// can decide if the quiet window has elapsed.
#[derive(Debug, Default)]
pub struct Debouncer {
    scheduled_at_ms: Option<u64>,
    pending_execution: bool,
}

impl Debouncer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Schedule using the ambient clock (production convenience).
    pub fn schedule_execution(&mut self) {
        self.schedule_execution_at(system_time_ms());
    }

    /// Schedule at an explicit time (deterministic; used by tests and the harness). Each new
    /// keystroke calls this, pushing the fire time `DEBOUNCE_MS` past the latest input.
    pub fn schedule_execution_at(&mut self, current_time_ms: u64) {
        self.scheduled_at_ms = Some(current_time_ms);
        self.pending_execution = true;
    }

    /// Whether a query should fire now, using the ambient clock (production convenience).
    pub fn should_execute(&self) -> bool {
        self.should_execute_at(system_time_ms())
    }

    /// Whether a query should fire at `current_time_ms`: only if one is pending and the quiet
    /// window has fully elapsed since the last schedule. Pure decision over the given time.
    pub fn should_execute_at(&self, current_time_ms: u64) -> bool {
        if !self.pending_execution {
            return false;
        }
        match self.scheduled_at_ms {
            // saturating_add: never panics/wraps near u64::MAX (debug overflow-check safe).
            Some(scheduled) => current_time_ms >= scheduled.saturating_add(DEBOUNCE_MS),
            None => false,
        }
    }

    /// Clear the pending state after a query has been dispatched.
    pub fn mark_executed(&mut self) {
        self.pending_execution = false;
        self.scheduled_at_ms = None;
    }

    /// Whether a query is waiting to fire.
    pub fn has_pending(&self) -> bool {
        self.pending_execution
    }
}

#[cfg(test)]
#[path = "debouncer_tests.rs"]
mod debouncer_tests;
