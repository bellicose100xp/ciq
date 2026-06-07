//! Tests for [`value_render`] — the temporal/decimal/blob/interval text rendering.
//!
//! Each expected string is DuckDB 1.10503.1's own `CAST(<value> AS VARCHAR)` output, probed
//! directly against the bundled engine, so these goldens pin the regression the
//! `Date32(19372)`/`Decimal(1250.50)` Debug-garbage defect produced. The end-to-end engine round
//! trip (a real DATE/DECIMAL cell flowing through `value_ref_to_cell`) is pinned in
//! `duckdb_engine_tests` and `tests/output_cli.rs`.

use super::render_value;
use duckdb::types::{TimeUnit, ValueRef};

// ── Date32 (days since 1970-01-01) ──────────────────────────────────────────────────────────

#[test]
fn date_epoch_is_1970_01_01() {
    assert_eq!(render_value(ValueRef::Date32(0)), "1970-01-01");
}

#[test]
fn date_2023_01_15_matches_duckdb() {
    // 19372 days after the epoch — the canonical sample.csv first-row value that surfaced the bug.
    assert_eq!(render_value(ValueRef::Date32(19372)), "2023-01-15");
}

#[test]
fn date_before_epoch_is_negative_offset() {
    assert_eq!(render_value(ValueRef::Date32(-1)), "1969-12-31");
}

#[test]
fn date_leap_day_renders_correctly() {
    // 2024-02-29 is day 19782 since the epoch.
    assert_eq!(render_value(ValueRef::Date32(19782)), "2024-02-29");
}

// ── Timestamp ───────────────────────────────────────────────────────────────────────────────

#[test]
fn timestamp_micros_no_fraction() {
    // 2023-01-15 12:34:56 = (19372 days * 86_400 + 45_296) seconds in micros.
    let micros = (19372i64 * 86_400 + 12 * 3600 + 34 * 60 + 56) * 1_000_000;
    assert_eq!(
        render_value(ValueRef::Timestamp(TimeUnit::Microsecond, micros)),
        "2023-01-15 12:34:56"
    );
}

#[test]
fn timestamp_trims_trailing_zero_micros() {
    let base = (19372i64 * 86_400 + 12 * 3600 + 34 * 60 + 56) * 1_000_000;
    // .100000 -> ".1"
    assert_eq!(
        render_value(ValueRef::Timestamp(TimeUnit::Microsecond, base + 100_000)),
        "2023-01-15 12:34:56.1"
    );
    // .000100 -> ".0001"
    assert_eq!(
        render_value(ValueRef::Timestamp(TimeUnit::Microsecond, base + 100)),
        "2023-01-15 12:34:56.0001"
    );
}

#[test]
fn timestamp_honors_time_unit() {
    // The same instant expressed in seconds and milliseconds renders identically.
    let secs = 19372i64 * 86_400;
    assert_eq!(
        render_value(ValueRef::Timestamp(TimeUnit::Second, secs)),
        "2023-01-15 00:00:00"
    );
    assert_eq!(
        render_value(ValueRef::Timestamp(TimeUnit::Millisecond, secs * 1000)),
        "2023-01-15 00:00:00"
    );
}

// ── Time64 (micros since midnight) ──────────────────────────────────────────────────────────

#[test]
fn time_no_fraction() {
    let micros = (12 * 3600 + 34 * 60 + 56) * 1_000_000;
    assert_eq!(
        render_value(ValueRef::Time64(TimeUnit::Microsecond, micros)),
        "12:34:56"
    );
}

#[test]
fn time_midnight() {
    assert_eq!(
        render_value(ValueRef::Time64(TimeUnit::Microsecond, 0)),
        "00:00:00"
    );
}

#[test]
fn time_trims_trailing_zero_micros() {
    let base = (3600 + 2 * 60 + 3) * 1_000_000;
    // .450000 -> ".45"
    assert_eq!(
        render_value(ValueRef::Time64(TimeUnit::Microsecond, base + 450_000)),
        "01:02:03.45"
    );
    // .000450 -> ".00045"
    assert_eq!(
        render_value(ValueRef::Time64(TimeUnit::Microsecond, base + 450)),
        "01:02:03.00045"
    );
}

// Decimal rendering (`rust_decimal::Decimal`'s scale-preserving Display) is pinned end-to-end
// through the real engine in `duckdb_engine_tests::decimal_and_date_cells_render_faithfully`,
// rather than here — constructing a `ValueRef::Decimal` in a pure test would pull `rust_decimal`
// (a duckdb transitive) in as a direct dev-dep with a version that must track duckdb exactly.

// ── Blob ────────────────────────────────────────────────────────────────────────────────────

#[test]
fn blob_printable_ascii_verbatim() {
    assert_eq!(render_value(ValueRef::Blob(b"abc")), "abc");
}

#[test]
fn blob_non_printable_is_hex_escaped() {
    // 0x41 'A', 0x42 'B', 0x09 tab, 0x7e '~', 0x7f DEL, 0x20 space — matching DuckDB's rule.
    assert_eq!(
        render_value(ValueRef::Blob(&[0x41, 0x42, 0x09, 0x7e, 0x7f, 0x20])),
        "AB\\x09~\\x7F "
    );
}

#[test]
fn blob_backslash_is_escaped() {
    assert_eq!(render_value(ValueRef::Blob(b"\\")), "\\x5C");
}

// ── Interval ────────────────────────────────────────────────────────────────────────────────

#[test]
fn interval_components_are_lossless() {
    assert_eq!(
        render_value(ValueRef::Interval {
            months: 12,
            days: 0,
            nanos: 0,
        }),
        "12mo"
    );
    assert_eq!(
        render_value(ValueRef::Interval {
            months: 0,
            days: 90,
            nanos: 0,
        }),
        "90d"
    );
    assert_eq!(
        render_value(ValueRef::Interval {
            months: 1,
            days: 2,
            nanos: 3,
        }),
        "1mo 2d 3ns"
    );
    assert_eq!(
        render_value(ValueRef::Interval {
            months: 0,
            days: 0,
            nanos: 0,
        }),
        "0"
    );
}
