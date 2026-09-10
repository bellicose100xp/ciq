//! The HTTP-backed providers: the Anthropic Messages API direct, and Claude on Amazon Bedrock,
//! which serves the same request body through `bedrock-runtime`'s `InvokeModel` (the path jiq's
//! Bedrock provider uses, reached there via the AWS SDK). Bedrock model ids carry a routing prefix
//! (`global.` for cross-region, `us.` / `eu.` / `jp.` / `apac.` for a regional inference profile)
//! ahead of `anthropic.`, and the model travels in the URL rather than the body.
//!
//! Rust has no official Anthropic SDK, so this is a thin blocking client over `ureq`. The pure
//! parts (request body, reply parsing, error mapping, endpoint) are separate functions with
//! tests; only [`HttpProvider::chat`] touches the network, and only it reads the clock (the
//! SigV4 `x-amz-date`), through the one `#[allow]`ed seam below.
//!
//! Thinking on Claude Fable models is always on and configured by `output_config.effort`; the
//! request sends `thinking: {type: adaptive}` alongside the effort so the same body is valid on
//! the Opus and Sonnet families too. `effort = "off"` omits both fields for models that reject
//! them.

use std::time::Duration;

use serde_json::{Value, json};

use crate::config::{AiConfig, AiEffort, AiProviderType};

use super::aws_creds;
use super::provider::{AiError, Provider};
use super::sigv4::{SignRequest, sign};

/// The Messages API version header every request carries.
pub const ANTHROPIC_VERSION: &str = "2023-06-01";

/// The first-party endpoint.
pub const ANTHROPIC_URL: &str = "https://api.anthropic.com/v1/messages";

/// The SigV4 service name for Bedrock runtime requests.
pub const BEDROCK_SERVICE: &str = "bedrock";

/// The `anthropic_version` every Bedrock request body carries (Bedrock's own contract; the
/// first-party API takes this as a header instead).
pub const BEDROCK_ANTHROPIC_VERSION: &str = "bedrock-2023-05-31";

/// The Bedrock runtime host for `region`.
pub fn bedrock_host(region: &str) -> String {
    format!("bedrock-runtime.{region}.amazonaws.com")
}

/// The `InvokeModel` path for `model`, with the id percent-encoded as a **single** path segment.
/// A plain id (`global.anthropic.claude-fable-5-1`) is unreserved and passes through unchanged; a
/// full inference-profile ARN carries `:` and `/`, which must be escaped or they would split the
/// path and the signature would cover the wrong resource.
pub fn bedrock_path(model: &str) -> String {
    let encoded: String = model
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            other => format!("%{other:02X}"),
        })
        .collect();
    format!("/model/{encoded}/invoke")
}

/// Build the Messages API request body: one user turn carrying the whole prompt, the output cap,
/// and (unless effort is `off`) adaptive thinking with the configured effort.
///
/// On Bedrock the model is in the URL and the body carries `anthropic_version` instead; on the
/// first-party API the body names the model and the version rides a header.
pub fn build_body(
    provider: AiProviderType,
    model: &str,
    max_tokens: u32,
    effort: AiEffort,
    system: &str,
    turns: &[(String, String)],
) -> Value {
    let messages: Vec<Value> = turns
        .iter()
        .map(|(role, text)| json!({"role": role, "content": text}))
        .collect();
    let mut body = json!({
        "max_tokens": max_tokens,
        "messages": messages,
    });
    if !system.trim().is_empty() {
        body["system"] = json!(system);
    }
    match provider {
        AiProviderType::Bedrock => {
            body["anthropic_version"] = json!(BEDROCK_ANTHROPIC_VERSION);
        }
        _ => body["model"] = json!(model),
    }
    if let Some(level) = effort.as_wire() {
        body["thinking"] = json!({"type": "adaptive"});
        body["output_config"] = json!({"effort": level});
    }
    body
}

/// Extract the completion text from a Messages API reply: every `text` content block joined in
/// order (thinking blocks are skipped). A `refusal` stop reason is a request failure the user
/// should see, not empty text.
pub fn parse_reply(body: &str) -> Result<String, AiError> {
    let v: Value =
        serde_json::from_str(body).map_err(|e| AiError::Parse(format!("invalid JSON: {e}")))?;
    if v["stop_reason"].as_str() == Some("max_tokens") {
        return Err(AiError::Request(
            "the reply was cut off at max_tokens; raise `[ai] max_tokens`".to_string(),
        ));
    }
    if v["stop_reason"].as_str() == Some("refusal") {
        let why = v["stop_details"]["explanation"]
            .as_str()
            .map(|s| format!(": {s}"))
            .unwrap_or_default();
        return Err(AiError::Request(format!(
            "the model declined the request{why}"
        )));
    }
    let Some(blocks) = v["content"].as_array() else {
        return Err(AiError::Parse("reply has no content array".to_string()));
    };
    let text: String = blocks
        .iter()
        .filter(|b| b["type"].as_str() == Some("text"))
        .filter_map(|b| b["text"].as_str())
        .collect::<Vec<_>>()
        .join("");
    if text.trim().is_empty() {
        return Err(AiError::Parse("reply contained no text".to_string()));
    }
    Ok(text)
}

/// Map a non-2xx response to an error, surfacing the API's own message when the body is the
/// standard `{"error": {"message": ...}}` envelope.
pub fn parse_error_body(status: u16, body: &str) -> AiError {
    let detail = serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| v["error"]["message"].as_str().map(str::to_string))
        .or_else(|| {
            let t = body.trim();
            (!t.is_empty()).then(|| t.chars().take(300).collect())
        })
        .unwrap_or_else(|| "no response body".to_string());
    match status {
        401 | 403 => AiError::NotConfigured(format!("HTTP {status}: {detail}")),
        _ => AiError::Request(format!("HTTP {status}: {detail}")),
    }
}

/// The Anthropic Messages API client, configured once from `[ai]`. Credentials are read per
/// request (env var, or the AWS credential files), so a key exported or refreshed after launch is
/// picked up without a restart.
#[derive(Debug, Clone)]
pub struct HttpProvider {
    provider: AiProviderType,
    model: String,
    effort: AiEffort,
    max_tokens: u32,
    timeout: Option<Duration>,
    api_key_env: String,
    region: Option<String>,
    profile: Option<String>,
}

impl HttpProvider {
    /// Capture the request settings from the `[ai]` config.
    pub fn from_config(cfg: &AiConfig) -> Self {
        Self {
            provider: cfg.provider,
            model: cfg.model().to_string(),
            effort: cfg.effort(),
            max_tokens: cfg.max_tokens(),
            timeout: cfg.request_timeout(),
            api_key_env: cfg.api_key_env().to_string(),
            region: cfg.region.clone(),
            profile: cfg.profile.clone(),
        }
    }

    /// The configured model id.
    pub fn model(&self) -> &str {
        &self.model
    }

    /// The configured provider.
    pub fn provider(&self) -> AiProviderType {
        self.provider
    }

    /// The env var this provider reads its key or token from.
    pub fn api_key_env(&self) -> &str {
        &self.api_key_env
    }

    /// Resolve the request auth for one call: the Anthropic key, a Bedrock bearer token, or AWS
    /// SigV4 credentials, as a list of headers to attach. Pure over the injected `env`,
    /// `read_file`, `home`, and `epoch_secs` so the whole decision is testable.
    pub fn auth_headers(
        &self,
        body: &[u8],
        env: &dyn Fn(&str) -> Option<String>,
        read_file: &dyn Fn(&std::path::Path) -> Option<String>,
        run_process: &dyn Fn(&str) -> Result<String, String>,
        home: Option<&std::path::Path>,
        epoch_secs: u64,
    ) -> Result<(String, Vec<(String, String)>), AiError> {
        let secret = env(&self.api_key_env).filter(|k| !k.trim().is_empty());
        match self.provider {
            AiProviderType::Anthropic => {
                let key = secret.ok_or_else(|| {
                    AiError::NotConfigured(format!(
                        "set the API key in the {} environment variable",
                        self.api_key_env
                    ))
                })?;
                Ok((
                    ANTHROPIC_URL.to_string(),
                    vec![("x-api-key".to_string(), key)],
                ))
            }
            AiProviderType::Bedrock => {
                let region = aws_creds::resolve_region(
                    self.region.as_deref(),
                    self.profile.as_deref(),
                    env,
                    read_file,
                    home,
                );
                let host = bedrock_host(&region);
                let path = bedrock_path(&self.model);
                let url = format!("https://{host}{path}");
                // A Bedrock API key is a bearer token on `bedrock-runtime` (the `x-api-key` form is
                // the first-party API's, and the Mantle endpoint's — not this one).
                if let Some(token) = secret {
                    return Ok((
                        url,
                        vec![("authorization".to_string(), format!("Bearer {token}"))],
                    ));
                }
                let creds = aws_creds::resolve_credentials(
                    self.profile.as_deref(),
                    env,
                    read_file,
                    run_process,
                    home,
                )
                .map_err(AiError::NotConfigured)?;
                let signed_headers = [("content-type".to_string(), "application/json".to_string())];
                let req = SignRequest {
                    method: "POST",
                    host: &host,
                    path: &path,
                    query: &[],
                    headers: &signed_headers,
                    body,
                    region: &region,
                    service: BEDROCK_SERVICE,
                };
                Ok((url, sign(&req, &creds, epoch_secs)))
            }
            AiProviderType::None => Err(AiError::NotConfigured(
                "no AI provider selected".to_string(),
            )),
        }
    }
}

/// The wall clock, read only here to stamp the SigV4 request date. This is the AI layer's one
/// clock seam (the same documented pattern as the debouncer's `system_time_ms`): the signer itself
/// takes the timestamp as a parameter and is tested with fixed values.
#[allow(clippy::disallowed_methods)]
fn epoch_secs_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

impl Provider for HttpProvider {
    fn chat(&self, system: &str, turns: &[(String, String)]) -> Result<String, AiError> {
        let body = build_body(
            self.provider,
            &self.model,
            self.max_tokens,
            self.effort,
            system,
            turns,
        )
        .to_string();
        let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
        let env = |k: &str| std::env::var(k).ok();
        let read_file = |p: &std::path::Path| std::fs::read_to_string(p).ok();
        let (url, auth) = self.auth_headers(
            body.as_bytes(),
            &env,
            &read_file,
            &aws_creds::run_credential_process,
            home.as_deref(),
            epoch_secs_now(),
        )?;

        // No redirects: the Messages endpoints never issue one, and following a 3xx would carry the
        // API key or the signed session token to whatever host the redirect named.
        let agent = ureq::Agent::config_builder()
            .timeout_global(self.timeout)
            .http_status_as_error(false)
            .max_redirects(0)
            .build()
            .new_agent();
        let mut req = agent.post(&url).header("content-type", "application/json");
        if self.provider != AiProviderType::Bedrock {
            req = req.header("anthropic-version", ANTHROPIC_VERSION);
        }
        for (k, v) in &auth {
            req = req.header(k.as_str(), v.as_str());
        }
        let mut resp = req.send(body.as_bytes()).map_err(|e| match e {
            ureq::Error::Timeout(_) => AiError::Request("request timed out".to_string()),
            other => AiError::Request(other.to_string()),
        })?;
        let status = resp.status().as_u16();
        let text = resp
            .body_mut()
            .read_to_string()
            .map_err(|e| AiError::Request(format!("reading reply: {e}")))?;
        if !(200..300).contains(&status) {
            return Err(parse_error_body(status, &text));
        }
        parse_reply(&text)
    }

    fn label(&self) -> String {
        let name = match self.provider {
            AiProviderType::Anthropic => "anthropic",
            AiProviderType::Bedrock => "bedrock",
            AiProviderType::None => "none",
        };
        format!("{name} · {}", self.model)
    }
}

#[cfg(test)]
#[path = "http_tests.rs"]
mod http_tests;
