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

/// Pixels per point of a captured image of `px_w` pixels for a `pt_w`-wide source (Retina = 2).
pub fn scale_of(px_w: u32, pt_w: f64) -> f64 {
    if pt_w <= 0.0 {
        1.0
    } else {
        px_w as f64 / pt_w
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
}
