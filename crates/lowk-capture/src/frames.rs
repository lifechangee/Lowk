//! Pure frame -> tone-bitmap pipeline of the continuous capture (no OS calls, unit-tested everywhere). The macOS
//! `SCStream` glue in `mac.rs` only hands over the locked pixel buffer (BGRA, row stride) and applies the geometry
//! below to the stream configuration.
//!
//! frame (BGRA, padded rows) -> unpad -> RGBA -> box-average to the visible part -> content gate ->
//! `compose_cells` (off-screen parts rebuilt from on-screen pixels) -> the `SAMPLE_W x SAMPLE_H` bitmap of the tone grid.
use crate::geometry::{self, GrabPlan};
use lowk_types::Rect;

/// Tone grid of the bitmap (must equal `lowk-stealth::tone::GRID_COLS/ROWS`; asserted in `lowk-app`).
pub const TONE_COLS: usize = 12;
pub const TONE_ROWS: usize = 8;
/// The stream renders this many times the visible part's size (a ~6x reduction for the default overlay, far from
/// the 25x that SCK answered with garbage); `box_down` averages the rest.
pub const STREAM_OVERSAMPLE: u32 = 2;
/// `minimumFrameInterval` of the stream: at most 25 frames per second reach us (SCK sends none while nothing changes).
pub const MIN_FRAME_INTERVAL_MS: i64 = 40;

/// After a reconfiguration SCK still delivers frames rendered for the OLD `sourceRect` (queue depth 3 x 40 ms plus
/// the async `updateConfiguration`); they have the right aspect, so only time can tell them apart. Frames are dropped
/// for this long after the geometry changed.
pub const RETARGET_SETTLE_MS: u64 = 200;
/// Colour space the stream is asked to deliver (`kCGColorSpaceSRGB`): without it SCK hands out the display's own
/// (Display P3) values and pure sRGB backdrops read shifted (yellow 240,220,40 -> 236,221,83).
pub const STREAM_COLOR_SPACE: &str = "kCGColorSpaceSRGB";

/// Everything a stream configuration + frame conversion needs for one overlay position.
#[derive(Debug, Clone, PartialEq)]
pub struct StreamGeometry {
    pub plan: GrabPlan,
    /// Pixels to request from SCK (`width`/`height` of the configuration), source aspect.
    pub req: (u32, u32),
}

impl StreamGeometry {
    pub fn new(rect: &Rect, displays: &[Rect], out_w: u32, out_h: u32) -> Option<Self> {
        let plan = geometry::plan(rect, displays, out_w, out_h)?;
        let req = geometry::request_px(plan.src.w, plan.src.h, plan.cap_w, plan.cap_h, STREAM_OVERSAMPLE);
        Some(Self { plan, req })
    }
    /// The numbers handed to `SCStreamConfiguration`: output pixels, then `sourceRect` in display-local points
    /// (top-left origin).
    pub fn config_spec(&self) -> ConfigSpec {
        ConfigSpec {
            width: self.req.0,
            height: self.req.1,
            src: [self.plan.src.x, self.plan.src.y, self.plan.src.w, self.plan.src.h],
        }
    }
    /// Same stream configuration (so a `retarget` needs no `update_configuration`)?
    pub fn same_config(&self, o: &Self) -> bool {
        self.plan.display == o.plan.display && self.req == o.req && self.plan.src == o.plan.src
    }
    /// Is a delivered frame of `w` x `h` px one that was rendered for this geometry (right aspect), not a stale one
    /// from before the last reconfiguration?
    pub fn accepts(&self, w: u32, h: u32) -> bool {
        if w == 0 || h == 0 || self.req.1 == 0 {
            return false;
        }
        let (a, b) = (
            f64::from(w) / f64::from(h),
            f64::from(self.req.0) / f64::from(self.req.1),
        );
        (a / b - 1.0).abs() < 0.04
    }
}

/// Pure view of the stream configuration (see [`StreamGeometry::config_spec`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ConfigSpec {
    pub width: u32,
    pub height: u32,
    /// `[x, y, w, h]` in points relative to the display's top-left.
    pub src: [f64; 4],
}

/// Converts frames to tone bitmaps and drops those whose content did not change.
#[derive(Default)]
pub struct FrameProcessor {
    last: Option<u64>,
    hold_until: Option<std::time::Instant>,
}

impl FrameProcessor {
    /// Forget the last content (after a retarget the same pixels mean something else).
    pub fn reset(&mut self) {
        self.last = None;
    }

    /// Drop every frame until `until` (frames in flight were rendered for the previous geometry).
    pub fn hold_until(&mut self, until: std::time::Instant) {
        self.hold_until = Some(until);
    }

    /// `bgra`: `ih` rows `stride` bytes apart, `iw` pixels of 4 bytes each. `None` = not a usable frame
    /// (wrong aspect, short buffer) or the content equals the previous frame's.
    #[allow(clippy::too_many_arguments)]
    pub fn process(
        &mut self,
        geo: &StreamGeometry,
        bgra: &[u8],
        stride: usize,
        iw: u32,
        ih: u32,
        cols: usize,
        rows: usize,
    ) -> Option<Vec<u8>> {
        if self.hold_until.is_some_and(|t| std::time::Instant::now() < t) {
            return None;
        }
        if !geo.accepts(iw, ih) {
            return None;
        }
        let mut px = geometry::unpad_rows(bgra, iw as usize, ih as usize, stride)?;
        geometry::swap_rb(&mut px);
        let cap = if (iw, ih) == (geo.plan.cap_w, geo.plan.cap_h) {
            px
        } else {
            geometry::box_down(&px, iw, ih, geo.plan.cap_w, geo.plan.cap_h)
        };
        let id = hash(&cap);
        if self.last == Some(id) {
            return None;
        }
        self.last = Some(id);
        Some(geometry::compose_cells(&geo.plan, &cap, cols, rows))
    }
}

fn hash(b: &[u8]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    b.hash(&mut h);
    h.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(x: f64, y: f64, w: f64, h: f64) -> Rect {
        Rect { x, y, w, h }
    }
    fn disp() -> Vec<Rect> {
        vec![r(0.0, 0.0, 1440.0, 900.0)]
    }
    /// BGRA frame `w` x `h` with `stride` bytes per row; left half `l` (RGB), right half `rt`.
    fn frame(w: usize, h: usize, stride: usize, l: [u8; 3], rt: [u8; 3]) -> Vec<u8> {
        let mut v = vec![0u8; stride * h];
        for y in 0..h {
            for x in 0..w {
                let c = if x < w / 2 { l } else { rt };
                v[y * stride + x * 4..y * stride + x * 4 + 4].copy_from_slice(&[c[2], c[1], c[0], 255]);
            }
        }
        v
    }

    #[test]
    fn request_has_the_source_aspect_and_is_small() {
        let g = StreamGeometry::new(&r(100.0, 100.0, 600.0, 384.0), &disp(), 48, 32).unwrap();
        assert!(
            (g.req.0 as f64 / g.req.1 as f64 - 600.0 / 384.0).abs() < 0.03,
            "{:?}",
            g.req
        );
        assert!(g.req.0 >= 96 && g.req.1 >= 64);
        assert!(g.req.0 * g.req.1 * 4 < 40_000, "frame stays small: {:?}", g.req);
    }

    #[test]
    fn frames_become_tone_bitmaps_through_padded_bgra_rows() {
        let g = StreamGeometry::new(&r(100.0, 100.0, 600.0, 384.0), &disp(), 48, 32).unwrap();
        let (w, h) = (g.req.0 as usize, g.req.1 as usize);
        for stride in [w * 4, w * 4 + 16, (w * 4).div_ceil(64) * 64] {
            let mut p = FrameProcessor::default();
            let bmp = p
                .process(
                    &g,
                    &frame(w, h, stride, [0, 0, 0], [255, 255, 255]),
                    stride,
                    w as u32,
                    h as u32,
                    12,
                    8,
                )
                .unwrap();
            assert_eq!(bmp.len(), 48 * 32 * 4);
            assert_eq!(&bmp[..3], &[0, 0, 0], "left dark, stride {stride}");
            let right = (47) * 4;
            assert_eq!(
                &bmp[right..right + 3],
                &[255, 255, 255],
                "right light, stride {stride}"
            );
        }
    }

    #[test]
    fn unchanged_content_is_dropped_changed_content_passes() {
        let g = StreamGeometry::new(&r(100.0, 100.0, 600.0, 384.0), &disp(), 48, 32).unwrap();
        let (w, h) = (g.req.0 as usize, g.req.1 as usize);
        let mut p = FrameProcessor::default();
        let a = frame(w, h, w * 4, [0, 0, 0], [255, 255, 255]);
        assert!(p.process(&g, &a, w * 4, w as u32, h as u32, 12, 8).is_some());
        assert!(
            p.process(&g, &a, w * 4, w as u32, h as u32, 12, 8).is_none(),
            "identical frame"
        );
        let b = frame(w, h, w * 4, [255, 255, 255], [0, 0, 0]);
        assert!(p.process(&g, &b, w * 4, w as u32, h as u32, 12, 8).is_some());
        p.reset();
        assert!(
            p.process(&g, &b, w * 4, w as u32, h as u32, 12, 8).is_some(),
            "after a retarget"
        );
    }

    #[test]
    fn stale_frame_of_the_old_size_is_rejected_after_a_resize() {
        let old = StreamGeometry::new(&r(100.0, 100.0, 600.0, 384.0), &disp(), 48, 32).unwrap();
        let new = StreamGeometry::new(&r(100.0, 100.0, 300.0, 384.0), &disp(), 48, 32).unwrap();
        assert!(!old.same_config(&new));
        let (w, h) = (old.req.0 as usize, old.req.1 as usize);
        let mut p = FrameProcessor::default();
        assert!(p
            .process(
                &new,
                &frame(w, h, w * 4, [9, 9, 9], [9, 9, 9]),
                w * 4,
                w as u32,
                h as u32,
                12,
                8
            )
            .is_none());
    }

    #[test]
    fn a_pure_move_keeps_the_stream_configuration_size() {
        let a = StreamGeometry::new(&r(100.0, 100.0, 600.0, 384.0), &disp(), 48, 32).unwrap();
        let b = StreamGeometry::new(&r(200.0, 150.0, 600.0, 384.0), &disp(), 48, 32).unwrap();
        assert_eq!(a.req, b.req, "same size, only sourceRect changes");
        assert!(!a.same_config(&b));
    }

    #[test]
    fn overlay_over_the_screen_edge_gets_no_black_from_beyond_the_display() {
        // 200 pt of 600 hang over the left edge; the visible part is all white
        let g = StreamGeometry::new(&r(-200.0, 100.0, 600.0, 384.0), &disp(), 48, 32).unwrap();
        let (w, h) = (g.req.0 as usize, g.req.1 as usize);
        let mut p = FrameProcessor::default();
        let bmp = p
            .process(
                &g,
                &frame(w, h, w * 4, [255, 255, 255], [255, 255, 255]),
                w * 4,
                w as u32,
                h as u32,
                12,
                8,
            )
            .unwrap();
        assert!(
            bmp.chunks_exact(4).all(|px| px[..3] == [255, 255, 255]),
            "all cells white, incl. the 4 off-screen columns"
        );
    }

    #[test]
    fn pure_srgb_colours_pass_through_bgra_unchanged_with_padded_rows() {
        let g = StreamGeometry::new(&r(100.0, 100.0, 600.0, 384.0), &disp(), 48, 32).unwrap();
        let (w, h) = (g.req.0 as usize, g.req.1 as usize);
        for (l, rt) in [([240, 220, 40], [200, 30, 30]), ([10, 20, 90], [30, 140, 60])] {
            let stride = w * 4 + 48;
            let bmp = FrameProcessor::default()
                .process(&g, &frame(w, h, stride, l, rt), stride, w as u32, h as u32, 12, 8)
                .unwrap();
            assert_eq!(&bmp[..4], &[l[0], l[1], l[2], 255], "BGRA -> RGBA, alpha kept");
            let i = 47 * 4;
            assert_eq!(&bmp[i..i + 3], &rt);
        }
        assert_eq!(STREAM_COLOR_SPACE, "kCGColorSpaceSRGB");
    }

    #[test]
    fn frames_in_flight_after_a_retarget_are_held_back() {
        let g = StreamGeometry::new(&r(100.0, 100.0, 600.0, 384.0), &disp(), 48, 32).unwrap();
        let (w, h) = (g.req.0 as usize, g.req.1 as usize);
        let f = frame(w, h, w * 4, [1, 2, 3], [4, 5, 6]);
        let mut p = FrameProcessor::default();
        p.hold_until(std::time::Instant::now() + std::time::Duration::from_secs(3600));
        assert!(p.process(&g, &f, w * 4, w as u32, h as u32, 12, 8).is_none());
        p.hold_until(std::time::Instant::now());
        assert!(p.process(&g, &f, w * 4, w as u32, h as u32, 12, 8).is_some());
    }

    #[test]
    fn config_maps_overlay_and_display_to_display_local_points_and_source_aspect() {
        // second display to the right of the first, origin (1440, 0); overlay on it at global (1540, 120)
        let ds = vec![r(0.0, 0.0, 1440.0, 900.0), r(1440.0, 0.0, 1920.0, 1080.0)];
        let g = StreamGeometry::new(&r(1540.0, 120.0, 600.0, 384.0), &ds, 48, 32).unwrap();
        let c = g.config_spec();
        assert_eq!(g.plan.display, 1);
        assert_eq!(c.src, [100.0, 120.0, 600.0, 384.0], "display-local, top-left");
        assert!((c.width as f64 / c.height as f64 - 600.0 / 384.0).abs() < 0.03);
        // hanging over the left edge of display 0: only the visible part is requested
        let g = StreamGeometry::new(&r(-200.0, 100.0, 600.0, 384.0), &ds[..1], 48, 32).unwrap();
        let c = g.config_spec();
        assert_eq!(c.src, [0.0, 100.0, 400.0, 384.0]);
        assert!((c.width as f64 / c.height as f64 - 400.0 / 384.0).abs() < 0.03);
    }

    #[test]
    fn stream_pixels_land_in_the_right_cells_including_the_off_screen_corner() {
        // overlay hangs 200 pt over the LEFT edge: the first cells are off-screen. The visible part is navy on top,
        // red at the bottom; off-screen cells must repeat the nearest on-screen pixels, never black.
        let g = StreamGeometry::new(&r(-200.0, 100.0, 600.0, 384.0), &disp(), 48, 32).unwrap();
        let (w, h) = (g.req.0 as usize, g.req.1 as usize);
        let mut v = vec![0u8; w * h * 4];
        for y in 0..h {
            for x in 0..w {
                let c: [u8; 3] = if y < h / 2 { [10, 20, 90] } else { [200, 30, 30] };
                v[(y * w + x) * 4..][..4].copy_from_slice(&[c[2], c[1], c[0], 255]);
            }
        }
        let bmp = FrameProcessor::default()
            .process(&g, &v, w * 4, w as u32, h as u32, 12, 8)
            .unwrap();
        let px = |x: usize, y: usize| bmp[(y * 48 + x) * 4..][..3].to_vec();
        for x in [0usize, 10, 47] {
            assert_eq!(px(x, 2), vec![10, 20, 90], "top row navy at column {x}");
            assert_eq!(px(x, 30), vec![200, 30, 30], "bottom row red at column {x}");
        }
    }
}
