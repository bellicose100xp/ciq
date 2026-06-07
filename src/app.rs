//! App — the TUI shell state and render entry point.
//!
//! **P1.7 minimal stub.** ciq has no interactive surface yet; this stands up just enough to
//! prove the headless render seam: an `App` with a render method that draws a placeholder
//! frame, exercised by `AppHarness` against `ratatui::TestBackend` (no real terminal). The
//! real focus/mode model, query bar, results grid, and crossterm event loop land in Phase 2
//! (`dev/TASKS.md` P2.8).
//!
//! The render path is deliberately a pure function of `App` state into a `Frame`, so a
//! `TestBackend` snapshot fully exercises it — the only terminal-touching code is the thin
//! crossterm flush at the outermost edge (added in P2), which is the §4.7 human surface.

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::widgets::{Block, Borders, Paragraph};

/// What the shell is currently doing. Expanded in Phase 2 (Querying, error, etc.).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppPhase {
    /// Parsing the CSV once at startup (the parse-once load).
    Loading,
    /// Loaded and idle, ready for queries.
    Ready,
}

/// The TUI application state. Minimal in P1.7; grows in P2.
#[derive(Debug, Clone)]
pub struct App {
    phase: AppPhase,
    /// The status-line text shown at the bottom (placeholder content in P1.7).
    status: String,
}

impl Default for App {
    fn default() -> Self {
        Self {
            phase: AppPhase::Loading,
            status: "loading…".to_string(),
        }
    }
}

impl App {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn phase(&self) -> &AppPhase {
        &self.phase
    }

    pub fn status(&self) -> &str {
        &self.status
    }

    /// Mark the app ready (called once the engine finishes loading, in P2).
    pub fn set_ready(&mut self, status: impl Into<String>) {
        self.phase = AppPhase::Ready;
        self.status = status.into();
    }

    /// Render the current state into `frame`. Pure function of `App` state — no terminal,
    /// no clock, no I/O — so `TestBackend` exercises it fully.
    pub fn render(&self, frame: &mut Frame) {
        let area = frame.area();
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(1)])
            .split(area);

        let body_text = match self.phase {
            AppPhase::Loading => "ciq — loading CSV…",
            AppPhase::Ready => "ciq — ready",
        };
        let body = Paragraph::new(body_text).block(Block::default().borders(Borders::ALL));
        frame.render_widget(body, chunks[0]);

        let status = Paragraph::new(self.status.as_str());
        frame.render_widget(status, chunks[1]);
    }
}

#[cfg(test)]
#[path = "app/app_tests.rs"]
mod app_tests;
