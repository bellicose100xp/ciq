//! Tests for the pure CSV dialect sniffer (`sniff.rs`).

use super::{sniff, sniff_bytes};

#[test]
fn detects_comma_delimiter() {
    let r = sniff("a,b,c\n1,2,3\n4,5,6\n");
    assert_eq!(r.delimiter, Some(','));
}

#[test]
fn detects_semicolon_delimiter() {
    let r = sniff("a;b;c\n1;2;3\n4;5;6\n");
    assert_eq!(r.delimiter, Some(';'));
}

#[test]
fn detects_tab_delimiter() {
    let r = sniff("a\tb\tc\n1\t2\t3\n");
    assert_eq!(r.delimiter, Some('\t'));
}

#[test]
fn detects_pipe_delimiter() {
    let r = sniff("a|b|c\n1|2|3\n");
    assert_eq!(r.delimiter, Some('|'));
}

#[test]
fn ambiguous_input_defaults_to_comma() {
    // Both comma and pipe split every line into the same field count; documented tie-break = comma.
    let r = sniff("a,b|c\n1,2|3\n4,5|6\n");
    assert_eq!(r.delimiter, Some(','));
}

#[test]
fn single_column_defaults_to_comma() {
    // No delimiter actually splits the data; we still default to comma so emitted SQL is valid.
    let r = sniff("name\nalice\nbob\n");
    assert_eq!(r.delimiter, Some(','));
}

#[test]
fn empty_input_determines_nothing() {
    let r = sniff("");
    assert_eq!(r.delimiter, None);
    assert_eq!(r.quote, None);
    assert_eq!(r.header, None);
}

#[test]
fn whitespace_only_input_determines_nothing() {
    let r = sniff("   \n\n  \n");
    assert_eq!(r.delimiter, None);
}

#[test]
fn quote_detected_when_present() {
    let r = sniff("a,b\n\"hello, world\",2\n");
    assert_eq!(r.quote, Some('"'));
    // The comma inside the quoted field must NOT inflate the field count -> still comma, 2 fields.
    assert_eq!(r.delimiter, Some(','));
}

#[test]
fn no_quote_when_absent() {
    let r = sniff("a,b\n1,2\n");
    assert_eq!(r.quote, None);
}

#[test]
fn escaped_quote_inside_quoted_field_does_not_split_or_flip() {
    // A `""` escape inside a quoted field must be consumed as one quote (not toggle quoting), so
    // the embedded delimiter stays inside the field and the field count is right. This exercises
    // both the counting (`field_count`) and the header-split (`split_fields`) escaped-quote paths.
    let r = sniff("label,note\n\"say \"\"hi, there\"\"\",2\n\"plain\",3\n");
    assert_eq!(r.quote, Some('"'));
    // Two columns despite the comma inside the escaped-quote field.
    assert_eq!(r.delimiter, Some(','));
    // First row is text names, body has a numeric -> header inferred.
    assert_eq!(r.header, Some(true));
}

#[test]
fn header_inferred_from_text_names_over_numeric_body() {
    let r = sniff("id,amount\n1,12.5\n2,7.0\n");
    assert_eq!(r.header, Some(true));
}

#[test]
fn no_header_when_first_row_is_numeric() {
    let r = sniff("1,12.5\n2,7.0\n3,9.9\n");
    assert_eq!(r.header, Some(false));
}

#[test]
fn no_header_when_all_rows_are_text() {
    // No numeric body to contrast against -> conservative false (DuckDB refines at load).
    let r = sniff("first,last\nalice,smith\nbob,jones\n");
    assert_eq!(r.header, Some(false));
}

#[test]
fn header_false_for_single_line() {
    let r = sniff("a,b,c\n");
    assert_eq!(r.delimiter, Some(','));
    assert_eq!(r.header, Some(false));
}

#[test]
fn ragged_lines_pick_the_modal_delimiter_count() {
    // One short comma line shouldn't unseat comma when most lines have 3 comma fields.
    let r = sniff("a,b,c\n1,2,3\n4,5,6\n7,8\n");
    assert_eq!(r.delimiter, Some(','));
}

#[test]
fn to_opts_only_sets_determined_fields() {
    let r = sniff("id,amount\n1,12.5\n");
    let opts = r.to_opts();
    assert_eq!(opts.delimiter, Some(','));
    assert_eq!(opts.header, Some(true));
    // sniffer never sets these -> they stay None for merge to defer below.
    assert_eq!(opts.escape, None);
    assert_eq!(opts.sample_size, None);
    assert_eq!(opts.null_string, None);
}

#[test]
fn invalid_utf8_bytes_do_not_panic() {
    // Lossy decode; a stray non-UTF8 byte must not crash the sniffer.
    let bytes = b"a,b\n\xff,2\n3,4\n";
    let r = sniff_bytes(bytes);
    assert_eq!(r.delimiter, Some(','));
}
