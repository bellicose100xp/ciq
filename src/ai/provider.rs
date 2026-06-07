//! The AI provider abstraction — synchronous, object-safe, mockable (`dev/PLAN.md` §7 P5.1).
//!
//! ## The trait
//!
//! ```ignore
//! trait Provider { fn complete(&self, prompt: &str) -> Result<String, AiError>; }
//! ```
//!
//! One **blocking** method: hand it a prompt (built by [`build_prompt`](super::prompt::build_prompt)),
//! get back the model's raw completion (which the App treats as a single SQL statement). It is
//! object-safe (`&self`, no generics, no associated types) so the App holds a `Box<dyn Provider>`
//! and tests swap a [`MockProvider`] for the real one with zero ceremony.
//!
//! **Why synchronous (not jiq's async).** ciq has no tokio; the director decision is that the
//! provider blocks. The real provider's blocking HTTP call runs on its own dedicated AI thread
//! (see [`ai_app`](super::ai_app)), mirroring the query worker, so the UI never blocks. The trait
//! itself says nothing about threading — it is just "prompt in, text out, may block".
//!
//! ## The real provider (documented escape hatch)
//!
//! ciq is a Rust crate; there is no official Anthropic SDK for Rust, so a real provider would be a
//! thin blocking HTTP client (`POST https://api.anthropic.com/v1/messages`, headers `x-api-key` +
//! `anthropic-version: 2023-06-01`, body `{model, max_tokens, messages:[{role:"user",...}]}`,
//! response `.content[0].text`). Per the P5.1 brief, rather than pull a network dependency
//! (`ureq`/`reqwest`) into the crate now — which would add build weight and a runtime egress
//! surface the headless suite must never exercise — [`RealProvider`] is a **documented, wired**
//! provider that reads the `[ai]` config + the API-key **env var** (named by `[ai] api_key_env`,
//! never stored in the file or source) and returns a clear, actionable [`AiError::NotConfigured`]
//! pointing at where the HTTP client plugs in. The trait is real and the mock fully exercises the
//! NL->SQL path end to end; swapping in the HTTP body is a localized change behind this same trait.
//!
//! The API key is read **only** from the environment at call time — it is never persisted to the
//! config file or hard-coded. An absent/empty key yields `NotConfigured`.

use crate::config::AiConfig;

/// Errors an AI provider can return. A normal `Result` (unlike the engine's `QueryOutcome`): an
/// AI failure is exceptional and surfaces in the popup, it is not a hot-path arm.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum AiError {
    /// The provider is not configured (feature off, no provider chosen, or the API-key env var is
    /// unset/empty). Carries an actionable, user-facing message naming what to set.
    #[error("AI not configured: {0}")]
    NotConfigured(String),

    /// A network/transport error talking to the provider (the real HTTP client surfaces these).
    #[error("AI request failed: {0}")]
    Request(String),

    /// The provider responded but the completion could not be parsed into a usable SQL string.
    #[error("AI response could not be parsed: {0}")]
    Parse(String),
}

/// The synchronous, object-safe AI provider contract (`dev/PLAN.md` §7 P5.1).
///
/// `complete` is **blocking** — the real implementation makes a synchronous HTTP request, so the
/// App must call it off the UI thread (the dedicated AI thread in [`ai_app`](super::ai_app)).
/// Returns the model's completion text on success; the App then validates it as read-only SQL
/// through the existing preprocess guard (the model can never smuggle DML).
pub trait Provider: Send {
    /// Send `prompt` to the model and return its completion (treated as a single SQL statement by
    /// the caller). Blocks until the response is available or fails.
    fn complete(&self, prompt: &str) -> Result<String, AiError>;
}

/// A deterministic, canned provider for tests — **never touches the network** (`dev/PLAN.md` §7
/// P5.1: "no network in tests").
///
/// Construct it with a fixed completion (or an error) and `complete` returns it verbatim for any
/// prompt, so the whole NL->SQL path — prompt build, validation, dispatch, grid — is exercised
/// without an API call. A counting hook (`call_count`) lets tests assert the provider was invoked
/// exactly once per submit.
pub struct MockProvider {
    /// What `complete` returns: `Ok(sql)` to drive the success path, `Err` to drive the error path.
    response: Result<String, AiError>,
    call_count: std::sync::atomic::AtomicUsize,
}

impl MockProvider {
    /// A mock that returns `sql` for any prompt (the success path).
    pub fn returning(sql: impl Into<String>) -> Self {
        Self {
            response: Ok(sql.into()),
            call_count: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    /// A mock that returns `err` for any prompt (the error path).
    pub fn failing(err: AiError) -> Self {
        Self {
            response: Err(err),
            call_count: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    /// How many times `complete` has been called (tests assert exactly-once-per-submit).
    pub fn call_count(&self) -> usize {
        self.call_count.load(std::sync::atomic::Ordering::SeqCst)
    }
}

impl Provider for MockProvider {
    fn complete(&self, _prompt: &str) -> Result<String, AiError> {
        self.call_count
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.response.clone()
    }
}

/// The real provider — reads the `[ai]` config + the API-key **env var** and (would) make a
/// blocking HTTP request to the configured model.
///
/// See the module docs for why the HTTP body is not pulled in now: this provider is real and
/// configuration-aware (it resolves the model + the env-var-named key exactly as a live client
/// would), but its `complete` currently returns an actionable [`AiError::NotConfigured`] rather
/// than hitting the network. The key is read **only** from the environment, never from the file.
pub struct RealProvider {
    /// The model id the request targets (from `[ai] model`, defaulted).
    model: String,
    /// The resolved API key (read from the env var named by `[ai] api_key_env`). `None` when the
    /// var is unset/empty — `complete` then returns `NotConfigured`.
    api_key: Option<String>,
    /// The name of the env var the key was (or would be) read from — surfaced in the
    /// `NotConfigured` message so the user knows exactly what to set.
    api_key_env: String,
}

impl RealProvider {
    /// Build a real provider from the `[ai]` config, reading the API key from the named env var.
    ///
    /// The key never comes from `cfg` (a checked-in config holds no secret) — only from
    /// `std::env::var(cfg.api_key_env())`. An unset/empty var leaves `api_key` `None`, and
    /// [`complete`](Provider::complete) then returns a clear [`AiError::NotConfigured`].
    pub fn from_config(cfg: &AiConfig) -> Self {
        let api_key_env = cfg.api_key_env().to_string();
        let api_key = std::env::var(&api_key_env)
            .ok()
            .filter(|k| !k.trim().is_empty());
        Self {
            model: cfg.model().to_string(),
            api_key,
            api_key_env,
        }
    }

    /// The model id this provider targets.
    pub fn model(&self) -> &str {
        &self.model
    }

    /// Whether an API key was resolved from the environment (so a real request *could* be made).
    pub fn has_key(&self) -> bool {
        self.api_key.is_some()
    }
}

impl Provider for RealProvider {
    fn complete(&self, _prompt: &str) -> Result<String, AiError> {
        // The key is required regardless — surfaced first so the message is actionable.
        if self.api_key.is_none() {
            return Err(AiError::NotConfigured(format!(
                "set the API key in the {} environment variable",
                self.api_key_env
            )));
        }
        // The HTTP client is intentionally not wired (no network dep in the crate — see module
        // docs). A configured-but-not-built provider returns a clear, non-fatal message rather
        // than silently failing or pretending to call out.
        Err(AiError::NotConfigured(format!(
            "the live HTTP provider for model `{}` is not built into this binary; the AI trait is \
             present and the path is exercised via the mock provider in tests",
            self.model
        )))
    }
}

/// Build the provider the App should use from the `[ai]` config: a [`RealProvider`] when the
/// feature is active ([`AiConfig::is_active`]), else `None` (the AI popup stays disabled). The App
/// holds the result as a `Box<dyn Provider>`; tests inject a [`MockProvider`] directly instead.
pub fn provider_from_config(cfg: &AiConfig) -> Option<Box<dyn Provider>> {
    if cfg.is_active() {
        Some(Box::new(RealProvider::from_config(cfg)))
    } else {
        None
    }
}

/// Alias kept for symmetry with jiq's `AsyncAiProvider` naming, should other modules want to refer
/// to the boxed trait object by a single name.
pub type AiProvider = Box<dyn Provider>;

#[cfg(test)]
#[path = "provider_tests.rs"]
mod provider_tests;
