//! AI NL->SQL App orchestration (`dev/PLAN.md` §7 P5.1) — an `impl App` block + the AI worker
//! thread bridge, lifted out of `app.rs` to keep that file under the 1000-line cap and because it
//! is cohesive: open/close the popup, route its keys, submit a built prompt to the AI thread, and
//! apply the AI thread's result back into the query bar through the **normal** dispatch path.
//!
//! ## The AI thread (ciq is synchronous — no tokio)
//!
//! [`spawn_ai_thread`] mirrors the query worker: a dedicated background thread that owns a
//! `Box<dyn Provider>`, blocks on `recv()` for an [`AiJob`] (a built prompt + a sequence id), calls
//! the **blocking** `Provider::complete`, and sends an [`AiResult`] back on the response channel.
//! The UI never blocks on the network — the event loop drains the result channel each turn and
//! calls [`App::on_ai_result`].
//!
//! ## The validation invariant (load-bearing)
//!
//! The generated SQL is **not** trusted: [`App::on_ai_result`] drops it into the query bar and
//! schedules it through the same `schedule` -> `tick` -> `prepare_interactive` (read-only
//! single-statement guard) -> `Dispatcher::dispatch` -> worker path a typed query uses. A model
//! reply that is DML or multi-statement is rejected by preprocess before it can touch the table —
//! the AI layer adds no second engine entry and no bypass.

use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread::JoinHandle;

use crate::ai::ai_state::AiPhase;
use crate::ai::prompt::{build_prompt, strip_code_fences};
use crate::ai::provider::{AiError, Provider};
use crate::app::{App, AppPhase, Key, KeyEvent};

/// A unit of work for the AI thread: the fully built model prompt and a monotonic sequence id so a
/// stale reply (from a superseded submit) can be discarded. The App builds the prompt (grounding
/// it on the live schema) before sending — the thread is provider-only, it never sees the schema.
#[derive(Debug, Clone)]
pub struct AiJob {
    /// The full prompt to hand the provider (already schema-grounded by [`build_prompt`]).
    pub prompt: String,
    /// Monotonic id; a reply whose id isn't the latest submit is discarded (stale-discard, like
    /// the query worker's `request_id`).
    pub seq: u64,
}

/// The AI thread's reply for one [`AiJob`], correlated by `seq`.
#[derive(Debug, Clone)]
pub struct AiResult {
    /// The sequence id of the job this answers.
    pub seq: u64,
    /// The provider's outcome: the generated SQL (`Ok`) or the error to surface (`Err`).
    pub outcome: Result<String, AiError>,
}

/// The App-side ends of the AI thread channels: the request `Sender` (handed to the App via
/// [`App::set_ai_channel`]) and the result `Receiver` (drained by the event loop). Returned by
/// [`spawn_ai_thread`] so the shell wires both halves.
pub struct AiBridge {
    /// Send built prompts to the AI thread.
    pub request_tx: Sender<AiJob>,
    /// Receive the AI thread's replies (drained each event-loop turn).
    pub result_rx: Receiver<AiResult>,
    /// The thread handle (joined on shutdown when the request sender drops).
    pub handle: JoinHandle<()>,
}

/// Spawn the AI worker thread over `provider`. Mirrors the query worker: owns the provider, blocks
/// on `recv()`, calls the blocking `Provider::complete` per job, and sends the [`AiResult`] back.
/// The loop ends when the request channel closes (the App's `Sender` drops). Returns the
/// [`AiBridge`] the shell wires into the App + event loop.
pub fn spawn_ai_thread(provider: Box<dyn Provider>) -> AiBridge {
    let (request_tx, request_rx) = channel::<AiJob>();
    let (result_tx, result_rx) = channel::<AiResult>();
    let handle = std::thread::spawn(move || {
        while let Ok(job) = request_rx.recv() {
            let outcome = provider.complete(&job.prompt);
            if result_tx
                .send(AiResult {
                    seq: job.seq,
                    outcome,
                })
                .is_err()
            {
                break; // App dropped the receiver; nothing left to serve.
            }
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
    /// startup with the bridge's sender **only when the `[ai]` feature is active** and a provider
    /// was built; otherwise the channel stays `None` and the AI popup is unavailable (the chord is
    /// a no-op). Tests inject a mock-backed channel here directly.
    pub fn set_ai_channel(&mut self, request_tx: Sender<AiJob>) {
        self.ai_tx = Some(request_tx);
    }

    /// Whether the AI feature is wired (a provider channel is installed). The `Ctrl+G` chord and
    /// the popup are no-ops unless this is true.
    pub fn ai_enabled(&self) -> bool {
        self.ai_tx.is_some()
    }

    /// Open the AI NL->SQL popup (P5.1). No-op when the feature is unwired (no provider), while
    /// loading / on a load error, or without a schema (the prompt grounds on the schema, so there
    /// is nothing to ask against yet). Closes the other popups so overlays never stack.
    pub(crate) fn open_ai(&mut self) {
        if self.ai_tx.is_none()
            || self.schema.is_none()
            || matches!(self.phase, AppPhase::Loading | AppPhase::LoadError(_))
        {
            return;
        }
        self.autocomplete.close();
        self.palette_open = false;
        self.history_open = false;
        self.ai.open();
    }

    /// Handle a key while the AI popup is open (P5.1). Returns whether the app should quit (only
    /// `Ctrl+C` quits). While editing: a printable char appends to the prompt, `Backspace` pops,
    /// `Enter` submits, `Esc` closes. After an error, the first edit resumes editing the preserved
    /// prompt. While `Pending`, edits are ignored (the request is in flight) but `Esc` still
    /// closes.
    pub(crate) fn handle_ai_key(&mut self, ev: &KeyEvent, _now_ms: u64) -> bool {
        if ev.mods.ctrl && matches!(ev.key, Key::Char('c') | Key::Char('C')) {
            return true; // Ctrl-C still quits
        }
        match &ev.key {
            Key::Esc => self.ai.close(),
            Key::Enter => self.submit_ai(),
            Key::Backspace => {
                self.ai.resume_editing();
                self.ai.backspace();
            }
            Key::Char(c) => {
                self.ai.resume_editing();
                self.ai.push_char(*c);
            }
            _ => {}
        }
        false
    }

    /// Submit the popup's prompt to the AI thread: build the schema-grounded model prompt, send it
    /// on the request channel under a fresh sequence id (`App::ai_seq`, bumped here), and move the
    /// popup to `Pending`. No-op when there is no prompt (empty input), no schema, or the channel
    /// is gone (the popup shows an error in the last case). A reply whose `seq` isn't the latest is
    /// discarded in [`on_ai_result`](Self::on_ai_result) — the AI analog of the query worker's
    /// `request_id` stale-discard.
    pub(crate) fn submit_ai(&mut self) {
        let Some(prompt) = self.ai.submit() else {
            return; // empty prompt or not in an editable phase
        };
        let Some(schema) = self.schema.as_ref() else {
            self.ai.set_error("no table loaded");
            return;
        };
        let model_prompt = build_prompt(&prompt, schema);
        self.ai_seq = self.ai_seq.wrapping_add(1);
        let seq = self.ai_seq;
        match self.ai_tx.as_ref() {
            Some(tx) => {
                if tx
                    .send(AiJob {
                        prompt: model_prompt,
                        seq,
                    })
                    .is_err()
                {
                    self.ai.set_error("AI thread unavailable");
                }
            }
            None => self.ai.set_error("AI is not configured"),
        }
    }

    /// Apply an [`AiResult`] from the AI thread (called by the event loop each turn). Drops a stale
    /// reply (an old `seq`, or one that arrives after the popup was closed). On success: drop the
    /// generated SQL into the query bar, close the popup, and schedule it through the **normal**
    /// path (preprocess read-only guard -> dispatch -> worker) — the AI never bypasses validation.
    /// On error: surface the message in the popup (which stays open so the user can edit + retry).
    /// `now_ms` is the synthetic/real timestamp for the debounce schedule. Returns `true` if the
    /// visible state changed.
    pub fn on_ai_result(&mut self, result: AiResult, now_ms: u64) -> bool {
        // Discard a reply for a superseded submit, or one that arrived after the user closed the
        // popup / it is no longer pending (only a Pending popup is awaiting a reply).
        if result.seq != self.ai_seq || !matches!(self.ai.phase(), AiPhase::Pending) {
            return false;
        }
        match result.outcome {
            Ok(sql) => {
                // Unwrap a single outer markdown code fence the model may have added despite the
                // prompt's no-fences rule, so a good SELECT isn't rejected for fence noise. This is
                // cosmetic only — the unwrapped text still crosses the read-only guard below.
                let sql = strip_code_fences(&sql);
                self.ai.set_success(&sql);
                // Drop the generated SQL into the bar and run it through the normal path — the
                // read-only single-statement guard rejects any DML/multi-statement reply.
                self.editor.set_text(&sql);
                self.refresh_autocomplete();
                self.ai.close();
                self.schedule(now_ms);
                true
            }
            Err(e) => {
                self.ai.set_error(e.to_string());
                true
            }
        }
    }
}
