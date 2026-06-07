//! The minimal `[csv]` TOML config section — the *config* layer of the ingest precedence
//! (`dev/PLAN.md` §6.6).
//!
//! The full ciq config schema (theme, default LIMIT, memory cap, history, the rest of the
//! `[csv]` keys) is **Q5, a Phase 5 deliverable** — locking it now, before the feature set
//! stabilizes, invites churn (§8/Q5). So this is deliberately a *minimal `[csv]`-only* loader: it
//! parses just enough to supply the `config` input to [`merge`](super::csv_opts::merge), reusing
//! jiq's TOML load/validate shape (parse-to-default-on-error, never panic on a bad file). When the
//! Phase 5 config module lands it subsumes this `[csv]` section.
//!
//! Parsed from a string ([`load_csv_config_str`]) so it is unit-tested over in-memory TOML with no
//! filesystem; the CLI wiring reads the file and hands the contents in.

use serde::Deserialize;

use super::csv_opts::{ColumnTypeOverride, CsvOpts};

/// The `[csv]` config section. Every key optional; an absent key stays `None` in the projected
/// [`CsvOpts`] (defer to sniffed / DuckDB). Mirrors the CLI flags one-for-one (§6.6 / R5).
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct CsvConfig {
    /// Field delimiter, as a one-character string (`delimiter = ";"`).
    pub delimiter: Option<String>,
    /// Quote character, as a one-character string.
    pub quote: Option<String>,
    /// Escape character, as a one-character string.
    pub escape: Option<String>,
    /// Whether the first row is a header.
    pub header: Option<bool>,
    /// String that ingests as SQL NULL (the Q12 lever).
    pub null_string: Option<String>,
    /// Rows the sniffer samples (`-1` = whole file).
    pub sample_size: Option<i64>,
    /// Per-column type overrides as a `column = "TYPE"` table (`[csv.types]`).
    pub types: Option<std::collections::BTreeMap<String, String>>,
    /// Ingest every column as VARCHAR.
    pub all_varchar: Option<bool>,
    /// Explicit date parse format.
    pub date_format: Option<String>,
}

/// The top-level config shape this loader cares about: just the `[csv]` table. Unknown *top-level*
/// tables are allowed (the full Phase 5 config will add `[theme]`, `[ai]`, … alongside) — only the
/// `[csv]` section's own keys are validated strictly.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
struct ConfigFile {
    csv: CsvConfig,
}

impl CsvConfig {
    /// Project the parsed `[csv]` section into a [`CsvOpts`] for [`merge`](super::csv_opts::merge).
    ///
    /// A one-character string becomes the `char` field; a multi-character delimiter/quote/escape
    /// string is ignored (left `None`) rather than silently truncated — a malformed override
    /// defers to the layer below instead of corrupting the dialect.
    pub fn to_opts(&self) -> CsvOpts {
        CsvOpts {
            delimiter: self.delimiter.as_deref().and_then(one_char),
            quote: self.quote.as_deref().and_then(one_char),
            escape: self.escape.as_deref().and_then(one_char),
            header: self.header,
            null_string: self.null_string.clone(),
            sample_size: self.sample_size,
            types: self.types.as_ref().map(|m| {
                m.iter()
                    .map(|(name, ty)| ColumnTypeOverride::new(name.clone(), ty.clone()))
                    .collect()
            }),
            all_varchar: self.all_varchar,
            date_format: self.date_format.clone(),
        }
    }
}

/// Parse a `[csv]` config from TOML text. Returns the default (empty) config on **any** parse
/// error — the jiq pattern: a malformed config never blocks startup, it just falls back to
/// defaults (the CLI surfaces a warning). An absent `[csv]` table yields the default too.
pub fn load_csv_config_str(toml_text: &str) -> CsvConfig {
    match toml::from_str::<ConfigFile>(toml_text) {
        Ok(cfg) => cfg.csv,
        Err(e) => {
            log::warn!("invalid [csv] config, using defaults: {e}");
            CsvConfig::default()
        }
    }
}

/// Return the single `char` of a one-character string, else `None` (empty or multi-char).
fn one_char(s: &str) -> Option<char> {
    let mut chars = s.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) => Some(c),
        _ => None,
    }
}

#[cfg(test)]
#[path = "csv_config_tests.rs"]
mod csv_config_tests;
