//! The full ciq config schema (`dev/PLAN.md` §0/Q5 — the Phase 5 deliverable that **subsumes**
//! the minimal `[csv]` loader of Phase 4).
//!
//! One [`Config`] parsed from `~/.config/ciq/config.toml` (XDG), with five sections:
//!  - `[general]` — default `LIMIT N`, DuckDB threads + memory cap ([`GeneralConfig`]);
//!  - `[theme]` — light/dark mode + a forward-compat override map ([`ThemeConfig`], a stub
//!    `theme.rs` reads once the polarity pass lands);
//!  - `[ai]` — NL->SQL provider/model + the **env var** naming the API key, never the key itself
//!    ([`AiConfig`]; the provider is wired in P5.1);
//!  - `[history]` — persistence on/off, entry cap, file path ([`HistoryConfig`]);
//!  - `[csv]` — the ingest dialect/type overrides, the same [`CsvConfig`] Phase 4 introduced (the
//!    `to_opts()` projection still lives next to [`CsvOpts`](crate::ingest::CsvOpts) in
//!    [`ingest`](crate::ingest), so the precedence merge is unchanged).
//!
//! **Subsumes `ingest/csv_config.rs`'s loader.** That file still *owns* the `[csv]` shape +
//! `to_opts` projection (it lives with `CsvOpts`); this module owns *config loading* — the
//! `[csv]` section is now parsed as part of the whole [`Config`], and
//! [`Config::csv`](Config::csv)`.to_opts()` is the input `ingest::merge` consumes. The thin
//! [`ingest::load_csv_config_str`](crate::ingest::load_csv_config_str) shim is retained
//! (delegating here) so existing callers compile unchanged.
//!
//! **Parsed from a string** ([`load_config_str`]) so it is exhaustively unit-tested over in-memory
//! TOML with **no `$HOME`/filesystem**. The real on-disk read ([`load_config`]) resolves the XDG
//! path from env vars and is the only filesystem touch; tests never invoke it.
//!
//! **Forward-compat by construction:** unknown *top-level* tables are tolerated (a future
//! `[export]` block won't break an older binary), while each known section validates its own keys
//! (every section carries `deny_unknown_fields`). A typo'd key in any known section therefore
//! surfaces a warning instead of being silently dropped — which for `[ai]` is secret hygiene: a
//! stray `api_key = "sk-…"` (a natural mistake for `api_key_env`) is rejected and warned about
//! rather than left at rest in a plaintext config. A parse error never blocks startup — it falls
//! back to defaults and surfaces a warning (jiq's parse-to-default-on-error pattern).

use serde::Deserialize;

use crate::ingest::CsvConfig;

pub mod ai_config;
pub mod general;
pub mod history_config;
pub mod theme_config;

pub use ai_config::{AiConfig, AiProviderType};
pub use general::GeneralConfig;
pub use history_config::HistoryConfig;
pub use theme_config::{ThemeConfig, ThemeMode};

/// The whole parsed config. Every section has a `Default`, so an empty file (or any absent
/// section) yields the conservative built-ins. Unknown top-level tables are ignored
/// (forward-compat); known sections validate their own keys.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Config {
    /// Engine-wide defaults: row limit, threads, memory cap.
    pub general: GeneralConfig,
    /// Theme / polarity settings (a minimal stub for now).
    pub theme: ThemeConfig,
    /// AI NL->SQL provider settings (no secret — the key is named by an env var).
    pub ai: AiConfig,
    /// Query-history persistence settings.
    pub history: HistoryConfig,
    /// CSV ingest dialect + type overrides (the Phase 4 `[csv]` section, subsumed here).
    pub csv: CsvConfig,
}

impl Config {
    /// The `[general]` section.
    pub fn general(&self) -> &GeneralConfig {
        &self.general
    }

    /// The `[theme]` section.
    pub fn theme(&self) -> &ThemeConfig {
        &self.theme
    }

    /// The `[ai]` section.
    pub fn ai(&self) -> &AiConfig {
        &self.ai
    }

    /// The `[history]` section.
    pub fn history(&self) -> &HistoryConfig {
        &self.history
    }

    /// The `[csv]` section.
    pub fn csv(&self) -> &CsvConfig {
        &self.csv
    }
}

/// The result of loading a config: the (possibly default) config plus an optional warning to
/// surface (a read or parse failure). Mirrors jiq's `ConfigResult` so the CLI can show "invalid
/// config, using defaults: …" without the load failing.
#[derive(Debug, Clone)]
pub struct ConfigResult {
    /// The effective config (defaults on any failure).
    pub config: Config,
    /// A warning to surface, if the file was present but unreadable/unparseable.
    pub warning: Option<String>,
}

/// Parse a full [`Config`] from TOML text. Returns the default (all sections default) config on
/// **any** parse error — the jiq pattern: a malformed config never blocks startup, it just falls
/// back to defaults (the CLI surfaces the warning). An empty string yields the default too.
///
/// Pure — no filesystem, no `$HOME` — so it is exhaustively unit-tested over in-memory TOML.
pub fn load_config_str(toml_text: &str) -> ConfigResult {
    match toml::from_str::<Config>(toml_text) {
        Ok(config) => ConfigResult {
            config,
            warning: None,
        },
        Err(e) => {
            log::warn!("invalid config, using defaults: {e}");
            ConfigResult {
                config: Config::default(),
                warning: Some(format!("invalid config, using defaults: {e}")),
            }
        }
    }
}

/// Resolve the on-disk config path: `$XDG_CONFIG_HOME/ciq/config.toml`, else
/// `$HOME/.config/ciq/config.toml`. `None` when neither env var is set (so the caller falls back
/// to defaults). The **only** `$HOME`/env touch in this module — kept out of [`load_config_str`]
/// so the parse logic stays filesystem-free and the unit tests never read the environment.
pub fn config_path() -> Option<std::path::PathBuf> {
    if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME").filter(|s| !s.is_empty()) {
        return Some(std::path::Path::new(&xdg).join("ciq").join("config.toml"));
    }
    let home = std::env::var_os("HOME")?;
    Some(
        std::path::Path::new(&home)
            .join(".config")
            .join("ciq")
            .join("config.toml"),
    )
}

/// Load the config from the resolved XDG path, or the default when the file is absent. A present
/// but unreadable/unparseable file yields the default config plus a warning. The filesystem edge
/// of this module (the CLI calls it once at startup); the unit tests drive [`load_config_str`]
/// instead and never touch `$HOME`.
pub fn load_config() -> ConfigResult {
    let Some(path) = config_path() else {
        return ConfigResult {
            config: Config::default(),
            warning: None,
        };
    };
    if !path.exists() {
        return ConfigResult {
            config: Config::default(),
            warning: None,
        };
    }
    match std::fs::read_to_string(&path) {
        Ok(text) => load_config_str(&text),
        Err(e) => ConfigResult {
            config: Config::default(),
            warning: Some(format!("failed to read config {}: {e}", path.display())),
        },
    }
}

#[cfg(test)]
#[path = "config/config_tests.rs"]
mod config_tests;
