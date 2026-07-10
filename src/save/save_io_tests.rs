//! Tests for the save subsystem's I/O seam — filename resolution and the CSV write. Every write
//! targets a `tempfile::TempDir`, never `$HOME`.

use super::{resolve, write};
use std::path::{Path, PathBuf};

#[test]
fn empty_name_errors() {
    assert!(resolve("   ", None).is_err());
}

#[test]
fn defaults_csv_extension_when_missing() {
    let p = resolve("report", None).expect("resolve");
    assert_eq!(p, PathBuf::from("report.csv"));
}

#[test]
fn keeps_explicit_extension() {
    let p = resolve("report.tsv", None).expect("resolve");
    assert_eq!(p, PathBuf::from("report.tsv"));
}

#[test]
fn expands_tilde_slash_to_home() {
    let home = Path::new("/home/tester");
    let p = resolve("~/sub/out.csv", Some(home)).expect("resolve");
    assert_eq!(p, PathBuf::from("/home/tester/sub/out.csv"));
}

#[test]
fn bare_tilde_is_home() {
    let home = Path::new("/home/tester");
    // A bare `~` has no extension, so it gains `.csv` after expansion.
    let p = resolve("~", Some(home)).expect("resolve");
    assert_eq!(p, PathBuf::from("/home/tester.csv"));
}

#[test]
fn tilde_without_home_errors() {
    assert!(resolve("~/out.csv", None).is_err());
}

#[test]
fn write_creates_file_and_parent_dirs() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("nested/deep/out.csv");
    write(&path, "a,b\n1,2\n").expect("write");
    let back = std::fs::read_to_string(&path).expect("read back");
    assert_eq!(back, "a,b\n1,2\n");
}

#[test]
fn write_overwrites_existing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("out.csv");
    write(&path, "old").expect("write 1");
    write(&path, "new").expect("write 2");
    assert_eq!(std::fs::read_to_string(&path).expect("read"), "new");
}
