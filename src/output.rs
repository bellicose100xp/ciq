//! Output modes — serialize the current result set to CSV / TSV / JSON / Markdown.
//!
//! `dev/PLAN.md` §6.7: ciq emits the post-query result set to stdout (the `--output` headless
//! path) or to the clipboard (via the shared [`crate::clipboard::osc52`]) in a chosen format.
//! This generalizes jiq's `save/` "write JSON to a file" into "render rows as one of four text
//! formats".
//!
//! The whole module is **pure** (`dev/PLAN.md` §6.7 + §4.6): [`emit::render_output`] is a
//! `(&Table, &Schema, OutputFormat) -> String` workhorse that touches no terminal, file, clock,
//! or engine — so every format is a byte-exact golden test, and the `--output csv` CLI path is a
//! fully headless integration test. The only §4.7 residue is the OSC 52 clipboard *write*, which
//! lives in `clipboard::osc52::copy`, not here (there is no `output/clipboard.rs`).
//!
//! Per ciq conventions: no `mod.rs`; the `emit` submodule is declared from this sibling file.

pub mod emit;

pub use emit::{OutputFormat, render_output};
