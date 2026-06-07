//! The `[ai]` config section — NL->SQL provider settings (`dev/PLAN.md` §0/Q5; the provider
//! itself is P5.1).
//!
//! This is the *config* surface the deferred AI layer (P5.1) reads to know which provider/model to
//! call and **where to find the API key** — the key is named by an environment variable, never
//! stored in the file (a checked-in config must hold no secret). Parsed now so the `[ai]` block is
//! accepted before the provider is wired; the provider trait consumes this as plain data.

use serde::Deserialize;

/// The default env var the API key is read from when `[ai] api_key_env` is absent.
pub const DEFAULT_API_KEY_ENV: &str = "ANTHROPIC_API_KEY";

/// The default model when `[ai] model` is absent (a current Claude model id).
pub const DEFAULT_MODEL: &str = "claude-sonnet-4-5";

/// Which NL->SQL provider the AI layer calls. A small closed set; `none` (the default) leaves the
/// AI feature off so a config with no `[ai]` block — the common case — opts out cleanly.
#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AiProviderType {
    /// AI NL->SQL disabled (the default).
    #[default]
    None,
    /// Anthropic (Claude) — the reference provider the P5.1 port targets.
    Anthropic,
}

/// The `[ai]` section: the provider, the model, and the env var naming the API key.
///
/// **No secret lives here.** `api_key_env` names an environment variable (e.g.
/// `ANTHROPIC_API_KEY`); the provider reads the key from the environment at call time. A config
/// file with `provider = "none"` (or no `[ai]` block) leaves the feature off.
///
/// `deny_unknown_fields` (matching `[csv]`) is load-bearing for secret hygiene: a user who
/// mistakes `api_key_env` for `api_key` and writes `api_key = "sk-…"` here gets a parse error ->
/// the "invalid config, using defaults" warning, rather than the secret being silently dropped and
/// left at rest in a plaintext config while the tool reports everything is fine. The key is still
/// only ever read from the environment (the secret was never usable from the file), so this is
/// defensive UX, not a credential path.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct AiConfig {
    /// Whether the AI NL->SQL feature is enabled. Even with a provider configured, `false` keeps
    /// it off (the explicit master switch).
    pub enabled: bool,
    /// The provider to call. `None` (the default) = disabled regardless of `enabled`.
    pub provider: AiProviderType,
    /// The model id (`None` -> [`DEFAULT_MODEL`]).
    pub model: Option<String>,
    /// The **name of the environment variable** holding the API key (`None` ->
    /// [`DEFAULT_API_KEY_ENV`]). The key itself is never stored in the config.
    pub api_key_env: Option<String>,
}

impl AiConfig {
    /// Whether AI NL->SQL is actually active: explicitly enabled **and** a real provider chosen.
    /// A `provider = "none"` always reads as inactive even if `enabled = true`.
    pub fn is_active(&self) -> bool {
        self.enabled && self.provider != AiProviderType::None
    }

    /// The effective model id (configured, or [`DEFAULT_MODEL`]).
    pub fn model(&self) -> &str {
        self.model.as_deref().unwrap_or(DEFAULT_MODEL)
    }

    /// The effective env var naming the API key (configured, or [`DEFAULT_API_KEY_ENV`]).
    pub fn api_key_env(&self) -> &str {
        self.api_key_env.as_deref().unwrap_or(DEFAULT_API_KEY_ENV)
    }
}
