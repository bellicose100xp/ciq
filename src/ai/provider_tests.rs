//! Tests for the AI provider trait, `MockProvider`, and the configuration-aware `HttpProvider`.
//!
//! No network: the mock returns canned SQL/errors; the real provider is exercised only for its
//! config + env-var resolution and its `NotConfigured` returns (it never makes a request here).

use super::*;
use crate::config::{AiConfig, AiProviderType};

// --- MockProvider ---

#[test]
fn mock_returns_canned_sql() {
    let p = MockProvider::returning("SELECT * FROM t WHERE status = 'active'");
    assert_eq!(
        p.complete("any prompt").unwrap(),
        "SELECT * FROM t WHERE status = 'active'"
    );
}

#[test]
fn mock_returns_same_sql_for_any_prompt() {
    let p = MockProvider::returning("SELECT 1");
    assert_eq!(p.complete("foo").unwrap(), "SELECT 1");
    assert_eq!(p.complete("bar").unwrap(), "SELECT 1");
}

#[test]
fn mock_counts_calls() {
    let p = MockProvider::returning("SELECT 1");
    assert_eq!(p.call_count(), 0);
    let _ = p.complete("a");
    let _ = p.complete("b");
    assert_eq!(p.call_count(), 2);
}

#[test]
fn mock_can_fail() {
    let p = MockProvider::failing(AiError::Request("boom".into()));
    let err = p.complete("x").unwrap_err();
    assert_eq!(err, AiError::Request("boom".into()));
}

#[test]
fn mock_is_object_safe_behind_dyn() {
    // The whole point of the trait: hold it as a boxed trait object.
    let p: Box<dyn Provider> = Box::new(MockProvider::returning("SELECT 42"));
    assert_eq!(p.complete("q").unwrap(), "SELECT 42");
}

// --- AiError display ---

#[test]
fn errors_render_actionable_messages() {
    assert_eq!(
        AiError::NotConfigured("set FOO".into()).to_string(),
        "AI not configured: set FOO"
    );
    assert_eq!(
        AiError::Request("timeout".into()).to_string(),
        "AI request failed: timeout"
    );
    assert_eq!(
        AiError::Parse("no SQL".into()).to_string(),
        "AI response could not be parsed: no SQL"
    );
}

// --- the config-driven factory ---

fn anthropic_cfg(model: Option<&str>, key_env: Option<&str>) -> AiConfig {
    AiConfig {
        enabled: true,
        provider: AiProviderType::Anthropic,
        model: model.map(str::to_string),
        api_key_env: key_env.map(str::to_string),
        ..AiConfig::default()
    }
}

#[test]
fn mock_labels_itself() {
    assert_eq!(MockProvider::returning("SELECT 1").label(), "mock");
}

#[test]
fn provider_from_config_none_when_inactive() {
    // provider = "none" and disabled.
    assert!(provider_from_config(&AiConfig::default()).is_none());
    // A provider chosen but the master switch off.
    assert!(
        provider_from_config(&AiConfig {
            enabled: false,
            provider: AiProviderType::Anthropic,
            ..AiConfig::default()
        })
        .is_none()
    );
    // Enabled but no provider chosen.
    assert!(
        provider_from_config(&AiConfig {
            enabled: true,
            provider: AiProviderType::None,
            ..AiConfig::default()
        })
        .is_none()
    );
}

#[test]
fn provider_from_config_builds_each_active_provider() {
    let anthropic = provider_from_config(&anthropic_cfg(Some("claude-x"), None))
        .expect("an active anthropic config yields a provider");
    assert_eq!(anthropic.label(), "anthropic · claude-x");

    let bedrock = provider_from_config(&AiConfig {
        enabled: true,
        provider: AiProviderType::Bedrock,
        model: Some("anthropic.claude-fable-5-1".to_string()),
        ..AiConfig::default()
    })
    .expect("an active bedrock config yields a provider");
    assert_eq!(bedrock.label(), "bedrock · anthropic.claude-fable-5-1");
}

#[test]
fn a_built_provider_is_shareable_across_threads() {
    // The AI dispatcher clones the provider onto a thread per job, so it must be Send + Sync.
    let p = provider_from_config(&anthropic_cfg(None, None)).expect("provider");
    let clone = p.clone();
    let label = std::thread::spawn(move || clone.label())
        .join()
        .expect("the provider crosses a thread boundary");
    assert_eq!(label, p.label());
}
