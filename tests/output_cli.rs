//! End-to-end headless test of the `--output` CLI path (`dev/PLAN.md` §6.7 / P4.8).
//!
//! Runs the *compiled* `ciq` binary with `--output` over the committed `tests/fixtures/sample.csv`
//! and asserts the exact bytes it writes to stdout. This exercises the full non-interactive seam —
//! CLI parse -> CSV ingest (real `DuckdbEngine`) -> query -> `render_output` -> stdout — with no
//! terminal, so it is fully agent-checkable (the §6.7 "the `--output csv` path doubles as a
//! non-TUI integration test" note).
//!
//! `CARGO_BIN_EXE_ciq` is set by Cargo for integration tests; it points at the freshly-built
//! binary, so no separate build step is needed.

use std::path::PathBuf;
use std::process::Command;

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("sample.csv")
}

/// Run `ciq <args...> <fixture>` and return (success, stdout-bytes).
fn run_ciq(args: &[&str]) -> (bool, String) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ciq"));
    cmd.args(args);
    cmd.arg(fixture());
    let out = cmd.output().expect("ciq binary runs");
    (
        out.status.success(),
        String::from_utf8(out.stdout).expect("utf-8 stdout"),
    )
}

#[test]
fn output_csv_projection_is_byte_exact() {
    // A deterministic ordered projection so the output is stable regardless of natural row order.
    let (ok, stdout) = run_ciq(&[
        "--output",
        "csv",
        "-q",
        "SELECT id, region FROM t WHERE region = 'EU' ORDER BY id",
    ]);
    assert!(ok, "exit success");
    assert_eq!(stdout, "id,region\n1,EU\n3,EU\n6,EU\n9,EU\n12,EU\n");
}

#[test]
fn output_json_has_type_fidelity() {
    let (ok, stdout) = run_ciq(&[
        "--output",
        "json",
        "-q",
        "SELECT id, name, active FROM t ORDER BY id LIMIT 2",
    ]);
    assert!(ok, "exit success");
    assert_eq!(
        stdout,
        "[\n  \
         {\"id\": 1, \"name\": \"Ada\", \"active\": true},\n  \
         {\"id\": 2, \"name\": \"Babbage\", \"active\": false}\n]"
    );
}

#[test]
fn output_markdown_aligns_numeric_right() {
    let (ok, stdout) = run_ciq(&[
        "--output",
        "markdown",
        "-q",
        "SELECT id, name FROM t ORDER BY id LIMIT 1",
    ]);
    assert!(ok, "exit success");
    // id is INT -> right-aligned separator; name is text -> left.
    assert_eq!(stdout, "| id | name |\n| ---: | --- |\n| 1 | Ada |\n");
}

#[test]
fn output_default_query_is_select_star() {
    let (ok, stdout) = run_ciq(&["--output", "csv", "-q", "SELECT count(*) AS n FROM t"]);
    assert!(ok, "exit success");
    assert_eq!(stdout, "n\n12\n");
}

#[test]
fn output_csv_renders_date_and_decimal_faithfully() {
    // Regression guard for the `Date32(19372)` / `Decimal(1250.50)` Debug-garbage defect: the DATE
    // column and a DECIMAL cast must emit DuckDB's canonical text, byte-exact, not the `{:?}` form.
    let (ok, stdout) = run_ciq(&[
        "--output",
        "csv",
        "-q",
        "SELECT created_at, CAST(amount AS DECIMAL(12,2)) AS amt FROM t ORDER BY id LIMIT 2",
    ]);
    assert!(ok, "exit success");
    assert_eq!(
        stdout,
        "created_at,amt\n2023-01-15,1250.50\n2023-02-20,980.00\n"
    );
}

#[test]
fn output_json_renders_date_as_iso_string() {
    // The DATE column emits a quoted ISO string in JSON (a date has no JSON-number form), NOT the
    // `Date32(...)` garbage that previously shipped.
    let (ok, stdout) = run_ciq(&[
        "--output",
        "json",
        "-q",
        "SELECT id, created_at FROM t ORDER BY id LIMIT 1",
    ]);
    assert!(ok, "exit success");
    assert_eq!(
        stdout,
        "[\n  {\"id\": 1, \"created_at\": \"2023-01-15\"}\n]"
    );
}

#[test]
fn output_rejects_non_select() {
    let (ok, stdout) = run_ciq(&["--output", "csv", "-q", "DROP TABLE t"]);
    assert!(!ok, "DML/DDL is rejected before the engine");
    assert!(stdout.is_empty(), "nothing written to stdout on rejection");
}

#[test]
fn output_rejects_unknown_format() {
    let (ok, stdout) = run_ciq(&["--output", "xml", "-q", "SELECT 1"]);
    assert!(!ok, "unknown format fails");
    assert!(stdout.is_empty());
}
