use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::*;

const CREDENTIALS_INI: &str = "\
[default]
aws_access_key_id = AKIADEFAULT
aws_secret_access_key = defaultsecret

[work]
aws_access_key_id=AKIAWORK
aws_secret_access_key =  worksecret
aws_session_token = worktoken
";

const CONFIG_INI: &str = "\
[default]
region = us-west-2

[profile work]
region = eu-central-1
aws_access_key_id = AKIACONFIGWORK
aws_secret_access_key = configworksecret

[profile sso]
sso_start_url = https://example.awsapps.com/start
sso_region = us-east-1

[profile proc]
credential_process=/usr/local/bin/get-creds print --profile=work

[profile assume]
role_arn = arn:aws:iam::123456789012:role/Dev
source_profile = default

[profile broken-role]
role_arn = arn:aws:iam::123456789012:role/Dev

[profile web]
web_identity_token_file = /var/run/token

[profile sess]
sso_session = my-sso
";

type Env = HashMap<&'static str, &'static str>;
type Files = HashMap<PathBuf, &'static str>;

fn env_fn(env: &Env) -> impl Fn(&str) -> Option<String> + '_ {
    move |key| env.get(key).map(|v| (*v).to_owned())
}

fn files_fn(files: &Files) -> impl Fn(&Path) -> Option<String> + '_ {
    move |path| files.get(path).map(|v| (*v).to_owned())
}

/// A runner that must never be reached; env or static keys must have satisfied the chain first.
fn no_process() -> impl Fn(&str) -> Result<String, String> {
    |command| panic!("credential_process should not run, got `{command}`")
}

fn canned_process(json: &'static str) -> impl Fn(&str) -> Result<String, String> {
    move |_| Ok(json.to_owned())
}

const PROCESS_JSON: &str = r#"{"AccessKeyId":"ASIAPROC","Expiration":"2026-09-09T12:00:00Z","SecretAccessKey":"procsecret","SessionToken":"proctoken","Version":1}"#;

fn home() -> PathBuf {
    PathBuf::from("/home/tester")
}

fn creds(key: &str, secret: &str, token: Option<&str>) -> Credentials {
    Credentials {
        access_key_id: key.to_owned(),
        secret_access_key: secret.to_owned(),
        session_token: token.map(str::to_owned),
    }
}

fn default_files() -> Files {
    let mut files = Files::new();
    files.insert(home().join(".aws/credentials"), CREDENTIALS_INI);
    files.insert(home().join(".aws/config"), CONFIG_INI);
    files
}

// ---- parse_credentials ----

#[test]
fn parse_credentials_bare_header_default_and_named() {
    assert_eq!(
        parse_credentials(CREDENTIALS_INI, "default"),
        Some(creds("AKIADEFAULT", "defaultsecret", None))
    );
    assert_eq!(
        parse_credentials(CREDENTIALS_INI, "work"),
        Some(creds("AKIAWORK", "worksecret", Some("worktoken")))
    );
}

#[test]
fn parse_credentials_profile_prefixed_header() {
    assert_eq!(
        parse_credentials(CONFIG_INI, "work"),
        Some(creds("AKIACONFIGWORK", "configworksecret", None))
    );
}

#[test]
fn parse_credentials_missing_profile_or_keys() {
    assert_eq!(parse_credentials(CREDENTIALS_INI, "nope"), None);
    // Exists in config but carries only a region, no keys.
    assert_eq!(parse_credentials(CONFIG_INI, "default"), None);
    let only_key = "[default]\naws_access_key_id = AKIA\n";
    assert_eq!(parse_credentials(only_key, "default"), None);
    let only_secret = "[default]\naws_secret_access_key = s\n";
    assert_eq!(parse_credentials(only_secret, "default"), None);
}

#[test]
fn parse_credentials_tolerates_comments_blank_lines_crlf_and_case() {
    let text = "; leading comment\r\n\r\n[Default]\r\n# another\r\nAWS_ACCESS_KEY_ID = AKIA ; trailing\r\naws_secret_access_key = se#cret # comment\r\n\r\n";
    assert_eq!(
        parse_credentials(text, "Default"),
        Some(creds("AKIA", "se#cret", None))
    );
}

#[test]
fn parse_credentials_profile_names_are_case_sensitive() {
    assert_eq!(parse_credentials(CREDENTIALS_INI, "Work"), None);
}

#[test]
fn parse_credentials_prefers_the_profile_prefixed_header_when_both_shapes_present() {
    let text = "[profile dup]\naws_access_key_id = A1\naws_secret_access_key = S1\n[dup]\naws_access_key_id = A2\naws_secret_access_key = S2\n";
    // Both shapes name the same profile; the SDK lets `[profile x]` replace bare `[x]` outright,
    // whatever their order in the file.
    assert_eq!(
        parse_credentials(text, "dup"),
        Some(creds("A1", "S1", None))
    );
    let reversed = "[dup]\naws_access_key_id = A2\naws_secret_access_key = S2\n[profile dup]\naws_access_key_id = A1\naws_secret_access_key = S1\n";
    assert_eq!(
        parse_credentials(reversed, "dup"),
        Some(creds("A1", "S1", None))
    );
}

#[test]
fn parse_credentials_ignores_lines_without_equals_and_empty_values() {
    let text = "[default]\ngarbage line\naws_access_key_id = AKIA\naws_secret_access_key =\n";
    assert_eq!(parse_credentials(text, "default"), None);
}

#[test]
fn parse_credentials_does_not_treat_profile_prefix_without_space_as_profile() {
    let text = "[profilework]\naws_access_key_id = A\naws_secret_access_key = S\n";
    assert_eq!(parse_credentials(text, "work"), None);
    assert_eq!(
        parse_credentials(text, "profilework"),
        Some(creds("A", "S", None))
    );
}

// ---- parse_region ----

#[test]
fn parse_region_both_header_shapes() {
    assert_eq!(
        parse_region(CONFIG_INI, "default"),
        Some("us-west-2".into())
    );
    assert_eq!(
        parse_region(CONFIG_INI, "work"),
        Some("eu-central-1".into())
    );
    assert_eq!(parse_region(CONFIG_INI, "sso"), None);
    assert_eq!(parse_region(CONFIG_INI, "missing"), None);
}

// ---- unsupported_mechanism ----

#[test]
fn unsupported_mechanism_detects_each_kind() {
    assert_eq!(
        unsupported_mechanism(CONFIG_INI, "sso"),
        Some("sso_start_url")
    );
    assert_eq!(
        unsupported_mechanism(CONFIG_INI, "sess"),
        Some("sso_session")
    );
    assert_eq!(
        unsupported_mechanism(CONFIG_INI, "web"),
        Some("web_identity_token_file")
    );
    assert_eq!(
        unsupported_mechanism(CONFIG_INI, "assume"),
        Some("role_arn")
    );
}

#[test]
fn unsupported_mechanism_none_for_static_keys_missing_or_lone_role_arn() {
    assert_eq!(unsupported_mechanism(CONFIG_INI, "work"), None);
    assert_eq!(unsupported_mechanism(CONFIG_INI, "missing"), None);
    assert_eq!(unsupported_mechanism(CONFIG_INI, "broken-role"), None);
}

#[test]
fn unsupported_mechanism_no_longer_flags_credential_process() {
    assert_eq!(unsupported_mechanism(CONFIG_INI, "proc"), None);
}

// ---- parse_credential_process ----

#[test]
fn parse_credential_process_both_header_shapes_and_no_spaces_form() {
    assert_eq!(
        parse_credential_process(CONFIG_INI, "proc"),
        Some("/usr/local/bin/get-creds print --profile=work".into())
    );
    let bare = "[proc]\ncredential_process = /usr/bin/tool creds\n";
    assert_eq!(
        parse_credential_process(bare, "proc"),
        Some("/usr/bin/tool creds".into())
    );
    assert_eq!(parse_credential_process(CONFIG_INI, "work"), None);
    assert_eq!(parse_credential_process(CONFIG_INI, "missing"), None);
}

// ---- parse_process_credentials ----

#[test]
fn parse_process_credentials_valid() {
    assert_eq!(
        parse_process_credentials(PROCESS_JSON),
        Ok(creds("ASIAPROC", "procsecret", Some("proctoken")))
    );
}

#[test]
fn parse_process_credentials_without_session_token() {
    let json = r#"{"Version": 1, "AccessKeyId": "AKIA", "SecretAccessKey": "s"}"#;
    assert_eq!(
        parse_process_credentials(json),
        Ok(creds("AKIA", "s", None))
    );
}

#[test]
fn parse_process_credentials_wrong_version() {
    let json = r#"{"Version": 2, "AccessKeyId": "AKIA", "SecretAccessKey": "s"}"#;
    let err = parse_process_credentials(json).expect_err("version 2");
    assert!(err.contains("Version 2"), "{err}");
    let json = r#"{"AccessKeyId": "AKIA", "SecretAccessKey": "s"}"#;
    let err = parse_process_credentials(json).expect_err("no version");
    assert!(err.contains("missing Version"), "{err}");
}

#[test]
fn parse_process_credentials_missing_required_fields() {
    let err = parse_process_credentials(r#"{"Version": 1, "SecretAccessKey": "s"}"#)
        .expect_err("no key id");
    assert!(err.contains("missing AccessKeyId"), "{err}");
    let err = parse_process_credentials(r#"{"Version": 1, "AccessKeyId": "AKIA"}"#)
        .expect_err("no secret");
    assert!(err.contains("missing SecretAccessKey"), "{err}");
    let err =
        parse_process_credentials(r#"{"Version": 1, "AccessKeyId": "", "SecretAccessKey": "s"}"#)
            .expect_err("empty key id");
    assert!(err.contains("missing AccessKeyId"), "{err}");
}

#[test]
fn parse_process_credentials_malformed_json() {
    let err = parse_process_credentials("not json").expect_err("malformed");
    assert!(err.contains("not valid JSON"), "{err}");
    let err = parse_process_credentials("[1, 2]").expect_err("array");
    assert!(err.contains("not a JSON object"), "{err}");
}

// ---- run_credential_process ----

#[test]
fn run_credential_process_nonexistent_binary_errors() {
    let err = run_credential_process("/nonexistent/ciq-test-binary credentials print")
        .expect_err("missing binary");
    assert!(err.contains("/nonexistent/ciq-test-binary"), "{err}");
    assert!(run_credential_process("   ").is_err());
}

// ---- effective_profile ----

#[test]
fn effective_profile_precedence() {
    let mut env = Env::new();
    assert_eq!(effective_profile(None, &env_fn(&env)), "default");
    env.insert("AWS_PROFILE", "");
    assert_eq!(effective_profile(None, &env_fn(&env)), "default");
    env.insert("AWS_PROFILE", "fromenv");
    assert_eq!(effective_profile(None, &env_fn(&env)), "fromenv");
    assert_eq!(
        effective_profile(Some("explicit"), &env_fn(&env)),
        "explicit"
    );
    assert_eq!(effective_profile(Some(""), &env_fn(&env)), "fromenv");
}

// ---- resolve_credentials ----

#[test]
fn resolve_credentials_env_wins_over_files() {
    let mut env = Env::new();
    env.insert("AWS_ACCESS_KEY_ID", "AKIAENV");
    env.insert("AWS_SECRET_ACCESS_KEY", "envsecret");
    env.insert("AWS_SESSION_TOKEN", "envtoken");
    let files = default_files();
    let got = resolve_credentials(
        Some("work"),
        &env_fn(&env),
        &files_fn(&files),
        &no_process(),
        Some(&home()),
    );
    assert_eq!(got, Ok(creds("AKIAENV", "envsecret", Some("envtoken"))));
}

#[test]
fn resolve_credentials_env_without_token() {
    let mut env = Env::new();
    env.insert("AWS_ACCESS_KEY_ID", "AKIAENV");
    env.insert("AWS_SECRET_ACCESS_KEY", "envsecret");
    env.insert("AWS_SESSION_TOKEN", "");
    let files = Files::new();
    let got = resolve_credentials(None, &env_fn(&env), &files_fn(&files), &no_process(), None);
    assert_eq!(got, Ok(creds("AKIAENV", "envsecret", None)));
}

#[test]
fn resolve_credentials_partial_env_falls_through_to_files() {
    let mut env = Env::new();
    env.insert("AWS_ACCESS_KEY_ID", "AKIAENV");
    let files = default_files();
    let got = resolve_credentials(
        None,
        &env_fn(&env),
        &files_fn(&files),
        &no_process(),
        Some(&home()),
    );
    assert_eq!(got, Ok(creds("AKIADEFAULT", "defaultsecret", None)));
}

#[test]
fn resolve_credentials_reads_credentials_file_for_named_profile() {
    let env = Env::new();
    let files = default_files();
    let got = resolve_credentials(
        Some("work"),
        &env_fn(&env),
        &files_fn(&files),
        &no_process(),
        Some(&home()),
    );
    assert_eq!(got, Ok(creds("AKIAWORK", "worksecret", Some("worktoken"))));
}

#[test]
fn resolve_credentials_uses_aws_profile_env() {
    let mut env = Env::new();
    env.insert("AWS_PROFILE", "work");
    let files = default_files();
    let got = resolve_credentials(
        None,
        &env_fn(&env),
        &files_fn(&files),
        &no_process(),
        Some(&home()),
    );
    assert_eq!(got, Ok(creds("AKIAWORK", "worksecret", Some("worktoken"))));
}

#[test]
fn resolve_credentials_falls_back_to_config_file_keys() {
    let env = Env::new();
    let mut files = Files::new();
    files.insert(home().join(".aws/config"), CONFIG_INI);
    let got = resolve_credentials(
        Some("work"),
        &env_fn(&env),
        &files_fn(&files),
        &no_process(),
        Some(&home()),
    );
    assert_eq!(got, Ok(creds("AKIACONFIGWORK", "configworksecret", None)));
}

#[test]
fn resolve_credentials_credentials_file_wins_over_config_file() {
    let env = Env::new();
    let files = default_files();
    let got = resolve_credentials(
        Some("work"),
        &env_fn(&env),
        &files_fn(&files),
        &no_process(),
        Some(&home()),
    );
    assert_eq!(
        got.map(|c| c.access_key_id),
        Ok("AKIAWORK".to_owned()),
        "credentials file must take precedence over config"
    );
}

#[test]
fn resolve_credentials_honors_file_path_overrides() {
    let mut env = Env::new();
    env.insert("AWS_SHARED_CREDENTIALS_FILE", "/etc/alt-creds");
    env.insert("AWS_CONFIG_FILE", "/etc/alt-config");
    let mut files = Files::new();
    files.insert(home().join(".aws/credentials"), CREDENTIALS_INI);
    files.insert(
        PathBuf::from("/etc/alt-creds"),
        "[default]\naws_access_key_id = ALT\naws_secret_access_key = altsecret\n",
    );
    let got = resolve_credentials(
        None,
        &env_fn(&env),
        &files_fn(&files),
        &no_process(),
        Some(&home()),
    );
    assert_eq!(got, Ok(creds("ALT", "altsecret", None)));

    // Override to a missing path must not silently fall back to ~/.aws/credentials.
    env.insert("AWS_SHARED_CREDENTIALS_FILE", "/etc/missing");
    let got = resolve_credentials(
        None,
        &env_fn(&env),
        &files_fn(&files),
        &no_process(),
        Some(&home()),
    );
    assert!(got.is_err());
}

#[test]
fn resolve_credentials_no_home_skips_files() {
    let env = Env::new();
    let files = default_files();
    let got = resolve_credentials(None, &env_fn(&env), &files_fn(&files), &no_process(), None);
    let err = got.expect_err("no home => no file lookups");
    assert!(
        err.contains("no AWS credentials found for profile `default`"),
        "{err}"
    );
    assert!(
        err.contains("AWS_ACCESS_KEY_ID/AWS_SECRET_ACCESS_KEY"),
        "{err}"
    );
    assert!(err.contains("~/.aws/credentials"), "{err}");
    assert!(err.contains("AWS_ACCESS_KEY_ID"), "{err}");
    assert!(err.contains("credential_process"), "{err}");
}

#[test]
fn resolve_credentials_missing_profile_error_names_profile() {
    let env = Env::new();
    let files = default_files();
    let err = resolve_credentials(
        Some("ghost"),
        &env_fn(&env),
        &files_fn(&files),
        &no_process(),
        Some(&home()),
    )
    .expect_err("profile absent");
    assert!(err.contains("profile `ghost`"), "{err}");
}

#[test]
fn resolve_credentials_reports_unsupported_mechanism() {
    let env = Env::new();
    let files = default_files();
    for (profile, mechanism) in [
        ("sso", "sso_start_url"),
        ("sess", "sso_session"),
        ("assume", "role_arn"),
        ("web", "web_identity_token_file"),
    ] {
        let err = resolve_credentials(
            Some(profile),
            &env_fn(&env),
            &files_fn(&files),
            &no_process(),
            Some(&home()),
        )
        .expect_err("unsupported mechanism");
        assert_eq!(
            err,
            format!(
                "profile `{profile}` uses {mechanism}, which ciq cannot run; export static credentials or use a profile with keys"
            )
        );
    }
}

#[test]
fn resolve_credentials_runs_credential_process_from_config() {
    let env = Env::new();
    let files = default_files();
    let seen = std::cell::RefCell::new(Vec::new());
    let runner = |command: &str| {
        seen.borrow_mut().push(command.to_owned());
        Ok(PROCESS_JSON.to_owned())
    };
    let got = resolve_credentials(
        Some("proc"),
        &env_fn(&env),
        &files_fn(&files),
        &runner,
        Some(&home()),
    );
    assert_eq!(got, Ok(creds("ASIAPROC", "procsecret", Some("proctoken"))));
    assert_eq!(
        seen.borrow().as_slice(),
        ["/usr/local/bin/get-creds print --profile=work"]
    );
}

#[test]
fn resolve_credentials_runs_credential_process_from_credentials_file() {
    let env = Env::new();
    let mut files = Files::new();
    files.insert(
        home().join(".aws/credentials"),
        "[default]\ncredential_process = /bin/x\n",
    );
    let got = resolve_credentials(
        None,
        &env_fn(&env),
        &files_fn(&files),
        &canned_process(PROCESS_JSON),
        Some(&home()),
    );
    assert_eq!(got, Ok(creds("ASIAPROC", "procsecret", Some("proctoken"))));
}

#[test]
fn resolve_credentials_env_wins_over_credential_process() {
    let mut env = Env::new();
    env.insert("AWS_ACCESS_KEY_ID", "AKIAENV");
    env.insert("AWS_SECRET_ACCESS_KEY", "envsecret");
    let files = default_files();
    let got = resolve_credentials(
        Some("proc"),
        &env_fn(&env),
        &files_fn(&files),
        &no_process(),
        Some(&home()),
    );
    assert_eq!(got, Ok(creds("AKIAENV", "envsecret", None)));
}

#[test]
fn resolve_credentials_static_keys_win_over_credential_process_in_same_profile() {
    let env = Env::new();
    let mut files = Files::new();
    files.insert(
        home().join(".aws/credentials"),
        "[both]\naws_access_key_id = AKIASTATIC\naws_secret_access_key = s\n",
    );
    files.insert(
        home().join(".aws/config"),
        "[profile both]\ncredential_process = /bin/x\n",
    );
    let got = resolve_credentials(
        Some("both"),
        &env_fn(&env),
        &files_fn(&files),
        &no_process(),
        Some(&home()),
    );
    assert_eq!(got, Ok(creds("AKIASTATIC", "s", None)));

    // Static keys in the config file also beat a credential_process in the same file.
    files.insert(
        home().join(".aws/config"),
        "[profile both]\ncredential_process = /bin/x\naws_access_key_id = AKIACFG\naws_secret_access_key = s2\n",
    );
    files.remove(&home().join(".aws/credentials"));
    let got = resolve_credentials(
        Some("both"),
        &env_fn(&env),
        &files_fn(&files),
        &no_process(),
        Some(&home()),
    );
    assert_eq!(got, Ok(creds("AKIACFG", "s2", None)));
}

#[test]
fn resolve_credentials_process_runner_error_names_profile() {
    let env = Env::new();
    let files = default_files();
    let runner = |_: &str| Err("exited with exit status: 1: session expired".to_owned());
    let err = resolve_credentials(
        Some("proc"),
        &env_fn(&env),
        &files_fn(&files),
        &runner,
        Some(&home()),
    )
    .expect_err("runner failed");
    assert_eq!(
        err,
        "profile `proc` credential_process failed: exited with exit status: 1: session expired"
    );
}

#[test]
fn resolve_credentials_process_bad_output_names_profile() {
    let env = Env::new();
    let files = default_files();
    let err = resolve_credentials(
        Some("proc"),
        &env_fn(&env),
        &files_fn(&files),
        &canned_process("{}"),
        Some(&home()),
    )
    .expect_err("bad json");
    assert!(
        err.starts_with("profile `proc` credential_process failed:"),
        "{err}"
    );
    assert!(err.contains("missing Version"), "{err}");
}

// ---- resolve_region ----

#[test]
fn resolve_region_configured_wins() {
    let mut env = Env::new();
    env.insert("AWS_REGION", "ap-south-1");
    let files = default_files();
    let got = resolve_region(
        Some("us-west-1"),
        Some("work"),
        &env_fn(&env),
        &files_fn(&files),
        Some(&home()),
    );
    assert_eq!(got, "us-west-1");
}

#[test]
fn resolve_region_empty_configured_is_unset() {
    let mut env = Env::new();
    env.insert("AWS_REGION", "ap-south-1");
    let files = Files::new();
    let got = resolve_region(Some(""), None, &env_fn(&env), &files_fn(&files), None);
    assert_eq!(got, "ap-south-1");
}

#[test]
fn resolve_region_env_precedence() {
    let mut env = Env::new();
    env.insert("AWS_REGION", "ap-south-1");
    env.insert("AWS_DEFAULT_REGION", "sa-east-1");
    let files = default_files();
    assert_eq!(
        resolve_region(
            None,
            Some("work"),
            &env_fn(&env),
            &files_fn(&files),
            Some(&home())
        ),
        "ap-south-1"
    );
    env.insert("AWS_REGION", "");
    assert_eq!(
        resolve_region(
            None,
            Some("work"),
            &env_fn(&env),
            &files_fn(&files),
            Some(&home())
        ),
        "sa-east-1"
    );
}

#[test]
fn resolve_region_from_config_profile() {
    let env = Env::new();
    let files = default_files();
    assert_eq!(
        resolve_region(
            None,
            Some("work"),
            &env_fn(&env),
            &files_fn(&files),
            Some(&home())
        ),
        "eu-central-1"
    );
    assert_eq!(
        resolve_region(None, None, &env_fn(&env), &files_fn(&files), Some(&home())),
        "us-west-2"
    );
}

#[test]
fn resolve_region_uses_aws_profile_and_config_file_override() {
    let mut env = Env::new();
    env.insert("AWS_PROFILE", "alt");
    env.insert("AWS_CONFIG_FILE", "/etc/alt-config");
    let mut files = default_files();
    files.insert(
        PathBuf::from("/etc/alt-config"),
        "[profile alt]\nregion = ca-central-1\n",
    );
    assert_eq!(
        resolve_region(None, None, &env_fn(&env), &files_fn(&files), Some(&home())),
        "ca-central-1"
    );
}

#[test]
fn resolve_region_fallback_default() {
    let env = Env::new();
    let files = default_files();
    // Profile exists but has no region.
    assert_eq!(
        resolve_region(
            None,
            Some("sso"),
            &env_fn(&env),
            &files_fn(&files),
            Some(&home())
        ),
        "us-east-1"
    );
    // No home at all.
    assert_eq!(
        resolve_region(None, None, &env_fn(&env), &files_fn(&files), None),
        "us-east-1"
    );
    // Config file missing.
    assert_eq!(
        resolve_region(
            None,
            None,
            &env_fn(&env),
            &files_fn(&Files::new()),
            Some(&home())
        ),
        "us-east-1"
    );
}

// --- profiles merge across the two shared files ---

#[test]
fn a_profile_split_across_credentials_and_config_still_resolves() {
    let files: Files = HashMap::from([
        (
            PathBuf::from("/home/u/.aws/credentials"),
            "[work]\naws_access_key_id = AKIDSPLIT\n",
        ),
        (
            PathBuf::from("/home/u/.aws/config"),
            "[profile work]\naws_secret_access_key = split-secret\n",
        ),
    ]);
    let creds = resolve_credentials(
        Some("work"),
        &|_| None,
        &files_fn(&files),
        &no_process(),
        Some(Path::new("/home/u")),
    )
    .expect("the SDK merges one profile across both files, and so do we");
    assert_eq!(creds.access_key_id, "AKIDSPLIT");
    assert_eq!(creds.secret_access_key, "split-secret");
}

#[test]
fn the_credentials_file_wins_a_key_both_files_define() {
    let files: Files = HashMap::from([
        (
            PathBuf::from("/home/u/.aws/credentials"),
            "[work]\naws_access_key_id = AKIDFROMCREDS\naws_secret_access_key = s1\n",
        ),
        (
            PathBuf::from("/home/u/.aws/config"),
            "[profile work]\naws_access_key_id = AKIDFROMCONFIG\naws_secret_access_key = s2\n",
        ),
    ]);
    let creds = resolve_credentials(
        Some("work"),
        &|_| None,
        &files_fn(&files),
        &no_process(),
        Some(Path::new("/home/u")),
    )
    .unwrap();
    assert_eq!(creds.access_key_id, "AKIDFROMCREDS");
}

#[test]
fn profile_default_section_overrides_bare_default_in_the_same_file() {
    // The SDK's rule for the config file: `[profile default]` replaces `[default]` wholesale,
    // whatever order they appear in.
    let text = "[profile default]\naws_access_key_id = AKIDPREFIXED\naws_secret_access_key = p\n\n[default]\naws_access_key_id = AKIDBARE\naws_secret_access_key = b\naws_session_token = only-in-bare\n";
    let creds = parse_credentials(text, "default").unwrap();
    assert_eq!(creds.access_key_id, "AKIDPREFIXED");
    assert_eq!(
        creds.session_token, None,
        "nothing from the bare section leaks into the prefixed one"
    );
}

// --- credential_process command tokenizing ---

#[test]
fn split_command_handles_plain_quoted_and_escaped_tokens() {
    assert_eq!(
        split_command("/usr/local/bin/get-creds print --profile=work"),
        vec!["/usr/local/bin/get-creds", "print", "--profile=work"]
    );
    assert_eq!(
        split_command(r#""/opt/Credential Helper/tool" --profile "dev account""#),
        vec!["/opt/Credential Helper/tool", "--profile", "dev account"]
    );
    assert_eq!(
        split_command("tool 'single quoted arg' plain"),
        vec!["tool", "single quoted arg", "plain"]
    );
    assert_eq!(
        split_command(r"tool escaped\ space"),
        vec!["tool", "escaped space"]
    );
    assert_eq!(
        split_command(r#"tool "with \"inner\" quotes""#),
        vec!["tool", r#"with "inner" quotes"#]
    );
    assert_eq!(split_command("   "), Vec::<String>::new());
    assert_eq!(
        split_command(r#"tool """#),
        vec!["tool", ""],
        "an empty quoted arg is kept"
    );
}

// --- diagnostics never carry a credential to the screen ---

#[test]
fn redact_secrets_masks_key_ids_and_long_opaque_tokens() {
    assert_eq!(
        redact_secrets("refresh failed for AKIAIOSFODNN7EXAMPLE please retry"),
        "refresh failed for [redacted] please retry"
    );
    assert_eq!(
        redact_secrets("token ASIAIOSFODNN7EXAMPLE expired"),
        "token [redacted] expired"
    );
    let secret = "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY";
    assert_eq!(
        redact_secrets(&format!("key {secret} rejected")),
        "key [redacted] rejected",
        "a 40-char base64 run is masked"
    );
    assert_eq!(
        redact_secrets("session expired, run the login command"),
        "session expired, run the login command",
        "ordinary words pass through"
    );
}

#[test]
fn run_credential_process_reports_only_the_first_redacted_stderr_line() {
    // A real process that fails and prints a key-shaped token to stderr: `sh -c` is used ONLY as
    // the helper under test (it is the program the config names), never by ciq itself.
    let err = run_credential_process(
        "sh -c 'echo AKIAIOSFODNN7EXAMPLE_is_bad AKIAIOSFODNN7EXAMPLE >&2; echo second line >&2; exit 3'",
    )
    .unwrap_err();
    assert!(err.contains("exited with"), "{err}");
    assert!(
        !err.contains("AKIAIOSFODNN7EXAMPLE "),
        "the key id is masked: {err}"
    );
    assert!(err.contains("[redacted]"), "{err}");
    assert!(
        !err.contains("second line"),
        "only the first line is shown: {err}"
    );
}

#[test]
fn run_credential_process_survives_a_chatty_helper() {
    // More output than a pipe buffer holds: the readers drain concurrently, so this must not hang
    // or be mistaken for a timeout.
    let out = run_credential_process(
        "sh -c 'head -c 200000 /dev/zero | tr \\\\0 x >&2; echo {\\\"Version\\\":1}'",
    )
    .expect("a helper that floods stderr still returns its stdout");
    assert!(out.contains("Version"), "{out}");
}
