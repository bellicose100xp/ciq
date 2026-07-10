//! The save popup's pure state machine — open/closed, the filename being typed, the resolved
//! path preview, and an inline write error.
//!
//! Mirrors the search bar's needle model (a plain `String`, pushed/popped per key — no textarea;
//! the filename is short and append-edited). All I/O — resolving `~`, checking existence, the
//! write itself — lives in [`super::save_io`]; this state only *holds* the results as data, so
//! it tests entirely in memory.

use std::path::PathBuf;

/// The resolved on-disk destination for the typed filename, recomputed by the App on every edit
/// (via [`super::save_io::resolve`] + an existence probe). Plain data: the render layer shows the
/// path (and an overwrite warning when `exists`), and Enter writes to exactly this path — so what
/// the user sees and what gets written can never disagree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathPreview {
    /// The fully resolved destination (tilde expanded, `.csv` defaulted).
    pub path: PathBuf,
    /// Whether a file already exists there (the write would overwrite it).
    pub exists: bool,
}

/// Save-popup state: visibility, the filename input, the live path preview, and an inline error.
#[derive(Debug, Clone, Default)]
pub struct SaveState {
    /// Whether the popup is on screen (it captures the keyboard while open).
    open: bool,
    /// The filename the user is typing (append-edited, like the search needle).
    filename: String,
    /// The resolved destination preview for the current filename (`None` while the name is empty
    /// or unresolvable — the render falls back to the hint line).
    preview: Option<PathPreview>,
    /// An inline error from the last Enter (resolve failure or write failure). Cleared on the
    /// next edit so the user can fix the name and retry without stale red text.
    error: Option<String>,
}

impl SaveState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Open the popup with `initial` prefilled as the filename (the `<csv-stem>-out.csv`
    /// default). A fresh open clears any prior error; the preview is recomputed by the caller.
    pub fn open(&mut self, initial: &str) {
        self.open = true;
        self.filename = initial.to_string();
        self.preview = None;
        self.error = None;
    }

    /// Close the popup, dropping the input/preview/error.
    pub fn close(&mut self) {
        self.open = false;
        self.filename.clear();
        self.preview = None;
        self.error = None;
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn filename(&self) -> &str {
        &self.filename
    }

    /// Append a typed char to the filename. Clears the inline error (the name changed, so the
    /// error no longer describes it).
    pub fn push(&mut self, c: char) {
        self.filename.push(c);
        self.error = None;
    }

    /// Pop the last filename char (Backspace). Clears the inline error.
    pub fn pop(&mut self) {
        self.filename.pop();
        self.error = None;
    }

    pub fn preview(&self) -> Option<&PathPreview> {
        self.preview.as_ref()
    }

    /// Install the resolved-path preview for the current filename (`None` when unresolvable).
    pub fn set_preview(&mut self, preview: Option<PathPreview>) {
        self.preview = preview;
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// Surface an inline error (a resolve or write failure); the popup stays open so the user
    /// can fix the name and retry.
    pub fn set_error(&mut self, error: impl Into<String>) {
        self.error = Some(error.into());
    }
}

#[cfg(test)]
#[path = "save_state_tests.rs"]
mod save_state_tests;
