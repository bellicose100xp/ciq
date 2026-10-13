//! AI chat App orchestration (`dev/PLAN.md` §7 P5.1) — an `impl App` block + the AI worker bridge,
//! lifted out of `app.rs` to keep that file under the 1000-line cap and because it is cohesive:
//! open/close the popup, route its keys, send a message with the conversation attached, and apply a
//! picked query back into the query bar through the **normal** dispatch path.
//!
//! ## The AI threads (ciq is synchronous — no tokio)
//!
//! [`spawn_ai_thread`] mirrors the query worker: a dispatcher thread owns a shared
//! [`AiProvider`](crate::ai::provider::AiProvider), receives an [`AiJob`], and runs each job on its
//! own short-lived thread so a slow answer never blocks a later one (jiq gets this from tokio tasks;
//! ciq gets it from a thread per job). The UI never blocks on the network — the event loop drains
//! the result channel each turn and calls [`App::on_ai_result`].
//!
//! ## What the model is told
//!
//! Every request carries the **system prompt** (schema grounding + the reply contract, stable across
//! the conversation so it stays a cacheable prefix) and the newest transcript turns that fit the
//! `[ai] max_context_length` budget. That is what makes a follow-up like "now group by status" work:
//! the model sees what it already proposed.
//!
//! ## The validation invariant (load-bearing)
//!
//! A picked query is **not** trusted: it is dropped into the query bar and scheduled through the
//! same `schedule` -> `tick` -> `prepare_interactive` (read-only single-statement guard) ->
//! `Dispatcher::dispatch` -> worker path a typed query uses. Suggestions are additionally filtered
//! by [`keep_valid`](crate::ai::suggestion::keep_valid) when the reply lands, so a DML or
//! multi-statement suggestion is never even offered.

use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread::JoinHandle;

use crate::ai::ai_state::AiPhase;
use crate::ai::chat_prompt::{
    QueryContext, build_failure_turn, build_question, build_system_prompt,
};
use crate::ai::provider::{AiError, AiProvider};
use crate::ai::suggestion::{ParseOutcome, keep_valid, parse_reply};
use crate::app::{App, AppPhase, Key, KeyEvent, QueryMode};
use crate::config::AiConfig;
use crate::query::worker::types::{QueryResponse, RequestKind};

/// How many transcript rows a `PageUp`/`PageDown` moves.
const PAGE_ROWS: usize = 5;

/// The 0-based suggestion index an `Alt+<digit>` key names (`Alt+1` -> 0 … up to
/// [`MAX_SUGGESTIONS`](crate::ai::suggestion::MAX_SUGGESTIONS)), or `None` for any other key.
pub(crate) fn alt_digit(ev: &KeyEvent) -> Option<usize> {
    if !ev.mods.alt {
        return None;
    }
    let Key::Char(c) = ev.key else {
        return None;
    };
    let d = c.to_digit(10)?;
    (1..=crate::ai::suggestion::MAX_SUGGESTIONS as u32)
        .contains(&d)
        .then(|| d as usize - 1)
}

/// One request for the AI thread: the system prompt, the conversation turns to send (already
/// budget-trimmed and rendered by the App), and a monotonic sequence id so a stale reply from a
/// superseded request is discarded. The thread is provider-only — it never sees the schema.
#[derive(Debug, Clone)]
pub struct AiJob {
    /// The system prompt (schema + reply contract).
    pub system: String,
    /// The conversation as `(role, text)` pairs, oldest first, ending on the new user message.
    pub turns: Vec<(String, String)>,
    /// Monotonic id; a reply whose id isn't the latest request is discarded.
    pub seq: u64,
}

/// The AI thread's reply for one [`AiJob`], correlated by `seq`.
#[derive(Debug, Clone)]
pub struct AiResult {
    /// The sequence id of the job this answers.
    pub seq: u64,
    /// The provider's outcome: the raw completion text (`Ok`) or the error to surface (`Err`).
    pub outcome: Result<String, AiError>,
}

/// The App-side ends of the AI thread channels: the request `Sender` (handed to the App via
/// [`App::set_ai_channel`]) and the result `Receiver` (drained by the event loop).
pub struct AiBridge {
    /// Send built requests to the AI dispatcher thread.
    pub request_tx: Sender<AiJob>,
    /// Receive replies (drained each event-loop turn).
    pub result_rx: Receiver<AiResult>,
    /// The dispatcher thread handle (joined on shutdown when the request sender drops).
    pub handle: JoinHandle<()>,
}

/// Spawn the AI dispatcher thread over `provider`. Each received job runs on its own thread with a
/// clone of the shared provider. The dispatcher ends when the request channel closes (the App's
/// `Sender` drops).
pub fn spawn_ai_thread(provider: AiProvider) -> AiBridge {
    let (request_tx, request_rx) = channel::<AiJob>();
    let (result_tx, result_rx) = channel::<AiResult>();
    let handle = std::thread::spawn(move || {
        while let Ok(job) = request_rx.recv() {
            let provider = provider.clone();
            let result_tx = result_tx.clone();
            // A detached per-job thread: the send is the only handoff, and a dropped receiver (the
            // App quit) just ends that thread.
            std::thread::spawn(move || {
                let outcome = provider.chat(&job.system, &job.turns);
                let _ = result_tx.send(AiResult {
                    seq: job.seq,
                    outcome,
                });
            });
        }
    });
    AiBridge {
        request_tx,
        result_rx,
        handle,
    }
}

impl App {
    /// Install the AI request channel (the App-side `Sender<AiJob>`). The shell calls this once at
    /// startup with the bridge's sender **only when the `[ai]` feature is active**; otherwise the
    /// channel stays `None` and the chord is a no-op. Tests inject a mock-backed channel here.
    pub fn set_ai_channel(&mut self, request_tx: Sender<AiJob>) {
        self.ai_tx = Some(request_tx);
    }

    /// Install the `[ai]` settings the App itself needs: the prompt context budget and the user's
    /// extra instructions. The provider settings are consumed by the provider, not here.
    pub fn configure_ai(&mut self, cfg: &AiConfig) {
        self.ai_extra_instructions = cfg.extra_instructions().map(str::to_string);
        self.ai_max_context_chars = cfg.max_context_chars();
        self.ai_open_on_start = cfg.open_on_start();
    }

    /// Show the chat as soon as the table is ready, when `[ai] open_on_start` asks for it and the
    /// feature is wired. Unfocused: the keyboard stays in the query bar, so typing SQL works as it
    /// always did while the assistant sits visible at the bottom right, ready to comment on a
    /// failing query.
    pub(crate) fn open_ai_on_load(&mut self) {
        if self.ai_open_on_start && self.ai_ready() && !self.ai.is_open() {
            self.ai.open_unfocused();
        }
    }

    /// Whether the AI feature is wired (a provider channel is installed). The chord and the popup
    /// are no-ops unless this is true.
    pub fn ai_enabled(&self) -> bool {
        self.ai_tx.is_some()
    }

    /// Whether the AI popup can open at all: the feature is wired, a schema is loaded (the prompt
    /// grounds on it), and the app isn't loading or in a load error.
    fn ai_ready(&self) -> bool {
        self.ai_tx.is_some()
            && self.schema.is_some()
            && !matches!(self.phase, AppPhase::Loading | AppPhase::LoadError(_))
    }

    /// `Ctrl+A`: show or hide the AI chat popup. Opening gives it the keyboard and keeps the
    /// existing transcript, so a reopened popup resumes the conversation. Closes the other popups so
    /// overlays never stack. Hiding works whether or not the chat has the keyboard, so the chord is
    /// a plain toggle.
    pub(crate) fn open_ai(&mut self) {
        if self.ai.is_open() {
            self.ai.close();
            return;
        }
        self.show_ai(true);
    }

    /// `Ctrl+G`: move the keyboard between the query bar and the chat. A hidden chat is opened and
    /// focused; a visible one gets the keyboard, or hands it back if it already has it.
    pub(crate) fn focus_ai(&mut self) {
        if !self.ai.is_open() {
            self.show_ai(true);
        } else if self.ai.is_focused() {
            self.ai.blur();
        } else {
            self.ai.focus();
        }
    }

    /// Open the popup, closing the other overlays first. No-op unless [`Self::ai_ready`].
    fn show_ai(&mut self, focused: bool) {
        if !self.ai_ready() {
            return;
        }
        self.autocomplete.close();
        self.palette_open = false;
        self.history_open = false;
        if focused {
            self.ai.open();
        } else {
            self.ai.open_unfocused();
        }
    }

    /// Handle a key while the chat popup is open. Returns whether the app should quit (only `Ctrl+C`).
    ///
    /// `Enter` sends the typed line, or applies the navigated suggestion when the cursor is on one and
    /// the line is blank. `Alt+1`..`Alt+5` apply that row of the latest answer. `Alt+Up`/`Alt+Down`
    /// (and `Alt+K`/`Alt+J`) move the cursor over every suggestion in the transcript. `Up`/`Down`,
    /// `PageUp`/`PageDown` scroll the transcript. `Ctrl+L` starts a new conversation. `Esc` and
    /// `Ctrl+G` hand the keyboard back to the query bar with the chat still visible; `Ctrl+A` hides it.
    pub(crate) fn handle_ai_key(&mut self, ev: &KeyEvent, now_ms: u64) -> bool {
        if ev.mods.ctrl && matches!(ev.key, Key::Char('c') | Key::Char('C')) {
            return true; // Ctrl-C still quits
        }
        // The popup captures the keyboard, so the two chat chords are handled here rather than in
        // `on_key`'s chord block: Ctrl+A hides the chat, Ctrl+G hands the keyboard back.
        if ev.mods.ctrl && matches!(ev.key, Key::Char('a') | Key::Char('A')) {
            self.ai.close();
            return false;
        }
        if ev.mods.ctrl && matches!(ev.key, Key::Char('g') | Key::Char('G')) {
            self.ai.blur();
            return false;
        }
        if ev.mods.ctrl && matches!(ev.key, Key::Char('l') | Key::Char('L')) {
            self.ai.reset();
            return false;
        }
        // Alt+<digit> applies that row of the latest answer (1-based, jiq's shortcut).
        if let Some(idx) = alt_digit(ev) {
            self.apply_suggestion(idx, now_ms);
            return false;
        }
        match &ev.key {
            Key::Esc => self.ai.blur(),
            Key::Enter => self.ai_enter(now_ms),
            Key::Backspace => self.ai.backspace(),
            Key::Up if ev.mods.alt => self.move_ai_cursor(false),
            Key::Down if ev.mods.alt => self.move_ai_cursor(true),
            Key::Char('k') | Key::Char('K') if ev.mods.alt => self.move_ai_cursor(false),
            Key::Char('j') | Key::Char('J') if ev.mods.alt => self.move_ai_cursor(true),
            Key::Up => self.scroll_ai(-1),
            Key::Down => self.scroll_ai(1),
            Key::PageUp => self.scroll_ai(-(PAGE_ROWS as isize)),
            Key::PageDown => self.scroll_ai(PAGE_ROWS as isize),
            Key::Paste(text) => self.ai.paste(text),
            Key::Char(c) if !ev.mods.ctrl && !ev.mods.alt => self.ai.push_char(*c),
            _ => {}
        }
        false
    }

    /// Move the suggestion cursor and scroll the transcript so it stays visible.
    fn move_ai_cursor(&mut self, forward: bool) {
        if forward {
            self.ai.select_next();
        } else {
            self.ai.select_prev();
        }
        let (width, height) = self.ai_transcript_size();
        if let Some(row) = crate::ai::ai_render::selected_row(&self.ai, width) {
            let total = crate::ai::ai_render::row_count(&self.ai, width);
            self.ai.reveal_selected(row, height, total);
        }
    }

    /// Scroll the transcript by `rows` (negative = up) inside the last painted window.
    fn scroll_ai(&mut self, rows: isize) {
        let (width, height) = self.ai_transcript_size();
        let total = crate::ai::ai_render::row_count(&self.ai, width);
        if rows < 0 {
            self.ai.scroll_up(rows.unsigned_abs(), height, total);
        } else {
            self.ai.scroll_down(rows as usize, height, total);
        }
    }

    /// The transcript window the last paint used: the popup's inner width, and its inner rows minus
    /// the input line. Zero before the first paint, which every caller treats as "nothing visible".
    pub(crate) fn ai_transcript_size(&self) -> (u16, usize) {
        match self.layout_regions.get().popup {
            Some((_, rect)) => (
                rect.width.saturating_sub(2),
                rect.height.saturating_sub(2).saturating_sub(1) as usize,
            ),
            None => (0, 0),
        }
    }

    /// `Enter`: send the typed line, or — when nothing is typed and the cursor sits on a suggestion —
    /// apply that suggestion.
    fn ai_enter(&mut self, now_ms: u64) {
        if self.ai.input().trim().is_empty() {
            if let Some(sql) = self.ai.take_selected() {
                self.accept_ai_sql(&sql, now_ms);
            }
            return;
        }
        self.send_ai_question();
    }

    /// Apply the `idx`-th suggestion of the latest answer (an `Alt+<n>` shortcut): its SQL lands in
    /// the bar and dispatches through the normal path.
    pub(crate) fn apply_suggestion(&mut self, idx: usize, now_ms: u64) {
        if let Some(sql) = self.ai.take_latest(idx) {
            self.accept_ai_sql(&sql, now_ms);
        }
    }

    /// Which flattened suggestion a click on visible transcript row `row` landed on, if any. The
    /// renderer owns the row map (prose and suggestion rows interleave), so this asks it, using the
    /// same inner height the last paint recorded.
    pub(crate) fn ai_suggestion_at_row(&self, row: usize) -> Option<usize> {
        let (width, height) = self.ai_transcript_size();
        let height = u16::try_from(height).unwrap_or(u16::MAX);
        crate::ai::ai_render::suggestion_at_row(&self.ai, row, width, height)
    }

    /// Apply a clicked suggestion's SQL, the pointer's equivalent of `Alt+<n>`.
    pub(crate) fn apply_ai_sql_from_click(&mut self, sql: &str, now_ms: u64) {
        self.accept_ai_sql(sql, now_ms);
    }

    /// Commit the typed line as a user turn and send the conversation.
    pub(crate) fn send_ai_question(&mut self) {
        let Some(question) = self.ai.send() else {
            return; // blank line, or a request already in flight
        };
        let Some(schema) = self.schema.as_ref() else {
            self.ai.set_error("no table loaded");
            return;
        };
        let sql = self
            .query_form
            .to_full_sql(self.viewport_row_limit)
            .unwrap_or_default();
        let outcome = self.query_context();
        let message = build_question(&question, &sql, &outcome, self.ai_max_context_chars);
        let system = build_system_prompt(schema, self.ai_extra_instructions.as_deref());
        self.dispatch_ai(system, message);
    }

    /// Apply a worker response and, when it is an **accepted main-lane** error, let the chat speak
    /// up about it. The gate matters: a value-fetch or facet error belongs to a popup, not the grid,
    /// and a stale error for a superseded query is already being replaced — neither is something the
    /// user is looking at, so neither earns a request. `on_response` returns `true` exactly when the
    /// grid took the response, which is the signal we want.
    pub fn on_response_with_ai(&mut self, resp: QueryResponse, now_ms: u64) -> bool {
        let failure = match &resp {
            QueryResponse::Error {
                message,
                kind: RequestKind::Main,
                ..
            } => Some(message.clone()),
            _ => None,
        };
        let changed = self.on_response(resp);
        if changed && let Some(error) = failure {
            self.ai_report_failure(&error, now_ms);
        }
        changed
    }

    /// The automatic turn: a query just failed, so ask for fixes. This is the only message ciq sends
    /// without the user typing — a successful query stays quiet. Only a **visible** chat speaks up:
    /// a hidden one stays hidden and sends nothing, so `Ctrl+A` is the one thing that shows or
    /// hides the box and a run of typos never drags it back on screen. No-op while a request is
    /// already in flight.
    pub(crate) fn ai_report_failure(&mut self, error: &str, _now_ms: u64) {
        if !self.ai_ready() || !self.ai.is_open() || self.ai.is_pending() {
            return;
        }
        let Some(schema) = self.schema.as_ref() else {
            return;
        };
        // The query that failed is the one last dispatched, not whatever the bar holds now: the
        // user may have kept typing while it ran. An accepted error always belongs to the latest
        // dispatch, so this is exact.
        let sql = self.last_dispatched_sql.clone().unwrap_or_else(|| {
            self.query_form
                .to_full_sql(self.viewport_row_limit)
                .unwrap_or_default()
        });
        let message = build_failure_turn(&sql, error, self.ai_max_context_chars);
        let system = build_system_prompt(schema, self.ai_extra_instructions.as_deref());
        // The transcript shows a short notice; the model gets the full generated message.
        if !self
            .ai
            .begin_failure_turn(format!("the query failed: {error}"))
        {
            return;
        }
        self.dispatch_ai(system, message);
    }

    /// What the last query did, for the model's context.
    fn query_context(&self) -> QueryContext<'_> {
        // The recorded error, not the status line: the status is overwritten by later notices
        // ("ai: switched to power mode…"), while `last_error` is cleared only by a success.
        if let Some(err) = self.last_error.as_deref() {
            return QueryContext::Error(err);
        }
        match self.result.as_ref() {
            Some(r) if r.rows.row_count() > 0 => QueryContext::Rows {
                sample: &r.rows,
                total_rows: r.rows.row_count(),
            },
            Some(_) => QueryContext::Empty,
            None => QueryContext::None,
        }
    }

    /// Send the system prompt plus the budget-trimmed transcript (whose last turn is the message just
    /// pushed) under a fresh sequence id.
    fn dispatch_ai(&mut self, system: String, latest_message: String) {
        // The transcript's last turn is the one just pushed; swap its display text for the fully
        // built message so the model sees the query and result context, while the popup keeps
        // showing the short version. The history budget is what is left after that message, so
        // the whole request stays within `max_context_length` rather than the short text alone.
        let history_budget = self
            .ai_max_context_chars
            .saturating_sub(latest_message.chars().count());
        let mut turns: Vec<(String, String)> = self
            .ai
            .chat()
            .wire_turns(history_budget)
            .into_iter()
            .map(|t| (t.role.as_wire().to_string(), t.wire_text()))
            .collect();
        if let Some(last) = turns.last_mut() {
            last.1 = latest_message;
        } else {
            turns.push(("user".to_string(), latest_message));
        }
        self.ai_seq = self.ai_seq.wrapping_add(1);
        let seq = self.ai_seq;
        match self.ai_tx.as_ref() {
            Some(tx) => {
                if tx.send(AiJob { system, turns, seq }).is_err() {
                    self.ai.set_error("AI thread unavailable");
                }
            }
            None => self.ai.set_error("AI is not configured"),
        }
    }

    /// Drop `sql` into the query bar and run it through the normal path (preprocess read-only guard
    /// -> dispatch -> worker). The user's mode is kept: in Simple mode the SQL is split into the five
    /// panes through the same simplifier `Ctrl+Q` uses, so picking a suggestion never yanks them
    /// into SQL mode. Only a query the panes cannot express (a CTE, a join, a subquery) falls back to
    /// Power mode, and the status line says so. The popup stays **open** so the conversation
    /// continues against the new result.
    fn accept_ai_sql(&mut self, sql: &str, now_ms: u64) {
        if matches!(self.query_form.mode(), QueryMode::Simple) {
            match crate::app::query_form::simplifier::try_simplify_from_sql(sql) {
                Ok(parts) => {
                    self.query_form
                        .enter_simple_with_parts(parts, self.viewport_row_limit);
                }
                Err(_) => {
                    self.query_form.enter_power_with_sql(sql);
                    self.set_status("ai: query needs SQL mode (too complex for the simple panes)");
                }
            }
        } else {
            self.query_form.enter_power_with_sql(sql);
        }
        self.refresh_autocomplete();
        self.schedule(now_ms);
    }

    /// Apply an [`AiResult`] from the AI thread (called by the event loop each turn). Drops a stale
    /// reply (an old `seq`, or one that arrives when nothing is pending). Returns `true` if the
    /// visible state changed.
    pub fn on_ai_result(&mut self, result: AiResult, _now_ms: u64) -> bool {
        if result.seq != self.ai_seq || !matches!(self.ai.phase(), AiPhase::Pending) {
            return false;
        }
        match result.outcome {
            Err(e) => {
                self.ai.set_error(e.to_string());
                true
            }
            Ok(raw) => {
                match parse_reply(&raw) {
                    ParseOutcome::Parsed(mut reply) => {
                        // Only offer suggestions that pass the same read-only guard as a typed query,
                        // so a bad one is never even presented.
                        reply.suggestions = keep_valid(reply.suggestions);
                        self.ai.set_reply(reply);
                    }
                    ParseOutcome::Unparseable => {
                        self.ai.set_error("could not parse the AI response");
                    }
                }
                true
            }
        }
    }
}
