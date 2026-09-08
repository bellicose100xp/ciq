use ratatui::style::Color;

use crate::theme::base::{
    DARK, LIGHT, Palette, pal, palette_for, polarity, tint_bg, with_polarity,
};
use crate::theme::polarity::{Polarity, contrast_ratio, from_background};

fn rgb(c: Color) -> (u8, u8, u8) {
    match c {
        Color::Rgb(r, g, b) => (r, g, b),
        other => panic!("palette color is not RGB: {other:?}"),
    }
}

/// The foregrounds that carry content and must clear the 3:1 legibility floor against `bg`.
fn primary_foregrounds(p: &Palette) -> Vec<Color> {
    let mut v = vec![
        p.text,
        p.text_muted,
        p.cyan,
        p.yellow,
        p.green,
        p.magenta,
        p.pink,
        p.red,
        p.orange,
        p.purple,
    ];
    v.extend(p.column_pastels);
    v
}

#[test]
fn light_foregrounds_clear_three_to_one_against_the_light_base() {
    let bg = rgb(LIGHT.bg);
    for fg in primary_foregrounds(&LIGHT) {
        let ratio = contrast_ratio(rgb(fg), bg);
        assert!(ratio >= 3.0, "{fg:?} on light bg is only {ratio:.2}:1");
    }
    // Dim is the deliberately quiet tier, but it still has to be visible.
    assert!(contrast_ratio(rgb(LIGHT.text_dim), bg) >= 2.0);
}

#[test]
fn dark_foregrounds_clear_three_to_one_against_the_dark_base() {
    let bg = rgb(DARK.bg);
    for fg in primary_foregrounds(&DARK) {
        let ratio = contrast_ratio(rgb(fg), bg);
        assert!(ratio >= 3.0, "{fg:?} on dark bg is only {ratio:.2}:1");
    }
    assert!(contrast_ratio(rgb(DARK.text_dim), bg) >= 2.0);
}

#[test]
fn each_palette_base_matches_its_own_polarity() {
    assert_eq!(from_background(rgb(DARK.bg)), Polarity::Dark);
    assert_eq!(from_background(rgb(LIGHT.bg)), Polarity::Light);
}

#[test]
fn background_ramps_run_away_from_the_base() {
    fn lum(c: Color) -> f64 {
        let (r, g, b) = rgb(c);
        crate::theme::polarity::relative_luminance(r, g, b)
    }
    // Dark: every elevation step is brighter than the last.
    let d = [
        DARK.bg,
        DARK.bg_surface,
        DARK.bg_hover,
        DARK.bg_highlight,
        DARK.bg_match,
    ];
    assert!(
        d.windows(2).all(|w| lum(w[0]) < lum(w[1])),
        "dark ramp must ascend"
    );
    // Light: every elevation step is darker than the last.
    let l = [
        LIGHT.bg,
        LIGHT.bg_surface,
        LIGHT.bg_hover,
        LIGHT.bg_highlight,
        LIGHT.bg_match,
    ];
    assert!(
        l.windows(2).all(|w| lum(w[0]) > lum(w[1])),
        "light ramp must descend"
    );
}

#[test]
fn semantic_aliases_point_at_their_accents_in_both_palettes() {
    for p in [&DARK, &LIGHT] {
        assert_eq!(p.success, p.green);
        assert_eq!(p.warning, p.yellow);
        assert_eq!(p.error, p.red);
        assert_eq!(p.info, p.cyan);
        assert_eq!(p.border_focused, p.cyan);
        assert_eq!(p.border_unfocused, p.text_dim);
        assert_eq!(p.border_error, p.red);
        assert_eq!(p.border_warning, p.yellow);
    }
}

#[test]
fn column_pastels_alternate_hue_families_in_both_palettes() {
    // Adjacent columns never share a color, and the two palettes agree on the slot count.
    for p in [&DARK, &LIGHT] {
        for w in p.column_pastels.windows(2) {
            assert_ne!(w[0], w[1]);
        }
    }
    assert_eq!(DARK.column_pastels.len(), LIGHT.column_pastels.len());
}

#[test]
fn default_polarity_is_dark_and_pal_follows_the_switch() {
    assert_eq!(polarity(), Polarity::Dark);
    assert_eq!(pal(), &DARK);
    with_polarity(Polarity::Light, || {
        assert_eq!(polarity(), Polarity::Light);
        assert_eq!(pal(), &LIGHT);
        assert_eq!(pal().text, LIGHT.text);
    });
    assert_eq!(polarity(), Polarity::Dark);
    assert_eq!(pal(), &DARK);
}

#[test]
fn with_polarity_restores_even_when_the_body_panics() {
    let result = std::panic::catch_unwind(|| {
        with_polarity(Polarity::Light, || panic!("boom"));
    });
    assert!(result.is_err());
    assert_eq!(polarity(), Polarity::Dark);
}

#[test]
fn palette_for_is_independent_of_the_switch() {
    assert_eq!(palette_for(Polarity::Dark), &DARK);
    assert_eq!(palette_for(Polarity::Light), &LIGHT);
    with_polarity(Polarity::Light, || {
        assert_eq!(palette_for(Polarity::Dark), &DARK);
    });
}

#[test]
fn tint_bg_blends_toward_the_active_base() {
    let accent = Color::Rgb(100, 200, 0);
    // 0% accent is the base itself; 100% is the accent itself.
    assert_eq!(tint_bg(accent, 0), DARK.bg);
    assert_eq!(tint_bg(accent, 100), accent);
    with_polarity(Polarity::Light, || {
        assert_eq!(tint_bg(accent, 0), LIGHT.bg);
        // A light tint sits between the accent and the light base on every channel.
        let (r, g, b) = rgb(tint_bg(accent, 18));
        let (br, bgc, bb) = rgb(LIGHT.bg);
        assert!(r <= br && r >= 100);
        assert!(g <= bgc.max(200) && g >= bgc.min(200));
        assert!(b <= bb);
    });
}

#[test]
fn tint_bg_falls_back_to_surface_for_non_rgb_accents() {
    assert_eq!(tint_bg(Color::Cyan, 18), DARK.bg_surface);
    with_polarity(Polarity::Light, || {
        assert_eq!(tint_bg(Color::Cyan, 18), LIGHT.bg_surface);
    });
}

#[test]
fn grid_accessors_switch_palette_with_polarity() {
    use crate::theme::grid;
    assert_eq!(grid::column(0).fg, Some(DARK.column_pastels[0]));
    assert_eq!(grid::header_column(4).fg, Some(DARK.column_pastels[4]));
    with_polarity(Polarity::Light, || {
        assert_eq!(grid::column(0).fg, Some(LIGHT.column_pastels[0]));
        assert_eq!(grid::header_column(4).fg, Some(LIGHT.column_pastels[4]));
        assert_eq!(grid::cell().fg, Some(LIGHT.text));
        assert_eq!(grid::hovered_bg().bg, Some(LIGHT.bg_highlight));
        assert_eq!(crate::theme::popup::surface().bg, Some(LIGHT.bg));
        assert_eq!(crate::theme::help_line::description().fg, Some(LIGHT.text));
    });
}
