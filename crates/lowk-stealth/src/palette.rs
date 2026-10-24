//! Colour base of the adaptive overlay (O4). Pure functions, no OS code; the mapping table is documented in
//! the mapping table below; every constant appears in it.
//!
//! The pipeline per region of the screen under the overlay:
//!
//! 1. the region's mean colour (averaged in linear light) is converted to **OKLab** / OKLCH (`L`, `C`, `h`);
//! 2. the **polarity** is decided on `L` with a Schmitt trigger whose upper threshold rises with chroma
//!    (saturated mid-tones such as red or green get a dark scrim and white text, not a pale one);
//! 3. the **scrim** keeps the hue of the background, lowers its chroma and sets its lightness for contrast:
//!    dark scrim `L = 0.17..0.30`, light scrim `L = 0.95..0.99`; near-neutral backgrounds use the fixed presets;
//! 4. the **text** is white (`247,247,249`) on a dark scrim and near-black (`10,11,15`) on a light one; the
//!    muted text is the same colour at the alpha the CSS gives it; **accents** (interactive green, moving amber)
//!    keep their role but move in lightness (and, next to a scrim of the same hue, in hue) until they reach
//!    WCAG 4.5:1 against the scrim;
//! 5. contrast is verified as WCAG 2.x ratio and APCA Lc (W3 0.0.98G-4g) against the scrim blended over the
//!    worst backdrop pixel.

/// sRGB 0..255 triple.
pub type Rgb = [u8; 3];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Oklab {
    pub l: f32,
    pub a: f32,
    pub b: f32,
}

impl Oklab {
    pub fn chroma(&self) -> f32 {
        self.a.hypot(self.b)
    }
    /// Hue in degrees 0..360.
    pub fn hue(&self) -> f32 {
        self.b.atan2(self.a).to_degrees().rem_euclid(360.0)
    }
    /// Euclidean distance (ΔE_OK).
    pub fn dist(&self, o: &Oklab) -> f32 {
        ((self.l - o.l).powi(2) + (self.a - o.a).powi(2) + (self.b - o.b).powi(2)).sqrt()
    }
}

pub fn srgb_to_linear(c: u8) -> f32 {
    let c = f32::from(c) / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(c: f32) -> f32 {
    if c <= 0.003_130_8 {
        12.92 * c
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

/// Linear-light RGB (0..1 each) to OKLab.
pub fn linear_to_oklab(r: f32, g: f32, b: f32) -> Oklab {
    let l = (0.412_221_47 * r + 0.536_332_55 * g + 0.051_445_995 * b).cbrt();
    let m = (0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b).cbrt();
    let s = (0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b).cbrt();
    Oklab {
        l: 0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s,
        a: 1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s,
        b: 0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s,
    }
}

pub fn rgb_to_oklab(c: Rgb) -> Oklab {
    linear_to_oklab(srgb_to_linear(c[0]), srgb_to_linear(c[1]), srgb_to_linear(c[2]))
}

fn oklab_to_linear(c: Oklab) -> [f32; 3] {
    let l = c.l + 0.396_337_78 * c.a + 0.215_803_76 * c.b;
    let m = c.l - 0.105_561_346 * c.a - 0.063_854_17 * c.b;
    let s = c.l - 0.089_484_18 * c.a - 1.291_485_5 * c.b;
    let (l, m, s) = (l * l * l, m * m * m, s * s * s);
    [
        4.076_741_7 * l - 3.307_711_6 * m + 0.230_969_94 * s,
        -1.268_438 * l + 2.609_757_4 * m - 0.341_319_4 * s,
        -0.004_196_086_3 * l - 0.703_418_6 * m + 1.707_614_7 * s,
    ]
}

pub fn oklch(l: f32, c: f32, h_deg: f32) -> Oklab {
    let h = h_deg.to_radians();
    Oklab {
        l,
        a: c * h.cos(),
        b: c * h.sin(),
    }
}

/// OKLCH to sRGB8. Out-of-gamut colours lose chroma (never lightness or hue) until they fit.
pub fn oklch_to_rgb(l: f32, c: f32, h_deg: f32) -> Rgb {
    let fits = |c: f32| {
        let lin = oklab_to_linear(oklch(l, c, h_deg));
        lin.iter().all(|v| (-0.0005..=1.0005).contains(v))
    };
    let mut c = c.max(0.0);
    if !fits(c) {
        let (mut lo, mut hi) = (0.0f32, c);
        for _ in 0..20 {
            let mid = (lo + hi) / 2.0;
            if fits(mid) {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        c = lo;
    }
    let lin = oklab_to_linear(oklch(l, c, h_deg));
    let q = |v: f32| (linear_to_srgb(v.clamp(0.0, 1.0)) * 255.0).round() as u8;
    [q(lin[0]), q(lin[1]), q(lin[2])]
}

/// WCAG relative luminance.
pub fn rel_luminance(c: Rgb) -> f32 {
    0.2126 * srgb_to_linear(c[0]) + 0.7152 * srgb_to_linear(c[1]) + 0.0722 * srgb_to_linear(c[2])
}

/// WCAG 2.x contrast ratio of two relative luminances.
pub fn wcag(a: f32, b: f32) -> f32 {
    let (hi, lo) = if a > b { (a, b) } else { (b, a) };
    (hi + 0.05) / (lo + 0.05)
}

/// APCA-W3 0.0.98G-4g luminance (simple sRGB 2.4 gamma, its own coefficients).
pub fn apca_y(c: Rgb) -> f32 {
    let p = |v: u8| (f32::from(v) / 255.0).powf(2.4);
    0.212_672_9 * p(c[0]) + 0.715_152_2 * p(c[1]) + 0.072_175 * p(c[2])
}

/// APCA lightness contrast Lc (signed: positive = dark text on a light background).
/// |Lc| 60 is the floor for body text, 75 is preferred, 90 is for small light-weight text.
pub fn apca_lc(text: Rgb, bg: Rgb) -> f32 {
    const CLAMP: f32 = 0.022;
    let soft = |y: f32| if y < CLAMP { y + (CLAMP - y).powf(1.414) } else { y };
    let (yt, yb) = (soft(apca_y(text)), soft(apca_y(bg)));
    if (yb - yt).abs() < 0.0005 {
        return 0.0;
    }
    if yb > yt {
        let s = (yb.powf(0.56) - yt.powf(0.57)) * 1.14;
        if s < 0.1 {
            0.0
        } else {
            (s - 0.027) * 100.0
        }
    } else {
        let s = (yb.powf(0.65) - yt.powf(0.62)) * 1.14;
        if s > -0.1 {
            0.0
        } else {
            (s + 0.027) * 100.0
        }
    }
}

/// `fg` (alpha `a`) over `bg`, per channel in sRGB space (what a browser does).
pub fn over(fg: Rgb, a: f32, bg: Rgb) -> Rgb {
    let m = |f: u8, b: u8| {
        (f32::from(f) * a + f32::from(b) * (1.0 - a))
            .round()
            .clamp(0.0, 255.0) as u8
    };
    [m(fg[0], bg[0]), m(fg[1], bg[1]), m(fg[2], bg[2])]
}

pub fn fmt(c: Rgb) -> String {
    format!("{},{},{}", c[0], c[1], c[2])
}

// ---- the mapping constants ----

/// Fixed presets for near-neutral backdrops (the original look of the overlay).
pub const DARK_SCRIM: Rgb = [12, 12, 15];
pub const DARK_INV: Rgb = [0, 0, 0];
pub const LIGHT_SCRIM: Rgb = [255, 255, 255];
pub const LIGHT_INV: Rgb = [255, 255, 255];
pub const WHITE_TEXT: Rgb = [247, 247, 249];
pub const BLACK_TEXT: Rgb = [10, 11, 15];

/// Backdrops with OKLCH chroma below this are neutral: preset scrim, no tint.
pub const NEUTRAL_C: f32 = 0.02;
/// Polarity thresholds on OKLab `L` (Schmitt): dark text above `UP`, white text below `DOWN`.
/// (`L` 0.60 / 0.52 equal the old linear-luma gates 0.22 / 0.14.)
pub const POL_UP: f32 = 0.60;
pub const POL_DOWN: f32 = 0.52;
/// The upper threshold rises by up to this much for fully saturated backdrops (`C >= CHROMA_FULL`).
pub const POL_CHROMA_BIAS: f32 = 0.20;
pub const CHROMA_FULL: f32 = 0.25;
/// Yellow-green hues (OKLCH `h`) look light at any mid lightness: the chroma bias is cut there to
/// `1 - YELLOW_CUT` (full cut inside `YELLOW_CORE`, linear ramp out to `YELLOW_EDGE`). Yellow, olive-yellow, lime,
/// yellow-orange and light green get a LIGHT tinted scrim with dark text; red, green, teal, blue, purple do not move.
pub const YELLOW_CUT: f32 = 0.85;
pub const YELLOW_CORE: (f32, f32) = (85.0, 125.0);
pub const YELLOW_EDGE: (f32, f32) = (65.0, 140.0);
/// Dark scrim: `L = clamp(0.17 + 0.25 * max(L_bg - 0.15, 0), 0.17, 0.30)`, chroma `min(0.6 * C_bg, 0.10)`.
pub const DARK_L0: f32 = 0.17;
pub const DARK_L_SLOPE: f32 = 0.25;
pub const DARK_L_MAX: f32 = 0.30;
pub const DARK_C_K: f32 = 0.60;
pub const DARK_C_MAX: f32 = 0.10;
/// Light scrim: `L = clamp(0.95 + 0.04 * (L_bg - 0.70) / 0.30, 0.95, 0.99)`, chroma `min(0.25 * C_bg, 0.04)`.
pub const LIGHT_L0: f32 = 0.95;
pub const LIGHT_L_MAX: f32 = 0.99;
pub const LIGHT_C_K: f32 = 0.25;
pub const LIGHT_C_MAX: f32 = 0.04;
/// An accent whose hue is closer than this (degrees) to a tinted scrim moves to the next candidate hue.
pub const ACCENT_HUE_GAP: f32 = 45.0;
/// Accent targets: WCAG ratio against the scrim colour, and against the scrim over the worst backdrop.
pub const ACCENT_CONTRAST: f32 = 4.5;
pub const ACCENT_WORST: f32 = 3.0;
/// Candidate hues, preferred first. `int` = interactive (green family), `mv` = moving (amber family).
pub const INT_HUES: [f32; 3] = [150.0, 205.0, 100.0];
pub const MV_HUES: [f32; 3] = [80.0, 25.0, 330.0];

/// What the mapping decides for one region.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Scheme {
    /// `true` = dark text on a light scrim.
    pub dark_text: bool,
    /// The scrim has a hue of the backdrop (not the fixed neutral preset).
    pub tinted: bool,
    pub scrim: Rgb,
    pub fg: Rgb,
    pub inv: Rgb,
    pub int: Rgb,
    pub mv: Rgb,
}

/// Polarity with hysteresis. `prev`: the previous decision of this region (`true` = dark text).
pub fn polarity(lab: &Oklab, prev: Option<bool>) -> bool {
    let bias = POL_CHROMA_BIAS
        * (lab.chroma() / CHROMA_FULL).min(1.0)
        * (1.0 - YELLOW_CUT * yellow_weight(lab.hue()));
    let (up, down) = (POL_UP + bias, POL_DOWN + bias);
    match prev {
        None => lab.l > (up + down) / 2.0,
        Some(true) => lab.l >= down,
        Some(false) => lab.l > up,
    }
}

/// 1 inside the yellow core hue band, 0 outside the edge band, linear in between.
fn yellow_weight(h: f32) -> f32 {
    let (c0, c1) = YELLOW_CORE;
    let (e0, e1) = YELLOW_EDGE;
    if h < e0 || h > e1 {
        0.0
    } else if h < c0 {
        (h - e0) / (c0 - e0)
    } else if h <= c1 {
        1.0
    } else {
        (e1 - h) / (e1 - c1)
    }
}

fn hue_gap(a: f32, b: f32) -> f32 {
    let d = (a - b).rem_euclid(360.0);
    d.min(360.0 - d)
}

/// An accent colour of one of `hues` (preferred first) that reaches WCAG [`ACCENT_CONTRAST`] against the scrim
/// and [`ACCENT_WORST`] against the scrim blended over the worst backdrop (black under a light scrim, white
/// under a dark one, at the CSS opacity floor).
fn accent(
    hues: &[f32; 3],
    l_dark: f32,
    l_light: f32,
    c: f32,
    scrim: Rgb,
    scrim_hue: Option<f32>,
    dark_text: bool,
) -> Rgb {
    let ys = rel_luminance(scrim);
    let worst = over(
        scrim,
        SCRIM_FLOOR,
        if dark_text { [0, 0, 0] } else { [255, 255, 255] },
    );
    let yw = rel_luminance(worst);
    let hue = scrim_hue
        .and_then(|sh| hues.iter().copied().find(|h| hue_gap(*h, sh) >= ACCENT_HUE_GAP))
        .unwrap_or(hues[0]);
    let mut l = if dark_text { l_light } else { l_dark };
    let step = if dark_text { -0.02 } else { 0.02 };
    for _ in 0..40 {
        let rgb = oklch_to_rgb(l, c, hue);
        let y = rel_luminance(rgb);
        if wcag(y, ys) >= ACCENT_CONTRAST && wcag(y, yw) >= ACCENT_WORST {
            return rgb;
        }
        l = (l + step).clamp(0.0, 1.0);
    }
    oklch_to_rgb(l, c, hue)
}

/// The scheme for a region whose mean colour is `lab`, with the polarity already decided.
pub fn scheme(lab: &Oklab, dark_text: bool) -> Scheme {
    let c_bg = lab.chroma();
    let h = lab.hue();
    let tinted = c_bg >= NEUTRAL_C;
    let (scrim, fg, inv) = if dark_text {
        let scrim = if tinted {
            let l =
                (LIGHT_L0 + (LIGHT_L_MAX - LIGHT_L0) * ((lab.l - 0.70) / 0.30)).clamp(LIGHT_L0, LIGHT_L_MAX);
            oklch_to_rgb(l, (LIGHT_C_K * c_bg).min(LIGHT_C_MAX), h)
        } else {
            LIGHT_SCRIM
        };
        let inv = if tinted {
            oklch_to_rgb(0.995, (LIGHT_C_K * c_bg).min(LIGHT_C_MAX) * 0.5, h)
        } else {
            LIGHT_INV
        };
        (scrim, BLACK_TEXT, inv)
    } else {
        let scrim = if tinted {
            let l = (DARK_L0 + DARK_L_SLOPE * (lab.l - 0.15).max(0.0)).clamp(DARK_L0, DARK_L_MAX);
            oklch_to_rgb(l, (DARK_C_K * c_bg).min(DARK_C_MAX), h)
        } else {
            DARK_SCRIM
        };
        // The halo is the scrim colour pushed towards black, so it never reads as a coloured glow.
        let inv = if tinted {
            oklch_to_rgb(0.08, (DARK_C_K * c_bg).min(DARK_C_MAX) * 0.5, h)
        } else {
            DARK_INV
        };
        (scrim, WHITE_TEXT, inv)
    };
    let sh = tinted
        .then_some(rgb_to_oklab(scrim))
        .filter(|s| s.chroma() > 0.02)
        .map(|s| s.hue());
    let int = accent(&INT_HUES, 0.82, 0.52, 0.17, scrim, sh, dark_text);
    let mv = accent(&MV_HUES, 0.86, 0.50, 0.14, scrim, sh, dark_text);
    Scheme {
        dark_text,
        tinted,
        scrim,
        fg,
        inv,
        int,
        mv,
    }
}

/// Alpha of the muted text at the default 60 % transparency (k = 0.4): `0.72 + k * 0.16` in overlay.css.
pub const MUTED_ALPHA: f32 = 0.784;
/// Scrim opacity floor of the CSS (`--floor`): at k = 0.4 the scrim covers at least this much.
pub const SCRIM_FLOOR: f32 = 0.72;

/// Contrast numbers of one scheme over a backdrop pixel colour `bg` (the worst pixel), at scrim alpha `alpha`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Verdict {
    /// Composite background (scrim over the backdrop pixel).
    pub bg: Rgb,
    pub body_wcag: f32,
    pub body_lc: f32,
    pub muted_wcag: f32,
    pub muted_lc: f32,
    pub int_wcag: f32,
    pub mv_wcag: f32,
}

pub fn verdict(s: &Scheme, bg_pixel: Rgb, alpha: f32) -> Verdict {
    let comp = over(s.scrim, alpha, bg_pixel);
    let muted = over(s.fg, MUTED_ALPHA, comp);
    let y = rel_luminance(comp);
    Verdict {
        bg: comp,
        body_wcag: wcag(rel_luminance(s.fg), y),
        body_lc: apca_lc(s.fg, comp).abs(),
        muted_wcag: wcag(rel_luminance(muted), y),
        muted_lc: apca_lc(muted, comp).abs(),
        int_wcag: wcag(rel_luminance(over(s.int, 1.0, comp)), y),
        mv_wcag: wcag(rel_luminance(over(s.mv, 1.0, comp)), y),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lab(c: Rgb) -> Oklab {
        rgb_to_oklab(c)
    }

    #[test]
    fn oklab_round_trip_and_anchors() {
        for c in [
            [0u8, 0, 0],
            [255, 255, 255],
            [255, 0, 0],
            [0, 0, 128],
            [12, 12, 15],
            [120, 200, 30],
        ] {
            let l = lab(c);
            let back = oklch_to_rgb(l.l, l.chroma(), l.hue());
            for i in 0..3 {
                assert!(
                    (i32::from(back[i]) - i32::from(c[i])).abs() <= 1,
                    "{c:?} -> {back:?}"
                );
            }
        }
        assert!((lab([255, 255, 255]).l - 1.0).abs() < 0.01);
        assert!(lab([0, 0, 0]).l < 0.01);
        let blue = lab([0, 0, 255]);
        assert!(
            (blue.l - 0.452).abs() < 0.01 && (blue.hue() - 264.0).abs() < 2.0,
            "{blue:?}"
        );
    }

    #[test]
    fn gamut_mapping_keeps_lightness_and_hue() {
        let c = oklch_to_rgb(0.3, 0.4, 264.0); // far outside sRGB
        let l = lab(c);
        assert!((l.l - 0.3).abs() < 0.02, "{l:?}");
        assert!(hue_gap(l.hue(), 264.0) < 6.0, "{l:?}");
    }

    #[test]
    fn apca_and_wcag_known_values() {
        // black on white / white on black: Lc ~106 / ~-107 (published reference values)
        assert!((apca_lc([0, 0, 0], [255, 255, 255]) - 106.0).abs() < 1.5);
        assert!((apca_lc([255, 255, 255], [0, 0, 0]) + 107.9).abs() < 1.5);
        assert!((wcag(1.0, 0.0) - 21.0).abs() < 0.01);
        // identical colours: no contrast
        assert_eq!(apca_lc([120, 120, 120], [120, 120, 120]), 0.0);
        // mid-grey #777 on white is ~4.48:1 (just below AA)
        assert!((wcag(rel_luminance([255, 255, 255]), rel_luminance([119, 119, 119])) - 4.48).abs() < 0.05);
    }

    #[test]
    fn neutral_backdrops_use_the_presets() {
        let d = scheme(&lab([24, 26, 32]), false);
        assert_eq!(
            (d.scrim, d.fg, d.inv, d.tinted),
            (DARK_SCRIM, WHITE_TEXT, DARK_INV, false)
        );
        let l = scheme(&lab([245, 245, 240]), true);
        assert_eq!((l.scrim, l.fg, l.tinted), (LIGHT_SCRIM, BLACK_TEXT, false));
    }

    #[test]
    fn dark_saturated_blue_keeps_its_hue_instead_of_going_black() {
        // O4: navy / dark blue used to fall back to the black preset.
        for navy in [[0u8, 0, 128], [10, 20, 90], [20, 40, 160], [0, 0, 255]] {
            let lb = lab(navy);
            let s = scheme(&lb, polarity(&lb, None));
            assert!(!s.dark_text, "{navy:?}");
            let sl = rgb_to_oklab(s.scrim);
            assert!(
                s.tinted && sl.chroma() > 0.03,
                "{navy:?} scrim {:?} is not tinted",
                s.scrim
            );
            assert!(
                hue_gap(sl.hue(), lb.hue()) < 12.0,
                "{navy:?} hue {} vs {}",
                sl.hue(),
                lb.hue()
            );
            assert!(
                sl.l < 0.32 && s.scrim[2] > s.scrim[0] && s.scrim[2] > s.scrim[1],
                "{:?}",
                s.scrim
            );
        }
    }

    #[test]
    fn saturated_midtones_get_a_dark_tinted_scrim() {
        for (name, c) in [
            ("red", [220u8, 30, 30]),
            ("purple", [128, 0, 160]),
            ("green", [0, 170, 60]),
            ("teal", [0, 128, 128]),
        ] {
            let lb = lab(c);
            let s = scheme(&lb, polarity(&lb, None));
            assert!(!s.dark_text, "{name}");
            let v = verdict(&s, c, SCRIM_FLOOR);
            assert!(v.body_wcag >= 4.5 && v.body_lc >= 75.0, "{name} {v:?}");
        }
    }

    #[test]
    fn light_backdrops_get_a_light_scrim_with_dark_text() {
        for (name, c) in [
            ("yellow", [255u8, 235, 40]),
            ("cyan", [80, 230, 240]),
            ("white", [250, 250, 250]),
            ("pastel", [200, 220, 255]),
        ] {
            let lb = lab(c);
            let s = scheme(&lb, polarity(&lb, None));
            assert!(s.dark_text, "{name}");
            assert!(rgb_to_oklab(s.scrim).l >= 0.94, "{name} {:?}", s.scrim);
            let v = verdict(&s, c, SCRIM_FLOOR);
            assert!(v.body_wcag >= 7.0 && v.body_lc >= 75.0, "{name} {v:?}");
        }
    }

    #[test]
    fn bright_saturated_colours_get_a_light_tinted_scrim() {
        for (name, c) in [
            ("yellow", [240u8, 220, 40]),
            ("yellow-dim (live mean)", [175, 164, 54]),
            ("olive-yellow (live mean)", [135, 123, 22]),
            ("pure yellow", [255, 255, 0]),
            ("lime", [190, 255, 0]),
            ("light green", [144, 238, 144]),
            ("bright green", [0, 255, 0]),
            ("light cyan", [120, 240, 250]),
            ("pink", [255, 150, 200]),
            ("light orange", [255, 190, 90]),
            ("orange", [255, 165, 0]),
            ("gold", [255, 215, 0]),
        ] {
            let lb = lab(c);
            let s = scheme(&lb, polarity(&lb, None));
            assert!(s.dark_text && s.fg == BLACK_TEXT, "{name} {c:?} L {:.2}", lb.l);
            let sl = rgb_to_oklab(s.scrim);
            assert!(sl.l >= 0.94 && s.tinted, "{name} {:?}", s.scrim);
            if name.starts_with("yellow") || name.contains("lime") || name == "gold" {
                // the scrim keeps the warm hue of the backdrop
                assert!(s.scrim[2] < s.scrim[0].min(s.scrim[1]), "{name} {:?}", s.scrim);
            }
            for a in [SCRIM_FLOOR, 0.9, 1.0] {
                let v = verdict(&s, c, a);
                assert!(v.body_wcag >= 7.0 && v.body_lc >= 75.0, "{name} {v:?}");
                assert!(
                    v.muted_wcag >= 4.5 && v.int_wcag >= 4.0 && v.mv_wcag >= 4.0,
                    "{name} {v:?}"
                );
            }
        }
    }

    #[test]
    fn dark_and_mid_saturated_colours_still_get_dark_scrims() {
        for (name, c) in [
            ("red", [200u8, 30, 30]),
            ("bright red", [255, 0, 0]),
            ("green", [30, 140, 60]),
            ("mid green", [0, 170, 60]),
            ("teal", [0, 110, 120]),
            ("blue", [0, 0, 255]),
            ("purple", [128, 0, 160]),
            ("magenta", [220, 0, 200]),
            ("dark olive", [90, 80, 0]),
        ] {
            let lb = lab(c);
            assert!(!polarity(&lb, None), "{name} L {:.2}", lb.l);
        }
    }

    #[test]
    fn yellow_polarity_does_not_flicker_on_mid_colours() {
        // sweep a yellow from dark olive to bright yellow and back with noise: the decision flips at most once each way
        let mut prev: Option<bool> = None;
        let mut flips = 0;
        let up: Vec<u8> = (60..=250).step_by(5).collect();
        for pass in [up.clone(), up.iter().rev().copied().collect()] {
            for (i, v) in pass.iter().enumerate() {
                let jitter = if i % 2 == 0 { 3i32 } else { -3 };
                let c = [
                    (*v as i32 + jitter).clamp(0, 255) as u8,
                    (*v as i32 + jitter - 12).clamp(0, 255) as u8,
                    20,
                ];
                let d = polarity(&lab(c), prev);
                if prev.is_some_and(|p| p != d) {
                    flips += 1;
                }
                prev = Some(d);
            }
        }
        assert!(flips <= 2, "{flips} flips");
    }

    #[test]
    fn polarity_has_hysteresis_and_chroma_bias() {
        let g = |v: u8| lab([v, v, v]);
        // inside the neutral band the previous decision holds
        assert!(polarity(&g(118), Some(true)) && !polarity(&g(118), Some(false)));
        // red (L 0.63) is above the neutral gate but below the chroma-raised one
        let red = lab([255, 0, 0]);
        assert!(!polarity(&red, None));
        assert!(polarity(&lab([255, 235, 40]), None));
    }

    #[test]
    fn accent_avoids_the_scrim_hue_and_keeps_contrast() {
        // a green backdrop gets a green-tinted scrim: the interactive accent must not be green on green
        let lb = lab([0, 150, 60]);
        let s = scheme(&lb, polarity(&lb, None));
        let (si, ai) = (rgb_to_oklab(s.scrim), rgb_to_oklab(s.int));
        assert!(
            hue_gap(si.hue(), ai.hue()) >= ACCENT_HUE_GAP - 8.0,
            "{:?} vs {:?}",
            s.scrim,
            s.int
        );
        for bg in [
            [0u8, 0, 128],
            [128, 0, 160],
            [200, 30, 30],
            [255, 235, 40],
            [30, 30, 30],
            [250, 250, 250],
        ] {
            let lb = lab(bg);
            let s = scheme(&lb, polarity(&lb, None));
            let v = verdict(&s, bg, SCRIM_FLOOR);
            assert!(v.int_wcag >= 4.0 && v.mv_wcag >= 4.0, "{bg:?} {v:?}");
        }
    }

    #[test]
    fn scheme_is_total_over_a_colour_sweep() {
        // every sampled colour yields readable body text (WCAG 4.5 and APCA 60) at the CSS scrim floor
        let mut worst: f32 = 99.0;
        for r in (0..=255).step_by(51) {
            for g in (0..=255).step_by(51) {
                for b in (0..=255).step_by(51) {
                    let c = [r as u8, g as u8, b as u8];
                    let lb = lab(c);
                    let s = scheme(&lb, polarity(&lb, None));
                    let v = verdict(&s, c, SCRIM_FLOOR);
                    worst = worst.min(v.body_wcag);
                    assert!(v.body_wcag >= 4.5 && v.body_lc >= 60.0, "{c:?} {v:?}");
                    assert!(v.muted_wcag >= 3.0, "{c:?} {v:?}");
                }
            }
        }
        assert!(worst >= 4.5);
    }
}
