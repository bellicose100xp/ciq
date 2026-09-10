//! AWS credential + region resolution (env, then `~/.aws/credentials` / `~/.aws/config`), pure over injected readers.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Static AWS credentials, the only kind a dependency-free blocking client can use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Credentials {
    pub access_key_id: String,
    pub secret_access_key: String,
    pub session_token: Option<String>,
}

const DEFAULT_REGION: &str = "us-east-1";

/// Parse an AWS shared credentials/config INI text and return the credentials for `profile`.
///
/// Accepts both `[name]` (credentials file) and `[profile name]` (config file) section headers.
/// Keys are case-insensitive. Returns `None` when the profile or either required key is missing.
pub fn parse_credentials(text: &str, profile: &str) -> Option<Credentials> {
    credentials_from(&profile_section(text, profile)?)
}

fn credentials_from(section: &[(String, String)]) -> Option<Credentials> {
    let access_key_id = lookup(section, "aws_access_key_id")?;
    let secret_access_key = lookup(section, "aws_secret_access_key")?;
    let session_token = lookup(section, "aws_session_token");
    Some(Credentials {
        access_key_id,
        secret_access_key,
        session_token,
    })
}

/// The profile's pairs from both shared files merged, `first` (the config file) then `second` (the
/// credentials file) so a lookup, which takes the last match, lets the credentials file win a key
/// both define. `None` when neither file has the profile.
fn merged_profile(
    first: Option<&str>,
    second: Option<&str>,
    profile: &str,
) -> Option<Vec<(String, String)>> {
    let a = first.and_then(|t| profile_section(t, profile));
    let b = second.and_then(|t| profile_section(t, profile));
    match (a, b) {
        (None, None) => None,
        (Some(mut a), Some(b)) => {
            a.extend(b);
            Some(a)
        }
        (a, b) => a.or(b),
    }
}

/// The `region` key of `profile` (both header shapes), or `None`.
pub fn parse_region(text: &str, profile: &str) -> Option<String> {
    let section = profile_section(text, profile)?;
    lookup(&section, "region")
}

/// The `credential_process` command line of `profile` (both header shapes), or `None`.
pub fn parse_credential_process(text: &str, profile: &str) -> Option<String> {
    let section = profile_section(text, profile)?;
    lookup(&section, "credential_process")
}

/// Parse the JSON a `credential_process` command prints, per the AWS credential-process contract:
/// `Version` (must be 1), `AccessKeyId`, `SecretAccessKey`, optional `SessionToken`. `Expiration`
/// is ignored because ciq never caches process credentials.
pub fn parse_process_credentials(json: &str) -> Result<Credentials, String> {
    let value: serde_json::Value = serde_json::from_str(json)
        .map_err(|e| format!("credential_process output is not valid JSON: {e}"))?;
    let object = value
        .as_object()
        .ok_or_else(|| "credential_process output is not a JSON object".to_owned())?;
    match object.get("Version").and_then(serde_json::Value::as_u64) {
        Some(1) => {}
        Some(other) => {
            return Err(format!(
                "credential_process output has Version {other}; only Version 1 is supported"
            ));
        }
        None => return Err("credential_process output is missing Version".to_owned()),
    }
    let required = |key: &str| -> Result<String, String> {
        object
            .get(key)
            .and_then(serde_json::Value::as_str)
            .filter(|v| !v.is_empty())
            .map(str::to_owned)
            .ok_or_else(|| format!("credential_process output is missing {key}"))
    };
    Ok(Credentials {
        access_key_id: required("AccessKeyId")?,
        secret_access_key: required("SecretAccessKey")?,
        session_token: object
            .get("SessionToken")
            .and_then(serde_json::Value::as_str)
            .filter(|v| !v.is_empty())
            .map(str::to_owned),
    })
}

/// Split a `credential_process` command line into program + args the way a POSIX shell would
/// tokenize it — double and single quotes group, a backslash escapes the next char outside single
/// quotes — without running a shell, so nothing is expanded or interpolated. Pure.
pub fn split_command(command: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_token = false;
    let mut quote: Option<char> = None;
    let mut chars = command.chars().peekable();
    while let Some(ch) = chars.next() {
        match quote {
            Some('\'') if ch == '\'' => quote = None,
            Some('\'') => cur.push(ch),
            Some('"') if ch == '"' => quote = None,
            Some('"') if ch == '\\' => {
                if let Some(next) = chars.next() {
                    cur.push(next);
                }
            }
            Some(_) => cur.push(ch),
            None if ch == '\'' || ch == '"' => {
                quote = Some(ch);
                in_token = true;
            }
            None if ch == '\\' => {
                if let Some(next) = chars.next() {
                    cur.push(next);
                    in_token = true;
                }
            }
            None if ch.is_whitespace() => {
                if in_token {
                    out.push(std::mem::take(&mut cur));
                    in_token = false;
                }
            }
            None => {
                cur.push(ch);
                in_token = true;
            }
        }
    }
    if in_token {
        out.push(cur);
    }
    out
}

/// Mask anything in a helper's diagnostic output that looks like a credential before it can reach
/// the screen: AWS access key ids and long base64-ish runs (secret keys, session tokens). Pure.
pub fn redact_secrets(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for word in text.split_inclusive(char::is_whitespace) {
        let (token, trail) = match word.char_indices().find(|(_, c)| c.is_whitespace()) {
            Some((i, _)) => (&word[..i], &word[i..]),
            None => (word, ""),
        };
        out.push_str(if looks_secret(token) {
            "[redacted]"
        } else {
            token
        });
        out.push_str(trail);
    }
    out
}

fn looks_secret(token: &str) -> bool {
    let is_key_id = token.len() == 20
        && (token.starts_with("AKIA") || token.starts_with("ASIA"))
        && token
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit());
    let is_long_opaque = token.len() >= 32
        && token
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'/' || b == b'=');
    is_key_id || is_long_opaque
}

/// Run a `credential_process` command line and return its stdout. Tokenized like a shell would
/// but run without one, so the config author's exact program and args execute and nothing is
/// expanded. Bounded: a helper that hangs is killed after [`CREDENTIAL_PROCESS_TIMEOUT_SECS`].
pub fn run_credential_process(command: &str) -> Result<String, String> {
    use std::io::Read;
    use std::process::Stdio;

    let mut parts = split_command(command).into_iter();
    let program = parts
        .next()
        .ok_or_else(|| "credential_process command is empty".to_owned())?;
    let mut child = Command::new(&program)
        .args(parts)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("failed to start credential_process `{program}`: {e}"))?;

    // Drain both pipes on their own threads so a chatty helper cannot fill a pipe and block before
    // it exits (which would read as a hang and get it killed).
    let stdout_reader = {
        let pipe = child.stdout.take();
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            if let Some(mut p) = pipe {
                let _ = p.read_to_end(&mut buf);
            }
            buf
        })
    };
    let stderr_reader = {
        let pipe = child.stderr.take();
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            if let Some(mut p) = pipe {
                let _ = p.read_to_end(&mut buf);
            }
            buf
        })
    };
    // Poll rather than block: a helper that hangs (an expired SSO session waiting on a browser, a
    // stuck network call) must not pin the AI thread forever. Counting sleeps keeps the clock out
    // of this code.
    let mut waited = 0u32;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if waited >= CREDENTIAL_PROCESS_POLLS => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!(
                    "credential_process `{program}` did not finish within {} seconds",
                    CREDENTIAL_PROCESS_TIMEOUT_SECS
                ));
            }
            Ok(None) => {
                std::thread::sleep(std::time::Duration::from_millis(CREDENTIAL_PROCESS_POLL_MS));
                waited += 1;
            }
            Err(e) => return Err(format!("credential_process `{program}` failed: {e}")),
        }
    };
    let stdout = stdout_reader.join().unwrap_or_default();
    let stderr = stderr_reader.join().unwrap_or_default();
    if !status.success() {
        // Only the first line, redacted: this string is shown in the popup, and a helper that
        // fails may echo the very credential it could not refresh.
        let stderr = String::from_utf8_lossy(&stderr);
        let first_line = stderr.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
        let snippet: String = redact_secrets(first_line.trim())
            .chars()
            .take(160)
            .collect();
        return Err(format!(
            "credential_process `{program}` exited with {status}: {snippet}"
        ));
    }
    String::from_utf8(stdout)
        .map_err(|_| format!("credential_process `{program}` printed non-UTF-8 output"))
}

/// How long a `credential_process` helper may run before ciq gives up on it.
const CREDENTIAL_PROCESS_TIMEOUT_SECS: u64 = 30;
const CREDENTIAL_PROCESS_POLL_MS: u64 = 50;
const CREDENTIAL_PROCESS_POLLS: u32 =
    (CREDENTIAL_PROCESS_TIMEOUT_SECS * 1000 / CREDENTIAL_PROCESS_POLL_MS) as u32;

/// Whether the profile section exists but relies on a credential mechanism ciq cannot run.
///
/// Lets the caller say *why* there are no usable credentials instead of just "none found".
pub fn unsupported_mechanism(text: &str, profile: &str) -> Option<&'static str> {
    mechanism_in(&profile_section(text, profile)?)
}

fn mechanism_in(section: &[(String, String)]) -> Option<&'static str> {
    const MECHANISMS: [&str; 4] = [
        "sso_start_url",
        "sso_session",
        "web_identity_token_file",
        "role_arn",
    ];
    MECHANISMS.into_iter().find(|key| {
        let present = lookup(section, key).is_some();
        // A bare role_arn without source_profile is a broken profile, not a supported mechanism
        // we are declining; only report the assume-role pair.
        present && (*key != "role_arn" || lookup(section, "source_profile").is_some())
    })
}

/// Resolve credentials the way the SDK default chain does, for the parts a blocking client can
/// support: env vars, then static keys in the shared credentials file, then static keys in the
/// shared config file, then the profile's `credential_process` from either file. `env`,
/// `read_file` and `run_process` are injected so this is pure and testable; `home` is the HOME
/// directory (`None` skips file lookups entirely).
pub fn resolve_credentials(
    profile: Option<&str>,
    env: &dyn Fn(&str) -> Option<String>,
    read_file: &dyn Fn(&Path) -> Option<String>,
    run_process: &dyn Fn(&str) -> Result<String, String>,
    home: Option<&Path>,
) -> Result<Credentials, String> {
    if let Some(creds) = env_credentials(env) {
        return Ok(creds);
    }
    let profile = effective_profile(profile, env);
    let credentials_text = shared_file(
        env,
        read_file,
        home,
        "AWS_SHARED_CREDENTIALS_FILE",
        "credentials",
    );
    let config_text = shared_file(env, read_file, home, "AWS_CONFIG_FILE", "config");

    // The SDK merges one profile across both files (the credentials file winning on a shared key),
    // so an access key in one file and its secret in the other still form a credential.
    let section = merged_profile(
        config_text.as_deref(),
        credentials_text.as_deref(),
        &profile,
    );
    if let Some(section) = &section
        && let Some(creds) = credentials_from(section)
    {
        return Ok(creds);
    }

    // Static keys win over credential_process, so the process runs only once neither file yielded
    // keys.
    if let Some(command) = section
        .as_ref()
        .and_then(|s| lookup(s, "credential_process"))
    {
        return run_process(&command)
            .and_then(|stdout| parse_process_credentials(&stdout))
            .map_err(|e| format!("profile `{profile}` credential_process failed: {e}"));
    }

    let mechanism = section.as_ref().and_then(|s| mechanism_in(s));
    Err(match mechanism {
        Some(mechanism) => format!(
            "profile `{profile}` uses {mechanism}, which ciq cannot run; export static credentials or use a profile with keys"
        ),
        None => format!(
            "no AWS credentials found for profile `{profile}`: set AWS_ACCESS_KEY_ID/AWS_SECRET_ACCESS_KEY, or give the profile static keys or a credential_process command in ~/.aws/credentials or ~/.aws/config"
        ),
    })
}

/// Region precedence: `configured`, `AWS_REGION`, `AWS_DEFAULT_REGION`, the profile's `region`
/// in the config file, else `us-east-1`.
pub fn resolve_region(
    configured: Option<&str>,
    profile: Option<&str>,
    env: &dyn Fn(&str) -> Option<String>,
    read_file: &dyn Fn(&Path) -> Option<String>,
    home: Option<&Path>,
) -> String {
    if let Some(region) = non_empty(configured.map(str::to_owned)) {
        return region;
    }
    if let Some(region) = env_var(env, "AWS_REGION").or_else(|| env_var(env, "AWS_DEFAULT_REGION"))
    {
        return region;
    }
    let profile = effective_profile(profile, env);
    shared_file(env, read_file, home, "AWS_CONFIG_FILE", "config")
        .and_then(|text| parse_region(&text, &profile))
        .unwrap_or_else(|| DEFAULT_REGION.to_owned())
}

/// The effective profile name: `profile`, else non-empty `AWS_PROFILE`, else `default`.
pub fn effective_profile(profile: Option<&str>, env: &dyn Fn(&str) -> Option<String>) -> String {
    non_empty(profile.map(str::to_owned))
        .or_else(|| env_var(env, "AWS_PROFILE"))
        .unwrap_or_else(|| "default".to_owned())
}

fn env_credentials(env: &dyn Fn(&str) -> Option<String>) -> Option<Credentials> {
    let access_key_id = env_var(env, "AWS_ACCESS_KEY_ID")?;
    let secret_access_key = env_var(env, "AWS_SECRET_ACCESS_KEY")?;
    Some(Credentials {
        access_key_id,
        secret_access_key,
        session_token: env_var(env, "AWS_SESSION_TOKEN"),
    })
}

/// Read a shared AWS file: the env override path wins, else `<home>/.aws/<name>`.
fn shared_file(
    env: &dyn Fn(&str) -> Option<String>,
    read_file: &dyn Fn(&Path) -> Option<String>,
    home: Option<&Path>,
    override_var: &str,
    name: &str,
) -> Option<String> {
    let path = match env_var(env, override_var) {
        Some(path) => PathBuf::from(path),
        None => home?.join(".aws").join(name),
    };
    read_file(&path)
}

fn env_var(env: &dyn Fn(&str) -> Option<String>, name: &str) -> Option<String> {
    non_empty(env(name))
}

fn non_empty(value: Option<String>) -> Option<String> {
    value.filter(|v| !v.trim().is_empty())
}

/// The `key = value` pairs of the section for `profile`, matching either header shape. Repeated
/// sections of the same shape extend one another (the SDK's INI merge). When a file carries both
/// `[profile name]` and bare `[name]`, the `profile`-prefixed one wins outright — that is the SDK's
/// rule for `[profile default]` versus `[default]` in the config file.
fn profile_section(text: &str, profile: &str) -> Option<Vec<(String, String)>> {
    let mut bare: Option<Vec<(String, String)>> = None;
    let mut prefixed: Option<Vec<(String, String)>> = None;
    let mut current: Option<&mut Vec<(String, String)>> = None;
    for raw in text.lines() {
        let line = strip_comment(raw).trim();
        if line.is_empty() {
            continue;
        }
        if let Some(name) = section_header(line) {
            current = match section_shape(name, profile) {
                Some(Shape::Bare) => Some(bare.get_or_insert_with(Vec::new)),
                Some(Shape::Prefixed) => Some(prefixed.get_or_insert_with(Vec::new)),
                None => None,
            };
            continue;
        }
        if let Some(pairs) = current.as_deref_mut()
            && let Some((key, value)) = line.split_once('=')
        {
            pairs.push((key.trim().to_ascii_lowercase(), value.trim().to_owned()));
        }
    }
    prefixed.or(bare)
}

enum Shape {
    Bare,
    Prefixed,
}

fn section_header(line: &str) -> Option<&str> {
    line.strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
        .map(str::trim)
}

/// Which header shape names `profile`: `[name]` or `[profile name]`; `None` for another profile.
fn section_shape(header: &str, profile: &str) -> Option<Shape> {
    if header == profile {
        return Some(Shape::Bare);
    }
    header
        .strip_prefix("profile")
        .filter(|rest| rest.starts_with(char::is_whitespace))
        .filter(|rest| rest.trim() == profile)
        .map(|_| Shape::Prefixed)
}

/// Drop a `;` or `#` comment. Only a comment that starts the line or follows whitespace counts,
/// so a `#` inside a secret key survives.
fn strip_comment(line: &str) -> &str {
    let bytes = line.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        if (b == b';' || b == b'#') && (i == 0 || bytes[i - 1].is_ascii_whitespace()) {
            return &line[..i];
        }
    }
    line
}

fn lookup(section: &[(String, String)], key: &str) -> Option<String> {
    section
        .iter()
        .rev()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.clone())
        .filter(|v| !v.is_empty())
}

#[cfg(test)]
#[path = "aws_creds_tests.rs"]
mod aws_creds_tests;
