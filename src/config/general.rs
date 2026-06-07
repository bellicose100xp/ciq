//! The `[general]` config section — engine-wide defaults (`dev/PLAN.md` §0/Q5, Phase 5).
//!
//! Pure owned data parsed from TOML; every field optional with a documented default so an absent
//! key (or an absent `[general]` table) yields the conservative built-in. The accessor methods
//! ([`row_limit`](GeneralConfig::row_limit), [`threads`](GeneralConfig::threads),
//! [`memory_limit`](GeneralConfig::memory_limit)) fold in the defaults so a consumer never has to
//! repeat them.

use serde::Deserialize;

/// The built-in interactive row cap when `[general] row_limit` is absent — mirrors the App's
/// [`VIEWPORT_ROW_LIMIT`](crate::app::VIEWPORT_ROW_LIMIT) so the config default and the wired
/// constant agree.
pub const DEFAULT_ROW_LIMIT: usize = 1000;

/// The `[general]` section: cross-cutting engine defaults the rest of ciq reads as plain data.
///
/// `threads` and `memory_limit` map onto DuckDB `SET` pragmas the engine applies at load (the
/// `SET threads=<n>` lever validated by the A1/A2 spike, `dev/ASSUMPTIONS.md`). For `threads`,
/// `None` means the engine's bounded default (`DEFAULT_THREADS`, the A2 cap); for `memory_limit`,
/// `None` leaves DuckDB's own default. The accessors return the raw `Option`; the engine
/// (`DuckdbEngine::open_with`) folds in the thread default and applies `memory_limit` only when set.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct GeneralConfig {
    /// Default interactive `LIMIT N` (the viewport cap). `None` -> [`DEFAULT_ROW_LIMIT`].
    pub row_limit: Option<usize>,
    /// DuckDB worker-thread bound (`SET threads=<n>`). `None` -> the engine's bounded default.
    pub threads: Option<u32>,
    /// DuckDB memory cap as a DuckDB size string (e.g. `"4GB"`, `"512MB"`) applied as
    /// `SET memory_limit='<s>'`. `None` -> DuckDB's own default. Validated by DuckDB at load — a
    /// malformed string surfaces as a clean load error, never a panic.
    pub memory_limit: Option<String>,
}

impl GeneralConfig {
    /// The effective interactive row cap: the configured value, or [`DEFAULT_ROW_LIMIT`]. A
    /// configured `0` is treated as "no explicit cap was meaningfully set" and clamped to at
    /// least 1 so the viewport always shows a row.
    pub fn row_limit(&self) -> usize {
        self.row_limit
            .map(|n| n.max(1))
            .unwrap_or(DEFAULT_ROW_LIMIT)
    }

    /// The configured DuckDB thread bound, if any (`None` = the engine applies its bounded default).
    pub fn threads(&self) -> Option<u32> {
        self.threads
    }

    /// The configured DuckDB memory limit string, if any (`None` = leave DuckDB's default).
    pub fn memory_limit(&self) -> Option<&str> {
        self.memory_limit.as_deref()
    }
}
