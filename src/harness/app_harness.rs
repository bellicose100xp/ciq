//! `AppHarness` — drive the `App` against `ratatui::TestBackend` with no real terminal.
//!
//! `dev/PLAN.md` §4.2: the App-level headless driver. It owns the `App`, an in-memory
//! `TestBackend`, the **receiver** end of the App's request channel (so a test can inspect /
//! count the SQL the App dispatched), and a synthetic clock. It mirrors the real
//! [`event_loop`](crate::app::event_loop) but with everything terminal/clock-bound replaced by
//! explicit calls:
//!   - [`key`](Self::key) / [`type_str`](Self::type_str) feed synthetic [`KeyEvent`]s,
//!   - [`advance`](Self::advance) moves the synthetic `now_ms` and ticks the debouncer,
//!   - [`dispatched`](Self::dispatched) drains the SQL the App actually sent the worker,
//!   - [`respond`](Self::respond) feeds a [`QueryResponse`] back to the App,
//!   - [`complete_load`](Self::complete_load) / [`fail_load`](Self::fail_load) drive the load
//!     state machine.
//!
//! `TestBackend` is an in-memory cell grid: no escape sequences, no real keyboard, no TTY — so
//! everything an `AppHarness` test asserts is in the headless majority (North Star 2).

use std::sync::mpsc::{Receiver, channel};

use ratatui::Terminal;
use ratatui::backend::TestBackend;

use crate::app::{App, Key, KeyEvent};
use crate::engine::InterruptHandle;
use crate::query::worker::types::{QueryRequest, QueryResponse, RequestKind};
use crate::schema::Schema;

/// A headless driver for `App`: owns the app, an in-memory terminal, the request receiver, and a
/// synthetic clock.
pub struct AppHarness {
    app: App,
    terminal: Terminal<TestBackend>,
    /// The receiver end of the App's request channel — drained by [`dispatched`](Self::dispatched).
    request_rx: Receiver<QueryRequest>,
    /// Monotonic synthetic test time fed to the debouncer. No wall clock ever enters here.
    now_ms: u64,
}

impl AppHarness {
    /// Build a harness with a fresh `App` (in `Loading`) and an in-memory terminal of the given
    /// size. The App's interrupt handle is a no-op by default; pass a real one via
    /// [`with_interrupt`](Self::with_interrupt) when exercising cancellation.
    pub fn new(width: u16, height: u16) -> Self {
        Self::with_interrupt(width, height, InterruptHandle::noop())
    }

    /// Build a harness whose `App` holds the given interrupt handle (for cancellation tests).
    pub fn with_interrupt(width: u16, height: u16, interrupt: InterruptHandle) -> Self {
        let (request_tx, request_rx) = channel();
        let app = App::new(request_tx, interrupt);
        let backend = TestBackend::new(width, height);
        let terminal = Terminal::new(backend).expect("TestBackend terminal");
        Self {
            app,
            terminal,
            request_rx,
            now_ms: 0,
        }
    }

    // --- driving input + time ---

    /// Feed one key event at the current synthetic time.
    pub fn key(&mut self, ev: KeyEvent) -> &mut Self {
        self.app.on_key(ev, self.now_ms);
        self
    }

    /// Type a string one `Char` key at a time (the common "user types SQL" path).
    pub fn type_str(&mut self, s: &str) -> &mut Self {
        for c in s.chars() {
            self.app.on_key(KeyEvent::char(c), self.now_ms);
        }
        self
    }

    /// Press a plain (unmodified) key.
    pub fn press(&mut self, key: Key) -> &mut Self {
        self.app.on_key(KeyEvent::plain(key), self.now_ms);
        self
    }

    /// Advance the synthetic clock by `ms` and drive the debouncer once (the harness analog of
    /// one event-loop turn after time passes). A debounced query is dispatched here if its quiet
    /// window has elapsed and the engine is `Ready`.
    pub fn advance(&mut self, ms: u64) -> &mut Self {
        self.now_ms += ms;
        self.app.tick(self.now_ms);
        self
    }

    /// Current synthetic time.
    pub fn now_ms(&self) -> u64 {
        self.now_ms
    }

    // --- load state machine ---

    /// Complete the off-thread load: drive `Loading -> Ready` and fire any query typed during
    /// load. (The interrupt handle was set at construction, so unlike the real event loop there
    /// is nothing to install here.) Returns whether a query was dispatched on becoming ready.
    pub fn complete_load(&mut self, summary: impl Into<String>) -> bool {
        self.app.on_loaded(summary)
    }

    /// Complete load and install the given schema (so autocomplete has its candidate source) —
    /// the harness analog of the event loop's `set_schema` + `on_loaded` on `LoadOutcome::Ready`.
    /// Returns whether a query was dispatched on becoming ready.
    pub fn complete_load_with_schema(
        &mut self,
        schema: Schema,
        summary: impl Into<String>,
    ) -> bool {
        self.app.set_schema(schema);
        self.app.on_loaded(summary)
    }

    /// Fail the off-thread load: drive into `LoadError`.
    pub fn fail_load(&mut self, message: impl Into<String>) -> &mut Self {
        self.app.on_load_error(message);
        self
    }

    // --- worker channel inspection / injection ---

    /// Drain every SQL string the App has dispatched to the worker since the last call. Lets a
    /// test assert "N debounced keystrokes produced exactly one query" by counting the result.
    pub fn dispatched(&mut self) -> Vec<String> {
        self.dispatched_requests()
            .into_iter()
            .map(|r| r.query)
            .collect()
    }

    /// Drain every full [`QueryRequest`] the App has dispatched since the last call — so a test can
    /// inspect the [`RequestKind`] (main grid vs value-completion fetch) and the SQL.
    pub fn dispatched_requests(&mut self) -> Vec<QueryRequest> {
        let mut out = Vec::new();
        while let Ok(req) = self.request_rx.try_recv() {
            out.push(req);
        }
        out
    }

    /// Drain only the value-completion fetches (P3.7) the App dispatched — `(column, sql)` pairs.
    pub fn value_fetches(&mut self) -> Vec<(String, String)> {
        self.dispatched_requests()
            .into_iter()
            .filter_map(|r| match r.kind {
                RequestKind::Value { column } => Some((column, r.query)),
                RequestKind::Main | RequestKind::Facet { .. } => None,
            })
            .collect()
    }

    /// Drain only the facet fetches (P4.6) the App dispatched — `(column, sql)` pairs.
    pub fn facet_fetches(&mut self) -> Vec<(String, String)> {
        self.dispatched_requests()
            .into_iter()
            .filter_map(|r| match r.kind {
                RequestKind::Facet { column } => Some((column, r.query)),
                RequestKind::Main | RequestKind::Value { .. } => None,
            })
            .collect()
    }

    /// Feed a [`QueryResponse`] back to the App (the worker-result edge), as the event loop does.
    /// Returns whether the visible state changed (stale responses are discarded inside the App).
    pub fn respond(&mut self, resp: QueryResponse) -> bool {
        self.app.on_response(resp)
    }

    // --- state access ---

    /// Read-only access to the app state (assert on structured state, not just the screen).
    pub fn app(&self) -> &App {
        &self.app
    }

    /// Mutable access to the app (rarely needed; prefer the driving methods above).
    pub fn app_mut(&mut self) -> &mut App {
        &mut self.app
    }

    /// Render the current app state and return the serialized buffer — a deterministic string
    /// suitable for `insta` snapshots.
    pub fn screen(&mut self) -> String {
        let app = &self.app;
        self.terminal
            .draw(|f| app.render(f))
            .expect("draw to TestBackend");
        self.terminal.backend().to_string()
    }
}
