//! Light/dark **polarity** — the pure half of theme adaptation.
//!
//! Everything here is data-in, data-out: parsing a terminal's OSC 11 background reply, reading
//! the `COLORFGBG` convention, classifying a background color as light or dark, and folding the
//! `[theme] mode` config over whatever was detected. The one real-terminal step (writing the OSC
//! 11 query and reading the reply off stdin) lives in the shell-exempt event loop and hands its
//! bytes to [`parse_osc11`].

use crate::config::ThemeMode;

/// Which side of the luminance midpoint the terminal background sits on. Selects the active
/// [`Palette`](super::base::Palette).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Polarity {
    /// Dark background — the galaxy dark palette (the default when nothing can be detected).
    #[default]
    Dark,
    /// Light background — the deeper, saturated palette that stays legible on white.
    Light,
}

/// The OSC 11 "report background color" query, ST-terminated. Written to the terminal verbatim;
/// the reply arrives on stdin in the shape [`parse_osc11`] accepts.
pub const OSC11_QUERY: &[u8] = b"\x1b]11;?\x1b\\";

/// Fold the configured mode over the detected polarity: `light` / `dark` pin the answer, `auto`
/// takes the detection and falls back to dark when the terminal did not answer.
pub fn resolve(mode: ThemeMode, detected: Option<Polarity>) -> Polarity {
    match mode {
        ThemeMode::Light => Polarity::Light,
        ThemeMode::Dark => Polarity::Dark,
        ThemeMode::Auto => detected.unwrap_or_default(),
    }
}

/// WCAG relative luminance of an sRGB color, 0.0 (black) to 1.0 (white).
pub fn relative_luminance(r: u8, g: u8, b: u8) -> f64 {
    fn linear(c: u8) -> f64 {
        let c = f64::from(c) / 255.0;
        if c <= 0.03928 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    }
    0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b)
}

/// WCAG contrast ratio between two colors, 1.0 (identical) to 21.0 (black on white).
pub fn contrast_ratio(a: (u8, u8, u8), b: (u8, u8, u8)) -> f64 {
    let la = relative_luminance(a.0, a.1, a.2);
    let lb = relative_luminance(b.0, b.1, b.2);
    let (hi, lo) = if la >= lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

/// Classify a terminal background color: above the luminance midpoint is light.
pub fn from_background(rgb: (u8, u8, u8)) -> Polarity {
    if relative_luminance(rgb.0, rgb.1, rgb.2) > 0.5 {
        Polarity::Light
    } else {
        Polarity::Dark
    }
}

/// Parse an OSC 11 reply — `ESC ] 11 ; rgb:RRRR/GGGG/BBBB` terminated by `BEL` or `ESC \` — into
/// 8-bit RGB. Accepts 8-, 12-, and 16-bit components (xterm reports 16-bit; some terminals send
/// 8-bit) and tolerates leading bytes before the `ESC ]`. `None` until a complete, well-formed
/// reply is present, so a caller can accumulate partial reads and retry.
pub fn parse_osc11(bytes: &[u8]) -> Option<(u8, u8, u8)> {
    let start = bytes.windows(2).position(|w| w == b"\x1b]")? + 2;
    let rest = &bytes[start..];
    let end = rest
        .iter()
        .position(|&b| b == 0x07 || b == 0x1b || b == b'\\')?;
    let body = std::str::from_utf8(&rest[..end]).ok()?;
    let payload = body.strip_prefix("11;")?;
    let spec = payload.strip_prefix("rgb:")?;
    let mut parts = spec.split('/');
    let r = component(parts.next()?)?;
    let g = component(parts.next()?)?;
    let b = component(parts.next()?)?;
    if parts.next().is_some() {
        return None;
    }
    Some((r, g, b))
}

/// Scale a 1–4 hex-digit X11 color component to 8 bits by taking its top byte.
fn component(hex: &str) -> Option<u8> {
    if hex.is_empty() || hex.len() > 4 {
        return None;
    }
    let v = u32::from_str_radix(hex, 16).ok()?;
    let bits = 4 * hex.len() as u32;
    Some((v >> bits.saturating_sub(8)) as u8)
}

/// Read the `COLORFGBG` convention (`"fg;bg"` or `"fg;x;bg"`, ANSI indices) that rxvt, konsole,
/// and some macOS terminals export. Vim's rule: background index 0–6 or 8 is dark, anything else
/// in 0–15 is light. `None` for anything that is not an ANSI index (e.g. `default`).
pub fn from_colorfgbg(value: &str) -> Option<Polarity> {
    let bg: u8 = value.rsplit(';').next()?.trim().parse().ok()?;
    match bg {
        0..=6 | 8 => Some(Polarity::Dark),
        7 | 9..=15 => Some(Polarity::Light),
        _ => None,
    }
}
