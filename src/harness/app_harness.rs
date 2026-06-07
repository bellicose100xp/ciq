//! `AppHarness` — drive the `App` against `ratatui::TestBackend` with no real terminal.
//!
//! **P1.7 minimal form.** It renders the `App` to an in-memory `TestBackend` and returns the
//! serialized buffer (the App-level analog of jiq's `render_to_string`). In Phase 2 it grows
//! a synthetic key-event feed and a `current_time_ms: u64` seam to drive the debouncer
//! deterministically, plus worker pumping — but the render-to-string core is established here.
//!
//! `TestBackend` is an in-memory cell grid: no escape sequences, no real keyboard, no TTY. So
//! everything an `AppHarness` test asserts is in the headless majority (North Star 2); the
//! true-terminal residue is the §4.7 human surface.

use ratatui::Terminal;
use ratatui::backend::TestBackend;

use crate::app::App;

/// A headless driver for `App`: owns the app and an in-memory terminal.
pub struct AppHarness {
    app: App,
    terminal: Terminal<TestBackend>,
    /// Monotonic synthetic test time fed to the debouncer (P2). Present now as the seam so
    /// no wall-clock ever enters harness logic (determinism rule).
    now_ms: u64,
}

impl AppHarness {
    /// Build a harness with an in-memory terminal of the given size.
    pub fn new(app: App, width: u16, height: u16) -> Self {
        let backend = TestBackend::new(width, height);
        let terminal = Terminal::new(backend).expect("TestBackend terminal");
        Self {
            app,
            terminal,
            now_ms: 0,
        }
    }

    /// Build a harness over a default (loading) `App`.
    pub fn loading(width: u16, height: u16) -> Self {
        Self::new(App::new(), width, height)
    }

    /// Advance the synthetic clock. (No-op on render in P1.7; drives the debouncer in P2.)
    pub fn advance(&mut self, ms: u64) {
        self.now_ms += ms;
    }

    /// Current synthetic time.
    pub fn now_ms(&self) -> u64 {
        self.now_ms
    }

    /// Mutable access to the app (to set state in tests).
    pub fn app_mut(&mut self) -> &mut App {
        &mut self.app
    }

    /// Read-only access to the app state (assert on structured state, not just the screen).
    pub fn app(&self) -> &App {
        &self.app
    }

    /// Render the current app state and return the serialized buffer — what the user would
    /// see, as a deterministic string suitable for `insta` snapshots.
    pub fn screen(&mut self) -> String {
        let app = &self.app;
        self.terminal
            .draw(|f| app.render(f))
            .expect("draw to TestBackend");
        self.terminal.backend().to_string()
    }
}
