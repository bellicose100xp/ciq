//! The ciq base palette, in two polarities.
//!
//! [`DARK`] is the galaxy dark palette verbatim from `jiq/src/theme/galaxy.rs::galaxy_dark`
//! (electric cyan / golden yellow / fresh green / soft magenta + pink on a deep-space-blue
//! background). [`LIGHT`] keeps the same hue families but pulls every foreground down to roughly
//! 30–45% luminance so it holds a WCAG contrast of at least 3:1 against a white terminal, and
//! inverts the background ramp so the popups' opaque surfaces and the focused-pane tint read as
//! light elevations instead of dark islands.
//!
//! Exactly one palette is active per process, chosen once at startup from `[theme] mode` plus the
//! terminal's reported background ([`set_polarity`]). Every semantic accessor in
//! [`theme`](crate::theme) reads it through [`pal`], so render files never care which is live.
//! Tests default to dark (nothing sets the switch), which keeps every existing snapshot stable.

use std::sync::atomic::{AtomicBool, Ordering};

use ratatui::style::Color;

use super::polarity::Polarity;

/// Every color the theme draws from. One instance per polarity; see [`DARK`] and [`LIGHT`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    // --- text + background ramp ---
    pub text: Color,
    /// Quieter than `text_muted` — the least visible foreground (unfocused borders, type hints).
    pub text_dim: Color,
    pub text_muted: Color,
    /// The base surface: the opaque popup fill and the color every tint blends toward.
    pub bg: Color,
    pub bg_surface: Color,
    pub bg_hover: Color,
    pub bg_highlight: Color,
    /// Search-match band — one step past `bg_highlight` so a non-current match run still reads
    /// against the grid.
    pub bg_match: Color,

    /// Per-column hues, rotated by absolute column index (`grid::column`) so adjacent columns
    /// read as distinct bands and a column keeps its hue as the grid scrolls horizontally. The
    /// order alternates hue families (blue / green / peach / mauve / …) so neighbors never share
    /// one.
    pub column_pastels: [Color; 8],

    // --- accents ---
    pub cyan: Color,
    pub yellow: Color,
    pub green: Color,
    pub magenta: Color,
    pub pink: Color,
    pub red: Color,
    pub orange: Color,
    pub purple: Color,

    // --- semantic aliases ---
    pub success: Color,
    pub warning: Color,
    pub error: Color,
    pub info: Color,

    // --- borders ---
    pub border_focused: Color,
    pub border_unfocused: Color,
    pub border_error: Color,
    pub border_warning: Color,
}

/// The galaxy dark palette — the default, and what every headless snapshot renders with.
pub const DARK: Palette = {
    const TEXT_DIM: Color = Color::Rgb(90, 92, 119);
    const CYAN: Color = Color::Rgb(0, 217, 255);
    const YELLOW: Color = Color::Rgb(255, 217, 61);
    const GREEN: Color = Color::Rgb(107, 203, 119);
    const RED: Color = Color::Rgb(224, 108, 117);
    Palette {
        text: Color::Rgb(236, 236, 244),
        text_dim: TEXT_DIM,
        text_muted: Color::Rgb(130, 133, 158),
        bg: Color::Rgb(26, 26, 46),
        bg_surface: Color::Rgb(35, 35, 58),
        bg_hover: Color::Rgb(45, 45, 72),
        bg_highlight: Color::Rgb(55, 55, 85),
        bg_match: Color::Rgb(85, 85, 115),
        column_pastels: [
            Color::Rgb(137, 180, 250), // blue
            Color::Rgb(166, 227, 161), // green
            Color::Rgb(250, 179, 135), // peach
            Color::Rgb(203, 166, 247), // mauve
            Color::Rgb(249, 226, 175), // yellow
            Color::Rgb(148, 226, 213), // teal
            Color::Rgb(243, 139, 168), // pink
            Color::Rgb(180, 190, 254), // lavender
        ],
        cyan: CYAN,
        yellow: YELLOW,
        green: GREEN,
        magenta: Color::Rgb(198, 120, 221),
        pink: Color::Rgb(255, 107, 157),
        red: RED,
        orange: Color::Rgb(255, 184, 108),
        purple: Color::Rgb(189, 147, 249),
        success: GREEN,
        warning: YELLOW,
        error: RED,
        info: CYAN,
        border_focused: CYAN,
        border_unfocused: TEXT_DIM,
        border_error: RED,
        border_warning: YELLOW,
    }
};

/// The light palette — the same hue families as [`DARK`], deepened for a white background. Every
/// primary foreground clears 3:1 against `bg` (asserted in the tests), and the background ramp
/// runs downward from near-white so elevation reads as a slightly darker band.
pub const LIGHT: Palette = {
    const TEXT_DIM: Color = Color::Rgb(156, 160, 176);
    const CYAN: Color = Color::Rgb(0, 130, 170);
    const YELLOW: Color = Color::Rgb(180, 110, 10);
    const GREEN: Color = Color::Rgb(45, 135, 40);
    const RED: Color = Color::Rgb(210, 15, 57);
    Palette {
        text: Color::Rgb(76, 79, 105),
        text_dim: TEXT_DIM,
        text_muted: Color::Rgb(108, 111, 133),
        bg: Color::Rgb(239, 241, 245),
        bg_surface: Color::Rgb(230, 233, 239),
        bg_hover: Color::Rgb(220, 224, 232),
        bg_highlight: Color::Rgb(204, 208, 218),
        bg_match: Color::Rgb(188, 192, 204),
        column_pastels: [
            Color::Rgb(30, 102, 245), // blue
            Color::Rgb(45, 135, 40),  // green
            Color::Rgb(215, 80, 10),  // peach
            Color::Rgb(136, 57, 239), // mauve
            Color::Rgb(180, 110, 10), // amber
            Color::Rgb(23, 146, 153), // teal
            Color::Rgb(200, 50, 140), // pink
            Color::Rgb(90, 105, 230), // lavender
        ],
        cyan: CYAN,
        yellow: YELLOW,
        green: GREEN,
        magenta: Color::Rgb(136, 57, 239),
        pink: Color::Rgb(200, 50, 140),
        red: RED,
        orange: Color::Rgb(215, 80, 10),
        purple: Color::Rgb(110, 70, 210),
        success: GREEN,
        warning: YELLOW,
        error: RED,
        info: CYAN,
        border_focused: CYAN,
        border_unfocused: TEXT_DIM,
        border_error: RED,
        border_warning: YELLOW,
    }
};

/// The process-wide polarity switch. `false` (dark) until the shell sets it at startup.
static LIGHT_ACTIVE: AtomicBool = AtomicBool::new(false);

/// Select the active palette for the rest of the process. Called once by the event loop before
/// the first draw; never from library logic.
pub fn set_polarity(polarity: Polarity) {
    LIGHT_ACTIVE.store(polarity == Polarity::Light, Ordering::Relaxed);
}

/// The currently active polarity.
pub fn polarity() -> Polarity {
    if LIGHT_ACTIVE.load(Ordering::Relaxed) {
        Polarity::Light
    } else {
        Polarity::Dark
    }
}

/// The palette for a polarity, independent of the process switch.
pub fn palette_for(polarity: Polarity) -> &'static Palette {
    match polarity {
        Polarity::Dark => &DARK,
        Polarity::Light => &LIGHT,
    }
}

/// The active palette — what every theme accessor reads.
pub fn pal() -> &'static Palette {
    palette_for(polarity())
}

/// Run `f` with `polarity` active, restoring the previous polarity afterwards (also on panic).
/// Test-only: the suite runs single-threaded, so the switch cannot race.
#[cfg(test)]
pub fn with_polarity<R>(polarity: Polarity, f: impl FnOnce() -> R) -> R {
    struct Restore(Polarity);
    impl Drop for Restore {
        fn drop(&mut self) {
            set_polarity(self.0);
        }
    }
    let _restore = Restore(self::polarity());
    set_polarity(polarity);
    f()
}

/// Blend `accent` toward the active base `bg` by `accent_pct`/100, yielding a faint accent-tinted
/// background (no alpha in a terminal, so we mix RGB toward the base). Used for the focused
/// query-pane elevation band — a low percentage reads as "the base, gently tinted by the mode
/// color" rather than a loud fill. Falls back to `bg_surface` if either color isn't RGB.
pub fn tint_bg(accent: Color, accent_pct: u16) -> Color {
    fn mix(a: u8, b: u8, pct: u16) -> u8 {
        ((a as u16 * pct + b as u16 * (100 - pct)) / 100) as u8
    }
    let p = pal();
    match (accent, p.bg) {
        (Color::Rgb(ar, ag, ab), Color::Rgb(br, bg, bb)) => Color::Rgb(
            mix(ar, br, accent_pct),
            mix(ag, bg, accent_pct),
            mix(ab, bb, accent_pct),
        ),
        _ => p.bg_surface,
    }
}
