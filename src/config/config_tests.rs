//! Tests for the full config schema (`config.rs`) — exhaustive over in-memory TOML, no `$HOME`,
//! no filesystem. Drives [`load_config_str`]; never [`load_config`] (which would read the env).

use super::ai_config::{DEFAULT_API_KEY_ENV, DEFAULT_MODEL};
use super::general::GeneralConfig;
use super::history_config::DEFAULT_MAX_ENTRIES;
use super::{AiProviderType, Config, ThemeMode, load_config_str};

fn cfg(toml: &str) -> Config {
    let r = load_config_str(toml);
    assert!(r.warning.is_none(), "unexpected warning: {:?}", r.warning);
    r.config
}

// --- defaults: an empty file yields every conservative built-in ---

#[test]
fn empty_string_is_all_defaults() {
    let r = load_config_str("");
    assert!(r.warning.is_none());
    assert_eq!(r.config, Config::default());
}

#[test]
fn default_accessors_fold_in_built_ins() {
    let c = Config::default();
    assert_eq!(c.general().row_limit(), None, "uncapped by default");
    assert_eq!(c.general().threads(), None);
    assert_eq!(c.general().memory_limit(), None);
    assert_eq!(c.theme().mode(), ThemeMode::Auto);
    assert!(!c.ai().is_active());
    assert_eq!(c.ai().model(), DEFAULT_MODEL);
    assert_eq!(c.ai().api_key_env(), DEFAULT_API_KEY_ENV);
    assert!(c.history().enabled());
    assert_eq!(c.history().max_entries(), DEFAULT_MAX_ENTRIES);
    assert_eq!(c.history().path(), None);
}

// --- [general] ---

#[test]
fn general_parses_all_keys() {
    let c = cfg(r#"
        [general]
        row_limit = 250
        threads = 4
        memory_limit = "4GB"
    "#);
    assert_eq!(c.general().row_limit(), Some(250));
    assert_eq!(c.general().threads(), Some(4));
    assert_eq!(c.general().memory_limit(), Some("4GB"));
}

#[test]
fn general_zero_row_limit_means_uncapped() {
    let g = GeneralConfig {
        row_limit: Some(0),
        ..GeneralConfig::default()
    };
    assert_eq!(g.row_limit(), None, "0 = explicitly uncapped");
}

// --- [theme] ---

#[test]
fn theme_mode_and_overrides() {
    let c = cfg(r#"
        [theme]
        mode = "dark"
        [theme.overrides]
        "grid.header" = "Cyan"
    "#);
    assert_eq!(c.theme().mode(), ThemeMode::Dark);
    assert_eq!(c.theme().override_for("grid.header"), Some("Cyan"));
    assert_eq!(c.theme().override_for("absent"), None);
}

#[test]
fn theme_mode_light() {
    assert_eq!(
        cfg("[theme]\nmode = \"light\"\n").theme().mode(),
        ThemeMode::Light
    );
}

// --- [ai]: no secret in the file; key named by an env var ---

#[test]
fn ai_active_only_when_enabled_and_provider_set() {
    let c = cfg(r#"
        [ai]
        enabled = true
        provider = "anthropic"
        model = "claude-x"
        api_key_env = "MY_KEY"
    "#);
    assert!(c.ai().is_active());
    assert_eq!(c.ai().provider, AiProviderType::Anthropic);
    assert_eq!(c.ai().model(), "claude-x");
    assert_eq!(c.ai().api_key_env(), "MY_KEY");
}

#[test]
fn ai_enabled_but_provider_none_is_inactive() {
    let c = cfg("[ai]\nenabled = true\nprovider = \"none\"\n");
    assert!(!c.ai().is_active());
}

#[test]
fn ai_provider_set_but_disabled_is_inactive() {
    let c = cfg("[ai]\nenabled = false\nprovider = \"anthropic\"\n");
    assert!(!c.ai().is_active());
}

// --- [history] ---

#[test]
fn history_parses_all_keys() {
    let c = cfg(r#"
        [history]
        enabled = false
        max_entries = 50
        path = "/tmp/h.txt"
    "#);
    assert!(!c.history().enabled());
    assert_eq!(c.history().max_entries(), 50);
    assert_eq!(c.history().path(), Some("/tmp/h.txt"));
}

#[test]
fn history_zero_max_clamps_to_one() {
    let c = cfg("[history]\nmax_entries = 0\n");
    assert_eq!(c.history().max_entries(), 1);
}

// --- [csv]: subsumed, projects to CsvOpts ---

#[test]
fn csv_section_projects_to_opts() {
    let c = cfg(r#"
        [csv]
        delimiter = ";"
        header = false
    "#);
    let opts = c.csv().to_opts();
    assert_eq!(opts.delimiter, Some(';'));
    assert_eq!(opts.header, Some(false));
}

// --- forward-compat: unknown top-level tables tolerated; known sections validate strictly ---

#[test]
fn unknown_top_level_table_is_tolerated() {
    // A future [export] block won't break an older binary.
    let r = load_config_str("[export]\nformat = \"csv\"\n[general]\nrow_limit = 7\n");
    assert!(r.warning.is_none());
    assert_eq!(r.config.general().row_limit(), Some(7));
}

#[test]
fn unknown_csv_key_falls_back_to_default_with_warning() {
    // deny_unknown_fields on [csv]: a typo fails the parse -> safe defaults + warning.
    let r = load_config_str("[csv]\ndelimeter = \";\"\n"); // misspelled
    assert!(r.warning.is_some());
    assert_eq!(r.config, Config::default());
}

// Every known section validates its own keys (the module-doc contract) — not just [csv]. A typo'd
// key in [general]/[theme]/[ai]/[history] fails the parse -> safe defaults + a warning, so the user
// gets a signal instead of the feature silently running on the default.

#[test]
fn unknown_general_key_falls_back_to_default_with_warning() {
    let r = load_config_str("[general]\nrowlimit = 250\n"); // should be row_limit
    assert!(r.warning.is_some());
    assert_eq!(r.config, Config::default());
}

#[test]
fn unknown_theme_key_falls_back_to_default_with_warning() {
    // A stray top-level [theme] key is rejected; the [theme.overrides] map stays open.
    let r = load_config_str("[theme]\nmod = \"dark\"\n"); // should be mode
    assert!(r.warning.is_some());
    assert_eq!(r.config, Config::default());
}

#[test]
fn theme_overrides_map_still_accepts_arbitrary_inner_keys() {
    // deny_unknown_fields rejects unknown [theme] keys but never the open overrides map.
    let c = cfg("[theme.overrides]\n\"some.future.surface\" = \"Cyan\"\n");
    assert_eq!(c.theme().override_for("some.future.surface"), Some("Cyan"));
}

#[test]
fn unknown_history_key_falls_back_to_default_with_warning() {
    let r = load_config_str("[history]\nmax_entry = 50\n"); // should be max_entries
    assert!(r.warning.is_some());
    assert_eq!(r.config, Config::default());
}

#[test]
fn raw_api_key_in_ai_section_is_rejected_with_warning() {
    // Secret hygiene: a user who mistypes `api_key_env` as `api_key` and pastes a real secret gets
    // a warning that the key is being ignored, rather than the secret silently sitting at rest in a
    // plaintext config. (The key is only ever read from the env var regardless — this is the
    // defensive signal, not a credential path.)
    let r = load_config_str("[ai]\nenabled = true\napi_key = \"sk-SECRET\"\n");
    assert!(
        r.warning.is_some(),
        "a raw api_key in [ai] must surface a warning"
    );
    assert_eq!(r.config, Config::default());
    assert!(!r.config.ai().is_active());
}

#[test]
fn malformed_toml_falls_back_to_default_with_warning() {
    let r = load_config_str("this = = not valid");
    assert!(r.warning.is_some());
    assert_eq!(r.config, Config::default());
}

#[test]
fn all_sections_together() {
    let c = cfg(r#"
        [general]
        row_limit = 100
        [theme]
        mode = "light"
        [ai]
        enabled = true
        provider = "anthropic"
        [history]
        max_entries = 200
        [csv]
        delimiter = "\t"
    "#);
    assert_eq!(c.general().row_limit(), Some(100));
    assert_eq!(c.theme().mode(), ThemeMode::Light);
    assert!(c.ai().is_active());
    assert_eq!(c.history().max_entries(), 200);
    assert_eq!(c.csv().to_opts().delimiter, Some('\t'));
}
