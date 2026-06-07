//! Tests for the minimal `[csv]` TOML config loader (`csv_config.rs`).

use super::super::csv_opts::ColumnTypeOverride;
use super::load_csv_config_str;

#[test]
fn empty_string_is_default() {
    let cfg = load_csv_config_str("");
    assert_eq!(cfg, super::CsvConfig::default());
    assert_eq!(cfg.to_opts(), crate::ingest::CsvOpts::default());
}

#[test]
fn absent_csv_section_is_default() {
    let cfg = load_csv_config_str("[other]\nkey = 1\n");
    assert_eq!(cfg.to_opts(), crate::ingest::CsvOpts::default());
}

#[test]
fn parses_basic_dialect_keys() {
    let toml = r#"
        [csv]
        delimiter = ";"
        quote = "'"
        header = false
        null_string = "NA"
        sample_size = 2000
    "#;
    let opts = load_csv_config_str(toml).to_opts();
    assert_eq!(opts.delimiter, Some(';'));
    assert_eq!(opts.quote, Some('\''));
    assert_eq!(opts.header, Some(false));
    assert_eq!(opts.null_string, Some("NA".to_string()));
    assert_eq!(opts.sample_size, Some(2000));
}

#[test]
fn multi_char_delimiter_is_ignored_not_truncated() {
    // A malformed multi-char delimiter defers (None) rather than silently using the first char.
    let opts = load_csv_config_str("[csv]\ndelimiter = \",,\"\n").to_opts();
    assert_eq!(opts.delimiter, None);
}

#[test]
fn empty_delimiter_string_is_none() {
    let opts = load_csv_config_str("[csv]\ndelimiter = \"\"\n").to_opts();
    assert_eq!(opts.delimiter, None);
}

#[test]
fn all_varchar_and_date_format() {
    let toml = "[csv]\nall_varchar = true\ndate_format = \"%d/%m/%Y\"\n";
    let opts = load_csv_config_str(toml).to_opts();
    assert_eq!(opts.all_varchar, Some(true));
    assert_eq!(opts.date_format, Some("%d/%m/%Y".to_string()));
}

#[test]
fn types_table_projects_to_overrides() {
    let toml = r#"
        [csv.types]
        zip = "VARCHAR"
        amount = "DECIMAL(12,2)"
    "#;
    let opts = load_csv_config_str(toml).to_opts();
    let mut types = opts.types.expect("types present");
    types.sort_by(|a, b| a.name.cmp(&b.name));
    assert_eq!(
        types,
        vec![
            ColumnTypeOverride::new("amount", "DECIMAL(12,2)"),
            ColumnTypeOverride::new("zip", "VARCHAR"),
        ]
    );
}

#[test]
fn malformed_toml_falls_back_to_default() {
    // jiq pattern: a bad config never blocks startup — parse error -> defaults.
    let cfg = load_csv_config_str("[csv]\nthis is not valid toml = = =\n");
    assert_eq!(cfg.to_opts(), crate::ingest::CsvOpts::default());
}

#[test]
fn unknown_csv_key_is_rejected_to_default() {
    // deny_unknown_fields: a typo'd `[csv]` key fails parse -> safe default, never a silent drop.
    let cfg = load_csv_config_str("[csv]\ndelimeter = \";\"\n"); // misspelled
    assert_eq!(cfg.to_opts(), crate::ingest::CsvOpts::default());
}
