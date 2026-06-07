//! Tests for `clipboard::osc52` — the OSC 52 escape-string builder.
//!
//! Mirrors jiq's `clipboard/osc52_tests.rs`: golden encodings plus a base64 round-trip property.
//! Only [`encode_osc52`] (the pure byte-production) is tested; [`copy`]'s terminal write is the
//! §4.7 human-validated residue and has no headless backend to assert against.

use super::*;
use proptest::prelude::*;

#[test]
fn encode_simple() {
    assert_eq!(encode_osc52("hello"), "\x1b]52;c;aGVsbG8=\x07");
}

#[test]
fn encode_empty() {
    assert_eq!(encode_osc52(""), "\x1b]52;c;\x07");
}

#[test]
fn encode_unicode_round_trips() {
    let result = encode_osc52("日本語");
    assert!(result.starts_with("\x1b]52;c;"));
    assert!(result.ends_with('\x07'));

    let base64_part = &result["\x1b]52;c;".len()..result.len() - "\x07".len()];
    let decoded = STANDARD.decode(base64_part).unwrap();
    assert_eq!(String::from_utf8(decoded).unwrap(), "日本語");
}

#[test]
fn encode_csv_payload_round_trips() {
    // A realistic export string (the kind `render_output` produces) survives the round-trip,
    // including the embedded newlines and commas that CSV carries.
    let csv = "id,name\n1,\"Ada, Lovelace\"\n2,Babbage\n";
    let encoded = encode_osc52(csv);
    let base64_part = &encoded["\x1b]52;c;".len()..encoded.len() - "\x07".len()];
    let decoded = STANDARD.decode(base64_part).unwrap();
    assert_eq!(String::from_utf8(decoded).unwrap(), csv);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    // For any input text, the OSC 52 sequence has the fixed prefix/terminator and its base64
    // body decodes back to the original text byte-for-byte.
    #[test]
    fn prop_encode_round_trip(text in ".*") {
        let encoded = encode_osc52(&text);
        prop_assert!(encoded.starts_with("\x1b]52;c;"));
        prop_assert!(encoded.ends_with('\x07'));

        let base64_part = &encoded["\x1b]52;c;".len()..encoded.len() - "\x07".len()];
        let decoded = STANDARD.decode(base64_part).expect("base64 decodes");
        let decoded_text = String::from_utf8(decoded).expect("valid UTF-8");
        prop_assert_eq!(decoded_text, text);
    }
}
