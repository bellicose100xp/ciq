//! OSC 52 clipboard escape sequence — encode any text into the `ESC ] 52 ; c ; <base64> BEL`
//! sequence a terminal interprets as "set the system clipboard to this text".
//!
//! Ported from jiq's `src/clipboard/osc52.rs` (`encode_osc52` + `copy`). The split that makes
//! this headless-testable (North Star 2):
//!
//! - [`encode_osc52`] is **pure** — text in, escape-string out, base64-encoded, no I/O. It is
//!   the byte-production and is golden- and round-trip-tested exactly as jiq tests it.
//! - [`copy`] writes those bytes to the real stdout. That final write to the terminal is the
//!   only part that cannot be exercised headlessly (there is no in-memory backend to receive an
//!   OSC sequence), so it is the §4.7 "clipboard / OSC 52" human-validated residue — marked
//!   `// ciq:shell-exempt` below.

use std::io::{self, Write};

use base64::{Engine as _, engine::general_purpose::STANDARD};

use super::{ClipboardError, ClipboardResult};

/// Build the OSC 52 escape sequence that sets the terminal clipboard to `text`.
///
/// Format: `ESC ] 52 ; c ; <base64(text)> BEL` (`\x1b]52;c;{}\x07`). Pure — this is the whole
/// of the testable "byte production"; the only impure part is [`copy`]'s write of these bytes.
///
/// `"hello"` -> `"\x1b]52;c;aGVsbG8=\x07"`; `""` -> `"\x1b]52;c;\x07"`.
pub fn encode_osc52(text: &str) -> String {
    let encoded = STANDARD.encode(text);
    format!("\x1b]52;c;{encoded}\x07")
}

// ciq:shell-exempt — §4.7 row 4 (system clipboard / OSC 52): the actual write of the escape
// bytes to the real terminal has no in-memory backend to receive it, so it is human-validated
// (copy a result, paste into another app, confirm contents). Everything that *decides what bytes
// to emit* — `encode_osc52` above and the `render_output` formatters in `output/emit.rs` — is
// pure and headless.
/// Copy `text` to the terminal clipboard by writing its OSC 52 escape sequence to stdout.
///
/// The escape string is built by the pure [`encode_osc52`]; this only performs the terminal
/// write (and its flush), which is why this is the single OSC-52 line in the human surface.
pub fn copy(text: &str) -> ClipboardResult {
    let sequence = encode_osc52(text);
    let mut stdout = io::stdout();
    stdout
        .write_all(sequence.as_bytes())
        .map_err(|_| ClipboardError::WriteError)?;
    stdout.flush().map_err(|_| ClipboardError::WriteError)
}

#[cfg(test)]
#[path = "osc52_tests.rs"]
mod osc52_tests;
