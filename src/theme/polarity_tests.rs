use crate::config::ThemeMode;
use crate::theme::polarity::{
    OSC11_QUERY, Polarity, contrast_ratio, from_background, from_colorfgbg, parse_osc11,
    relative_luminance, resolve,
};

#[test]
fn osc11_query_is_the_st_terminated_background_request() {
    assert_eq!(OSC11_QUERY, b"\x1b]11;?\x1b\\");
}

#[test]
fn resolve_pins_light_and_dark_regardless_of_detection() {
    for detected in [None, Some(Polarity::Dark), Some(Polarity::Light)] {
        assert_eq!(resolve(ThemeMode::Light, detected), Polarity::Light);
        assert_eq!(resolve(ThemeMode::Dark, detected), Polarity::Dark);
    }
}

#[test]
fn resolve_auto_takes_detection_and_falls_back_to_dark() {
    assert_eq!(
        resolve(ThemeMode::Auto, Some(Polarity::Light)),
        Polarity::Light
    );
    assert_eq!(
        resolve(ThemeMode::Auto, Some(Polarity::Dark)),
        Polarity::Dark
    );
    assert_eq!(resolve(ThemeMode::Auto, None), Polarity::Dark);
}

#[test]
fn relative_luminance_spans_black_to_white() {
    assert!(relative_luminance(0, 0, 0).abs() < 1e-9);
    assert!((relative_luminance(255, 255, 255) - 1.0).abs() < 1e-9);
    // Green carries the most weight, blue the least.
    assert!(relative_luminance(0, 255, 0) > relative_luminance(255, 0, 0));
    assert!(relative_luminance(255, 0, 0) > relative_luminance(0, 0, 255));
    // The low-end linear segment is exercised too.
    assert!(relative_luminance(5, 5, 5) > 0.0);
}

#[test]
fn contrast_ratio_is_symmetric_and_bounded() {
    let black = (0, 0, 0);
    let white = (255, 255, 255);
    assert!((contrast_ratio(black, white) - 21.0).abs() < 1e-6);
    assert!((contrast_ratio(white, black) - 21.0).abs() < 1e-6);
    assert!((contrast_ratio(white, white) - 1.0).abs() < 1e-9);
}

#[test]
fn from_background_splits_on_the_luminance_midpoint() {
    assert_eq!(from_background((255, 255, 255)), Polarity::Light);
    assert_eq!(from_background((239, 241, 245)), Polarity::Light);
    assert_eq!(from_background((0, 0, 0)), Polarity::Dark);
    assert_eq!(from_background((26, 26, 46)), Polarity::Dark);
    // Solarized light / dark land on the expected sides.
    assert_eq!(from_background((253, 246, 227)), Polarity::Light);
    assert_eq!(from_background((0, 43, 54)), Polarity::Dark);
}

#[test]
fn parse_osc11_reads_xterm_16_bit_reply_with_st_terminator() {
    let reply = b"\x1b]11;rgb:1a1a/1a1a/2e2e\x1b\\";
    assert_eq!(parse_osc11(reply), Some((0x1a, 0x1a, 0x2e)));
}

#[test]
fn parse_osc11_reads_bel_terminated_reply() {
    let reply = b"\x1b]11;rgb:ffff/ffff/ffff\x07";
    assert_eq!(parse_osc11(reply), Some((255, 255, 255)));
}

#[test]
fn parse_osc11_accepts_8_and_12_bit_components() {
    assert_eq!(
        parse_osc11(b"\x1b]11;rgb:ef/f1/f5\x07"),
        Some((0xef, 0xf1, 0xf5))
    );
    assert_eq!(
        parse_osc11(b"\x1b]11;rgb:eff/f1f/f5f\x07"),
        Some((0xef, 0xf1, 0xf5))
    );
}

#[test]
fn parse_osc11_skips_leading_noise_before_the_reply() {
    let reply = b"junk\x1b]11;rgb:0000/0000/0000\x1b\\";
    assert_eq!(parse_osc11(reply), Some((0, 0, 0)));
}

#[test]
fn parse_osc11_returns_none_for_incomplete_or_malformed_input() {
    assert_eq!(parse_osc11(b""), None);
    assert_eq!(parse_osc11(b"\x1b]11;rgb:ffff/ffff"), None); // no terminator yet
    assert_eq!(parse_osc11(b"\x1b]10;rgb:ffff/ffff/ffff\x07"), None); // foreground reply
    assert_eq!(parse_osc11(b"\x1b]11;#ffffff\x07"), None); // not an rgb: spec
    assert_eq!(parse_osc11(b"\x1b]11;rgb:ffff/ffff\x07"), None); // two components
    assert_eq!(parse_osc11(b"\x1b]11;rgb:ff/ff/ff/ff\x07"), None); // four components
    assert_eq!(parse_osc11(b"\x1b]11;rgb:zz/ff/ff\x07"), None); // non-hex
    assert_eq!(parse_osc11(b"\x1b]11;rgb:fffff/ff/ff\x07"), None); // too wide
    assert_eq!(parse_osc11(b"\x1b]11;rgb:/ff/ff\x07"), None); // empty component
    assert_eq!(parse_osc11(b"\x1b]11;rgb:\xff\xfe/ff/ff\x07"), None); // invalid utf-8
}

#[test]
fn from_colorfgbg_follows_vims_index_rule() {
    assert_eq!(from_colorfgbg("15;0"), Some(Polarity::Dark));
    assert_eq!(from_colorfgbg("0;15"), Some(Polarity::Light));
    assert_eq!(from_colorfgbg("7;8"), Some(Polarity::Dark));
    assert_eq!(from_colorfgbg("0;7"), Some(Polarity::Light));
    assert_eq!(from_colorfgbg("15;default;0"), Some(Polarity::Dark));
    assert_eq!(from_colorfgbg("0;0;15"), Some(Polarity::Light));
    assert_eq!(from_colorfgbg("0; 15 "), Some(Polarity::Light));
}

#[test]
fn from_colorfgbg_rejects_non_ansi_values() {
    assert_eq!(from_colorfgbg(""), None);
    assert_eq!(from_colorfgbg("default;default"), None);
    assert_eq!(from_colorfgbg("15;16"), None);
    assert_eq!(from_colorfgbg("15;-1"), None);
}
