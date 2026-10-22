//! Adaptive tone: colour of the pixels under the overlay -> palette tokens (O4: OKLab colour base, see
//! [`crate::palette`] and docs/design/overlay-color-base.md).
//!
//! Pure functions plus a small state machine ([`AdaptiveTone`]); no OS code, so
//! it is unit-tested with synthetic bitmaps. The sampler (a task in the app
//! layer driving a `RegionSampler`) feeds it a 48x32 RGBA bitmap every 100 ms (see POLL_MS).
//!
//! Algorithm (docs/ARCHITECTURE.md 7.4):
//! 1. linear-light luminance per pixel; median, p10, p90, "busyness"; the mean colour in OKLab;
//! 2. fast smoothing ([`Chan::smooth`]): small drifts are followed at once, a large change is adopted when a
//!    second consecutive sample agrees (one-frame flashes are rejected, a step costs two polls);
//! 3. polarity: Schmitt trigger on OKLab lightness, its upper gate rising with chroma
//!    ([`palette::polarity`]); light backdrop = dark text on a light scrim, otherwise white text on a dark one;
//! 4. the scrim keeps the backdrop's hue with lower chroma and a lightness set for contrast, the accents are
//!    re-derived for it ([`palette::scheme`]); the `Color` kind marks a clearly tinted dark scrim (with hysteresis);
//! 5. a new [`Tone`] is only emitted when it differs enough
//!    ([`ToneParams::deadband`], scrim colour distance) so the UI never animates on noise.
//! 6. (F5) the same pipeline runs per cell of a 12 x 8 grid; the result rides on
//!    the emitted tone as [`Tone::grid`], the global tone stays as the fallback. A cell that straddles a
//!    backdrop border (bimodal lightness) is judged by its MAJORITY side only ([`dominant_pixels`]) instead of
//!    the grey average of both, so a dark region never gets a light haze from the cells on its edge.

use crate::palette::{self, Oklab, Rgb};
use lowk_types::{Tone, ToneGrid, ToneKind, ToneMode};

/// Sample bitmap size requested from the sampler.
pub const SAMPLE_W: u32 = 48;
pub const SAMPLE_H: u32 = 32;
/// F5: the sample is split into `GRID_COLS` x `GRID_ROWS` regions (4x4 px each at the
/// default sample size), each with its own smoothed, hysteretic tone.
pub const GRID_COLS: usize = 12;
pub const GRID_ROWS: usize = 8;
/// A cell whose perceptual-lightness spread (max - min of sqrt(luminance)) exceeds this is a border cell.
const MIXED_SPREAD: f32 = 0.30;

#[derive(Clone, Copy, Debug)]
pub struct ToneParams {
    /// Minimum luma change that justifies emitting a new tone.
    pub deadband: f32,
    /// Minimum OKLab distance of the scrim colour that justifies emitting a new tone.
    pub scrim_delta: f32,
    /// Backdrop chroma (OKLCH C) to enter / leave the `Color` kind.
    pub color_on: f32,
    pub color_off: f32,
    /// Target WCAG contrast for the scrim computation.
    pub target_contrast: f32,
    /// A pure-black FRAME is a real black backdrop (macOS: a denied grab is an error, never a black frame).
    /// Windows returns black for protected surfaces, so there it means "unknown, keep the previous tone".
    /// A pure-black CELL inside a frame that is not black is always real black: never "unknown".
    pub black_known: bool,
}

impl Default for ToneParams {
    fn default() -> Self {
        Self {
            deadband: 0.03,
            scrim_delta: 0.02,
            color_on: 0.05,
            color_off: 0.035,
            target_contrast: 4.5,
            black_known: !cfg!(windows),
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
    /// Mean colour (averaged in linear light) in OKLab.
    pub lab: Oklab,
}

pub fn luminance(r: u8, g: u8, b: u8) -> f32 {
    palette::rel_luminance([r, g, b])
}

/// Analyse an RGBA8 bitmap. Returns `None` for an unusable sample: wrong
/// length, empty, or pure black (the sampler's "unknown, keep previous" value,
/// e.g. a GDI grab of a protected surface or no Screen Recording permission).
pub fn analyze(rgba: &[u8], w: u32, h: u32) -> Option<SampleStats> {
    analyze_opts(rgba, w, h, false)
}

/// [`analyze`], with `black_known` accepting an all-black bitmap as real black.
pub fn analyze_opts(rgba: &[u8], w: u32, h: u32, black_known: bool) -> Option<SampleStats> {
    let n = (w as usize).checked_mul(h as usize)?;
    if n == 0 || rgba.len() < n * 4 {
        return None;
    }
    let px = &rgba[..n * 4];
    if !black_known && px.chunks_exact(4).all(|p| p[0] == 0 && p[1] == 0 && p[2] == 0) {
        return None;
    }
    let mut ys: Vec<f32> = Vec::with_capacity(n);
    let mut sum = [0f64; 3];
    for p in px.chunks_exact(4) {
        ys.push(luminance(p[0], p[1], p[2]));
        for (s, c) in sum.iter_mut().zip(p) {
            *s += f64::from(palette::srgb_to_linear(*c));
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
    Some(SampleStats {
        median,
        p10,
        p90,
        busy: (p90.sqrt() - p10.sqrt()).clamp(0.0, 1.0),
        lab: palette::linear_to_oklab(mean[0], mean[1], mean[2]),
    })
}

/// WCAG contrast ratio of two relative luminances.
pub fn contrast(a: f32, b: f32) -> f32 {
    palette::wcag(a, b)
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

/// Original neutral look (docs/design/lowk-mockups-v3.html); the accents of the neutral presets.
struct Preset {
    rgb: &'static str,
    fg: &'static str,
    inv: &'static str,
    int: &'static str,
    mv: &'static str,
}
const DARK: Preset = Preset {
    rgb: "12,12,15",
    fg: "247,247,249",
    inv: "0,0,0",
    int: "94,224,138",
    mv: "255,194,74",
};
const LIGHT: Preset = Preset {
    rgb: "255,255,255",
    fg: "10,11,15",
    inv: "255,255,255",
    int: "0,113,49",
    mv: "130,91,0",
};

fn tone_from(kind: ToneKind, sch: &palette::Scheme, stats: &SampleStats, target: f32) -> Tone {
    // Neutral dark backdrops keep the exact original look; everything else uses the derived colours
    // (the neutral light accents are re-derived too: the old ones fell below 3:1 over a dark pixel under the scrim).
    let (rgb, fg, inv, int, mv) = if sch.tinted || sch.dark_text {
        (
            palette::fmt(sch.scrim),
            palette::fmt(sch.fg),
            palette::fmt(sch.inv),
            palette::fmt(sch.int),
            palette::fmt(sch.mv),
        )
    } else {
        (
            DARK.rgb.into(),
            DARK.fg.into(),
            DARK.inv.into(),
            DARK.int.into(),
            DARK.mv.into(),
        )
    };
    let worst = if sch.dark_text { stats.p10 } else { stats.p90 };
    Tone {
        tone: kind,
        rgb,
        fg,
        inv,
        int,
        mv,
        luma: stats.median,
        busy: stats.busy,
        scrim_min: required_scrim_alpha(
            palette::rel_luminance(sch.scrim),
            palette::rel_luminance(sch.fg),
            worst,
            target,
        ),
        grid: None,
    }
}

/// Smoothing + polarity Schmitt trigger + colour gate for one region.
#[derive(Default)]
struct Chan {
    /// Accepted [median luma, L, a, b].
    cur: Option<[f32; 4]>,
    /// A large change seen once, waiting for a second agreeing sample.
    pend: Option<[f32; 4]>,
    dark_text: Option<bool>,
    color: bool,
}

/// OKLab distance of two smoothing vectors (components 1..4).
fn vdist(a: &[f32; 4], b: &[f32; 4]) -> f32 {
    ((a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2) + (a[3] - b[3]).powi(2)).sqrt()
}
/// Below this a sample is a drift/noise and is followed directly (the deadband and the Schmitt trigger damp it).
const FOLLOW_DIST: f32 = 0.04;
/// Two samples this close count as "the same new backdrop".
const CONFIRM_DIST: f32 = 0.06;

impl Chan {
    /// Latency-first smoothing: first sample and small changes pass at once; a large change is adopted on the
    /// second consecutive sample that agrees with it (a one-frame flash never gets that second sample).
    fn smooth(&mut self, v: [f32; 4]) -> [f32; 4] {
        let Some(cur) = self.cur else {
            self.cur = Some(v);
            return v;
        };
        let confirmed = matches!(self.pend, Some(p) if vdist(&p, &v) < CONFIRM_DIST);
        if vdist(&v, &cur) < FOLLOW_DIST || confirmed {
            self.pend = None;
            self.cur = Some(v);
        } else {
            self.pend = Some(v);
        }
        self.cur.unwrap_or(v)
    }

    /// Feed one analysed sample; returns the tone (grid unset) built from the smoothed state.
    fn select(&mut self, params: &ToneParams, stats: SampleStats) -> Tone {
        let sm = self.smooth([stats.median, stats.lab.l, stats.lab.a, stats.lab.b]);
        let lab = Oklab {
            l: sm[1],
            a: sm[2],
            b: sm[3],
        };
        let mut stats = stats;
        stats.median = sm[0];
        stats.lab = lab;

        let dark_text = palette::polarity(&lab, self.dark_text);
        self.dark_text = Some(dark_text);
        let scheme = palette::scheme(&lab, dark_text);
        let gate = if self.color {
            params.color_off
        } else {
            params.color_on
        };
        self.color = !dark_text && scheme.tinted && lab.chroma() >= gate;
        let kind = if dark_text {
            ToneKind::Light
        } else if self.color {
            ToneKind::Color
        } else {
            ToneKind::Dark
        };
        tone_from(kind, &scheme, &stats, params.target_contrast)
    }
}

/// Does `new` differ enough from `prev` to be worth emitting (ignores `grid`)?
fn differs(prev: &Tone, new: &Tone, params: &ToneParams) -> bool {
    prev.tone != new.tone
        || (prev.luma - new.luma).abs() >= params.deadband
        || (prev.scrim_min - new.scrim_min).abs() >= 0.05
        || scrim_dist(&prev.rgb, &new.rgb) >= params.scrim_delta
}

/// RGBA8 sub-bitmap of grid cell (`col`, `row`).
fn cell_bitmap(rgba: &[u8], w: usize, h: usize, col: usize, row: usize) -> (Vec<u8>, u32, u32) {
    let (x0, x1) = (col * w / GRID_COLS, (col + 1) * w / GRID_COLS);
    let (y0, y1) = (row * h / GRID_ROWS, (row + 1) * h / GRID_ROWS);
    let mut out = Vec::with_capacity((x1 - x0) * (y1 - y0) * 4);
    for y in y0..y1 {
        out.extend_from_slice(&rgba[(y * w + x0) * 4..(y * w + x1) * 4]);
    }
    (out, (x1 - x0) as u32, (y1 - y0) as u32)
}

/// Border cells: when a cell's lightness is bimodal (spread of sqrt-luminance above [`MIXED_SPREAD`]) only the
/// pixels on its MAJORITY side of the midpoint are kept (a tie goes to the darker side), so a cell on a
/// dark/light border is judged by what mostly lies behind it, never by the grey mean of both. Returns the
/// cell unchanged when it is not bimodal.
fn dominant_pixels(cell: Vec<u8>) -> Vec<u8> {
    let v: Vec<f32> = cell
        .chunks_exact(4)
        .map(|p| luminance(p[0], p[1], p[2]).sqrt())
        .collect();
    let (mn, mx) = v
        .iter()
        .fold((f32::MAX, f32::MIN), |(a, b), x| (a.min(*x), b.max(*x)));
    if v.is_empty() || mx - mn <= MIXED_SPREAD {
        return cell;
    }
    let mid = (mn + mx) / 2.0;
    let dark = v.iter().filter(|x| **x < mid).count();
    let keep_dark = dark * 2 >= v.len();
    let mut out = Vec::with_capacity(cell.len());
    for (p, x) in cell.chunks_exact(4).zip(&v) {
        if (*x < mid) == keep_dark {
            out.extend_from_slice(p);
        }
    }
    out
}

/// Raw mean sRGB colour of each grid cell of a sample bitmap (row-major, GRID_COLS x GRID_ROWS); diagnostics only.
pub fn cell_means(rgba: &[u8], w: u32, h: u32) -> Vec<[u8; 3]> {
    let (wu, hu) = (w as usize, h as usize);
    if wu < GRID_COLS || hu < GRID_ROWS || rgba.len() < wu * hu * 4 {
        return vec![];
    }
    (0..GRID_COLS * GRID_ROWS)
        .map(|i| {
            let (b, ..) = cell_bitmap(rgba, wu, hu, i % GRID_COLS, i / GRID_COLS);
            let n = (b.len() / 4).max(1) as u64;
            let mut s = [0u64; 3];
            for p in b.chunks_exact(4) {
                for (a, c) in s.iter_mut().zip(p) {
                    *a += u64::from(*c);
                }
            }
            [(s[0] / n) as u8, (s[1] / n) as u8, (s[2] / n) as u8]
        })
        .collect()
}

fn uniform_grid(t: &Tone) -> ToneGrid {
    let mut cell = t.clone();
    cell.grid = None;
    ToneGrid {
        cols: GRID_COLS as u8,
        rows: GRID_ROWS as u8,
        cells: vec![cell; GRID_COLS * GRID_ROWS],
    }
}

fn fixed_with_grid(kind: ToneKind, p: &Preset) -> Tone {
    let mut t = fixed(kind, p);
    t.grid = Some(uniform_grid(&t));
    t
}

/// How often the sampler looks at the screen under the overlay. Latency budget of a backdrop step:
/// wait for the next poll (0..100 ms, avg 50) + capture/analysis (~30-40 ms) + one more agreeing sample
/// (100 ms, see [`Chan::smooth`]) + IPC + the 120 ms CSS fade = ~0.3 s typical, <= ~0.4 s worst case.
pub const POLL_MS: u64 = 100;
/// A bitmap is fed to [`AdaptiveTone`] when it changed, and this many more times after a change even
/// though it is identical, so the confirming sample of a large change arrives without waiting for
/// the next change (the old 2 s sampler + skip-unchanged made a step take 4-6 s).
pub const SETTLE_POLLS: u8 = 1;

/// Cheap change detector in front of the analysis: skips identical bitmaps once the smoothing settled.
#[derive(Default)]
pub struct ChangeGate {
    last: Option<u64>,
    settle: u8,
}

impl ChangeGate {
    /// `true` when `bmp` should be analysed now.
    pub fn admit(&mut self, bmp: &[u8]) -> bool {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        bmp.hash(&mut h);
        let id = h.finish();
        if self.last != Some(id) {
            self.last = Some(id);
            self.settle = SETTLE_POLLS;
            return true;
        }
        if self.settle > 0 {
            self.settle -= 1;
            return true;
        }
        false
    }
}

/// Stateful tone selector with smoothing and hysteresis.
pub struct AdaptiveTone {
    params: ToneParams,
    mode: ToneMode,
    global: Chan,
    cells: Vec<Chan>,
    cell_last: Vec<Option<Tone>>,
    last: Option<Tone>,
    /// Scrim and text luminance of the last global decision + its worst backdrop luma (for [`Self::min_scrim_alpha`]).
    last_floor: Option<f32>,
}

impl AdaptiveTone {
    pub fn new(mode: ToneMode) -> Self {
        Self::with_params(mode, ToneParams::default())
    }
    pub fn with_params(mode: ToneMode, params: ToneParams) -> Self {
        Self {
            params,
            mode,
            global: Chan::default(),
            cells: (0..GRID_COLS * GRID_ROWS).map(|_| Chan::default()).collect(),
            cell_last: vec![None; GRID_COLS * GRID_ROWS],
            last: None,
            last_floor: None,
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
            (_, ToneMode::Dark) => fixed_with_grid(ToneKind::Dark, &DARK),
            (_, ToneMode::Light) => fixed_with_grid(ToneKind::Light, &LIGHT),
            (Some(t), _) => t.clone(),
            (None, ToneMode::Auto) => fixed_with_grid(ToneKind::Dark, &DARK),
        }
    }

    /// Scrim opacity floor (0..1) that keeps the contrast target for the last
    /// sample, for the UI to use as a lower bound of `--k * .85`.
    pub fn min_scrim_alpha(&self) -> f32 {
        self.last_floor.unwrap_or(0.0)
    }

    /// Feed one sample. Returns the new tone to emit, or `None` when the
    /// sample is unusable or the result is within the deadband of the last one
    /// (global and every grid cell).
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
        let stats = analyze_opts(rgba, w, h, self.params.black_known)?;
        let mut tone = self.global.select(&self.params, stats);
        self.last_floor = Some(tone.scrim_min);

        // Per-region tones (F5). Skipped when the bitmap does not tile evenly.
        let mut grid_changed = false;
        let (wu, hu) = (w as usize, h as usize);
        if wu % GRID_COLS == 0 && hu % GRID_ROWS == 0 {
            let mut cells = Vec::with_capacity(GRID_COLS * GRID_ROWS);
            for i in 0..GRID_COLS * GRID_ROWS {
                let (col, row) = (i % GRID_COLS, i / GRID_COLS);
                let (bmp, ..) = cell_bitmap(rgba, wu, hu, col, row);
                let bmp = dominant_pixels(bmp);
                let fresh = match analyze_opts(&bmp, (bmp.len() / 4) as u32, 1, true) {
                    Some(cs) => self.cells[i].select(&self.params, cs),
                    // Unknown cell (pure black): keep what we had, else the global tone.
                    None => self.cell_last[i].clone().unwrap_or_else(|| tone.clone()),
                };
                let kept = match &self.cell_last[i] {
                    Some(prev) if !differs(prev, &fresh, &self.params) => prev.clone(),
                    _ => {
                        grid_changed = true;
                        fresh
                    }
                };
                self.cell_last[i] = Some(kept.clone());
                cells.push(kept);
            }
            tone.grid = Some(ToneGrid {
                cols: GRID_COLS as u8,
                rows: GRID_ROWS as u8,
                cells,
            });
        }

        let global_changed = match &self.last {
            None => true,
            Some(prev) => differs(prev, &tone, &self.params),
        };
        if global_changed {
            self.last = Some(tone.clone());
            Some(tone)
        } else if grid_changed {
            // Global tone unchanged (stay inside its deadband): only the grid moved.
            let mut out = self.last.clone().expect("last set when global unchanged");
            out.grid = tone.grid;
            self.last = Some(out.clone());
            Some(out)
        } else {
            None
        }
    }
}

fn fixed(kind: ToneKind, p: &Preset) -> Tone {
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
        grid: None,
    }
}

fn parse_rgb(s: &str) -> Option<Rgb> {
    let v: Vec<u8> = s.split(',').filter_map(|x| x.trim().parse().ok()).collect();
    <[u8; 3]>::try_from(v).ok()
}

/// OKLab distance of two `r,g,b` strings (large when either does not parse).
fn scrim_dist(a: &str, b: &str) -> f32 {
    match (parse_rgb(a), parse_rgb(b)) {
        (Some(a), Some(b)) => palette::rgb_to_oklab(a).dist(&palette::rgb_to_oklab(b)),
        _ => 1.0,
    }
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

    /// Latency check: a background that jumps from dark to light at poll 0 must reach the UI within
    /// `LATENCY_POLLS` polls (x `POLL_MS`), and the same for light to dark and for a single cell of the grid.
    #[test]
    fn step_reaches_the_ui_within_two_polls() {
        const LATENCY_POLLS: usize = 2;
        let step = |from: [u8; 3], to: [u8; 3]| {
            let mut t = AdaptiveTone::new(ToneMode::Auto);
            let mut gate = ChangeGate::default();
            let a = flat(from[0], from[1], from[2]);
            let b = flat(to[0], to[1], to[2]);
            for _ in 0..8 {
                if gate.admit(&a) {
                    feed(&mut t, &a);
                }
            }
            let want = if to[0] > 128 {
                ToneKind::Light
            } else {
                ToneKind::Dark
            };
            for poll in 0..LATENCY_POLLS {
                if gate.admit(&b) {
                    if let Some(x) = feed(&mut t, &b) {
                        if x.tone == want {
                            return Some(poll + 1);
                        }
                    }
                }
            }
            None
        };
        let up = step([20, 20, 24], [240, 240, 240]).expect("dark -> light within the budget");
        assert!(up <= 2);
        let down = step([240, 240, 240], [20, 20, 24]).expect("light -> dark within the budget");
        assert!(
            up * POLL_MS as usize <= 200 && down * POLL_MS as usize <= 200,
            "{up} {down}"
        );
    }

    #[test]
    fn gate_skips_identical_bitmaps_only_after_settling() {
        let mut g = ChangeGate::default();
        let a = flat(1, 2, 3);
        assert!(g.admit(&a));
        assert!(g.admit(&a));
        assert!(!g.admit(&a), "settled: identical frames are free");
        assert!(g.admit(&flat(9, 9, 9)), "a change is always analysed");
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
        let mut t = AdaptiveTone::with_params(
            ToneMode::Auto,
            ToneParams {
                black_known: false,
                ..ToneParams::default()
            },
        );
        assert!(feed(&mut t, &flat(0, 0, 0)).is_none());
        assert!(analyze(&[1, 2, 3], 4, 4).is_none());
        assert!(analyze(&[], 0, 0).is_none());
    }

    /// Regression (owner: only the LEFT of the backdrop changed): a pure-black cell used to be "unknown" and kept the
    /// previous (light) scrim forever. Black next to white must be a dark cell next to a light one, and a black frame is dark.
    #[test]
    fn black_cells_and_black_frames_are_real_black() {
        let mut t = AdaptiveTone::new(ToneMode::Auto);
        for _ in 0..4 {
            feed(&mut t, &flat(255, 255, 255));
        }
        let bmp = split_lr(0, 255);
        for _ in 0..4 {
            feed(&mut t, &bmp);
        }
        let g = t.current().grid.unwrap();
        for (i, c) in g.cells.iter().enumerate() {
            let left = i % GRID_COLS < GRID_COLS / 2;
            assert_eq!(c.tone == ToneKind::Light, !left, "cell {i}: {:?}", c.rgb);
        }
        let mut t = AdaptiveTone::with_params(
            ToneMode::Auto,
            ToneParams {
                black_known: true,
                ..ToneParams::default()
            },
        );
        feed(&mut t, &flat(255, 255, 255));
        for _ in 0..4 {
            feed(&mut t, &flat(0, 0, 0));
        }
        assert_eq!(t.current().tone, ToneKind::Dark);
        assert!(t
            .current()
            .grid
            .unwrap()
            .cells
            .iter()
            .all(|c| c.tone == ToneKind::Dark));
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
        assert_eq!(c.fg, "247,247,249");
        let rgb: Vec<u8> = c.rgb.split(',').map(|x| x.parse().unwrap()).collect();
        assert!(rgb[2] > rgb[1], "tint follows the hue: {rgb:?}");
        assert!(rgb.iter().all(|v| *v <= 90));
    }

    fn rgbs(t: &Tone) -> Vec<u8> {
        t.rgb.split(',').map(|x| x.parse().unwrap()).collect()
    }

    /// O4: dark / saturated blue used to fall through to the black preset; light blue was fine.
    #[test]
    fn navy_and_dark_blue_keep_their_hue() {
        for navy in [
            [0u8, 0, 128],
            [8, 16, 70],
            [20, 40, 160],
            [0, 0, 255],
            [30, 60, 130],
        ] {
            let mut t = AdaptiveTone::new(ToneMode::Auto);
            let c = feed(&mut t, &flat(navy[0], navy[1], navy[2])).unwrap();
            assert_eq!(c.tone, ToneKind::Color, "{navy:?}");
            let rgb = rgbs(&c);
            assert!(
                rgb[2] > rgb[0] + 20 && rgb[2] > rgb[1] + 8,
                "{navy:?} -> {rgb:?} is not blue"
            );
            assert_ne!(c.rgb, "12,12,15", "{navy:?} fell back to the black preset");
            // the accents are re-derived for the tinted scrim and stay readable on it
            assert_ne!(c.int, "94,224,138");
        }
        // light blue: dark text on a pale blue-ish scrim
        let mut t = AdaptiveTone::new(ToneMode::Auto);
        let c = feed(&mut t, &flat(170, 205, 255)).unwrap();
        assert_eq!(c.tone, ToneKind::Light);
        assert_eq!(c.fg, "10,11,15");
        let rgb = rgbs(&c);
        assert!(rgb[2] >= rgb[0] && rgb.iter().all(|v| *v >= 235), "{rgb:?}");
    }

    #[test]
    fn red_purple_green_teal_get_dark_tinted_scrims() {
        let want = |bg: [u8; 3], dom: usize| {
            let mut t = AdaptiveTone::new(ToneMode::Auto);
            let c = feed(&mut t, &flat(bg[0], bg[1], bg[2])).unwrap();
            assert_eq!(c.tone, ToneKind::Color, "{bg:?}");
            let rgb = rgbs(&c);
            let max = *rgb.iter().max().unwrap();
            assert_eq!(rgb[dom], max, "{bg:?} -> {rgb:?}");
            assert!(max <= 110, "dark scrim: {rgb:?}");
        };
        want([200, 30, 30], 0);
        want([160, 20, 40], 0);
        want([0, 150, 60], 1);
        want([0, 128, 128], 2); // teal: blue and green channels lead
    }

    #[test]
    fn colour_transition_has_hysteresis_and_no_flicker() {
        let mut t = AdaptiveTone::new(ToneMode::Auto);
        for _ in 0..4 {
            feed(&mut t, &flat(20, 40, 150));
        }
        // a slow drift of the blue backdrop must not flip polarity back and forth around the gate
        let mut flips = 0;
        let mut last = ToneKind::Color;
        for i in 0..40 {
            let v = 40 + (i % 5) as u8 * 3; // 40..52 jitter around the gate
            if let Some(x) = feed(&mut t, &flat(v, v + 30, 175)) {
                if x.tone != last {
                    flips += 1;
                    last = x.tone;
                }
            }
        }
        assert!(flips <= 1, "{flips} flips");
        // a one-frame flash of white does not move the scrim
        let before = t.current();
        let flash = feed(&mut t, &flat(255, 255, 255));
        assert!(flash.is_none() || flash.unwrap().tone != ToneKind::Light);
        assert_eq!(before.tone, t.current().tone);
    }

    #[test]
    fn photo_like_noise_gets_a_stable_legible_tone() {
        // deterministic pseudo-random "photo": mid brightness, strong local contrast, warm cast
        let mut x = 0x2545_f491_u32;
        let mut r = || {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            (x >> 8) as u8
        };
        let bmp: Vec<u8> = (0..SAMPLE_W * SAMPLE_H)
            .flat_map(|_| [r() / 2 + 90, r() / 2 + 50, r() / 3 + 30, 255])
            .collect();
        let mut t = AdaptiveTone::new(ToneMode::Auto);
        let tone = feed(&mut t, &bmp).unwrap();
        assert!(tone.scrim_min <= 0.95);
        assert!(feed(&mut t, &bmp).is_none(), "the same frame never re-emits");
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
        // Clearly bright for 3 samples (a large change is adopted on the 2nd) -> light.
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
    fn a_one_frame_flash_is_rejected() {
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

    /// Left half bright, right half dark.
    fn split_lr(l: u8, r: u8) -> Vec<u8> {
        (0..SAMPLE_W * SAMPLE_H)
            .flat_map(|i| {
                let v = if (i % SAMPLE_W) < SAMPLE_W / 2 { l } else { r };
                [v, v, v, 255]
            })
            .collect()
    }

    #[test]
    fn grid_adapts_each_region_independently() {
        let mut t = AdaptiveTone::new(ToneMode::Auto);
        let tone = feed(&mut t, &split_lr(245, 20)).unwrap();
        let g = tone.grid.expect("grid rides on the tone");
        assert_eq!((g.cols as usize, g.rows as usize), (GRID_COLS, GRID_ROWS));
        assert_eq!(g.cells.len(), GRID_COLS * GRID_ROWS);
        for (i, c) in g.cells.iter().enumerate() {
            let left = i % GRID_COLS < GRID_COLS / 2;
            assert_eq!(
                c.tone,
                if left { ToneKind::Light } else { ToneKind::Dark },
                "cell {i}"
            );
            assert!(c.grid.is_none());
        }
    }

    #[test]
    fn grid_change_alone_emits_and_noise_does_not() {
        let mut t = AdaptiveTone::new(ToneMode::Auto);
        feed(&mut t, &split_lr(30, 30)).unwrap();
        assert!(
            feed(&mut t, &split_lr(31, 30)).is_none(),
            "noise stays inside every deadband"
        );
        // Only the left half turns bright for 3 samples; the global median stays dark.
        let mut got = None;
        for _ in 0..3 {
            if let Some(x) = feed(&mut t, &split_lr(250, 30)) {
                got = Some(x);
            }
        }
        let g = got.expect("grid change is emitted").grid.unwrap();
        assert_eq!(g.cells[0].tone, ToneKind::Light);
        assert_eq!(g.cells[GRID_COLS - 1].tone, ToneKind::Dark);
    }

    #[test]
    fn forced_mode_grid_is_uniform() {
        let mut t = AdaptiveTone::new(ToneMode::Light);
        let g = feed(&mut t, &flat(0, 0, 10)).unwrap().grid.unwrap();
        assert!(g.cells.iter().all(|c| c.tone == ToneKind::Light));
    }

    #[test]
    fn opacity_k_clamps() {
        assert_eq!(opacity_k(0), 0.0);
        assert!((opacity_k(60) - 0.6).abs() < 1e-6);
        assert!((opacity_k(200) - 0.9).abs() < 1e-6);
    }

    // ---- regression: "every cell dark whatever the backdrop" (live check, qa-tone) ----
    fn quad(tl: [u8; 3], tr: [u8; 3], bl: [u8; 3], br: [u8; 3]) -> Vec<u8> {
        let mut v = Vec::new();
        for y in 0..SAMPLE_H {
            for x in 0..SAMPLE_W {
                let c = match (x < SAMPLE_W / 2, y < SAMPLE_H / 2) {
                    (true, true) => tl,
                    (false, true) => tr,
                    (true, false) => bl,
                    (false, false) => br,
                };
                v.extend_from_slice(&[c[0], c[1], c[2], 255]);
            }
        }
        v
    }
    fn settle(bmp: &[u8]) -> Tone {
        let mut t = AdaptiveTone::new(ToneMode::Auto);
        let mut last = None;
        for _ in 0..4 {
            if let Some(x) = t.update(bmp, SAMPLE_W, SAMPLE_H) {
                last = Some(x);
            }
        }
        last.unwrap_or_else(|| t.current())
    }
    fn scrim_l(c: &Tone) -> f32 {
        let v: Vec<u8> = c.rgb.split(',').map(|x| x.trim().parse().unwrap()).collect();
        luminance(v[0], v[1], v[2])
    }

    #[test]
    fn light_backdrops_never_leave_the_grid_dark() {
        let t = settle(&quad([255; 3], [240, 220, 40], [225; 3], [255; 3]));
        let g = t.grid.expect("grid");
        assert_eq!(g.cells.len(), GRID_COLS * GRID_ROWS);
        for (i, c) in g.cells.iter().enumerate() {
            assert!(scrim_l(c) > 0.6, "cell {i} stayed dark: {} ({:?})", c.rgb, c.tone);
        }
    }

    #[test]
    fn every_quadrant_of_a_mixed_backdrop_gets_its_own_polarity() {
        let t = settle(&quad([0; 3], [255; 3], [10, 20, 90], [240, 220, 40]));
        let g = t.grid.expect("grid");
        let l = |col: usize, row: usize| scrim_l(&g.cells[row * GRID_COLS + col]);
        let (hc, hr) = (GRID_COLS / 2, GRID_ROWS / 2);
        for c in 0..GRID_COLS {
            for r in 0..GRID_ROWS {
                // cells on the quadrant borders may go either way (<= 1 cell of slack); the rest must be exact
                if c + 1 == hc || c == hc || r + 1 == hr || r == hr {
                    continue;
                }
                let light = c >= hc;
                assert_eq!(l(c, r) > 0.6, light, "cell ({c},{r})");
                if !light {
                    assert!(l(c, r) < 0.12, "cell ({c},{r})");
                }
            }
        }
        assert_eq!(
            cell_means(
                &quad([0; 3], [255; 3], [10, 20, 90], [240, 220, 40]),
                SAMPLE_W,
                SAMPLE_H
            )[GRID_COLS - 1],
            [255, 255, 255]
        );
    }

    #[test]
    fn an_all_black_frame_is_dark_but_a_black_cell_next_to_white_does_not_darken_the_rest() {
        let t = settle(&quad([0; 3], [255; 3], [255; 3], [255; 3]));
        let g = t.grid.expect("grid");
        assert!(scrim_l(&g.cells[0]) < 0.12);
        assert!(scrim_l(&g.cells[GRID_COLS * GRID_ROWS - 1]) > 0.6);
    }

    #[test]
    fn cell_means_reports_raw_colours() {
        let m = cell_means(
            &quad([1, 2, 3], [4, 5, 6], [7, 8, 9], [10, 11, 12]),
            SAMPLE_W,
            SAMPLE_H,
        );
        let at = |c: usize, r: usize| m[r * GRID_COLS + c];
        assert_eq!(
            (
                m.len(),
                at(0, 0),
                at(GRID_COLS - 1, 0),
                at(0, GRID_ROWS - 1),
                at(GRID_COLS - 1, GRID_ROWS - 1)
            ),
            (
                GRID_COLS * GRID_ROWS,
                [1, 2, 3],
                [4, 5, 6],
                [7, 8, 9],
                [10, 11, 12]
            )
        );
        assert!(cell_means(&[0; 8], 2, 1).is_empty());
    }

    // ---- 12x8 grid: sharp borders, latency, cost ----
    const NAVY: [u8; 3] = [10, 20, 90];
    const RED: [u8; 3] = [200, 30, 30];
    const YELLOW: [u8; 3] = [240, 220, 40];
    const TEAL: [u8; 3] = [0, 128, 128];
    const WHITE: [u8; 3] = [250, 250, 250];

    fn paint(mut f: impl FnMut(usize, usize) -> [u8; 3]) -> Vec<u8> {
        let mut v = Vec::new();
        for y in 0..SAMPLE_H as usize {
            for x in 0..SAMPLE_W as usize {
                let c = f(x, y);
                v.extend_from_slice(&[c[0], c[1], c[2], 255]);
            }
        }
        v
    }
    fn is_light(c: [u8; 3]) -> bool {
        luminance(c[0], c[1], c[2]) > 0.3
    }
    /// Polarity (light?) the pixels of a cell vote for; a tie goes to dark (same rule as `dominant_pixels`).
    fn cell_vote(bmp: &[u8], col: usize, row: usize) -> bool {
        let (cw, ch) = (SAMPLE_W as usize / GRID_COLS, SAMPLE_H as usize / GRID_ROWS);
        let mut light = 0;
        for y in row * ch..(row + 1) * ch {
            for x in col * cw..(col + 1) * cw {
                let p = &bmp[(y * SAMPLE_W as usize + x) * 4..];
                light += usize::from(is_light([p[0], p[1], p[2]]));
            }
        }
        light * 2 > cw * ch
    }
    fn light_cells(g: &ToneGrid) -> Vec<bool> {
        g.cells.iter().map(|c| c.tone == ToneKind::Light).collect()
    }
    /// Every cell follows the majority of its own pixels; returns the settled grid.
    fn assert_follows_pixels(bmp: &[u8], what: &str) -> ToneGrid {
        let g = settle(bmp).grid.expect("grid");
        for (i, l) in light_cells(&g).into_iter().enumerate() {
            let (c, r) = (i % GRID_COLS, i / GRID_COLS);
            assert_eq!(l, cell_vote(bmp, c, r), "{what}: cell ({c},{r})");
        }
        g
    }

    /// The owner's live case: navy | red over yellow | teal with borders that are NOT on cell edges.
    #[test]
    fn quadrants_with_off_grid_borders_follow_the_pixels() {
        for (bx, by) in [(22usize, 13usize), (21, 14), (27, 9), (19, 21)] {
            let bmp = paint(|x, y| match (x < bx, y < by) {
                (true, true) => NAVY,
                (false, true) => RED,
                (true, false) => YELLOW,
                (false, false) => TEAL,
            });
            let g = assert_follows_pixels(&bmp, &format!("border ({bx},{by})"));
            // no light cell on the dark (navy/red) top side beyond the one cell row that straddles the border
            let (cw, ch) = (SAMPLE_W as usize / GRID_COLS, SAMPLE_H as usize / GRID_ROWS);
            for (i, l) in light_cells(&g).into_iter().enumerate() {
                let (c, r) = (i % GRID_COLS, i / GRID_COLS);
                let (x0, x1, y0, y1) = (c * cw, (c + 1) * cw, r * ch, (r + 1) * ch);
                let fully_top = y1 <= by;
                let fully_left = x1 <= bx;
                if fully_top {
                    assert!(!l, "({bx},{by}): light haze at ({c},{r}) over the dark top");
                }
                if y0 >= by && x1 <= bx {
                    assert!(l, "({bx},{by}): dark cell ({c},{r}) inside the yellow area");
                }
                if fully_left && y1 <= by {
                    assert!(!l);
                }
                let _ = x0;
            }
            // border position error <= 1 cell: in the top and bottom rows the dark -> light switch sits at bx
            let row_switch = |r: usize, a: bool| {
                (0..GRID_COLS)
                    .find(|c| light_cells(&g)[r * GRID_COLS + c] == a)
                    .map(|c| c * cw)
            };
            let bottom_first_dark = row_switch(GRID_ROWS - 1, false).expect("teal is dark");
            assert!(
                (bottom_first_dark as i64 - bx as i64).abs() <= cw as i64,
                "teal edge {bottom_first_dark} vs {bx}"
            );
            // vertical position: column 0 turns light at by
            let col0_light_from = (0..GRID_ROWS).find(|r| light_cells(&g)[r * GRID_COLS]).unwrap() * ch;
            assert!(
                (col0_light_from as i64 - by as i64).abs() <= ch as i64,
                "yellow top {col0_light_from} vs {by}"
            );
        }
    }

    #[test]
    fn navy_yellow_split_in_the_middle_of_a_cell_has_no_grey_cell() {
        for bx in [20usize, 21, 22, 23, 25, 26] {
            let bmp = paint(|x, _| if x < bx { NAVY } else { YELLOW });
            let g = assert_follows_pixels(&bmp, &format!("split at {bx}"));
            let cw = SAMPLE_W as usize / GRID_COLS;
            for (i, c) in g.cells.iter().enumerate() {
                let col = i % GRID_COLS;
                // never a mid-grey scrim: every cell is clearly dark or clearly light
                let y = scrim_l(c);
                assert!(
                    y < 0.15 || y > 0.55,
                    "split {bx}: cell col {col} scrim {} (L {y})",
                    c.rgb
                );
                if (col + 1) * cw <= bx {
                    assert!(
                        c.tone != ToneKind::Light,
                        "split {bx}: light haze over navy at col {col}"
                    );
                }
                if col * cw >= bx {
                    assert!(
                        c.tone == ToneKind::Light,
                        "split {bx}: dark cell inside yellow at col {col}"
                    );
                }
            }
        }
    }

    #[test]
    fn thin_stripes_do_not_haze_a_dark_field() {
        // 1 px light stripes (less than half a 4 px cell) are ignored: the field stays dark everywhere
        let bmp = paint(|x, _| if x % 8 == 3 { WHITE } else { NAVY });
        let g = assert_follows_pixels(&bmp, "1px stripes");
        assert!(
            light_cells(&g).iter().all(|l| !l),
            "1 px stripes must not light any cell"
        );
        // the same on a light field: dark hairlines do not darken it
        let bmp = paint(|x, _| if x % 8 == 3 { NAVY } else { WHITE });
        let g = assert_follows_pixels(&bmp, "1px dark hairlines");
        assert!(light_cells(&g).iter().all(|l| *l));
        // a 4 px wide stripe (exactly one cell) lights exactly its own column
        let bmp = paint(|x, _| if (24..28).contains(&x) { WHITE } else { NAVY });
        let g = assert_follows_pixels(&bmp, "cell wide stripe");
        for (i, l) in light_cells(&g).into_iter().enumerate() {
            assert_eq!(l, i % GRID_COLS == 6, "cell {i}");
        }
    }

    /// Number of polls (each = one admitted frame) until the cell/global tone reaches `want`.
    fn polls_to(
        t: &mut AdaptiveTone,
        gate: &mut ChangeGate,
        bmp: &[u8],
        max: usize,
        ok: impl Fn(&Tone) -> bool,
    ) -> Option<usize> {
        for poll in 1..=max {
            if gate.admit(bmp) {
                if let Some(x) = feed(t, bmp) {
                    if ok(&x) {
                        return Some(poll);
                    }
                }
            }
        }
        None
    }

    /// Latency budget: a backdrop step (whole frame, or one region of it) is painted-ready after 2 polls
    /// (= 2 x POLL_MS), a first sample after 1.
    #[test]
    fn region_step_settles_in_two_polls() {
        let mut t = AdaptiveTone::new(ToneMode::Auto);
        let mut gate = ChangeGate::default();
        let dark = paint(|_, _| NAVY);
        for _ in 0..4 {
            if gate.admit(&dark) {
                feed(&mut t, &dark);
            }
        }
        // the right-hand quarter of the frame turns white
        let part = paint(|x, _| if x >= 36 { WHITE } else { NAVY });
        let n = polls_to(&mut t, &mut gate, &part, 4, |x| {
            let g = x.grid.as_ref().unwrap();
            g.cells[GRID_COLS - 1].tone == ToneKind::Light && g.cells[0].tone != ToneKind::Light
        })
        .expect("region step reaches the UI");
        assert!(n <= 2, "{n} polls");
        assert!(n as u64 * POLL_MS <= 200);
        // and back
        let n = polls_to(&mut t, &mut gate, &dark, 4, |x| {
            x.grid.as_ref().unwrap().cells[GRID_COLS - 1].tone != ToneKind::Light
        })
        .expect("step back");
        assert!(n <= 2, "{n} polls");
        // a first sample is adopted at once
        let mut t = AdaptiveTone::new(ToneMode::Auto);
        assert!(feed(&mut t, &part).is_some());
    }

    #[test]
    fn single_frame_blip_in_one_region_is_ignored_but_a_held_change_is_not() {
        let mut t = AdaptiveTone::new(ToneMode::Auto);
        let dark = paint(|_, _| NAVY);
        let blip = paint(|x, y| {
            if (20..28).contains(&x) && y < 8 {
                WHITE
            } else {
                NAVY
            }
        });
        for _ in 0..3 {
            feed(&mut t, &dark);
        }
        feed(&mut t, &blip);
        feed(&mut t, &dark);
        assert!(
            light_cells(&t.current().grid.unwrap()).iter().all(|l| !l),
            "one-frame blip"
        );
        feed(&mut t, &blip);
        feed(&mut t, &blip);
        assert!(
            light_cells(&t.current().grid.unwrap()).iter().any(|l| *l),
            "held change"
        );
    }

    /// Cost of one analysis tick (all 96 cells + global) on a busy changing frame. The release number is
    /// documented in docs/ARCHITECTURE.md 7.4 (well under 1 ms on an M4); here the bound is generous so a
    /// debug build on a slow CI runner does not flake.
    #[test]
    fn tick_cost_is_small() {
        let mut x = 0x1234_5678_u32;
        let mut r = move || {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            (x >> 8) as u8
        };
        let frames: Vec<Vec<u8>> = (0..16)
            .map(|_| {
                paint(|px, py| {
                    if (px / 6 + py / 5) % 2 == 0 {
                        [r(), r(), r()]
                    } else {
                        [r() / 4, 40, r() / 2]
                    }
                })
            })
            .collect();
        let mut t = AdaptiveTone::new(ToneMode::Auto);
        let n = 160;
        let t0 = std::time::Instant::now();
        for i in 0..n {
            t.update(&frames[i % frames.len()], SAMPLE_W, SAMPLE_H);
        }
        let per = t0.elapsed().as_secs_f64() * 1000.0 / n as f64;
        eprintln!("tone tick: {per:.3} ms per analysed frame");
        let limit = if cfg!(debug_assertions) { 20.0 } else { 1.0 };
        assert!(per < limit, "{per} ms");
    }
}
