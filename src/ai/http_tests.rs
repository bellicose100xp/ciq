//! Tests for the HTTP providers' pure parts: the request body, the reply and error parsing, the
//! endpoint, and the per-request auth decision (which is pure over injected env / file readers /
//! timestamp). The network call itself is the one untested line — no test opens a socket.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::*;
use crate::config::{AiConfig, AiProviderType};

fn cfg(provider: AiProviderType) -> AiConfig {
    AiConfig {
        enabled: true,
        provider,
        ..AiConfig::default()
    }
}

fn env_of(pairs: &[(&str, &str)]) -> HashMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
        .collect()
}

/// A reader over an in-memory filesystem keyed by absolute path.
fn files_of(pairs: &[(&str, &str)]) -> HashMap<PathBuf, String> {
    pairs
        .iter()
        .map(|(k, v)| (PathBuf::from(*k), (*v).to_string()))
        .collect()
}

/// No test spawns a process; a profile that would need one fails loudly instead.
fn no_process(_cmd: &str) -> Result<String, String> {
    Err("no credential process in tests".to_string())
}

/// One user turn, the common shape in these tests.
fn turns(text: &str) -> Vec<(String, String)> {
    vec![("user".to_string(), text.to_string())]
}

const CREDS_INI: &str = "[default]\naws_access_key_id = AKIDEXAMPLE\naws_secret_access_key = wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY\n";

// --- the request body ---

#[test]
fn body_carries_model_cap_and_one_user_turn() {
    let b = build_body(
        AiProviderType::Anthropic,
        "claude-fable-5-1",
        4096,
        AiEffort::Low,
        "sys",
        &turns("hello"),
    );
    assert_eq!(b["model"], "claude-fable-5-1");
    assert_eq!(b["max_tokens"], 4096);
    assert_eq!(b["messages"][0]["role"], "user");
    assert_eq!(b["messages"][0]["content"], "hello");
    assert_eq!(b["messages"].as_array().map(Vec::len), Some(1));
}

#[test]
fn body_sends_adaptive_thinking_with_the_configured_effort() {
    for (effort, wire) in [
        (AiEffort::Low, "low"),
        (AiEffort::Medium, "medium"),
        (AiEffort::High, "high"),
        (AiEffort::Xhigh, "xhigh"),
        (AiEffort::Max, "max"),
    ] {
        let b = build_body(AiProviderType::Anthropic, "m", 16, effort, "", &turns("p"));
        assert_eq!(b["thinking"]["type"], "adaptive", "effort {wire}");
        assert_eq!(b["output_config"]["effort"], wire);
    }
}

#[test]
fn effort_off_omits_thinking_and_output_config() {
    let b = build_body(
        AiProviderType::Anthropic,
        "m",
        16,
        AiEffort::Off,
        "",
        &turns("p"),
    );
    assert!(b.get("thinking").is_none());
    assert!(b.get("output_config").is_none());
}

// --- reply parsing ---

#[test]
fn parse_reply_joins_text_blocks_and_skips_thinking() {
    let body = r#"{"content":[
        {"type":"thinking","thinking":""},
        {"type":"text","text":"SELECT *"},
        {"type":"text","text":" FROM t"}
    ],"stop_reason":"end_turn"}"#;
    assert_eq!(parse_reply(body).unwrap(), "SELECT * FROM t");
}

#[test]
fn parse_reply_reports_a_refusal_as_a_request_failure() {
    let body = r#"{"content":[],"stop_reason":"refusal","stop_details":{"type":"refusal","category":"cyber","explanation":"nope"}}"#;
    match parse_reply(body).unwrap_err() {
        AiError::Request(m) => {
            assert!(m.contains("declined"), "{m}");
            assert!(m.contains("nope"), "the explanation is surfaced: {m}");
        }
        other => panic!("expected Request, got {other:?}"),
    }
}

#[test]
fn parse_reply_reports_a_max_tokens_cutoff_as_a_request_failure() {
    // A cut-off JSON reply would otherwise surface as "could not parse", hiding the real fix.
    let body = r#"{"content":[{"type":"text","text":"{\"reply\":\"here are"}],"stop_reason":"max_tokens"}"#;
    match parse_reply(body).unwrap_err() {
        AiError::Request(m) => assert!(m.contains("max_tokens"), "{m}"),
        other => panic!("expected Request, got {other:?}"),
    }
}

#[test]
fn parse_reply_reports_a_refusal_without_an_explanation() {
    let body = r#"{"content":[],"stop_reason":"refusal"}"#;
    assert!(matches!(parse_reply(body), Err(AiError::Request(_))));
}

#[test]
fn parse_reply_rejects_malformed_or_empty_replies() {
    assert!(matches!(parse_reply("not json"), Err(AiError::Parse(_))));
    assert!(matches!(parse_reply("{}"), Err(AiError::Parse(_))));
    assert!(matches!(
        parse_reply(r#"{"content":[{"type":"text","text":"   "}]}"#),
        Err(AiError::Parse(_))
    ));
    assert!(matches!(
        parse_reply(r#"{"content":[{"type":"thinking","thinking":"x"}]}"#),
        Err(AiError::Parse(_))
    ));
}

// --- error bodies ---

#[test]
fn error_body_surfaces_the_api_message() {
    let e = parse_error_body(
        400,
        r#"{"error":{"type":"invalid_request_error","message":"max_tokens too large"}}"#,
    );
    match e {
        AiError::Request(m) => {
            assert!(m.contains("400"), "{m}");
            assert!(m.contains("max_tokens too large"), "{m}");
        }
        other => panic!("expected Request, got {other:?}"),
    }
}

#[test]
fn auth_status_codes_map_to_not_configured() {
    for status in [401u16, 403] {
        let e = parse_error_body(status, r#"{"error":{"message":"invalid x-api-key"}}"#);
        assert!(
            matches!(e, AiError::NotConfigured(_)),
            "HTTP {status} is a credentials problem, not a transient one"
        );
    }
}

#[test]
fn error_body_falls_back_to_raw_text_then_to_a_placeholder() {
    match parse_error_body(500, "upstream exploded") {
        AiError::Request(m) => assert!(m.contains("upstream exploded"), "{m}"),
        other => panic!("expected Request, got {other:?}"),
    }
    match parse_error_body(502, "   ") {
        AiError::Request(m) => assert!(m.contains("no response body"), "{m}"),
        other => panic!("expected Request, got {other:?}"),
    }
}

#[test]
fn error_body_truncates_a_huge_raw_body() {
    let huge = "x".repeat(5_000);
    match parse_error_body(500, &huge) {
        AiError::Request(m) => assert!(
            m.len() < 400,
            "message stays readable, got {} chars",
            m.len()
        ),
        other => panic!("expected Request, got {other:?}"),
    }
}

// --- endpoints + labels ---

#[test]
fn body_carries_the_system_prompt_and_every_turn_in_order() {
    let convo = vec![
        ("user".to_string(), "only EU rows".to_string()),
        ("assistant".to_string(), "filter on region".to_string()),
        ("user".to_string(), "now group by status".to_string()),
    ];
    let b = build_body(
        AiProviderType::Anthropic,
        "m",
        512,
        AiEffort::Low,
        "you are a SQL assistant",
        &convo,
    );
    assert_eq!(b["system"], "you are a SQL assistant");
    let msgs = b["messages"].as_array().expect("messages array");
    assert_eq!(msgs.len(), 3, "the whole conversation is sent");
    assert_eq!(msgs[0]["role"], "user");
    assert_eq!(msgs[1]["role"], "assistant");
    assert_eq!(msgs[2]["content"], "now group by status");
}

#[test]
fn an_empty_system_prompt_is_omitted() {
    let b = build_body(
        AiProviderType::Anthropic,
        "m",
        512,
        AiEffort::Low,
        "   ",
        &turns("q"),
    );
    assert!(
        b.get("system").is_none(),
        "a blank system field would be wasted tokens"
    );
}

#[test]
fn bedrock_body_names_the_version_not_the_model() {
    // On Bedrock the model is in the URL, so the body carries `anthropic_version` instead.
    let b = build_body(
        AiProviderType::Bedrock,
        "global.anthropic.claude-fable-5-1",
        256,
        AiEffort::Low,
        "sys",
        &turns("hello"),
    );
    assert_eq!(b["anthropic_version"], BEDROCK_ANTHROPIC_VERSION);
    assert!(b.get("model").is_none(), "the model travels in the path");
    assert_eq!(b["output_config"]["effort"], "low");
    assert_eq!(b["thinking"]["type"], "adaptive");
}

#[test]
fn bedrock_host_and_path_are_the_runtime_invoke_endpoint() {
    assert_eq!(
        bedrock_host("us-east-1"),
        "bedrock-runtime.us-east-1.amazonaws.com"
    );
    assert_eq!(
        bedrock_host("eu-west-1"),
        "bedrock-runtime.eu-west-1.amazonaws.com"
    );
    assert_eq!(
        bedrock_path("global.anthropic.claude-fable-5-1"),
        "/model/global.anthropic.claude-fable-5-1/invoke"
    );
}

#[test]
fn bedrock_path_escapes_an_inference_profile_arn_into_one_segment() {
    // An ARN's `:` and `/` would otherwise split the path, so the signature would cover the wrong
    // resource and the request would 403 rather than fail loudly.
    let arn = "arn:aws:bedrock:us-east-1:123456789012:inference-profile/global.anthropic.claude-fable-5-1";
    let path = bedrock_path(arn);
    assert!(path.starts_with("/model/"), "{path}");
    assert!(path.ends_with("/invoke"), "{path}");
    let segment = path
        .strip_prefix("/model/")
        .and_then(|p| p.strip_suffix("/invoke"))
        .expect("the id sits in one segment");
    assert!(!segment.contains('/'), "no bare slash survives: {segment}");
    assert!(!segment.contains(':'), "no bare colon survives: {segment}");
    assert!(
        segment.contains("%3A") && segment.contains("%2F"),
        "{segment}"
    );
}

#[test]
fn from_config_captures_the_provider_defaults() {
    let p = HttpProvider::from_config(&cfg(AiProviderType::Bedrock));
    assert_eq!(p.model(), crate::config::ai_config::DEFAULT_BEDROCK_MODEL);
    assert_eq!(p.provider(), AiProviderType::Bedrock);
    assert_eq!(
        p.api_key_env(),
        crate::config::ai_config::DEFAULT_BEDROCK_TOKEN_ENV
    );
    let a = HttpProvider::from_config(&cfg(AiProviderType::Anthropic));
    assert_eq!(a.model(), crate::config::ai_config::DEFAULT_ANTHROPIC_MODEL);
    assert_eq!(
        a.api_key_env(),
        crate::config::ai_config::DEFAULT_API_KEY_ENV
    );
}

#[test]
fn label_names_the_provider_and_model() {
    assert_eq!(
        HttpProvider::from_config(&cfg(AiProviderType::Bedrock)).label(),
        format!(
            "bedrock · {}",
            crate::config::ai_config::DEFAULT_BEDROCK_MODEL
        )
    );
    assert!(
        HttpProvider::from_config(&cfg(AiProviderType::Anthropic))
            .label()
            .starts_with("anthropic · ")
    );
}

// --- the auth decision ---

#[test]
fn anthropic_sends_the_key_from_its_env_var() {
    let p = HttpProvider::from_config(&cfg(AiProviderType::Anthropic));
    let env = env_of(&[("ANTHROPIC_API_KEY", "sk-test")]);
    let (url, headers) = p
        .auth_headers(
            b"{}",
            &|k| env.get(k).cloned(),
            &|_| None,
            &no_process,
            None,
            0,
        )
        .expect("key present");
    assert_eq!(url, ANTHROPIC_URL);
    assert_eq!(
        headers,
        vec![("x-api-key".to_string(), "sk-test".to_string())]
    );
}

#[test]
fn anthropic_without_a_key_names_the_env_var_to_set() {
    let p = HttpProvider::from_config(&AiConfig {
        api_key_env: Some("MY_CIQ_KEY".to_string()),
        ..cfg(AiProviderType::Anthropic)
    });
    match p
        .auth_headers(b"{}", &|_| None, &|_| None, &no_process, None, 0)
        .unwrap_err()
    {
        AiError::NotConfigured(m) => assert!(m.contains("MY_CIQ_KEY"), "{m}"),
        other => panic!("expected NotConfigured, got {other:?}"),
    }
}

#[test]
fn an_empty_key_env_var_counts_as_unset() {
    let p = HttpProvider::from_config(&cfg(AiProviderType::Anthropic));
    let env = env_of(&[("ANTHROPIC_API_KEY", "   ")]);
    assert!(
        p.auth_headers(
            b"{}",
            &|k| env.get(k).cloned(),
            &|_| None,
            &no_process,
            None,
            0
        )
        .is_err()
    );
}

#[test]
fn bedrock_prefers_a_bearer_token_over_sigv4() {
    let p = HttpProvider::from_config(&AiConfig {
        region: Some("us-west-2".to_string()),
        ..cfg(AiProviderType::Bedrock)
    });
    let env = env_of(&[("AWS_BEARER_TOKEN_BEDROCK", "bedrock-token")]);
    let (url, headers) = p
        .auth_headers(
            b"{}",
            &|k| env.get(k).cloned(),
            &|_| None,
            &no_process,
            None,
            0,
        )
        .expect("token present");
    assert_eq!(
        url,
        format!(
            "https://bedrock-runtime.us-west-2.amazonaws.com/model/{}/invoke",
            crate::config::ai_config::DEFAULT_BEDROCK_MODEL
        )
    );
    assert_eq!(
        headers,
        vec![(
            "authorization".to_string(),
            "Bearer bedrock-token".to_string()
        )],
        "a Bedrock API key is a bearer token and needs no signature"
    );
}

#[test]
fn bedrock_signs_with_credentials_from_the_environment() {
    let p = HttpProvider::from_config(&cfg(AiProviderType::Bedrock));
    let env = env_of(&[
        ("AWS_REGION", "eu-central-1"),
        ("AWS_ACCESS_KEY_ID", "AKIDEXAMPLE"),
        ("AWS_SECRET_ACCESS_KEY", "secret"),
        ("AWS_SESSION_TOKEN", "session-token"),
    ]);
    let (url, headers) = p
        .auth_headers(
            b"{\"a\":1}",
            &|k| env.get(k).cloned(),
            &|_| None,
            &no_process,
            None,
            1440938160,
        )
        .expect("credentials present");
    assert!(
        url.starts_with("https://bedrock-runtime.eu-central-1.amazonaws.com/model/"),
        "{url}"
    );
    let map: HashMap<_, _> = headers.into_iter().collect();
    assert_eq!(
        map.get("x-amz-date").map(String::as_str),
        Some("20150830T123600Z")
    );
    assert_eq!(
        map.get("x-amz-security-token").map(String::as_str),
        Some("session-token")
    );
    let auth = map.get("authorization").expect("authorization header");
    assert!(auth.starts_with("AWS4-HMAC-SHA256 "), "{auth}");
    assert!(
        auth.contains("/eu-central-1/bedrock/aws4_request"),
        "the scope names the region and service: {auth}"
    );
    assert!(
        auth.contains("SignedHeaders=content-type;host;x-amz-date;x-amz-security-token"),
        "{auth}"
    );
}

#[test]
fn bedrock_reads_credentials_and_region_from_the_aws_files() {
    let p = HttpProvider::from_config(&cfg(AiProviderType::Bedrock));
    let files = files_of(&[
        ("/home/u/.aws/credentials", CREDS_INI),
        ("/home/u/.aws/config", "[default]\nregion = ap-south-1\n"),
    ]);
    let read = |p: &Path| files.get(p).cloned();
    let (url, headers) = p
        .auth_headers(
            b"{}",
            &|_| None,
            &read,
            &no_process,
            Some(Path::new("/home/u")),
            0,
        )
        .expect("file credentials");
    assert!(
        url.contains("ap-south-1"),
        "region comes from the config file: {url}"
    );
    let map: HashMap<_, _> = headers.into_iter().collect();
    assert!(map.contains_key("authorization"));
    assert!(
        !map.contains_key("x-amz-security-token"),
        "static file credentials carry no session token"
    );
}

#[test]
fn bedrock_honors_the_configured_profile() {
    let p = HttpProvider::from_config(&AiConfig {
        profile: Some("work".to_string()),
        ..cfg(AiProviderType::Bedrock)
    });
    let files = files_of(&[(
        "/home/u/.aws/credentials",
        "[default]\naws_access_key_id = WRONG\naws_secret_access_key = wrong\n\n[work]\naws_access_key_id = AKIDWORK\naws_secret_access_key = right\n",
    )]);
    let read = |p: &Path| files.get(p).cloned();
    let (_, headers) = p
        .auth_headers(
            b"{}",
            &|_| None,
            &read,
            &no_process,
            Some(Path::new("/home/u")),
            0,
        )
        .expect("profile credentials");
    let auth = headers
        .into_iter()
        .find(|(k, _)| k == "authorization")
        .map(|(_, v)| v)
        .expect("authorization header");
    assert!(auth.contains("Credential=AKIDWORK/"), "{auth}");
}

#[test]
fn bedrock_without_any_credentials_is_not_configured() {
    let p = HttpProvider::from_config(&cfg(AiProviderType::Bedrock));
    match p
        .auth_headers(b"{}", &|_| None, &|_| None, &no_process, None, 0)
        .unwrap_err()
    {
        AiError::NotConfigured(m) => assert!(
            m.contains("credential") || m.contains("AWS_ACCESS_KEY_ID"),
            "the message says how to fix it: {m}"
        ),
        other => panic!("expected NotConfigured, got {other:?}"),
    }
}

#[test]
fn a_none_provider_has_no_auth() {
    let p = HttpProvider::from_config(&AiConfig::default());
    assert!(matches!(
        p.auth_headers(b"{}", &|_| None, &|_| None, &no_process, None, 0),
        Err(AiError::NotConfigured(_))
    ));
}

#[test]
fn the_signature_changes_with_the_body() {
    let p = HttpProvider::from_config(&cfg(AiProviderType::Bedrock));
    let env = env_of(&[
        ("AWS_ACCESS_KEY_ID", "AKIDEXAMPLE"),
        ("AWS_SECRET_ACCESS_KEY", "secret"),
    ]);
    let get = |k: &str| env.get(k).cloned();
    let sig = |body: &[u8]| {
        p.auth_headers(body, &get, &|_| None, &no_process, None, 1440938160)
            .unwrap()
            .1
            .into_iter()
            .find(|(k, _)| k == "authorization")
            .unwrap()
            .1
    };
    assert_ne!(
        sig(b"{\"a\":1}"),
        sig(b"{\"a\":2}"),
        "the body is part of the signature"
    );
}
