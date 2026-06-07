//! Debug logging (`--debug`).
//!
//! Mirrors jiq's `log` + `env_logger` + RAII `Timer` pattern, with two ciq-specific changes:
//! output goes to a **`/tmp/ciq/` folder** (created on demand), to a **file only** (never
//! stdout/stderr, which would corrupt the TUI); and logging activates **only on explicit
//! `--debug` / `CIQ_DEBUG=1`** — *not* in debug builds (jiq auto-enables on `debug_assertions`;
//! ciq does not). When the logger isn't initialised — the default for any run without the flag —
//! every `log::debug!` is a no-op (disabled-level check only) and `Timer` never reads the clock.
//! So "no flag → no effect on performance" is literal, not approximate.
//!
//! ## Determinism seam (dev/PLAN.md §0/D5, `clippy.toml`)
//!
//! This module is the **single sanctioned place** in the crate that may call the wall clock
//! (`Instant::now` / `SystemTime::now`). The `disallowed_methods` lint bans them everywhere
//! else so that *logic* stays deterministic (time enters logic as a `u64` parameter). Debug
//! logging is diagnostic, off the tested-logic path, and inherently about real elapsed time —
//! so the calls here are `#[allow(clippy::disallowed_methods)]` with this rationale. Do not
//! copy that allow into logic modules.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;

/// The on-demand log directory and file under it.
const LOG_DIR: &str = "/tmp/ciq";
const LOG_FILE: &str = "ciq-debug.log";

/// Full path to the debug log file (`/tmp/ciq/ciq-debug.log`).
pub fn log_path() -> PathBuf {
    PathBuf::from(LOG_DIR).join(LOG_FILE)
}

/// Initialise debug logging if, and only if, **explicitly requested**.
///
/// Enabled by the `--debug` flag (`cli_debug`) **or** the `CIQ_DEBUG=1` env var — nothing else.
/// Crucially it is **NOT** auto-enabled in debug builds: without the flag/env, this returns
/// before installing any logger or creating any file, so logging has **zero effect** on a
/// normal run (no logger means every `log::debug!` is a disabled-level no-op, and `Timer`
/// never reads the clock — see [`Timer`]). This is a deliberate divergence from jiq, which
/// also activates on `debug_assertions`; ciq requires an explicit opt-in.
///
/// When enabled, creates `/tmp/ciq/` if missing and appends timestamped lines to
/// `ciq-debug.log`. Failures to open the file are swallowed (debug logging must never take down
/// the app). Idempotent: a second call is a harmless no-op (`env_logger` is already installed).
pub fn init_logger(cli_debug: bool) {
    let env_debug = std::env::var("CIQ_DEBUG").is_ok_and(|v| v == "1");
    if !(cli_debug || env_debug) {
        return;
    }

    // Create /tmp/ciq/ on demand; bail silently if we can't.
    if std::fs::create_dir_all(LOG_DIR).is_err() {
        return;
    }
    let file = match OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path())
    {
        Ok(f) => f,
        Err(_) => return,
    };

    // `try_init` so a double-init (e.g. in a test) is a harmless error, not a panic.
    let _ = env_logger::Builder::new()
        .filter_level(log::LevelFilter::Debug)
        .target(env_logger::Target::Pipe(Box::new(file)))
        .format(|buf, record| {
            writeln!(
                buf,
                "[{}] [{}] {}",
                now_millis(),
                record.level(),
                record.args()
            )
        })
        .try_init();
}

/// Milliseconds since the Unix epoch, for log line timestamps.
///
/// Wall-clock seam (see module docs). Not used by any tested logic.
#[allow(clippy::disallowed_methods)]
fn now_millis() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

/// RAII timer that logs `[TIMING] {label} took {ms}ms` when dropped — **only when debug logging
/// is active**. It is safe to sprinkle on the per-keystroke hot path: when logging is off
/// (the default, no `--debug`/`CIQ_DEBUG`), `Timer::new` does **not** read the clock and drop
/// does nothing, so the cost is a single `log_enabled!` check (a relaxed atomic load) and a
/// `None` field — effectively zero. This is what makes "no flag → no effect on performance"
/// literally true, not just approximately.
///
/// ```ignore
/// let _t = ciq::logging::Timer::new("load");
/// // ... work ...
/// // on drop: "[TIMING] load took 1240ms" (only when --debug; nothing otherwise)
/// ```
pub struct Timer {
    label: &'static str,
    /// `Some` only when debug logging was active at construction; `None` makes drop a no-op
    /// and means the wall clock was never read.
    start: Option<std::time::Instant>,
}

impl Timer {
    /// Start a timer. Reads the clock **only if** debug logging is enabled; otherwise it's a
    /// no-op shell that does nothing on drop.
    ///
    /// Wall-clock seam (see module docs) — gated behind `log_enabled!`.
    #[allow(clippy::disallowed_methods)]
    pub fn new(label: &'static str) -> Self {
        let start = if log::log_enabled!(log::Level::Debug) {
            Some(std::time::Instant::now())
        } else {
            None
        };
        Self { label, start }
    }

    /// Whether the timer read the wall clock (i.e. debug logging was active at construction).
    /// `None` means it's a free no-op shell. Exposed for tests asserting the zero-cost path.
    pub fn clock_was_read(&self) -> Option<()> {
        self.start.map(|_| ())
    }
}

impl Drop for Timer {
    fn drop(&mut self) {
        if let Some(start) = self.start {
            log::debug!(
                "[TIMING] {} took {}ms",
                self.label,
                start.elapsed().as_millis()
            );
        }
    }
}

#[cfg(test)]
#[path = "logging_tests.rs"]
mod logging_tests;
