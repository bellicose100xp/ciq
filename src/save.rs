//! Save-to-CSV subsystem (`Ctrl+W`) — the in-session "write the current result to a file" flow.
//!
//! Generalizes jiq's `save/` subsystem (filename popup -> write on Enter) to ciq's tabular world:
//! the payload is the **displayed** result (the Ctrl+F-filtered view when a filter is active)
//! serialized by the existing pure [`crate::output::render_output`] CSV writer — no second
//! serializer, no engine round trip.
//!
//! Split by purity, like every ciq feature:
//! - [`save_state`] — the pure popup state machine (open/closed, the filename being typed, an
//!   inline error). No I/O, no clock.
//! - [`save_io`] — the one I/O seam: resolve the typed name (tilde expansion, `.csv` default
//!   extension) and write the bytes. Tempdir-tested, never `$HOME`.
//! - [`save_render`] — the thin popup blit over the shared modern popup chrome
//!   ([`crate::theme::popup`]), `TestBackend`-snapshot-tested.
//!
//! The App orchestration (open/close/handle keys) lives in `crate::app::save_app`.

pub mod save_io;
pub mod save_render;
pub mod save_state;

pub use save_state::SaveState;
