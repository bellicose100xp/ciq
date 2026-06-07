//! The `[history]` config section — query-history persistence settings (`dev/PLAN.md` §7.6 P5.2).
//!
//! Drives the history subsystem ([`crate::history`]): whether to persist at all, how many entries
//! to keep, and the on-disk file path. Parsed as plain data; the path default is resolved by the
//! history storage layer (XDG data dir), not here, so this section stays filesystem-free and
//! unit-testable over in-memory TOML.

use serde::Deserialize;

/// The built-in cap on persisted history entries when `[history] max_entries` is absent. Mirrors
/// jiq's `MAX_HISTORY_ENTRIES` (1000) — a generous ring that still bounds the file.
pub const DEFAULT_MAX_ENTRIES: usize = 1000;

/// The `[history]` section: the on/off switch, the entry cap, and an optional explicit file path.
///
/// `enabled` defaults **on** so history works out of the box; `max_entries` bounds the on-disk
/// ring; `path` overrides the default XDG location (`None` -> the storage layer's default). The
/// in-session ring always works even when `enabled = false` — `false` only disables the on-disk
/// read/write (the same "session-only" fallback jiq uses when a save fails).
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct HistoryConfig {
    /// Whether to persist history to disk. `false` keeps history session-only (in-memory ring
    /// still works). Default `true`.
    pub enabled: bool,
    /// Max entries kept (in-session ring + on-disk). `None` -> [`DEFAULT_MAX_ENTRIES`].
    pub max_entries: Option<usize>,
    /// Explicit on-disk history file path. `None` -> the storage layer's XDG default.
    pub path: Option<String>,
}

impl Default for HistoryConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_entries: None,
            path: None,
        }
    }
}

impl HistoryConfig {
    /// Whether on-disk persistence is enabled.
    pub fn enabled(&self) -> bool {
        self.enabled
    }

    /// The effective entry cap (configured, or [`DEFAULT_MAX_ENTRIES`]), clamped to at least 1 so
    /// a `0` never silently disables the ring (use `enabled = false` to turn persistence off).
    pub fn max_entries(&self) -> usize {
        self.max_entries
            .map(|n| n.max(1))
            .unwrap_or(DEFAULT_MAX_ENTRIES)
    }

    /// The configured on-disk path override, if any (`None` = the storage layer's XDG default).
    pub fn path(&self) -> Option<&str> {
        self.path.as_deref()
    }
}
