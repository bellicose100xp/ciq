//! Tests for `CsvOpts`, the `merge` precedence, and the `to_read_csv_sql` builder.
//!
//! `to_read_csv_sql` goldens are asserted byte-for-byte **without executing** any SQL (North
//! Star 2: an agent verifies the engine invocation headlessly).

use super::{ColumnTypeOverride, CsvOpts, merge, parse_types_spec, to_read_csv_sql};

fn opt_delim(c: char) -> CsvOpts {
    CsvOpts {
        delimiter: Some(c),
        ..CsvOpts::default()
    }
}

// ---- to_read_csv_sql: the all-None auto-detect fast path ----

#[test]
fn default_opts_emit_read_csv_auto_with_whole_file_scan() {
    let sql = to_read_csv_sql(&CsvOpts::default(), "/tmp/data.csv");
    assert_eq!(sql, "read_csv_auto('/tmp/data.csv', sample_size = -1)");
}

#[test]
fn default_opts_match_the_pre_ingest_engine_golden() {
    // The engine's `created_at -> DATE` golden and A1 guard depend on this exact form. Pin it.
    let sql = to_read_csv_sql(&CsvOpts::default(), "input.csv");
    assert_eq!(sql, "read_csv_auto('input.csv', sample_size = -1)");
}

#[test]
fn path_with_single_quote_is_escaped() {
    let sql = to_read_csv_sql(&CsvOpts::default(), "/tmp/o'brien.csv");
    assert_eq!(sql, "read_csv_auto('/tmp/o''brien.csv', sample_size = -1)");
}

// ---- to_read_csv_sql: explicit-override forms ----

#[test]
fn delimiter_override_emits_explicit_read_csv() {
    let sql = to_read_csv_sql(&opt_delim(';'), "data.csv");
    assert_eq!(
        sql,
        "read_csv('data.csv', auto_detect = true, delim = ';', sample_size = -1)"
    );
}

#[test]
fn tab_delimiter_emits_escaped_tab_char() {
    // A literal tab char inside the single-quoted string arg (DuckDB accepts '\t' as the tab byte).
    let sql = to_read_csv_sql(&opt_delim('\t'), "data.tsv");
    assert_eq!(
        sql,
        "read_csv('data.tsv', auto_detect = true, delim = '\t', sample_size = -1)"
    );
}

#[test]
fn full_dialect_override_emits_all_args_in_order() {
    let opts = CsvOpts {
        delimiter: Some('|'),
        quote: Some('\''),
        escape: Some('\\'),
        header: Some(false),
        null_string: Some("NA".to_string()),
        sample_size: Some(1000),
        ..CsvOpts::default()
    };
    let sql = to_read_csv_sql(&opts, "f.csv");
    assert_eq!(
        sql,
        "read_csv('f.csv', auto_detect = true, delim = '|', quote = '''', escape = '\\', header = false, nullstr = 'NA', sample_size = 1000)"
    );
}

#[test]
fn header_true_only_still_emits_explicit_form() {
    let opts = CsvOpts {
        header: Some(true),
        ..CsvOpts::default()
    };
    let sql = to_read_csv_sql(&opts, "h.csv");
    assert_eq!(
        sql,
        "read_csv('h.csv', auto_detect = true, header = true, sample_size = -1)"
    );
}

#[test]
fn all_varchar_emits_flag() {
    let opts = CsvOpts {
        all_varchar: Some(true),
        ..CsvOpts::default()
    };
    let sql = to_read_csv_sql(&opts, "v.csv");
    assert_eq!(
        sql,
        "read_csv('v.csv', auto_detect = true, sample_size = -1, all_varchar = true)"
    );
}

#[test]
fn all_varchar_false_does_not_emit_flag_but_is_still_an_override() {
    let opts = CsvOpts {
        all_varchar: Some(false),
        ..CsvOpts::default()
    };
    let sql = to_read_csv_sql(&opts, "v.csv");
    // Some(false) counts as an override (explicit form), but the flag itself isn't emitted.
    assert_eq!(
        sql,
        "read_csv('v.csv', auto_detect = true, sample_size = -1)"
    );
}

#[test]
fn date_format_emits_dateformat_arg() {
    let opts = CsvOpts {
        date_format: Some("%d/%m/%Y".to_string()),
        ..CsvOpts::default()
    };
    let sql = to_read_csv_sql(&opts, "d.csv");
    assert_eq!(
        sql,
        "read_csv('d.csv', auto_detect = true, sample_size = -1, dateformat = '%d/%m/%Y')"
    );
}

#[test]
fn types_override_emits_sorted_struct_map() {
    let opts = CsvOpts {
        types: Some(vec![
            ColumnTypeOverride::new("zip", "VARCHAR"),
            ColumnTypeOverride::new("amount", "DECIMAL(12,2)"),
        ]),
        ..CsvOpts::default()
    };
    let sql = to_read_csv_sql(&opts, "t.csv");
    // types are normalized (sorted by column name) so emit is deterministic regardless of order.
    assert_eq!(
        sql,
        "read_csv('t.csv', auto_detect = true, sample_size = -1, types = {'amount': 'DECIMAL(12,2)', 'zip': 'VARCHAR'})"
    );
}

#[test]
fn types_column_name_with_quote_is_escaped_as_string_key() {
    let opts = CsvOpts {
        types: Some(vec![ColumnTypeOverride::new("we'ird", "VARCHAR")]),
        ..CsvOpts::default()
    };
    let sql = to_read_csv_sql(&opts, "t.csv");
    assert_eq!(
        sql,
        "read_csv('t.csv', auto_detect = true, sample_size = -1, types = {'we''ird': 'VARCHAR'})"
    );
}

#[test]
fn empty_types_vec_is_not_an_override() {
    let opts = CsvOpts {
        types: Some(vec![]),
        ..CsvOpts::default()
    };
    // An empty types list shouldn't force the explicit form.
    let sql = to_read_csv_sql(&opts, "e.csv");
    assert_eq!(sql, "read_csv_auto('e.csv', sample_size = -1)");
}

// ---- merge: CLI > config > sniffed, field by field ----

#[test]
fn merge_cli_wins_over_config_and_sniffed() {
    let sniffed = opt_delim(',');
    let config = opt_delim(';');
    let cli = opt_delim('|');
    assert_eq!(merge(&config, &cli, &sniffed).delimiter, Some('|'));
}

#[test]
fn merge_config_wins_when_cli_absent() {
    let sniffed = opt_delim(',');
    let config = opt_delim(';');
    let cli = CsvOpts::default();
    assert_eq!(merge(&config, &cli, &sniffed).delimiter, Some(';'));
}

#[test]
fn merge_sniffed_used_when_cli_and_config_absent() {
    let sniffed = opt_delim('\t');
    let config = CsvOpts::default();
    let cli = CsvOpts::default();
    assert_eq!(merge(&config, &cli, &sniffed).delimiter, Some('\t'));
}

#[test]
fn merge_all_none_stays_none() {
    let merged = merge(
        &CsvOpts::default(),
        &CsvOpts::default(),
        &CsvOpts::default(),
    );
    assert_eq!(merged, CsvOpts::default());
}

#[test]
fn merge_is_field_independent() {
    // Each field resolves on its own: delimiter from CLI, header from config, quote from sniffed.
    let sniffed = CsvOpts {
        quote: Some('"'),
        delimiter: Some(','),
        ..CsvOpts::default()
    };
    let config = CsvOpts {
        header: Some(true),
        delimiter: Some(';'),
        ..CsvOpts::default()
    };
    let cli = CsvOpts {
        delimiter: Some('|'),
        ..CsvOpts::default()
    };
    let merged = merge(&config, &cli, &sniffed);
    assert_eq!(merged.delimiter, Some('|')); // CLI
    assert_eq!(merged.header, Some(true)); // config
    assert_eq!(merged.quote, Some('"')); // sniffed
}

#[test]
fn merge_false_header_is_a_real_override_not_a_skip() {
    // `--no-header` -> Some(false): must win over a sniffed Some(true), not be treated as "unset".
    let sniffed = CsvOpts {
        header: Some(true),
        ..CsvOpts::default()
    };
    let cli = CsvOpts {
        header: Some(false),
        ..CsvOpts::default()
    };
    let merged = merge(&CsvOpts::default(), &cli, &sniffed);
    assert_eq!(merged.header, Some(false));
}

#[test]
fn merge_normalizes_types_last_write_wins_and_sorted() {
    let cli = CsvOpts {
        types: Some(vec![
            ColumnTypeOverride::new("b", "INTEGER"),
            ColumnTypeOverride::new("a", "VARCHAR"),
            ColumnTypeOverride::new("a", "DOUBLE"), // later write wins for `a`
        ]),
        ..CsvOpts::default()
    };
    let merged = merge(&CsvOpts::default(), &cli, &CsvOpts::default());
    assert_eq!(
        merged.types,
        Some(vec![
            ColumnTypeOverride::new("a", "DOUBLE"),
            ColumnTypeOverride::new("b", "INTEGER"),
        ])
    );
}

#[test]
fn merge_then_emit_roundtrips_deterministically() {
    // Two merges with the same inputs in different type orders emit identical SQL.
    let cli_a = CsvOpts {
        types: Some(vec![
            ColumnTypeOverride::new("zip", "VARCHAR"),
            ColumnTypeOverride::new("amount", "DOUBLE"),
        ]),
        ..CsvOpts::default()
    };
    let cli_b = CsvOpts {
        types: Some(vec![
            ColumnTypeOverride::new("amount", "DOUBLE"),
            ColumnTypeOverride::new("zip", "VARCHAR"),
        ]),
        ..CsvOpts::default()
    };
    let merged_a = merge(&CsvOpts::default(), &cli_a, &CsvOpts::default());
    let merged_b = merge(&CsvOpts::default(), &cli_b, &CsvOpts::default());
    assert_eq!(
        to_read_csv_sql(&merged_a, "x.csv"),
        to_read_csv_sql(&merged_b, "x.csv")
    );
}

// ---- parse_types_spec ----

#[test]
fn parse_types_simple_pairs() {
    let got = parse_types_spec("zip=VARCHAR,amount=DOUBLE").unwrap();
    assert_eq!(
        got,
        vec![
            ColumnTypeOverride::new("zip", "VARCHAR"),
            ColumnTypeOverride::new("amount", "DOUBLE"),
        ]
    );
}

#[test]
fn parse_types_keeps_parameterized_type_with_comma() {
    let got = parse_types_spec("amount=DECIMAL(12,2),zip=VARCHAR").unwrap();
    assert_eq!(
        got,
        vec![
            ColumnTypeOverride::new("amount", "DECIMAL(12,2)"),
            ColumnTypeOverride::new("zip", "VARCHAR"),
        ]
    );
}

#[test]
fn parse_types_trims_whitespace_and_skips_blanks() {
    let got = parse_types_spec(" zip = VARCHAR , , amount = DOUBLE ").unwrap();
    assert_eq!(
        got,
        vec![
            ColumnTypeOverride::new("zip", "VARCHAR"),
            ColumnTypeOverride::new("amount", "DOUBLE"),
        ]
    );
}

#[test]
fn parse_types_rejects_entry_without_equals() {
    assert!(parse_types_spec("zip VARCHAR").is_err());
}

#[test]
fn parse_types_rejects_empty_name_or_type() {
    assert!(parse_types_spec("=VARCHAR").is_err());
    assert!(parse_types_spec("zip=").is_err());
}

#[test]
fn parse_types_empty_spec_is_empty_vec() {
    assert_eq!(parse_types_spec("").unwrap(), vec![]);
}
