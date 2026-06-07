//! On-disk history persistence — the **only** I/O in the history subsystem (`dev/PLAN.md` §7.6).
//!
//! A newline-delimited file (one query per line, newest first), ported from jiq's
//! `history/storage.rs`. The pure ring ([`HistoryState`](super::history_state::HistoryState)) is
//! tested in-memory; this file's load/save are tested against a **tempdir** path (never `$HOME`):
//! every function takes the file path explicitly, so a test passes a `tempfile::TempDir` path and
//! the suite never reads or writes the user's real history.
//!
//! The default on-disk location ([`default_history_path`]) is resolved from XDG env vars for the
//! real CLI only; tests pass an explicit path and never call it.

use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

/// Resolve the default history file: `$XDG_DATA_HOME/ciq/history`, else
/// `$HOME/.local/share/ciq/history`. `None` when neither env var is set. The only `$HOME`/env touch
/// here — kept out of [`load`]/[`save`] (which take an explicit path) so the suite never reads the
/// environment.
pub fn default_history_path() -> Option<PathBuf> {
    if let Some(xdg) = std::env::var_os("XDG_DATA_HOME").filter(|s| !s.is_empty()) {
        return Some(Path::new(&xdg).join("ciq").join("history"));
    }
    let home = std::env::var_os("HOME")?;
    Some(
        Path::new(&home)
            .join(".local")
            .join("share")
            .join("ciq")
            .join("history"),
    )
}

/// Load history entries from `path` (newest first). A missing/unreadable file yields an empty
/// list (never an error — like jiq, an absent history file is a clean first run). Blank lines are
/// dropped.
pub fn load(path: &Path) -> Vec<String> {
    let Ok(file) = File::open(path) else {
        return Vec::new();
    };
    BufReader::new(file)
        .lines()
        .map_while(Result::ok)
        .filter(|line| !line.trim().is_empty())
        .collect()
}

/// Save `entries` to `path` (newest first), creating parent dirs as needed. Dedupes (keeping the
/// first/newest occurrence) and trims to `max_entries` so the file is bounded. No file locking —
/// last writer wins if two instances run at once (jiq's documented behavior).
pub fn save(path: &Path, entries: &[String], max_entries: usize) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = File::create(path)?;
    for entry in trim_to_max(&deduplicate(entries), max_entries) {
        writeln!(file, "{entry}")?;
    }
    Ok(())
}

/// Add `query` to the on-disk history at `path` (load, move-to-front, save), bounded to
/// `max_entries`. A blank query is a no-op. The App calls this when persistence is enabled; the
/// in-memory ring is updated separately.
pub fn add(path: &Path, query: &str, max_entries: usize) -> io::Result<()> {
    let query = query.trim();
    if query.is_empty() {
        return Ok(());
    }
    let mut entries = load(path);
    entries.retain(|e| e != query);
    entries.insert(0, query.to_string());
    save(path, &entries, max_entries)
}

/// Remove duplicate entries, keeping the first (newest) occurrence of each.
fn deduplicate(entries: &[String]) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    entries
        .iter()
        .filter(|e| seen.insert(e.as_str()))
        .cloned()
        .collect()
}

/// Keep at most `max` entries (the newest, since the list is newest-first).
fn trim_to_max(entries: &[String], max: usize) -> Vec<String> {
    entries.iter().take(max).cloned().collect()
}

#[cfg(test)]
#[path = "storage_tests.rs"]
mod storage_tests;
