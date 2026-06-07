//! Tests for the AI provider trait, `MockProvider`, and the configuration-aware `RealProvider`.
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

// --- RealProvider config/env resolution ---

fn anthropic_cfg(model: Option<&str>, key_env: Option<&str>) -> AiConfig {
    AiConfig {
        enabled: true,
        provider: AiProviderType::Anthropic,
        model: model.map(str::to_string),
        api_key_env: key_env.map(str::to_string),
    }
}

#[test]
fn real_provider_resolves_model_default() {
    let cfg = anthropic_cfg(None, None);
    let p = RealProvider::from_config(&cfg);
    // Defaulted model id (a current Claude id from the [ai] config defaults).
    assert_eq!(p.model(), cfg.model());
}

#[test]
fn real_provider_unset_key_env_yields_not_configured() {
    // A deterministic, almost-certainly-unset env var name — no $HOME, no real key.
    let cfg = anthropic_cfg(Some("claude-x"), Some("CIQ_TEST_UNSET_AI_KEY_PROVIDER"));
    // SAFETY: single-threaded test suite (`--test-threads=1`); we only ensure the var is absent.
    unsafe { std::env::remove_var("CIQ_TEST_UNSET_AI_KEY_PROVIDER") };
    let p = RealProvider::from_config(&cfg);
    assert!(!p.has_key(), "no key in the unset env var");
    let err = p.complete("show me everything").unwrap_err();
    match err {
        AiError::NotConfigured(msg) => {
            assert!(
                msg.contains("CIQ_TEST_UNSET_AI_KEY_PROVIDER"),
                "message names the env var to set: {msg}"
            );
        }
        other => panic!("expected NotConfigured, got {other:?}"),
    }
}

#[test]
fn real_provider_with_key_is_not_built_but_clear() {
    // SAFETY: single-threaded test suite; set then remove our own scoped var.
    unsafe { std::env::set_var("CIQ_TEST_SET_AI_KEY_PROVIDER", "sk-fake-not-used") };
    let cfg = anthropic_cfg(Some("claude-x"), Some("CIQ_TEST_SET_AI_KEY_PROVIDER"));
    let p = RealProvider::from_config(&cfg);
    assert!(p.has_key(), "key resolved from the set env var");
    // With a key present it still returns a clear NotConfigured (the HTTP client is not built in).
    // It never hits the network — the message says so.
    let err = p.complete("q").unwrap_err();
    match err {
        AiError::NotConfigured(msg) => assert!(
            msg.contains("not built"),
            "message explains the HTTP client is not built: {msg}"
        ),
        other => panic!("expected NotConfigured, got {other:?}"),
    }
    unsafe { std::env::remove_var("CIQ_TEST_SET_AI_KEY_PROVIDER") };
}

// --- provider_from_config gate ---

#[test]
fn provider_from_config_none_when_inactive() {
    let cfg = AiConfig::default(); // provider = None, disabled
    assert!(provider_from_config(&cfg).is_none());
}

#[test]
fn provider_from_config_some_when_active() {
    let cfg = anthropic_cfg(None, None);
    assert!(provider_from_config(&cfg).is_some());
}
