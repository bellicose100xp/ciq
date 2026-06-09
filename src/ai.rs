//! AI natural-language-to-SQL (`dev/PLAN.md` §7 P5.1, §1.5 "deferred from launch but ports
//! cleanly"). Type a plain-English request, get back ONE read-only DuckDB `SELECT` that flows
//! through the **same** preprocess validation + worker path as a hand-typed query.
//!
//! ## ciq is synchronous — no tokio (director decision, do not re-litigate)
//!
//! jiq's AI layer is async (tokio + `CancellationToken`, streaming chunks). ciq is fully
//! synchronous: a blocking worker thread + `mpsc` channels, no async runtime. So this is a port
//! of the *ideas*, not the code — the provider trait, the prompt builder, the popup state, and
//! the render all sit on ciq's sync thread+channel model. The real provider does a **blocking
//! HTTP call on its own dedicated thread** (mirroring the query worker), so the UI never blocks
//! on the network; the AI thread sends its result back over a channel the App drains each turn.
//!
//! ## The flow
//!
//! ```text
//! Ctrl+A opens the AI popup
//!   -> user types a natural-language request, Enter submits
//!   -> build_prompt(nl, &schema)  (PURE — embeds table name + every column name + ColumnType)
//!   -> Provider::complete(prompt) on the AI thread (blocking HTTP, or the MockProvider in tests)
//!   -> returned SQL is dropped into the query bar AND scheduled through the normal path:
//!        prepare_interactive (read-only single-statement guard) -> Dispatcher::dispatch -> worker -> grid
//! ```
//!
//! A non-SELECT or multi-statement model response is **rejected by the existing preprocess guard**
//! — the model can never smuggle DML past the read-only validation (tested). The AI layer adds no
//! second engine entry: generated SQL is just text the bar receives.
//!
//! ## Determinism + tests
//!
//! No network is ever hit by the suite: tests use [`MockProvider`] (canned, deterministic SQL).
//! The prompt builder is pure and golden-tested. The popup state is pure transitions. The AI
//! thread wiring is exercised with the mock over the same channel the real provider uses (no
//! sleeps, no `$HOME`, no network).
//!
//! Conventions (jiq-inherited): no `mod.rs`; tests in separate `{name}_tests.rs`; the App-side
//! orchestration lives in [`ai_app`] (an `impl App` block) to keep `app.rs` under the 1000-line
//! cap.

pub mod ai_app;
pub mod ai_render;
pub mod ai_state;
pub mod prompt;
pub mod provider;

pub use ai_state::{AiPhase, AiState};
pub use prompt::{build_prompt, build_repair_prompt};
pub use provider::{AiError, AiProvider, MockProvider, Provider, RealProvider};
