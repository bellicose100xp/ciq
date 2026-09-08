//! The `[theme]` config section (`dev/PLAN.md` §0/Q5, §6.8).
//!
//! ciq centralizes every color in [`crate::theme`]; this section is the *config* surface for it.
//! `mode` picks the palette polarity: `auto` (the default) asks the terminal for its background
//! color at startup and falls back to dark, while `light` / `dark` pin it
//! ([`crate::theme::polarity::resolve`]). `overrides` is a free-form per-surface map kept for
//! forward compatibility; nothing reads it yet, but a user's `[theme.overrides]` block is accepted
//! rather than rejected.

use std::collections::BTreeMap;

use serde::Deserialize;

/// Light/dark adaptation mode. `Auto` queries the terminal background (OSC 11, then
/// `COLORFGBG`) and falls back to dark; `Light`/`Dark` pin the polarity when the user knows their
/// terminal or it does not answer the query.
#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ThemeMode {
    /// Detect from the terminal's reported background (the default); dark when it does not answer.
    #[default]
    Auto,
    /// Pin to a light-terminal palette.
    Light,
    /// Pin to a dark-terminal palette.
    Dark,
}

/// The `[theme]` section: the polarity mode plus a forward-compat override map.
///
/// `overrides` is a `surface.role = "Color"` map (e.g. `grid.header = "Cyan"`) reserved for a
/// later pass; unknown surfaces/roles inside that map are simply ignored (forward-compat), and
/// the values are validated where they are consumed, not here — this section stays a pure data
/// carrier. `deny_unknown_fields` (matching the other sections) rejects an unknown *top-level*
/// `[theme]` key — e.g. a `mod` typo — without touching the deliberately open `[theme.overrides]`
/// map, whose arbitrary inner keys are still accepted.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct ThemeConfig {
    /// Light/dark adaptation mode.
    pub mode: ThemeMode,
    /// Free-form per-surface color overrides (`"grid.header" = "Cyan"`). Reserved, not yet read;
    /// ordered (`BTreeMap`) so any future emit is stable.
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
