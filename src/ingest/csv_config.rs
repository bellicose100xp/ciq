//! The `[csv]` TOML config section — the ingest dialect/type-override shape and its
//! [`CsvOpts`](super::csv_opts::CsvOpts) projection (`dev/PLAN.md` §6.6).
//!
//! The [`CsvConfig`] type + its `to_opts()` projection live **here**, next to
//! [`CsvOpts`](super::csv_opts), so the precedence merge's `config` input is built in one place.
//! Config *loading*, by contrast, was subsumed by the Phase 5 [`config`](crate::config) module:
//! the `[csv]` section is now parsed as part of the whole [`Config`](crate::config::Config), and
//! [`load_csv_config_str`] is a thin shim that delegates to
//! [`config::load_config_str`](crate::config::load_config_str) and projects out the `[csv]`
//! section — so a single parse path covers every section (DRY) while existing
//! `ingest::load_csv_config_str` callers compile unchanged.
//!
//! Parsed from a string so it is unit-tested over in-memory TOML with no filesystem; the CLI
//! wiring reads the file and hands the contents in.

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

/// Parse the `[csv]` section from a full-config TOML text. A thin shim over the Phase 5
/// [`config::load_config_str`](crate::config::load_config_str) (which owns the single parse path
/// for every section), projecting out the `[csv]` section. Returns the default (empty) `[csv]`
/// config on **any** parse error — the jiq pattern: a malformed config never blocks startup, it
/// just falls back to defaults. An absent `[csv]` table yields the default too.
pub fn load_csv_config_str(toml_text: &str) -> CsvConfig {
    crate::config::load_config_str(toml_text).config.csv
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
