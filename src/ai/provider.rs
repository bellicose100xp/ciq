//! The AI provider abstraction — synchronous, object-safe, mockable (`dev/PLAN.md` §7 P5.1).
//!
//! ## The trait
//!
//! ```ignore
//! trait Provider: Send + Sync {
//!     fn chat(&self, system: &str, turns: &[(String, String)]) -> Result<String, AiError>;
//! }
//! ```
//!
//! One **blocking** method: hand it the standing system prompt plus the conversation so far, get back
//! the model's raw completion. It is object-safe (`&self`, no generics, no associated types) so the
//! App holds an `Arc<dyn Provider>` and tests swap a [`MockProvider`] for the real one with zero
//! ceremony. `Sync` because every request runs on its own thread over a shared `Arc` (a slow reply
//! never queues behind a fast one), so the provider must tolerate concurrent calls; the HTTP provider
//! holds only immutable config.
//!
//! **Why synchronous (not jiq's async).** ciq has no tokio; the director decision is that the
//! provider blocks. The blocking HTTP call runs on its own AI thread (see
//! [`ai_app`](super::ai_app)), mirroring the query worker, so the UI never blocks.
//!
//! The live providers (Anthropic direct, Claude in Amazon Bedrock) live in [`http`](super::http);
//! this module owns the trait, the error type, and the config-driven factory.

use std::sync::Arc;

use crate::config::{AiConfig, AiProviderType};

use super::http::HttpProvider;

/// Errors an AI provider can return. A normal `Result` (unlike the engine's `QueryOutcome`): an
/// AI failure is exceptional and surfaces in the popup, it is not a hot-path arm.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum AiError {
    /// The provider is not usable as configured (missing key/credentials, unusable profile).
    /// Actionable: the message says what to set.
    #[error("AI not configured: {0}")]
    NotConfigured(String),
    /// The request failed (network, timeout, non-2xx, or the model declined the request).
    #[error("AI request failed: {0}")]
    Request(String),
    /// The reply did not have the expected shape.
    #[error("AI response could not be parsed: {0}")]
    Parse(String),
}

/// A blocking completion provider: messages in, raw completion text out.
pub trait Provider: Send + Sync {
    /// Run one conversation turn: `system` is the standing instruction set, `turns` the transcript as
    /// `(role, text)` pairs, oldest first, ending on the new user message. Blocks until the reply
    /// arrives, the request fails, or it times out.
    fn chat(&self, system: &str, turns: &[(String, String)]) -> Result<String, AiError>;

    /// One-shot convenience: a single user turn with no system prompt.
    fn complete(&self, prompt: &str) -> Result<String, AiError> {
        self.chat("", &[("user".to_string(), prompt.to_string())])
    }

    /// A short human label for the popup title (`bedrock · anthropic.claude-fable-5-1`).
    fn label(&self) -> String;
}

/// A deterministic in-memory provider for tests: returns a canned result for every request and
/// counts calls. Never touches the network.
#[derive(Debug)]
pub struct MockProvider {
    response: Result<String, AiError>,
    call_count: std::sync::atomic::AtomicUsize,
}

impl MockProvider {
    /// A mock that succeeds with `sql` for every request.
    pub fn returning(sql: impl Into<String>) -> Self {
        Self {
            response: Ok(sql.into()),
            call_count: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    /// A mock that fails with `err` for every request.
    pub fn failing(err: AiError) -> Self {
        Self {
            response: Err(err),
            call_count: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    /// How many times the provider has been called.
    pub fn call_count(&self) -> usize {
        self.call_count.load(std::sync::atomic::Ordering::SeqCst)
    }
}

impl Provider for MockProvider {
    fn chat(&self, _system: &str, _turns: &[(String, String)]) -> Result<String, AiError> {
        self.call_count
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.response.clone()
    }

    fn label(&self) -> String {
        "mock".to_string()
    }
}

/// Build the provider the App should use from the `[ai]` config: an HTTP provider when the feature
/// is active ([`AiConfig::is_active`]), else `None` (the AI popup stays disabled). Credentials are
/// resolved per request, so a missing key surfaces as a popup error rather than disabling the
/// feature at startup.
pub fn provider_from_config(cfg: &AiConfig) -> Option<AiProvider> {
    match cfg.provider {
        AiProviderType::None => None,
        AiProviderType::Anthropic | AiProviderType::Bedrock if cfg.is_active() => {
            Some(Arc::new(HttpProvider::from_config(cfg)))
        }
        _ => None,
    }
}

/// The shared provider handle every AI request thread clones.
pub type AiProvider = Arc<dyn Provider>;

#[cfg(test)]
#[path = "provider_tests.rs"]
mod provider_tests;
