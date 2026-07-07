//! The `[general]` config section — engine-wide defaults (`dev/PLAN.md` §0/Q5, Phase 5).
//!
//! Pure owned data parsed from TOML; every field optional with a documented default so an absent
//! key (or an absent `[general]` table) yields the conservative built-in. The accessor methods
//! ([`row_limit`](GeneralConfig::row_limit), [`threads`](GeneralConfig::threads),
//! [`memory_limit`](GeneralConfig::memory_limit)) fold in the defaults so a consumer never has to
//! repeat them.

use serde::Deserialize;

// There is deliberately NO built-in row cap: by default ciq shows every row a query returns.
// Capping the interactive viewport is a user choice, opted into via `[general] row_limit`.

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
    /// Optional interactive `LIMIT N` (the viewport cap). `None` (the default) -> no cap: every
    /// row the query returns is shown.
    pub row_limit: Option<usize>,
    /// DuckDB worker-thread bound (`SET threads=<n>`). `None` -> the engine's bounded default.
    pub threads: Option<u32>,
    /// DuckDB memory cap as a DuckDB size string (e.g. `"4GB"`, `"512MB"`) applied as
    /// `SET memory_limit='<s>'`. `None` -> DuckDB's own default. Validated by DuckDB at load — a
    /// malformed string surfaces as a clean load error, never a panic.
    pub memory_limit: Option<String>,
}

impl GeneralConfig {
    /// The configured interactive row cap, or `None` when uncapped (the default — no limit is a
    /// user choice). A configured `0` means "explicitly uncapped" and also yields `None`.
    pub fn row_limit(&self) -> Option<usize> {
        self.row_limit.filter(|&n| n > 0)
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
