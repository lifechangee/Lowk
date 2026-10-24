//! Pure geometry of a region grab: window frame (global top-left POINTS) -> display + source rect + output size.
//! No OS calls, so Retina / multi-display / negative-origin / edge cases are unit-tested everywhere.
//!
//! The tone grid (4x3) maps cell (col,row) to the SAME fractional box of the overlay window, so the grab must
//! keep that proportion: when the window hangs over a display edge only the visible part can be captured, and
//! it is placed at its proportional offset inside the output (edge pixels replicated into the uncovered part)
//! instead of being stretched over the whole output (which shifted every cell).
use lowk_types::Rect;

/// What to ask the capture API for.
#[derive(Debug, Clone, PartialEq)]
pub struct GrabPlan {
    /// Index into the display list passed to [`plan`].
    pub display: usize,
    /// Source rect in POINTS relative to that display's top-left origin.
    pub src: Rect,
    /// Pixels to request for `src` (the visible part).
    pub cap_w: u32,
    pub cap_h: u32,
    /// Final output size and where the captured part sits inside it.
    pub out_w: u32,
    pub out_h: u32,
    pub off_x: u32,
    pub off_y: u32,
}

fn inter(a: &Rect, b: &Rect) -> Option<Rect> {
    let (x0, y0) = (a.x.max(b.x), a.y.max(b.y));
    let (x1, y1) = ((a.x + a.w).min(b.x + b.w), (a.y + a.h).min(b.y + b.h));
    (x1 > x0 && y1 > y0).then_some(Rect {
        x: x0,
        y: y0,
        w: x1 - x0,
        h: y1 - y0,
    })
}

/// The display holding most of `rect` (ties: the first). `None` when it overlaps no display.
pub fn pick_display(rect: &Rect, displays: &[Rect]) -> Option<usize> {
    let mut best: Option<(usize, f64)> = None;
    for (i, d) in displays.iter().enumerate() {
        if let Some(i2) = inter(rect, d) {
            let a = i2.w * i2.h;
            if best.is_none_or(|(_, b)| a > b + 1e-9) {
                best = Some((i, a));
            }
        }
    }
    best.map(|(i, _)| i)
}

/// Plan a grab of `rect` into an `out_w` x `out_h` bitmap. Falls back to the first display when `rect` is off-screen.
pub fn plan(rect: &Rect, displays: &[Rect], out_w: u32, out_h: u32) -> Option<GrabPlan> {
    let (out_w, out_h) = (out_w.max(1), out_h.max(1));
    let rect = Rect {
        w: rect.w.max(1.0),
        h: rect.h.max(1.0),
        ..*rect
    };
    let display = pick_display(&rect, displays).or(if displays.is_empty() { None } else { Some(0) })?;
    let d = &displays[display];
    let vis = inter(&rect, d).unwrap_or(Rect {
        x: d.x,
        y: d.y,
        w: d.w.min(rect.w),
        h: d.h.min(rect.h),
    });
    let (fx0, fy0) = ((vis.x - rect.x) / rect.w, (vis.y - rect.y) / rect.h);
    let (fx1, fy1) = (
        (vis.x + vis.w - rect.x) / rect.w,
        (vis.y + vis.h - rect.y) / rect.h,
    );
    let ox0 = (fx0 * out_w as f64).round().clamp(0.0, out_w as f64 - 1.0) as u32;
    let oy0 = (fy0 * out_h as f64).round().clamp(0.0, out_h as f64 - 1.0) as u32;
    let ox1 = ((fx1 * out_w as f64).round() as u32).clamp(ox0 + 1, out_w);
    let oy1 = ((fy1 * out_h as f64).round() as u32).clamp(oy0 + 1, out_h);
    Some(GrabPlan {
        display,
        src: Rect {
            x: vis.x - d.x,
            y: vis.y - d.y,
            w: vis.w,
            h: vis.h,
        },
        cap_w: ox1 - ox0,
        cap_h: oy1 - oy0,
        out_w,
        out_h,
        off_x: ox0,
        off_y: oy0,
    })
}

/// Place the captured RGBA (`cap_w` x `cap_h`) into the output bitmap, replicating edge pixels into uncovered area.
pub fn compose(p: &GrabPlan, cap: &[u8]) -> Vec<u8> {
    let (cw, ch) = (p.cap_w as usize, p.cap_h as usize);
    let mut out = Vec::with_capacity((p.out_w * p.out_h * 4) as usize);
    for y in 0..p.out_h as usize {
        let sy = y.saturating_sub(p.off_y as usize).min(ch - 1);
        for x in 0..p.out_w as usize {
            let sx = x.saturating_sub(p.off_x as usize).min(cw - 1);
            let i = (sy * cw + sx) * 4;
            out.extend_from_slice(&cap[i..i + 4]);
        }
    }
    out
}

/// Like [`compose`], but built for the tone grid (`cols` x `rows` cells over the output): every cell is built from its
/// ON-SCREEN pixels only. The captured part (`off_*`, `cap_*`) is the overlay clipped to the display, so
/// - a cell fully on screen is copied;
/// - a cell partly on screen is filled by repeating (tiling) its on-screen pixels over the missing part, so neither
///   black from beyond the display nor one smeared edge column enters its average or its dark/light majority;
/// - a cell fully off screen is a copy of the nearest on-screen cell (clamped column / row).
pub fn compose_cells(p: &GrabPlan, cap: &[u8], cols: usize, rows: usize) -> Vec<u8> {
    let (ow, oh) = (p.out_w as usize, p.out_h as usize);
    let (cw, ch) = (p.cap_w as usize, p.cap_h as usize);
    let (ox0, oy0) = (p.off_x as usize, p.off_y as usize);
    let (ox1, oy1) = (ox0 + cw, oy0 + ch);
    let mut out = vec![0u8; ow * oh * 4];
    if cols == 0 || rows == 0 || ow < cols || oh < rows || cap.len() < cw * ch * 4 || cw == 0 || ch == 0 {
        return compose(p, cap);
    }
    // Cell c spans pixels [c*ow/cols, (c+1)*ow/cols); the on-screen part of a span is its overlap with [ox0, ox1).
    let span = |c: usize, n: usize, size: usize| (c * size / n, (c + 1) * size / n);
    let clip = |(a, b): (usize, usize), lo: usize, hi: usize| (a.max(lo), b.min(hi));
    let has = |(a, b): (usize, usize)| b > a;
    let mut on_cols = (usize::MAX, 0usize);
    let mut on_rows = (usize::MAX, 0usize);
    for c in 0..cols {
        if has(clip(span(c, cols, ow), ox0, ox1)) {
            on_cols = (on_cols.0.min(c), on_cols.1.max(c));
        }
    }
    for r in 0..rows {
        if has(clip(span(r, rows, oh), oy0, oy1)) {
            on_rows = (on_rows.0.min(r), on_rows.1.max(r));
        }
    }
    for r in 0..rows {
        let ys = span(r, rows, oh);
        let yv = clip(ys, oy0, oy1);
        for c in 0..cols {
            let xs = span(c, cols, ow);
            let xv = clip(xs, ox0, ox1);
            if !has(xv) || !has(yv) {
                continue; // off-screen cell: second pass
            }
            let (vw, vh) = (xv.1 - xv.0, yv.1 - yv.0);
            for y in ys.0..ys.1 {
                for x in xs.0..xs.1 {
                    let sx = if x >= xv.0 && x < xv.1 {
                        x
                    } else {
                        // tile the on-screen part over the missing part
                        xv.0 + (x as isize - xv.0 as isize).rem_euclid(vw as isize) as usize
                    };
                    let sy = if y >= yv.0 && y < yv.1 {
                        y
                    } else {
                        yv.0 + (y as isize - yv.0 as isize).rem_euclid(vh as isize) as usize
                    };
                    let i = ((sy - oy0) * cw + (sx - ox0)) * 4;
                    let o = (y * ow + x) * 4;
                    out[o..o + 4].copy_from_slice(&cap[i..i + 4]);
                }
            }
        }
    }
    for r in 0..rows {
        let (ys, _) = (span(r, rows, oh), ());
        let rr = r.clamp(on_rows.0, on_rows.1);
        for c in 0..cols {
            let xs = span(c, cols, ow);
            let cc = c.clamp(on_cols.0, on_cols.1);
            if (cc, rr) == (c, r) {
                continue;
            }
            let (sxs, sys) = (span(cc, cols, ow), span(rr, rows, oh));
            for y in ys.0..ys.1 {
                for x in xs.0..xs.1 {
                    // same offset inside the cell, clamped to the source cell's size
                    let sx = (sxs.0 + (x - xs.0)).min(sxs.1 - 1);
                    let sy = (sys.0 + (y - ys.0)).min(sys.1 - 1);
                    let (o, i) = ((y * ow + x) * 4, (sy * ow + sx) * 4);
                    let px: [u8; 4] = out[i..i + 4].try_into().unwrap_or([0; 4]);
                    out[o..o + 4].copy_from_slice(&px);
                }
            }
        }
    }
    out
}

/// Pixels per point of a captured image of `px_w` pixels for a `pt_w`-wide source (Retina = 2).
pub fn scale_of(px_w: u32, pt_w: f64) -> f64 {
    if pt_w <= 0.0 {
        1.0
    } else {
        px_w as f64 / pt_w
    }
}

/// Copy `h` rows of `w` RGBA pixels out of a buffer whose rows are `bytes_per_row` apart (CGImage rows are padded to
/// 16/32/64 bytes). A tightly packed buffer is returned as is. `None` when the buffer is too short.
pub fn unpad_rows(buf: &[u8], w: usize, h: usize, bytes_per_row: usize) -> Option<Vec<u8>> {
    let row = w.checked_mul(4)?;
    if bytes_per_row < row || h == 0 {
        return None;
    }
    if bytes_per_row == row {
        return (buf.len() >= row * h).then(|| buf[..row * h].to_vec());
    }
    if buf.len() < bytes_per_row * (h - 1) + row {
        return None;
    }
    let mut out = Vec::with_capacity(row * h);
    for y in 0..h {
        out.extend_from_slice(&buf[y * bytes_per_row..y * bytes_per_row + row]);
    }
    Some(out)
}

/// Swap B and R in place (BGRA <-> RGBA).
pub fn swap_rb(px: &mut [u8]) {
    for p in px.chunks_exact_mut(4) {
        p.swap(0, 2);
    }
}

/// Resample RGBA `iw` x `ih` to `ow` x `oh`: a box average when shrinking (so one stray source pixel cannot decide a
/// tone cell), nearest when enlarging. Channels are averaged in sRGB space, which is what the tone analysis expects.
pub fn box_down(px: &[u8], iw: u32, ih: u32, ow: u32, oh: u32) -> Vec<u8> {
    let (iw, ih, ow, oh) = (iw as usize, ih as usize, ow as usize, oh as usize);
    let mut out = Vec::with_capacity(ow * oh * 4);
    for y in 0..oh {
        let y0 = y * ih / oh;
        let y1 = ((y + 1) * ih / oh).max(y0 + 1).min(ih);
        for x in 0..ow {
            let x0 = x * iw / ow;
            let x1 = ((x + 1) * iw / ow).max(x0 + 1).min(iw);
            let mut acc = [0u32; 4];
            for sy in y0..y1 {
                for sx in x0..x1 {
                    let i = (sy * iw + sx) * 4;
                    for (a, c) in acc.iter_mut().zip(&px[i..i + 4]) {
                        *a += u32::from(*c);
                    }
                }
            }
            let n = ((y1 - y0) * (x1 - x0)) as u32;
            out.extend(acc.iter().map(|a| ((a + n / 2) / n) as u8));
        }
    }
    out
}

/// Pixel size to request from ScreenCaptureKit for a `src_w` x `src_h` pt source rect so that the answer is at least
/// `cap_w*ss` x `cap_h*ss`. The request MUST have the source's aspect ratio: SCK keeps the aspect of `sourceRect` and
/// letterboxes the rest with black (top-anchored), so a 24x18 (4:3) request for a 600x384 pt window used to come back
/// with the top ~85% filled and black below, which `box_down` then averaged in (lower tone rows dimmed to 0.5625).
pub fn request_px(src_w: f64, src_h: f64, cap_w: u32, cap_h: u32, ss: u32) -> (u32, u32) {
    let (tw, th) = (f64::from(cap_w * ss), f64::from(cap_h * ss));
    if src_w <= 0.0 || src_h <= 0.0 {
        return (cap_w * ss, cap_h * ss);
    }
    let k = (tw / src_w).max(th / src_h);
    (
        ((src_w * k).round() as u32).max(1),
        ((src_h * k).round() as u32).max(1),
    )
}

/// Mean RGB of an RGBA bitmap (diagnostics).
pub fn mean_rgb(px: &[u8]) -> [u8; 3] {
    let n = (px.len() / 4).max(1) as u64;
    let mut s = [0u64; 3];
    for p in px.chunks_exact(4) {
        for (a, c) in s.iter_mut().zip(p) {
            *a += u64::from(*c);
        }
    }
    [(s[0] / n) as u8, (s[1] / n) as u8, (s[2] / n) as u8]
}

/// What the last tone grab did (QA diagnostics; the sampler loop swallows errors, so this is the only trace).
#[derive(Debug, Clone, Default)]
pub struct GrabDiag {
    pub path: &'static str,
    pub display: usize,
    pub src_pts: Option<Rect>,
    pub requested_px: (u32, u32),
    pub image_px: (u32, u32),
    pub out_px: (u32, u32),
    pub own_excluded: usize,
    pub own_kept: usize,
    pub attempts: u32,
    pub mean_rgb: [u8; 3],
    pub error: Option<String>,
}

impl GrabDiag {
    /// JSON for the qa channel.
    pub fn json(&self) -> String {
        let r = self.src_pts.map_or("null".to_string(), |r| {
            format!("{{\"x\":{},\"y\":{},\"w\":{},\"h\":{}}}", r.x, r.y, r.w, r.h)
        });
        format!(
            "{{\"path\":\"{}\",\"display\":{},\"src_pts\":{},\"requested_px\":[{},{}],\"image_px\":[{},{}],\"out_px\":[{},{}],\"own_excluded\":{},\"own_kept\":{},\"attempts\":{},\"mean_rgb\":[{},{},{}],\"error\":{}}}",
            self.path, self.display, r, self.requested_px.0, self.requested_px.1, self.image_px.0, self.image_px.1,
            self.out_px.0, self.out_px.1, self.own_excluded, self.own_kept, self.attempts, self.mean_rgb[0], self.mean_rgb[1],
            self.mean_rgb[2],
            self.error.as_ref().map_or("null".to_string(), |e| format!("{:?}", e))
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    /// What SCK does with a request whose aspect differs from the source: scale to fit, top-left anchored, black rest.
    fn sck_fit(src: &[u8], sw: usize, sh: usize, rw: usize, rh: usize) -> (Vec<u8>, usize, usize) {
        let k = (rw as f64 / sw as f64).min(rh as f64 / sh as f64);
        let (fw, fh) = (
            ((sw as f64 * k) as usize).max(1),
            ((sh as f64 * k) as usize).max(1),
        );
        let mut out = vec![0u8; rw * rh * 4];
        for y in 0..fh {
            for x in 0..fw {
                let (sx, sy) = (x * sw / fw, y * sh / fh);
                out[(y * rw + x) * 4..(y * rw + x) * 4 + 4]
                    .copy_from_slice(&src[(sy * sw + sx) * 4..(sy * sw + sx) * 4 + 4]);
            }
        }
        (out, rw, rh)
    }

    /// A 600x384 pt window, top half yellow, bottom half navy (the harness backdrop), rendered at 1 px/pt.
    fn split_src() -> (Vec<u8>, usize, usize) {
        let (w, h) = (600usize, 384usize);
        let mut v = Vec::new();
        for y in 0..h {
            for _ in 0..w {
                v.extend_from_slice(if y < h / 2 {
                    &[240, 220, 40, 255]
                } else {
                    &[10, 20, 90, 255]
                });
            }
        }
        (v, w, h)
    }

    fn row_means(px: &[u8], w: usize, h: usize) -> Vec<[u8; 3]> {
        // 3 tone rows of an RGBA bitmap
        (0..3)
            .map(|r| {
                let (y0, y1) = (r * h / 3, (r + 1) * h / 3);
                let mut s = [0u64; 3];
                for y in y0..y1 {
                    for x in 0..w {
                        for c in 0..3 {
                            s[c] += u64::from(px[(y * w + x) * 4 + c]);
                        }
                    }
                }
                let n = ((y1 - y0) * w) as u64;
                [(s[0] / n) as u8, (s[1] / n) as u8, (s[2] / n) as u8]
            })
            .collect()
    }

    #[test]
    fn mismatched_aspect_request_letterboxes_and_dims_the_bottom_row() {
        // The bug: ask 8x the 24x18 output for a 600x384 source -> black bars -> bottom row 0.5625 of the truth.
        let (src, sw, sh) = split_src();
        let (img, iw, ih) = sck_fit(&src, sw, sh, 192, 144);
        let px = box_down(&img, iw as u32, ih as u32, 24, 18);
        let rows = row_means(&px, 24, 18);
        assert!(rows[2][2] < 60, "bottom navy dimmed: {:?}", rows[2]);
    }

    #[test]
    fn request_px_keeps_source_aspect_so_every_tone_row_is_true() {
        let (src, sw, sh) = split_src();
        for (cw, ch) in [(24u32, 18u32), (12, 18), (24, 9)] {
            let (rw, rh) = request_px(sw as f64, sh as f64, cw, ch, 8);
            assert!(rw >= cw * 8 && rh >= ch * 8);
            assert!((rw as f64 / rh as f64 - sw as f64 / sh as f64).abs() < 0.02);
            let (img, iw, ih) = sck_fit(&src, sw, sh, rw as usize, rh as usize);
            let px = box_down(&img, iw as u32, ih as u32, cw, ch);
            let rows = row_means(&px, cw as usize, ch as usize);
            assert_eq!(rows[0], [240, 220, 40], "{cw}x{ch}");
            assert!(
                rows[2][2] >= 88 && rows[2][2] <= 92,
                "navy bottom row true {cw}x{ch}: {:?}",
                rows[2]
            );
        }
    }

    #[test]
    fn lower_rows_only_changes_reach_every_cell_through_padded_buffers() {
        // frames where ONLY the lower two tone rows change; strides padded to 16/64/256 bytes
        let (w, h) = (24usize, 18usize);
        for pad in [0usize, 8, 40, 232] {
            let bpr = w * 4 + pad;
            for (n, col) in [[200u8, 10, 10], [10, 200, 10], [10, 10, 200]].iter().enumerate() {
                let mut buf = vec![0xEEu8; bpr * h];
                for y in 0..h {
                    for x in 0..w {
                        let c = if y >= 6 { *col } else { [50, 50, 50] };
                        buf[y * bpr + x * 4..y * bpr + x * 4 + 4].copy_from_slice(&[c[0], c[1], c[2], 255]);
                    }
                }
                let px = unpad_rows(&buf, w, h, bpr).unwrap();
                assert_eq!(px.len(), w * h * 4, "pad {pad} frame {n}");
                let means = lowk_stealth_free_cell_means(&px, w, h);
                assert_eq!(&means[..4], &[[50, 50, 50]; 4]);
                assert!(
                    means[4..].iter().all(|m| m == col),
                    "pad {pad} frame {n}: {means:?}"
                );
            }
        }
    }

    /// Local 4x3 mean (this crate does not depend on lowk-stealth; same math as `tone::cell_means`).
    fn lowk_stealth_free_cell_means(px: &[u8], w: usize, h: usize) -> Vec<[u8; 3]> {
        (0..12)
            .map(|i| {
                let (c, r) = (i % 4, i / 4);
                let mut s = [0u64; 3];
                let mut n = 0u64;
                for y in r * h / 3..(r + 1) * h / 3 {
                    for x in c * w / 4..(c + 1) * w / 4 {
                        for k in 0..3 {
                            s[k] += u64::from(px[(y * w + x) * 4 + k]);
                        }
                        n += 1;
                    }
                }
                [(s[0] / n) as u8, (s[1] / n) as u8, (s[2] / n) as u8]
            })
            .collect()
    }

    const fn r(x: f64, y: f64, w: f64, h: f64) -> Rect {
        Rect { x, y, w, h }
    }
    fn two() -> Vec<Rect> {
        // primary 1440x900 and a second one to the LEFT and slightly UP (negative origin in top-left space)
        vec![r(0.0, 0.0, 1440.0, 900.0), r(-1920.0, -180.0, 1920.0, 1080.0)]
    }

    #[test]
    fn inside_primary_is_identity_scaled() {
        let p = plan(&r(100.0, 200.0, 480.0, 300.0), &two(), 24, 18).unwrap();
        assert_eq!((p.display, p.cap_w, p.cap_h, p.off_x, p.off_y), (0, 24, 18, 0, 0));
        assert_eq!(p.src, r(100.0, 200.0, 480.0, 300.0));
    }

    #[test]
    fn second_display_negative_origin_is_relative() {
        let p = plan(&r(-1000.0, -100.0, 400.0, 200.0), &two(), 24, 18).unwrap();
        assert_eq!(p.display, 1);
        assert_eq!(p.src, r(920.0, 80.0, 400.0, 200.0));
    }

    #[test]
    fn retina_is_a_pure_pixel_count_not_a_geometry_change() {
        assert_eq!(scale_of(960, 480.0), 2.0);
        assert_eq!(scale_of(480, 480.0), 1.0);
        assert_eq!(scale_of(10, 0.0), 1.0);
    }

    #[test]
    fn most_area_wins_straddling_two_displays() {
        // 600 wide: 100 on the primary, 500 on the left display
        let p = plan(&r(-500.0, 0.0, 600.0, 300.0), &two(), 24, 18).unwrap();
        assert_eq!(p.display, 1);
        assert_eq!((p.off_x, p.cap_w), (0, 20));
        assert_eq!(p.src.x, 1420.0);
    }

    #[test]
    fn hanging_over_the_right_edge_keeps_cell_proportions() {
        let p = plan(
            &r(1340.0, 100.0, 400.0, 300.0),
            &[r(0.0, 0.0, 1440.0, 900.0)],
            24,
            18,
        )
        .unwrap();
        assert_eq!((p.off_x, p.cap_w), (0, 6));
        assert_eq!(p.src, r(1340.0, 100.0, 100.0, 300.0));
    }

    #[test]
    fn hanging_over_the_left_and_top_edges_offsets_the_capture() {
        let p = plan(
            &r(-100.0, -75.0, 400.0, 300.0),
            &[r(0.0, 0.0, 1440.0, 900.0)],
            24,
            18,
        )
        .unwrap();
        assert_eq!((p.off_x, p.cap_w, p.off_y, p.cap_h), (6, 18, 5, 13));
        assert_eq!(p.src, r(0.0, 0.0, 300.0, 225.0));
    }

    #[test]
    fn compose_places_and_replicates() {
        let p = GrabPlan {
            display: 0,
            src: r(0.0, 0.0, 1.0, 1.0),
            cap_w: 2,
            cap_h: 1,
            out_w: 4,
            out_h: 1,
            off_x: 1,
            off_y: 0,
        };
        let cap = [10, 0, 0, 255, 20, 0, 0, 255];
        let reds: Vec<u8> = compose(&p, &cap).chunks(4).map(|c| c[0]).collect();
        assert_eq!(reds, vec![10, 10, 20, 20]);
    }

    /// Output 48x32 bitmap, 12x8 cells of 4x4 px; `cap` coloured by column so every cell's source is identifiable.
    fn cells_plan(off_x: u32, cap_w: u32, off_y: u32, cap_h: u32) -> (GrabPlan, Vec<u8>) {
        let p = GrabPlan {
            display: 0,
            src: r(0.0, 0.0, 1.0, 1.0),
            cap_w,
            cap_h,
            out_w: 48,
            out_h: 32,
            off_x,
            off_y,
        };
        let mut cap = Vec::new();
        for y in 0..cap_h {
            for x in 0..cap_w {
                // red = on-screen x (global px), green = on-screen y, blue = 7 (never black)
                cap.extend_from_slice(&[(off_x + x) as u8 * 5 + 1, (off_y + y) as u8 * 7 + 1, 7, 255]);
            }
        }
        (p, cap)
    }

    #[test]
    fn compose_cells_fully_visible_is_a_plain_copy() {
        let (p, cap) = cells_plan(0, 48, 0, 32);
        assert_eq!(compose_cells(&p, &cap, 12, 8), cap);
    }

    #[test]
    fn partly_off_screen_cell_uses_only_its_on_screen_pixels() {
        // overlay hangs over the left edge: cell column 1 spans px 4..8, only 6..8 are on screen
        let (p, cap) = cells_plan(6, 42, 0, 32);
        let out = compose_cells(&p, &cap, 12, 8);
        for y in 0..32usize {
            for x in 4..8usize {
                let px = &out[(y * 48 + x) * 4..(y * 48 + x) * 4 + 4];
                assert_ne!(px[2], 0, "no black from beyond the display at {x},{y}");
                assert!(
                    px[0] == 6 * 5 + 1 || px[0] == 7 * 5 + 1,
                    "only on-screen columns 6/7 at x={x}: {}",
                    px[0]
                );
            }
        }
    }

    #[test]
    fn fully_off_screen_cells_inherit_the_nearest_on_screen_cell() {
        // 10 px hang over the left edge: cells 0 and 1 (px 0..8) are off screen, cell 2 (8..12) is partly on screen
        let (p, cap) = cells_plan(10, 38, 0, 32);
        let out = compose_cells(&p, &cap, 12, 8);
        let cell = |c: usize, r: usize| -> Vec<u8> {
            (0..4)
                .flat_map(|y| {
                    out[((r * 4 + y) * 48 + c * 4) * 4..((r * 4 + y) * 48 + c * 4 + 4) * 4].to_vec()
                })
                .collect()
        };
        for r in 0..8 {
            assert_eq!(cell(0, r), cell(2, r), "cell 0 inherits cell 2, row {r}");
            assert_eq!(cell(1, r), cell(2, r), "cell 1 inherits cell 2, row {r}");
        }
        assert!(out.chunks_exact(4).all(|px| px[2] == 7), "no black anywhere");
    }

    #[test]
    fn off_screen_bottom_and_right_inherit_too() {
        let (p, cap) = cells_plan(0, 30, 0, 20);
        let out = compose_cells(&p, &cap, 12, 8);
        assert!(out.chunks_exact(4).all(|px| px[2] == 7), "no black anywhere");
        let at = |x: usize, y: usize| out[(y * 48 + x) * 4..(y * 48 + x) * 4 + 3].to_vec();
        // last visible cell column is 7 (px 28..32, on screen up to 30), last visible row is 4 (px 16..20)
        assert_eq!(at(44, 17), at(28, 17));
        assert_eq!(at(45, 29), at(29, 17));
    }

    #[test]
    fn border_cell_at_the_edge_keeps_its_majority_side() {
        // 2 px of a 4 px cell are on screen: left px light, right px dark -> after tiling the cell is half/half,
        // never grey-averaged with black
        let p = GrabPlan {
            display: 0,
            src: r(0.0, 0.0, 1.0, 1.0),
            cap_w: 46,
            cap_h: 32,
            out_w: 48,
            out_h: 32,
            off_x: 2,
            off_y: 0,
        };
        let mut cap = Vec::new();
        for _ in 0..32 {
            for x in 0..46 {
                cap.extend_from_slice(if x == 0 {
                    &[250, 250, 250, 255]
                } else {
                    &[20, 20, 20, 255]
                });
            }
        }
        let out = compose_cells(&p, &cap, 12, 8);
        let row0: Vec<u8> = (0..4).map(|x| out[x * 4]).collect();
        assert!(row0.iter().all(|v| *v == 250 || *v == 20), "{row0:?}");
        assert!(row0.contains(&250) && row0.contains(&20));
    }

    #[test]
    fn dark_left_stays_left_wherever_the_window_is() {
        // Backdrop: x < 700 dark, else light. After any move, output column c must sample the point under cell c.
        let disp = [r(0.0, 0.0, 1440.0, 900.0)];
        for x in [-100.0, 0.0, 300.0, 500.0, 650.0, 1000.0, 1200.0] {
            let rect = r(x, 300.0, 400.0, 300.0);
            let p = plan(&rect, &disp, 24, 18).unwrap();
            for col in 0..24u32 {
                let cell_center = rect.x + (col as f64 + 0.5) * rect.w / 24.0;
                if cell_center < 0.0 || cell_center > 1440.0 {
                    continue;
                }
                let c = (col.max(p.off_x) - p.off_x).min(p.cap_w - 1);
                let sampled = p.src.x + (c as f64 + 0.5) * p.src.w / p.cap_w as f64;
                assert_eq!(sampled < 700.0, cell_center < 700.0, "x={x} col={col}");
            }
        }
    }

    #[test]
    fn off_screen_and_empty() {
        assert!(plan(&r(0.0, 0.0, 10.0, 10.0), &[], 24, 18).is_none());
        let p = plan(&r(9000.0, 9000.0, 10.0, 10.0), &two(), 24, 18).unwrap();
        assert_eq!(p.display, 0);
    }

    fn scene(w: usize, h: usize, f: impl Fn(usize, usize) -> [u8; 3]) -> Vec<u8> {
        let mut v = Vec::new();
        for y in 0..h {
            for x in 0..w {
                let c = f(x, y);
                v.extend_from_slice(&[c[0], c[1], c[2], 255]);
            }
        }
        v
    }

    #[test]
    fn unpad_removes_row_padding() {
        // 3 px wide, rows padded to 16 bytes: a wrong stride would smear colours into the next row
        let mut buf = Vec::new();
        for y in 0..2u8 {
            for x in 0..3u8 {
                buf.extend_from_slice(&[10 * y + x, 0, 0, 255]);
            }
            buf.extend_from_slice(&[9, 9, 9, 9]);
        }
        let t = unpad_rows(&buf, 3, 2, 16).unwrap();
        let reds: Vec<u8> = t.chunks(4).map(|c| c[0]).collect();
        assert_eq!(reds, vec![0, 1, 2, 10, 11, 12]);
        assert!(unpad_rows(&buf[..10], 3, 2, 16).is_none());
        assert!(unpad_rows(&buf, 3, 2, 8).is_none());
        assert_eq!(unpad_rows(&t, 3, 2, 12).unwrap(), t);
    }

    #[test]
    fn bgra_swap_roundtrips() {
        let mut p = vec![1, 2, 3, 4, 5, 6, 7, 8];
        swap_rb(&mut p);
        assert_eq!(p, vec![3, 2, 1, 4, 7, 6, 5, 8]);
        swap_rb(&mut p);
        assert_eq!(p, vec![1, 2, 3, 4, 5, 6, 7, 8]);
    }

    #[test]
    fn box_down_keeps_left_right_split_at_retina_sizes() {
        // 1200x768 px (a 600x384 pt overlay at 2x): left half black, right half white -> 24x18 keeps the split
        let img = scene(
            1200,
            768,
            |x, _| if x < 600 { [0, 0, 0] } else { [255, 255, 255] },
        );
        let out = box_down(&img, 1200, 768, 24, 18);
        assert_eq!(out.len(), 24 * 18 * 4);
        for y in 0..18 {
            for x in 0..24 {
                let v = out[(y * 24 + x) * 4];
                assert_eq!(v, if x < 12 { 0 } else { 255 }, "x={x} y={y}");
            }
        }
        assert_eq!(mean_rgb(&out)[0], 127);
    }

    #[test]
    fn box_down_is_not_all_dark_for_light_or_coloured_input() {
        for (c, name) in [
            ([255, 255, 255], "white"),
            ([240, 220, 40], "yellow"),
            ([200, 30, 30], "red"),
        ] {
            let img = scene(192, 144, |_, _| c);
            let out = box_down(&img, 192, 144, 24, 18);
            assert!(out.chunks(4).all(|p| p[..3] == c), "{name}");
        }
    }

    #[test]
    fn box_down_averages_and_enlarges() {
        let two = scene(2, 1, |x, _| if x == 0 { [0, 0, 0] } else { [100, 200, 50] });
        assert_eq!(&box_down(&two, 2, 1, 1, 1)[..3], &[50, 100, 25]);
        let up = box_down(&two, 2, 1, 4, 1);
        assert_eq!(
            up.chunks(4).map(|p| p[0]).collect::<Vec<_>>(),
            vec![0, 0, 100, 100]
        );
    }

    #[test]
    fn full_pipeline_quadrants_through_plan_and_compose() {
        // backdrop: navy | red over yellow | teal on a 1440x900 display, window straddling the middle
        let disp = [r(0.0, 0.0, 1440.0, 900.0)];
        let win = r(420.0, 258.0, 600.0, 384.0);
        let colour = |px: f64, py: f64| match (px < 720.0, py < 450.0) {
            (true, true) => [10, 20, 90],
            (false, true) => [200, 30, 30],
            (true, false) => [240, 220, 40],
            (false, false) => [0, 110, 120],
        };
        let p = plan(&win, &disp, 24, 18).unwrap();
        let (iw, ih) = (p.cap_w * 8, p.cap_h * 8);
        let img = scene(iw as usize, ih as usize, |x, y| {
            colour(
                p.src.x + (x as f64 + 0.5) * p.src.w / iw as f64,
                p.src.y + (y as f64 + 0.5) * p.src.h / ih as f64,
            )
        });
        let small = box_down(&img, iw, ih, p.cap_w, p.cap_h);
        let out = compose(&p, &small);
        let at = |c: usize, rw: usize| {
            let (x, y) = (c * 24 / 4 + 3, rw * 18 / 3 + 2);
            let i = (y * 24 + x) * 4;
            [out[i], out[i + 1], out[i + 2]]
        };
        assert_eq!(at(0, 0), [10, 20, 90]);
        assert_eq!(at(3, 0), [200, 30, 30]);
        assert_eq!(at(0, 2), [240, 220, 40]);
        assert_eq!(at(3, 2), [0, 110, 120]);
    }
}
