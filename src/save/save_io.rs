//! The save subsystem's one I/O seam: filename resolution and the CSV write.
//!
//! [`resolve`] is pure (`&str` + an optional home dir in, `PathBuf` out — the env read happens in
//! the App-layer wrapper, so tests pass an explicit home). [`write`] touches the filesystem and is
//! tempdir-tested, never `$HOME`.

use std::path::{Path, PathBuf};

/// Resolve a typed filename to its on-disk destination:
/// - a leading `~/` expands to `home` (a bare `~` is exactly `home`);
/// - a name with no extension gains `.csv` (the output is CSV — the extension should say so);
///   an explicit extension (any `.something` on the final component) is kept verbatim.
///
/// Returns `Err` with a user-facing message for an empty name or a `~/` name with no home dir.
/// Relative paths stay relative (they land in the process's working directory, which is where
/// the user launched ciq — the natural "next to my data" default).
pub fn resolve(typed: &str, home: Option<&Path>) -> Result<PathBuf, String> {
    let typed = typed.trim();
    if typed.is_empty() {
        return Err("enter a filename".to_string());
    }
    let expanded: PathBuf = if typed == "~" {
        home.ok_or("no home directory to expand ~")?.to_path_buf()
    } else if let Some(rest) = typed.strip_prefix("~/") {
        home.ok_or("no home directory to expand ~")?.join(rest)
    } else {
        PathBuf::from(typed)
    };
    if expanded.extension().is_some() {
        Ok(expanded)
    } else {
        Ok(expanded.with_extension("csv"))
    }
}

/// Write `contents` to `path`, creating parent directories as needed. Overwrites an existing
/// file (the popup's preview warns the user first). Returns a user-facing message on failure.
pub fn write(path: &Path, contents: &str) -> Result<(), String> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent).map_err(|e| format!("cannot create {parent:?}: {e}"))?;
    }
    std::fs::write(path, contents).map_err(|e| format!("cannot write {path:?}: {e}"))
}

#[cfg(test)]
#[path = "save_io_tests.rs"]
mod save_io_tests;
