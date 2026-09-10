//! The `[ai]` config section — the AI assistant's provider, model, and request settings
//! (`dev/PLAN.md` §0/Q5; the providers themselves live in [`crate::ai::http`]).
//!
//! Two providers speak the same Anthropic Messages API shape: `anthropic` (the first-party API,
//! keyed by an env var) and `bedrock` (Claude in Amazon Bedrock, signed with the AWS credentials
//! the SDK default chain would find). **No secret lives in this file**: `api_key_env` names an
//! environment variable; AWS keys come from the environment or `~/.aws/credentials`.
//!
//! The defaults pin the assistant to Claude Fable 5.1 on Bedrock at `low` effort — the model has
//! thinking always on, so effort is the lever that keeps a SQL suggestion fast and cheap.

use serde::Deserialize;

/// The default env var the Anthropic API key is read from when `[ai] api_key_env` is absent.
pub const DEFAULT_API_KEY_ENV: &str = "ANTHROPIC_API_KEY";

/// The env var holding a Bedrock bearer token, the alternative to SigV4 for `provider = "bedrock"`
/// when `api_key_env` is absent.
pub const DEFAULT_BEDROCK_TOKEN_ENV: &str = "AWS_BEARER_TOKEN_BEDROCK";

/// The default model on the first-party Anthropic API.
pub const DEFAULT_ANTHROPIC_MODEL: &str = "claude-fable-5-1";

/// The default model on Bedrock. Bedrock ids carry a routing prefix ahead of `anthropic.`:
/// `global.` routes cross-region with no pricing premium, `us.` / `eu.` / `jp.` / `apac.` pin an
/// inference profile to one geography at a 10% premium.
pub const DEFAULT_BEDROCK_MODEL: &str = "global.anthropic.claude-fable-5-1";

/// The default `max_tokens` — room for five suggestions with SQL and a one-line note each.
pub const DEFAULT_MAX_TOKENS: u32 = 4096;

/// The default whole-request timeout in seconds. `0` disables the timeout.
pub const DEFAULT_REQUEST_TIMEOUT_SECS: u64 = 60;

/// The default character budget per request: the transcript turns sent, and the result sample /
/// error text embedded in the new message.
pub const DEFAULT_MAX_CONTEXT_CHARS: u32 = 20_000;

/// Which provider the AI layer calls. `none` (the default) leaves the feature off so a config with
/// no `[ai]` block, the common case, opts out cleanly.
#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AiProviderType {
    /// AI disabled (the default).
    #[default]
    None,
    /// The first-party Anthropic Messages API, keyed by the env var `api_key_env` names.
    Anthropic,
    /// Claude on Amazon Bedrock (`InvokeModel` on `bedrock-runtime.<region>.amazonaws.com`), signed
    /// with AWS credentials or authenticated with a bearer token.
    Bedrock,
}

/// How hard the model thinks per request (`output_config.effort`). `off` omits both the effort
/// and the adaptive-thinking fields, for models that reject them.
#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AiEffort {
    Off,
    #[default]
    Low,
    Medium,
    High,
    Xhigh,
    Max,
}

impl AiEffort {
    /// The wire value for `output_config.effort`; `None` for [`AiEffort::Off`].
    pub fn as_wire(self) -> Option<&'static str> {
        match self {
            AiEffort::Off => None,
            AiEffort::Low => Some("low"),
            AiEffort::Medium => Some("medium"),
            AiEffort::High => Some("high"),
            AiEffort::Xhigh => Some("xhigh"),
            AiEffort::Max => Some("max"),
        }
    }
}

/// The `[ai]` section.
///
/// `deny_unknown_fields` (matching `[csv]`) is load-bearing for secret hygiene: a user who
/// mistakes `api_key_env` for `api_key` and writes `api_key = "sk-…"` here gets a parse error and
/// the "invalid config, using defaults" warning, rather than the secret being silently dropped and
/// left at rest in a plaintext config while the tool reports everything is fine.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct AiConfig {
    /// Whether the AI feature is enabled. Even with a provider configured, `false` keeps it off.
    pub enabled: bool,
    /// The provider to call. `None` (the default) = disabled regardless of `enabled`.
    pub provider: AiProviderType,
    /// The model id (`None` -> the provider's default).
    pub model: Option<String>,
    /// The **name of the environment variable** holding the API key (Anthropic) or bearer token
    /// (Bedrock). The secret itself is never stored in the config.
    pub api_key_env: Option<String>,
    /// Bedrock only: the AWS region (`None` -> `AWS_REGION`, `AWS_DEFAULT_REGION`, the profile's
    /// `region`, then `us-east-1`).
    pub region: Option<String>,
    /// Bedrock only: the AWS named profile (`None` -> `AWS_PROFILE`, then `default`).
    pub profile: Option<String>,
    /// Thinking effort per request (`None` -> `low`).
    pub effort: Option<AiEffort>,
    /// Output token cap per request (`None` -> [`DEFAULT_MAX_TOKENS`]).
    pub max_tokens: Option<u32>,
    /// Whole-request timeout in seconds; `0` disables it (`None` ->
    /// [`DEFAULT_REQUEST_TIMEOUT_SECS`]).
    pub request_timeout_secs: Option<u64>,
    /// Show the chat as soon as the CSV loads (`None` -> `true`). It opens without taking the
    /// keyboard, so typing still goes to the query bar until you click the chat or press `Ctrl+A`.
    pub open_on_start: Option<bool>,
    /// Free-text preferences appended to every prompt (never replaces the built-in rules).
    pub extra_instructions: Option<String>,
    /// Character budget per request, covering both the conversation turns sent and the result
    /// sample / error text in the new message (`None` -> [`DEFAULT_MAX_CONTEXT_CHARS`]).
    pub max_context_length: Option<u32>,
}

impl AiConfig {
    /// Whether the AI feature is actually active: explicitly enabled **and** a real provider
    /// chosen. A `provider = "none"` always reads as inactive even if `enabled = true`.
    pub fn is_active(&self) -> bool {
        self.enabled && self.provider != AiProviderType::None
    }

    /// The effective model id: configured, or the provider's default.
    pub fn model(&self) -> &str {
        match (&self.model, self.provider) {
            (Some(m), _) => m,
            (None, AiProviderType::Bedrock) => DEFAULT_BEDROCK_MODEL,
            (None, _) => DEFAULT_ANTHROPIC_MODEL,
        }
    }

    /// The effective env var naming the API key or bearer token: configured, or the provider's
    /// default ([`DEFAULT_API_KEY_ENV`] / [`DEFAULT_BEDROCK_TOKEN_ENV`]).
    pub fn api_key_env(&self) -> &str {
        match (&self.api_key_env, self.provider) {
            (Some(e), _) => e,
            (None, AiProviderType::Bedrock) => DEFAULT_BEDROCK_TOKEN_ENV,
            (None, _) => DEFAULT_API_KEY_ENV,
        }
    }

    /// The effective effort (configured, or `low`).
    pub fn effort(&self) -> AiEffort {
        self.effort.unwrap_or_default()
    }

    /// The effective output token cap. A configured `0` falls back to the default rather than
    /// asking for an empty reply.
    pub fn max_tokens(&self) -> u32 {
        match self.max_tokens {
            Some(n) if n > 0 => n,
            _ => DEFAULT_MAX_TOKENS,
        }
    }

    /// The effective request timeout; `None` means no timeout (configured `0`).
    pub fn request_timeout(&self) -> Option<std::time::Duration> {
        match self
            .request_timeout_secs
            .unwrap_or(DEFAULT_REQUEST_TIMEOUT_SECS)
        {
            0 => None,
            secs => Some(std::time::Duration::from_secs(secs)),
        }
    }

    /// Whether the chat opens on load (default `true`).
    pub fn open_on_start(&self) -> bool {
        self.open_on_start.unwrap_or(true)
    }

    /// The user's extra instructions, if non-blank.
    pub fn extra_instructions(&self) -> Option<&str> {
        self.extra_instructions
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
    }

    /// The effective prompt context budget in chars (a configured `0` falls back to the default).
    pub fn max_context_chars(&self) -> usize {
        match self.max_context_length {
            Some(n) if n > 0 => n as usize,
            _ => DEFAULT_MAX_CONTEXT_CHARS as usize,
        }
    }
}
