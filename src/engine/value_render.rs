//! Faithful text rendering of DuckDB's non-primitive scalar values (temporal / decimal / blob /
//! interval) into ciq's [`Cell::Text`](crate::engine::types::Cell) — the **display form** ciq
//! carries through the grid and every `--output` writer.
//!
//! Why this module exists: a `ValueRef` only derives `Debug`, so `format!("{vr:?}")` yields the
//! *variant name wrapping the raw value* (`Date32(19372)`, `Decimal(1250.50)`,
//! `Timestamp(Microsecond, …)`) — **not** the value's string form. Routing temporal/decimal/blob
//! cells through `{:?}` corrupted every CSV/TSV/JSON/Markdown export and the grid for any table
//! with a date/decimal column (the common case), and is reachable from the default `SELECT *`. These
//! functions render each value to DuckDB's own canonical text instead.
//!
//! Pure data-in/data-out (no engine, no clock, no I/O), so it is exhaustively unit-tested against
//! DuckDB's observed canonical output and sits on the pure-core hard floor (`dev/core-modules.txt`).
//!
//! Fidelity:
//!  - **Date** (`Date32`, days since 1970-01-01) → `YYYY-MM-DD`, exact for the proleptic-Gregorian
//!    AD range (the range a CSV date column ever holds). Pre-year-1 (BC) dates — which DuckDB tags
//!    `(BC)` and which cannot appear in a sniffed CSV `DATE` column — render with a negative year
//!    rather than DuckDB's `(BC)` suffix; documented partial, not reachable from CSV data.
//!  - **Timestamp / Time** (micro/milli/nano/second `TimeUnit`) → `…HH:MM:SS[.frac]`, the fractional
//!    part being the sub-second microseconds with trailing zeros trimmed and the `.` omitted when
//!    zero — byte-matching DuckDB.
//!  - **Decimal** → `rust_decimal::Decimal`'s `Display`, which preserves scale (`1250.50`), matching
//!    DuckDB.
//!  - **Blob** → printable ASCII bytes verbatim, every other byte (and `\`) as `\xNN` uppercase hex,
//!    matching DuckDB's `BLOB`→`VARCHAR` rendering.
//!  - **Interval** → a lossless, deterministic `<months>mo <days>d <nanos>ns`-style canonical form
//!    (only the non-zero components). DuckDB's own interval text (`1 year`, `90 days`) uses
//!    pluralized English ciq does not chase; an interval is never a sniffed CSV column type (it only
//!    arises from explicit SQL), so byte-matching DuckDB here buys nothing.

use duckdb::types::{TimeUnit, ValueRef};

/// Render a non-primitive DuckDB value (temporal / decimal / blob / interval) to its canonical
/// display string. The caller ([`value_ref_to_cell`](super::duckdb_engine)) handles the primitive
/// arms directly; this owns the arms that would otherwise fall through to a lossy `{:?}`.
pub fn render_value(vr: ValueRef<'_>) -> String {
    match vr {
        ValueRef::Decimal(d) => d.to_string(),
        ValueRef::Date32(days) => render_date(days),
        ValueRef::Timestamp(unit, v) => render_timestamp(unit, v),
        ValueRef::Time64(unit, v) => render_time(unit, v),
        ValueRef::Blob(bytes) => render_blob(bytes),
        ValueRef::Interval {
            months,
            days,
            nanos,
        } => render_interval(months, days, nanos),
        // Genuinely unrenderable / structural variants (List/Struct/Map/Union/Enum/Array) keep the
        // Debug fallback — they cannot appear in a sniffed CSV column and have no flat text form.
        other => format!("{other:?}"),
    }
}

/// `Date32` (days since the Unix epoch, 1970-01-01) → `YYYY-MM-DD`.
fn render_date(days: i32) -> String {
    let (y, m, d) = civil_from_days(days as i64);
    format!("{y:04}-{m:02}-{d:02}")
}

/// `Timestamp` (a count in `unit` since the epoch) → `YYYY-MM-DD HH:MM:SS[.frac]`.
fn render_timestamp(unit: TimeUnit, value: i64) -> String {
    let micros = unit.to_micros(value);
    // Floor-divide so a pre-epoch (negative) timestamp keeps a non-negative time-of-day.
    let day = micros.div_euclid(MICROS_PER_DAY);
    let tod = micros.rem_euclid(MICROS_PER_DAY);
    let (y, mo, d) = civil_from_days(day);
    format!("{y:04}-{mo:02}-{d:02} {}", time_of_day(tod))
}

/// `Time64` (a count in `unit` since midnight) → `HH:MM:SS[.frac]`.
fn render_time(unit: TimeUnit, value: i64) -> String {
    let micros = unit.to_micros(value).rem_euclid(MICROS_PER_DAY);
    time_of_day(micros)
}

const MICROS_PER_DAY: i64 = 86_400_000_000;

/// Format a microsecond-of-day count as `HH:MM:SS[.frac]`, trimming trailing zeros from the
/// fractional microseconds and omitting the `.` entirely when the sub-second part is zero (DuckDB's
/// rule).
fn time_of_day(micros_of_day: i64) -> String {
    let secs = micros_of_day / 1_000_000;
    let frac = micros_of_day % 1_000_000;
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    if frac == 0 {
        format!("{h:02}:{m:02}:{s:02}")
    } else {
        // Six-digit microseconds, trailing zeros stripped (`.1`, `.0001`, `.45`, …).
        let frac = format!("{frac:06}");
        let frac = frac.trim_end_matches('0');
        format!("{h:02}:{m:02}:{s:02}.{frac}")
    }
}

/// Civil (year, month, day) from a day count relative to 1970-01-01, via Howard Hinnant's
/// `civil_from_days` algorithm (exact over the proleptic Gregorian calendar, no library).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Render a `BLOB` exactly as DuckDB's `BLOB`→`VARCHAR` cast does: a printable ASCII byte
/// (`0x20..=0x7E`, excluding `\`) appears verbatim; every other byte — and `\` itself — as
/// `\xNN` uppercase hex.
fn render_blob(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len());
    for &b in bytes {
        if (0x20..=0x7e).contains(&b) && b != b'\\' {
            out.push(b as char);
        } else {
            out.push_str(&format!("\\x{b:02X}"));
        }
    }
    out
}

/// Render an `INTERVAL` as a lossless, deterministic `<months>mo <days>d <nanos>ns` form (only the
/// non-zero components; `0` for a wholly-zero interval). Not DuckDB's pluralized English — see the
/// module docs.
fn render_interval(months: i32, days: i32, nanos: i64) -> String {
    let mut parts: Vec<String> = Vec::new();
    if months != 0 {
        parts.push(format!("{months}mo"));
    }
    if days != 0 {
        parts.push(format!("{days}d"));
    }
    if nanos != 0 {
        parts.push(format!("{nanos}ns"));
    }
    if parts.is_empty() {
        "0".to_string()
    } else {
        parts.join(" ")
    }
}

#[cfg(test)]
#[path = "value_render_tests.rs"]
mod value_render_tests;
