//! The `[theme]` config section — a minimal palette-mode stub (`dev/PLAN.md` §0/Q5, §6.8).
//!
//! ciq centralizes every color in [`crate::theme`]; this section is the *config* surface that a
//! future polarity pass reads, deliberately kept minimal for Phase 5. Today it carries only the
//! light/dark mode hint (`auto` = let the terminal decide, the §4.7 human-validated default) and a
//! free-form per-surface override map that `theme.rs` can consult once the polarity work lands.
//! Parsing it now means the schema is forward-stable: a user's `[theme]` block is accepted (not a
//! parse error) before the renderer wires every key.

use std::collections::BTreeMap;

use serde::Deserialize;

/// Light/dark adaptation mode. `Auto` defers to the terminal (the current behavior — colors are
/// chosen for legibility in both); `Light`/`Dark` pin the polarity when the user knows their
/// terminal. The renderer consults this once the polarity pass lands; for now it is parsed and
/// stored so the `[theme]` block is forward-compatible.
#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ThemeMode {
    /// Let the terminal/theme decide (the default — legible in light and dark).
    #[default]
    Auto,
    /// Pin to a light-terminal palette.
    Light,
    /// Pin to a dark-terminal palette.
    Dark,
}

/// The `[theme]` section: the polarity mode plus a forward-compat override map.
///
/// `overrides` is a `surface.role = "Color"` map (e.g. `grid.header = "Cyan"`) the renderer can
/// resolve later; unknown surfaces/roles inside that map are simply ignored (forward-compat), and
/// the values are validated where they are consumed, not here — this section stays a pure data
/// carrier. `deny_unknown_fields` (matching the other sections) rejects an unknown *top-level*
/// `[theme]` key — e.g. a `mod` typo — without touching the deliberately open `[theme.overrides]`
/// map, whose arbitrary inner keys are still accepted.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct ThemeConfig {
    /// Light/dark adaptation mode.
    pub mode: ThemeMode,
    /// Free-form per-surface color overrides (`"grid.header" = "Cyan"`). Consulted by the
    /// renderer once the polarity pass lands; ordered (`BTreeMap`) so any future emit is stable.
    pub overrides: BTreeMap<String, String>,
}

impl ThemeConfig {
    /// The configured polarity mode.
    pub fn mode(&self) -> ThemeMode {
        self.mode
    }

    /// A configured override for `key` (e.g. `"grid.header"`), if present.
    pub fn override_for(&self, key: &str) -> Option<&str> {
        self.overrides.get(key).map(String::as_str)
    }
}
