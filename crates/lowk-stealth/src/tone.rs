//! Adaptive tone: luminance of the pixels under the overlay -> palette tokens.
//!
//! Pure functions plus a small state machine ([`AdaptiveTone`]); no OS code, so
//! it is unit-tested with synthetic bitmaps. The sampler (a task in the app
//! layer driving a `RegionSampler`) feeds it a ~24x16 RGBA bitmap every ~2 s.
//!
//! Algorithm (docs/ARCHITECTURE.md 7.4):
//! 1. linear-light luminance per pixel; median, p10, p90, "busyness";
//! 2. median of the last three medians (rejects one-frame flashes);
//! 3. Schmitt trigger on that value: to dark text above [`ToneParams::up`],
//!    to light text below [`ToneParams::down`];
//! 4. saturated mid-tone backgrounds select the tinted `Color` palette (also
//!    with hysteresis);
//! 5. a new [`Tone`] is only emitted when it differs enough
//!    ([`ToneParams::deadband`]) so the UI never animates on noise.

use lowk_types::{Tone, ToneKind, ToneMode};
use std::collections::VecDeque;

/// Sample bitmap size requested from the sampler.
pub const SAMPLE_W: u32 = 24;
pub const SAMPLE_H: u32 = 16;

#[derive(Clone, Copy, Debug)]
pub struct ToneParams {
    /// Background luma above which text turns dark (light palette).
    pub up: f32,
    /// Background luma below which text turns light (dark palette).
    pub down: f32,
    /// Minimum luma change that justifies emitting a new tone.
    pub deadband: f32,
    /// Chroma (max-min of mean sRGB, 0..1) to enter / leave the `Color` palette.
    pub color_on: f32,
    pub color_off: f32,
    /// Saturated backgrounds brighter than this are treated as light.
    pub color_max_luma: f32,
    /// Target WCAG contrast for the scrim computation.
    pub target_contrast: f32,
}

impl Default for ToneParams {
    fn default() -> Self {
        Self {
            up: 0.22,
            down: 0.14,
            deadband: 0.03,
            color_on: 0.30,
            color_off: 0.22,
            color_max_luma: 0.5,
            target_contrast: 4.5,
        }
    }
}

/// Statistics of one sample bitmap.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SampleStats {
    /// Median linear luminance 0..1.
    pub median: f32,
    pub p10: f32,
    pub p90: f32,
    /// Spread of perceptual lightness between p90 and p10, 0..1.
    pub busy: f32,
    /// Mean colour, sRGB 0..255.
    pub mean_rgb: [f32; 3],
    /// max-min of the mean colour, 0..1.
    pub chroma: f32,
}

fn srgb_to_linear(c: u8) -> f32 {
    let c = c as f32 / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

pub fn luminance(r: u8, g: u8, b: u8) -> f32 {
    0.2126 * srgb_to_linear(r) + 0.7152 * srgb_to_linear(g) + 0.0722 * srgb_to_linear(b)
}

/// Analyse an RGBA8 bitmap. Returns `None` for an unusable sample: wrong
/// length, empty, or pure black (the sampler's "unknown, keep previous" value,
/// e.g. a GDI grab of a protected surface or no Screen Recording permission).
pub fn analyze(rgba: &[u8], w: u32, h: u32) -> Option<SampleStats> {
    let n = (w as usize).checked_mul(h as usize)?;
    if n == 0 || rgba.len() < n * 4 {
        return None;
    }
    let px = &rgba[..n * 4];
    if px.chunks_exact(4).all(|p| p[0] == 0 && p[1] == 0 && p[2] == 0) {
        return None;
    }
    let mut ys: Vec<f32> = Vec::with_capacity(n);
    let mut sum = [0f64; 3];
    for p in px.chunks_exact(4) {
        ys.push(luminance(p[0], p[1], p[2]));
        for (s, c) in sum.iter_mut().zip(p) {
            *s += *c as f64;
        }
    }
    ys.sort_by(|a, b| a.total_cmp(b));
    let at = |q: f32| ys[(((n - 1) as f32) * q).round() as usize];
    let (median, p10, p90) = (at(0.5), at(0.1), at(0.9));
    let mean = [
        (sum[0] / n as f64) as f32,
        (sum[1] / n as f64) as f32,
        (sum[2] / n as f64) as f32,
    ];
    let chroma =
        (mean.iter().cloned().fold(0.0, f32::max) - mean.iter().cloned().fold(255.0, f32::min)) / 255.0;
    Some(SampleStats {
        median,
        p10,
        p90,
        busy: (p90.sqrt() - p10.sqrt()).clamp(0.0, 1.0),
        mean_rgb: mean,
        chroma,
    })
}

/// WCAG contrast ratio of two relative luminances.
pub fn contrast(a: f32, b: f32) -> f32 {
    let (hi, lo) = if a > b { (a, b) } else { (b, a) };
    (hi + 0.05) / (lo + 0.05)
}

/// Smallest scrim opacity (0..1) such that text of luminance `fg` over a
/// background of luminance `bg_worst` seen through a scrim of luminance
/// `scrim` reaches `target` contrast. `bg_worst` is the brightest backdrop
/// pixel for light text (p90) and the darkest for dark text (p10). Result is
/// clamped to `[0.0, 0.95]` (a fully opaque scrim defeats the overlay).
pub fn required_scrim_alpha(scrim: f32, fg: f32, bg_worst: f32, target: f32) -> f32 {
    // Composite luminance (linear light blend) is monotonic in alpha, so bisect.
    let comp = |a: f32| a * scrim + (1.0 - a) * bg_worst;
    let ok = |a: f32| contrast(fg, comp(a)) >= target;
    if ok(0.0) {
        return 0.0;
    }
    if !ok(0.95) {
        return 0.95;
    }
    let (mut lo, mut hi) = (0.0f32, 0.95f32);
    for _ in 0..24 {
        let mid = (lo + hi) / 2.0;
        if ok(mid) {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    hi
}

/// User opacity slider (0..=90 percent) to the `--k` CSS variable (0..0.9).
/// `k` scales *every* adaptive token (background, border, chips, menu, shadow,
/// text alpha floors), i.e. the whole overlay; there is no window-level alpha.
pub fn opacity_k(percent: u8) -> f32 {
    f32::from(percent.min(90)) / 100.0
}

struct Palette {
    rgb: &'static str,
    fg: &'static str,
    inv: &'static str,
    int: &'static str,
    mv: &'static str,
    scrim_luma: f32,
    fg_luma: f32,
}

// Values from docs/design/lowk-mockups-v3.html.
const DARK: Palette = Palette {
    rgb: "12,12,15",
    fg: "247,247,249",
    inv: "0,0,0",
    int: "94,224,138",
    mv: "255,194,74",
    scrim_luma: 0.004,
    fg_luma: 0.93,
};
const LIGHT: Palette = Palette {
    rgb: "255,255,255",
    fg: "10,11,15",
    inv: "255,255,255",
    int: "14,130,62",
    mv: "166,96,0",
    scrim_luma: 1.0,
    fg_luma: 0.004,
};

fn tone_from(kind: ToneKind, p: &Palette, rgb: String, inv: String, stats: &SampleStats) -> Tone {
    Tone {
        tone: kind,
        rgb,
        fg: p.fg.into(),
        inv,
        int: p.int.into(),
        mv: p.mv.into(),
        luma: stats.median,
        busy: stats.busy,
        scrim_min: scrim_floor(kind, stats_worst(kind, stats)),
    }
}

fn stats_worst(kind: ToneKind, s: &SampleStats) -> f32 {
    if kind == ToneKind::Light {
        s.p10
    } else {
        s.p90
    }
}

/// Scrim opacity floor for the worst-case backdrop pixel behind the text.
fn scrim_floor(kind: ToneKind, worst: f32) -> f32 {
    let (scrim, fg) = match kind {
        ToneKind::Light => (LIGHT.scrim_luma, LIGHT.fg_luma),
        _ => (DARK.scrim_luma, DARK.fg_luma),
    };
    required_scrim_alpha(scrim, fg, worst, ToneParams::default().target_contrast)
}

/// Stateful tone selector with smoothing and hysteresis.
pub struct AdaptiveTone {
    params: ToneParams,
    mode: ToneMode,
    hist: VecDeque<f32>,
    light_bg: Option<bool>,
    color: bool,
    last: Option<Tone>,
    last_stats: Option<SampleStats>,
}

impl AdaptiveTone {
    pub fn new(mode: ToneMode) -> Self {
        Self::with_params(mode, ToneParams::default())
    }
    pub fn with_params(mode: ToneMode, params: ToneParams) -> Self {
        Self {
            params,
            mode,
            hist: VecDeque::with_capacity(3),
            light_bg: None,
            color: false,
            last: None,
            last_stats: None,
        }
    }

    /// Change the forced mode. The next [`update`](Self::update) /
    /// [`current`](Self::current) reflects it.
    pub fn set_mode(&mut self, mode: ToneMode) {
        if self.mode != mode {
            self.mode = mode;
            self.last = None;
        }
    }

    /// Last emitted tone, or the dark default.
    pub fn current(&self) -> Tone {
        match (&self.last, self.mode) {
            (_, ToneMode::Dark) => fixed(ToneKind::Dark, &DARK),
            (_, ToneMode::Light) => fixed(ToneKind::Light, &LIGHT),
            (Some(t), _) => t.clone(),
            (None, ToneMode::Auto) => fixed(ToneKind::Dark, &DARK),
        }
    }

    /// Scrim opacity floor (0..1) that keeps the contrast target for the last
    /// sample, for the UI to use as a lower bound of `--k * .85`.
    pub fn min_scrim_alpha(&self) -> f32 {
        let (Some(s), Some(t)) = (&self.last_stats, &self.last) else {
            return 0.0;
        };
        let (scrim, fg, worst) = match t.tone {
            ToneKind::Light => (LIGHT.scrim_luma, LIGHT.fg_luma, s.p10),
            _ => (DARK.scrim_luma, DARK.fg_luma, s.p90),
        };
        required_scrim_alpha(scrim, fg, worst, self.params.target_contrast)
    }

    /// Feed one sample. Returns the new tone to emit, or `None` when the
    /// sample is unusable or the result is within the deadband of the last one.
    pub fn update(&mut self, rgba: &[u8], w: u32, h: u32) -> Option<Tone> {
        if self.mode != ToneMode::Auto {
            // Forced palette: emit once after a mode change, never again.
            if self.last.is_none() {
                let t = self.current();
                self.last = Some(t.clone());
                return Some(t);
            }
            return None;
        }
        let stats = analyze(rgba, w, h)?;
        if self.hist.len() == 3 {
            self.hist.pop_front();
        }
        self.hist.push_back(stats.median);
        let smooth = {
            let mut v: Vec<f32> = self.hist.iter().copied().collect();
            v.sort_by(|a, b| a.total_cmp(b));
            v[v.len() / 2]
        };
        let light_bg = match self.light_bg {
            None => smooth > (self.params.up + self.params.down) / 2.0,
            Some(true) => smooth >= self.params.down,
            Some(false) => smooth > self.params.up,
        };
        self.light_bg = Some(light_bg);

        let chroma_gate = if self.color {
            self.params.color_off
        } else {
            self.params.color_on
        };
        let luma_gate = if self.color {
            self.params.color_max_luma + 0.05
        } else {
            self.params.color_max_luma
        };
        self.color = stats.chroma >= chroma_gate && smooth < luma_gate && smooth > self.params.down * 0.4;

        let mut stats = stats;
        stats.median = smooth;
        let tone = if self.color {
            let (rgb, inv) = tint(&stats.mean_rgb);
            tone_from(ToneKind::Color, &COLOR_PAL, rgb, inv, &stats)
        } else if light_bg {
            tone_from(
                ToneKind::Light,
                &LIGHT,
                LIGHT.rgb.into(),
                LIGHT.inv.into(),
                &stats,
            )
        } else {
            tone_from(ToneKind::Dark, &DARK, DARK.rgb.into(), DARK.inv.into(), &stats)
        };
        self.last_stats = Some(stats);

        let emit = match &self.last {
            None => true,
            Some(prev) => {
                prev.tone != tone.tone
                    || (prev.luma - tone.luma).abs() >= self.params.deadband
                    || (prev.scrim_min - tone.scrim_min).abs() >= 0.05
                    || (tone.tone == ToneKind::Color && rgb_dist(&prev.rgb, &tone.rgb) > 12.0)
            }
        };
        if emit {
            self.last = Some(tone.clone());
            Some(tone)
        } else {
            None
        }
    }
}

const COLOR_PAL: Palette = Palette {
    rgb: "",
    fg: "255,255,255",
    inv: "",
    int: "90,255,150",
    mv: "255,214,102",
    scrim_luma: 0.012,
    fg_luma: 1.0,
};

fn fixed(kind: ToneKind, p: &Palette) -> Tone {
    Tone {
        tone: kind,
        rgb: p.rgb.into(),
        fg: p.fg.into(),
        inv: p.inv.into(),
        int: p.int.into(),
        mv: p.mv.into(),
        luma: if kind == ToneKind::Light { 0.9 } else { 0.05 },
        busy: 0.0,
        scrim_min: 0.0,
    }
}

/// Dark scrim tinted with the background hue, and the matching `inv` colour.
fn tint(mean: &[f32; 3]) -> (String, String) {
    let max = mean.iter().cloned().fold(1.0, f32::max);
    let k = 80.0 / max;
    let c: Vec<u8> = mean
        .iter()
        .zip([12.0f32, 12.0, 15.0])
        .map(|(m, base)| (0.85 * m * k + 0.15 * base).clamp(0.0, 90.0) as u8)
        .collect();
    let inv: Vec<u8> = c.iter().map(|v| (*v as f32 * 0.7) as u8).collect();
    (
        format!("{},{},{}", c[0], c[1], c[2]),
        format!("{},{},{}", inv[0], inv[1], inv[2]),
    )
}

fn rgb_dist(a: &str, b: &str) -> f32 {
    let p = |s: &str| -> Vec<f32> { s.split(',').filter_map(|x| x.trim().parse().ok()).collect() };
    let (a, b) = (p(a), p(b));
    if a.len() != 3 || b.len() != 3 {
        return 255.0;
    }
    a.iter().zip(&b).map(|(x, y)| (x - y).abs()).fold(0.0, f32::max)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat(r: u8, g: u8, b: u8) -> Vec<u8> {
        (0..SAMPLE_W * SAMPLE_H).flat_map(|_| [r, g, b, 255]).collect()
    }
    fn feed(t: &mut AdaptiveTone, bmp: &[u8]) -> Option<Tone> {
        t.update(bmp, SAMPLE_W, SAMPLE_H)
    }

    #[test]
    fn luminance_extremes() {
        assert!(luminance(0, 0, 0) < 1e-6);
        assert!((luminance(255, 255, 255) - 1.0).abs() < 1e-3);
        assert!(luminance(0, 255, 0) > luminance(255, 0, 0));
    }

    #[test]
    fn black_is_unknown() {
        assert!(analyze(&flat(0, 0, 0), SAMPLE_W, SAMPLE_H).is_none());
        let mut t = AdaptiveTone::new(ToneMode::Auto);
        assert!(feed(&mut t, &flat(0, 0, 0)).is_none());
        assert!(analyze(&[1, 2, 3], 4, 4).is_none());
        assert!(analyze(&[], 0, 0).is_none());
    }

    #[test]
    fn dark_light_basic() {
        let mut t = AdaptiveTone::new(ToneMode::Auto);
        let d = feed(&mut t, &flat(24, 26, 32)).unwrap();
        assert_eq!(d.tone, ToneKind::Dark);
        assert_eq!(d.fg, "247,247,249");
        let mut t = AdaptiveTone::new(ToneMode::Auto);
        let l = feed(&mut t, &flat(245, 245, 240)).unwrap();
        assert_eq!(l.tone, ToneKind::Light);
        assert_eq!(l.rgb, "255,255,255");
    }

    #[test]
    fn color_background_tints() {
        let mut t = AdaptiveTone::new(ToneMode::Auto);
        let c = feed(&mut t, &flat(120, 30, 190)).unwrap();
        assert_eq!(c.tone, ToneKind::Color);
        assert_eq!(c.fg, "255,255,255");
        let rgb: Vec<u8> = c.rgb.split(',').map(|x| x.parse().unwrap()).collect();
        assert!(rgb[2] > rgb[1], "tint follows the hue: {rgb:?}");
        assert!(rgb.iter().all(|v| *v <= 90));
    }

    #[test]
    fn bright_saturated_is_light_not_color() {
        let mut t = AdaptiveTone::new(ToneMode::Auto);
        let y = feed(&mut t, &flat(255, 235, 40)).unwrap();
        assert_eq!(y.tone, ToneKind::Light);
    }

    #[test]
    fn schmitt_hysteresis_no_flapping() {
        // sRGB gray with linear luma ~0.18 sits inside the 0.14..0.22 band.
        let mid = |v: u8| flat(v, v, v);
        let mut t = AdaptiveTone::new(ToneMode::Auto);
        feed(&mut t, &mid(20)).unwrap(); // dark
                                         // Alternate between two in-band levels: the polarity must not flip.
        for i in 0..20 {
            let v = if i % 2 == 0 { 118 } else { 108 }; // luma ~0.18 / ~0.15
            if let Some(tn) = feed(&mut t, &mid(v)) {
                assert_eq!(tn.tone, ToneKind::Dark);
            }
        }
        // Clearly bright for 3 samples (median of 3 flips on the 2nd) -> light.
        let mut got = None;
        for _ in 0..3 {
            if let Some(tn) = feed(&mut t, &mid(200)) {
                got = Some(tn);
            }
        }
        assert_eq!(got.unwrap().tone, ToneKind::Light);
        // Back inside the band: stays light.
        for _ in 0..4 {
            if let Some(tn) = feed(&mut t, &mid(118)) {
                assert_eq!(tn.tone, ToneKind::Light);
            }
        }
        // Below `down`: flips back.
        let mut got = None;
        for _ in 0..3 {
            if let Some(tn) = feed(&mut t, &mid(40)) {
                got = Some(tn);
            }
        }
        assert_eq!(got.unwrap().tone, ToneKind::Dark);
    }

    #[test]
    fn median_of_three_rejects_flash() {
        let mut t = AdaptiveTone::new(ToneMode::Auto);
        feed(&mut t, &flat(20, 20, 20)).unwrap();
        feed(&mut t, &flat(20, 20, 20));
        // one white frame must not flip the tone
        let r = feed(&mut t, &flat(255, 255, 255));
        assert!(r.is_none() || r.unwrap().tone == ToneKind::Dark);
        assert_eq!(t.current().tone, ToneKind::Dark);
    }

    #[test]
    fn deadband_suppresses_noise() {
        let mut t = AdaptiveTone::new(ToneMode::Auto);
        assert!(feed(&mut t, &flat(30, 30, 30)).is_some());
        assert!(feed(&mut t, &flat(31, 31, 31)).is_none());
        assert!(feed(&mut t, &flat(30, 31, 30)).is_none());
    }

    #[test]
    fn forced_modes() {
        let mut t = AdaptiveTone::new(ToneMode::Light);
        let l = feed(&mut t, &flat(0, 0, 10)).unwrap();
        assert_eq!(l.tone, ToneKind::Light);
        assert!(feed(&mut t, &flat(0, 0, 10)).is_none());
        t.set_mode(ToneMode::Dark);
        assert_eq!(t.current().tone, ToneKind::Dark);
        assert_eq!(feed(&mut t, &flat(250, 250, 250)).unwrap().tone, ToneKind::Dark);
    }

    #[test]
    fn busy_is_high_for_text_like_content() {
        let mut bmp = flat(10, 10, 10);
        for (i, p) in bmp.chunks_exact_mut(4).enumerate() {
            if i % 3 == 0 {
                p[0] = 240;
                p[1] = 240;
                p[2] = 240;
            }
        }
        let s = analyze(&bmp, SAMPLE_W, SAMPLE_H).unwrap();
        assert!(s.busy > 0.5, "{s:?}");
        let flat_s = analyze(&flat(10, 10, 10), SAMPLE_W, SAMPLE_H).unwrap();
        assert!(flat_s.busy < 0.01);
    }

    #[test]
    fn scrim_alpha_reaches_target_contrast() {
        // Bright backdrop under light text needs a scrim; dark backdrop needs none.
        assert_eq!(required_scrim_alpha(0.004, 0.93, 0.01, 4.5), 0.0);
        let a = required_scrim_alpha(0.004, 0.93, 0.9, 4.5);
        assert!(a > 0.3 && a < 0.95, "{a}");
        let comp = a * 0.004 + (1.0 - a) * 0.9;
        assert!(contrast(0.93, comp) >= 4.49);
        // Dark text on a dark backdrop: needs a strong white scrim.
        let b = required_scrim_alpha(1.0, 0.004, 0.05, 4.5);
        assert!(b > 0.1 && b < 0.5, "{b}");
        // Impossible -> clamped.
        assert_eq!(required_scrim_alpha(0.5, 0.5, 0.5, 4.5), 0.95);
    }

    #[test]
    fn min_scrim_alpha_follows_busy_bright_background() {
        let mut t = AdaptiveTone::new(ToneMode::Auto);
        feed(&mut t, &flat(20, 20, 20));
        let calm = t.min_scrim_alpha();
        let mut bmp = flat(20, 20, 20);
        for (i, p) in bmp.chunks_exact_mut(4).enumerate() {
            if i % 4 == 0 {
                p[..3].copy_from_slice(&[255, 255, 255]);
            }
        }
        let mut t2 = AdaptiveTone::new(ToneMode::Auto);
        feed(&mut t2, &bmp);
        assert!(t2.min_scrim_alpha() >= calm);
    }

    #[test]
    fn opacity_k_clamps() {
        assert_eq!(opacity_k(0), 0.0);
        assert!((opacity_k(60) - 0.6).abs() < 1e-6);
        assert!((opacity_k(200) - 0.9).abs() < 1e-6);
    }
}
