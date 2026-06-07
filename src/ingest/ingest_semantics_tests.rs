//! Q3 / Q7 / Q12 ingest-semantics fixtures — end-to-end against a real `DuckdbEngine`.
//!
//! These pin the *engine-observed* behavior the three resolved decisions lean on
//! (`dev/DECISIONS.md` Q3/Q7/Q12): DuckDB's duplicate/empty-header dedup, its ragged-row handling,
//! and its empty-field NULL semantics. A future `duckdb` bump that changes any of these turns the
//! build red here (the R8 pin-drift guard).
//!
//! Fixtures are committed under `tests/fixtures/` (resolved via `CARGO_MANIFEST_DIR`) so the data
//! is reviewable, per the task brief.

use std::path::PathBuf;

use crate::engine::duckdb_engine::DuckdbEngine;
use crate::engine::types::{Cell, QueryOutcome};
use crate::engine::{CsvOpts, QueryEngine};
use crate::error::EngineError;
use crate::ingest::{merge, sniff_bytes};

/// Resolve the effective ingest opts the way `main::resolve_opts` does for a default invocation
/// (no CLI flags, no config): sniff the file's bytes and merge the sniffed layer in. This is the
/// production default-load path the bare `ciq <file>` invocation takes, which the engine tests'
/// `CsvOpts::default()` bypasses.
fn sniffed_opts(name: &str) -> CsvOpts {
    let bytes = std::fs::read(fixture_path(name)).expect("read fixture");
    let sniffed = sniff_bytes(&bytes).to_opts();
    merge(&CsvOpts::default(), &CsvOpts::default(), &sniffed)
}

/// Absolute path to a committed fixture under `tests/fixtures/`.
fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

// ---- Q3: column-name policy (raw names; DuckDB dedups duplicate/empty headers) ----

/// `dup_empty_header.csv` has headers `id,name,,name,` — a duplicate `name` and two empty
/// headers. ciq keeps raw names and leans on DuckDB's own dedup. We pin the resulting schema:
/// five columns, all names unique, `id` and a `name` survive, empty/duplicate headers are
/// auto-suffixed by DuckDB (the documented Q3 dedup behavior).
#[test]
fn q3_duplicate_and_empty_headers_are_deduped_by_duckdb() {
    let engine = DuckdbEngine::open(&fixture_path("dup_empty_header.csv"), &CsvOpts::default())
        .expect("load dup/empty header fixture");
    let schema = engine.schema();
    let names: Vec<&str> = schema.columns().iter().map(|c| c.name.as_str()).collect();

    // Five header cells -> five columns.
    assert_eq!(names.len(), 5, "names were {names:?}");

    // All resulting names are unique (the whole point of DuckDB's dedup).
    let mut sorted = names.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), names.len(), "names not unique: {names:?}");

    // The non-colliding raw names survive verbatim.
    assert!(names.contains(&"id"), "names were {names:?}");
    assert!(names.contains(&"name"), "names were {names:?}");

    // Exact dedup scheme DuckDB 1.10503.1 emits (pinned; a bump that changes it fails here).
    assert_eq!(names, vec!["id", "name", "column2", "name_1", "column4"]);
}

/// An all-text CSV (`first,last` over `alice,smith` …) opened with NO `--header` flag must keep
/// its real header names. The sniffer can't contrast (no numeric body), so it leaves
/// `header = None` and DuckDB's own header detection runs — recovering `first`/`last` rather than
/// asserting `header = false` and renaming the columns to `column0`/`column1` (the swallowed-header
/// bug). Mirrors the production default-load path (sniff -> merge), which the engine tests'
/// `CsvOpts::default()` bypasses.
#[test]
fn q3_all_text_header_survives_default_load() {
    let opts = sniffed_opts("all_text_header.csv");
    // The sniffer deferred the header decision rather than asserting no-header.
    assert_eq!(opts.header, None, "sniffer must defer the all-text header");

    let engine =
        DuckdbEngine::open(&fixture_path("all_text_header.csv"), &opts).expect("load all-text CSV");
    let names: Vec<&str> = engine
        .schema()
        .columns()
        .iter()
        .map(|c| c.name.as_str())
        .collect();
    assert_eq!(
        names,
        vec!["first", "last"],
        "the header row must survive as column names, not become column0/column1"
    );

    // The header row was NOT ingested as data: three data rows, none equal to the header text.
    match engine.query("SELECT count(*) AS n FROM t") {
        QueryOutcome::Rows(t) => assert_eq!(t.columns()[0].cells[0], Cell::Int(3)),
        other => panic!("expected Rows, got {other:?}"),
    }
}

// ---- Q7: ragged-row policy (lean on DuckDB's detector; fail-safe, never a panic) ----

/// Under the **default** auto-detect, DuckDB's sniffer can't agree on a delimiter for a ragged
/// file (`ragged.csv` has a 2-field row where others have 3), so it degrades gracefully to a
/// **single text column** — no error, no panic, no silent data loss (every line is preserved as
/// one field). Pinning that graceful degradation: the load succeeds with one column.
#[test]
fn q7_ragged_under_auto_detect_degrades_to_single_column_no_panic() {
    let engine = DuckdbEngine::open(&fixture_path("ragged.csv"), &CsvOpts::default())
        .expect("ragged load under auto-detect should not error or panic");
    // DuckDB couldn't split it -> one text column holding the whole line.
    assert_eq!(engine.schema().len(), 1, "schema: {:?}", engine.schema());
    match engine.query("SELECT count(*) AS n FROM t") {
        QueryOutcome::Rows(t) => assert_eq!(t.columns()[0].cells[0], Cell::Int(3)),
        other => panic!("expected Rows, got {other:?}"),
    }
}

/// With an **explicit delimiter** (so DuckDB commits to 3 columns), the short row makes the strict
/// sniffer reject the file — a clean `EngineError::Load`, never a panic. This is the genuine
/// "ragged row → clean error" path; the error message itself names the `null_padding` /
/// `ignore_errors` escape hatches a future flag can opt into.
#[test]
fn q7_ragged_with_explicit_delimiter_is_a_clean_load_error_not_a_panic() {
    let opts = CsvOpts {
        delimiter: Some(','),
        ..CsvOpts::default()
    };
    match DuckdbEngine::open(&fixture_path("ragged.csv"), &opts) {
        Err(EngineError::Load { path, source }) => {
            assert!(path.contains("ragged.csv"), "path was {path}");
            assert!(
                !source.to_string().is_empty(),
                "expected a DuckDB error message"
            );
        }
        Err(other) => panic!("expected EngineError::Load, got {other:?}"),
        Ok(_) => panic!("expected a clean load error for a ragged row under an explicit delimiter"),
    }
}

// ---- Q12: empty vs NULL (DuckDB default: any empty field -> NULL; null_string is the lever) ----

/// `empty_vs_null.csv`: row 1 has an unquoted empty `note`, row 2 a quoted empty `note` (`""`),
/// row 3 `hello`. Under DuckDB's **default**, BOTH empty forms ingest as SQL `NULL` (DuckDB does
/// not distinguish quoted-empty from unquoted-empty by default). So `WHERE note IS NULL` returns
/// rows 1 AND 2, and `WHERE note = ''` returns none (a `NULL = ''` comparison is NULL, not true).
/// This is ciq's Q12 default — pinned here so a bump that changes it is caught.
#[test]
fn q12_default_ingests_every_empty_field_as_null() {
    let engine = DuckdbEngine::open(&fixture_path("empty_vs_null.csv"), &CsvOpts::default())
        .expect("load empty-vs-null fixture");

    // Both empty rows (unquoted + quoted) are NULL under the default.
    match engine.query("SELECT id FROM t WHERE note IS NULL ORDER BY id") {
        QueryOutcome::Rows(t) => {
            assert_eq!(
                t.row_count(),
                2,
                "both empty forms should be NULL by default"
            );
            assert_eq!(t.columns()[0].cells[0], Cell::Int(1));
            assert_eq!(t.columns()[0].cells[1], Cell::Int(2));
        }
        other => panic!("expected Rows, got {other:?}"),
    }

    // No row equals the empty string under the default (NULLs don't match `= ''`).
    match engine.query("SELECT id FROM t WHERE note = '' ORDER BY id") {
        QueryOutcome::Rows(t) => assert_eq!(t.row_count(), 0, "no empty-string rows by default"),
        other => panic!("expected Rows, got {other:?}"),
    }
}

/// The `null_string` knob is the Q12 user lever: set it to a sentinel that does NOT appear in the
/// data (`\N`), and the empty fields stay **empty strings** rather than NULL — so `WHERE note = ''`
/// now matches them and `WHERE note IS NULL` matches none. Pins that the lever works end-to-end
/// through `to_read_csv_sql`'s `nullstr=` arg.
#[test]
fn q12_null_string_lever_makes_empty_distinct_from_null() {
    let opts = CsvOpts {
        null_string: Some("\\N".to_string()),
        ..CsvOpts::default()
    };
    let engine = DuckdbEngine::open(&fixture_path("empty_vs_null.csv"), &opts)
        .expect("load with null_string sentinel");

    // With a non-matching null sentinel, no field is NULL.
    match engine.query("SELECT count(*) AS n FROM t WHERE note IS NULL") {
        QueryOutcome::Rows(t) => assert_eq!(t.columns()[0].cells[0], Cell::Int(0)),
        other => panic!("expected Rows, got {other:?}"),
    }
    // The two empty fields are now the empty string.
    match engine.query("SELECT id FROM t WHERE note = '' ORDER BY id") {
        QueryOutcome::Rows(t) => {
            assert_eq!(t.row_count(), 2);
            assert_eq!(t.columns()[0].cells[0], Cell::Int(1));
            assert_eq!(t.columns()[0].cells[1], Cell::Int(2));
        }
        other => panic!("expected Rows, got {other:?}"),
    }
}

// ---- to_read_csv_sql wired into the real engine (override applied end-to-end) ----

/// `--all-varchar` forces every column to text. Pinning that the override flows through
/// `to_read_csv_sql` into the real load: a column that would sniff to INTEGER comes back as text.
#[test]
fn all_varchar_override_forces_text_columns_end_to_end() {
    let opts = CsvOpts {
        all_varchar: Some(true),
        ..CsvOpts::default()
    };
    let engine = DuckdbEngine::open(&fixture_path("empty_vs_null.csv"), &opts)
        .expect("load with all_varchar");
    use crate::schema::ColumnType;
    // `id` would normally sniff to Int; with all_varchar it is Text.
    assert_eq!(
        engine.schema().column_type("id"),
        Some(&ColumnType::Text),
        "all_varchar should force id to text"
    );
}
