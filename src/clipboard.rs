//! Clipboard — copy text to the terminal's clipboard via OSC 52.
//!
//! Ported from jiq's `clipboard/` module (`dev/PLAN.md` §6.7: "reuse jiq's existing top-level
//! `clipboard` module … the OSC52 copy path is `clipboard::osc52::copy(text)`"). ciq's output
//! modes (`output/emit.rs`) produce the result string; the clipboard write hands that string to
//! [`osc52::copy`]. There is **no** `output/clipboard.rs` — the copy path lives here, once.
//!
//! ciq ports only the OSC 52 *write* path (the analog of jiq's `clipboard::osc52::copy`); jiq's
//! OSC 52 *read*, `system` (arboard), and `auto`-fallback backends are not needed for ciq's
//! export-to-clipboard feature and are intentionally left out.
//!
//! Per ciq conventions: no `mod.rs`; the `osc52` submodule is declared from this sibling file.

pub mod osc52;

/// Result of a clipboard copy. `Ok(())` on success; an error only when the final terminal write
/// itself fails (the byte-production never fails).
pub type ClipboardResult = Result<(), ClipboardError>;

/// A clipboard copy failure. The only failure mode is the terminal write itself (stdout closed,
/// broken pipe); the OSC 52 escape-string is built infallibly from any UTF-8 text.
#[derive(Debug, PartialEq, Eq)]
pub enum ClipboardError {
    /// Writing the escape sequence to stdout failed.
    WriteError,
}
