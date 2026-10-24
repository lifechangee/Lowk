//! Area selection geometry (pure, unit-tested): drag corners -> rectangle, clamp to the display,
//! window-local CSS pixels (= points) -> global points and physical pixels, display under the pointer.
//! The selection window covers exactly one display; every coordinate the page sends is relative to it.
use lowk_types::Rect;

/// A drag shorter than this (both axes, window px) is a click and cancels.
pub const CLICK_PX: f64 = 5.0;
/// Smallest selection edge in points / pixels (same floor as `capture_area`).
pub const MIN_EDGE: f64 = 4.0;

/// One display as the selection window covers it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AreaDisplay {
    /// Global top-left points (negative origins are normal on multi-monitor setups).
    pub points: Rect,
    /// Physical pixels per point.
    pub scale: f64,
    /// Global physical-pixel origin of the display (the virtual desktop on Windows).
    pub px_origin: (f64, f64),
}

/// The picked area in both units; macOS grabs `points`, Windows grabs `pixels`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AreaPick {
    pub points: Rect,
    pub pixels: Rect,
}

/// Rectangle between two drag corners (any direction).
pub fn normalize(a: (f64, f64), b: (f64, f64)) -> Rect {
    Rect {
        x: a.0.min(b.0),
        y: a.1.min(b.1),
        w: (a.0 - b.0).abs(),
        h: (a.1 - b.1).abs(),
    }
}

/// True when press and release are so close that it was a click, not a drag.
pub fn is_click(a: (f64, f64), b: (f64, f64)) -> bool {
    (a.0 - b.0).abs() < CLICK_PX && (a.1 - b.1).abs() < CLICK_PX
}

/// Clamp `r` (window-local) to a `w` x `h` window. `None` when nothing is left or a non-finite value came in.
pub fn clamp(r: Rect, w: f64, h: f64) -> Option<Rect> {
    if ![r.x, r.y, r.w, r.h, w, h].iter().all(|v| v.is_finite()) {
        return None;
    }
    let (x0, y0) = (r.x.max(0.0), r.y.max(0.0));
    let (x1, y1) = ((r.x + r.w).min(w), (r.y + r.h).min(h));
    (x1 > x0 && y1 > y0).then_some(Rect {
        x: x0,
        y: y0,
        w: x1 - x0,
        h: y1 - y0,
    })
}

/// Window-local `local` (points) on `d` -> global points and physical pixels. Edges are rounded as
/// edges (not as sizes) so adjacent areas never overlap by a pixel. `None` when smaller than [`MIN_EDGE`].
pub fn resolve(d: &AreaDisplay, local: Rect) -> Option<AreaPick> {
    let l = clamp(local, d.points.w, d.points.h)?;
    if l.w < MIN_EDGE || l.h < MIN_EDGE || !(d.scale.is_finite() && d.scale > 0.0) {
        return None;
    }
    let edge = |o: f64, v: f64| (o + v * d.scale).round();
    let (px0, py0) = (edge(d.px_origin.0, l.x), edge(d.px_origin.1, l.y));
    let (px1, py1) = (edge(d.px_origin.0, l.x + l.w), edge(d.px_origin.1, l.y + l.h));
    let pixels = Rect {
        x: px0,
        y: py0,
        w: px1 - px0,
        h: py1 - py0,
    };
    (pixels.w >= MIN_EDGE && pixels.h >= MIN_EDGE).then_some(AreaPick {
        points: Rect {
            x: d.points.x + l.x,
            y: d.points.y + l.y,
            w: l.w,
            h: l.h,
        },
        pixels,
    })
}

/// Index of the display holding the pointer (`cursor` in the grab's native unit: points on macOS,
/// pixels on Windows, see `native`); the first display when the pointer is on none; `None` without displays.
pub fn display_at(displays: &[AreaDisplay], cursor: (f64, f64), native_px: bool) -> Option<usize> {
    let hit = displays.iter().position(|d| {
        let f = native(d, native_px);
        cursor.0 >= f.x && cursor.0 < f.x + f.w && cursor.1 >= f.y && cursor.1 < f.y + f.h
    });
    hit.or(if displays.is_empty() { None } else { Some(0) })
}

/// The display frame in the unit the platform grabs and reports the pointer in.
pub fn native(d: &AreaDisplay, px: bool) -> Rect {
    if px {
        Rect {
            x: d.px_origin.0,
            y: d.px_origin.1,
            w: d.points.w * d.scale,
            h: d.points.h * d.scale,
        }
    } else {
        d.points
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(x: f64, y: f64, w: f64, h: f64) -> Rect {
        Rect { x, y, w, h }
    }
    fn disp(x: f64, y: f64, w: f64, h: f64, scale: f64) -> AreaDisplay {
        AreaDisplay {
            points: r(x, y, w, h),
            scale,
            px_origin: (x * scale, y * scale),
        }
    }

    #[test]
    fn drag_in_any_direction_normalises() {
        let want = r(10.0, 20.0, 30.0, 40.0);
        assert_eq!(normalize((10.0, 20.0), (40.0, 60.0)), want);
        assert_eq!(normalize((40.0, 60.0), (10.0, 20.0)), want);
        assert_eq!(normalize((40.0, 20.0), (10.0, 60.0)), want);
        assert_eq!(normalize((10.0, 60.0), (40.0, 20.0)), want);
    }

    #[test]
    fn under_five_px_is_a_click() {
        assert!(is_click((100.0, 100.0), (104.0, 103.0)));
        assert!(is_click((100.0, 100.0), (100.0, 100.0)));
        assert!(!is_click((100.0, 100.0), (105.0, 100.0)));
        assert!(!is_click((100.0, 100.0), (100.0, 94.0)));
    }

    #[test]
    fn clamps_to_the_display() {
        assert_eq!(
            clamp(r(-20.0, -5.0, 100.0, 50.0), 800.0, 600.0),
            Some(r(0.0, 0.0, 80.0, 45.0))
        );
        assert_eq!(
            clamp(r(700.0, 500.0, 400.0, 400.0), 800.0, 600.0),
            Some(r(700.0, 500.0, 100.0, 100.0))
        );
        assert_eq!(
            clamp(r(900.0, 10.0, 50.0, 50.0), 800.0, 600.0),
            None,
            "fully outside"
        );
        assert_eq!(clamp(r(f64::NAN, 0.0, 5.0, 5.0), 800.0, 600.0), None);
    }

    #[test]
    fn retina_points_become_pixels() {
        let d = disp(0.0, 0.0, 1440.0, 900.0, 2.0);
        let p = resolve(&d, r(100.0, 50.0, 300.5, 200.0)).unwrap();
        assert_eq!(p.points, r(100.0, 50.0, 300.5, 200.0));
        assert_eq!(p.pixels, r(200.0, 100.0, 601.0, 400.0));
    }

    #[test]
    fn negative_origin_display_to_the_left_and_above() {
        // a 1920x1080 @1x monitor left of the primary and a 1440x900 @2x one above it
        let left = disp(-1920.0, 100.0, 1920.0, 1080.0, 1.0);
        let p = resolve(&left, r(10.0, 20.0, 300.0, 200.0)).unwrap();
        assert_eq!(p.points, r(-1910.0, 120.0, 300.0, 200.0));
        assert_eq!(p.pixels, r(-1910.0, 120.0, 300.0, 200.0));
        let above = disp(0.0, -900.0, 1440.0, 900.0, 2.0);
        let p = resolve(&above, r(5.0, 5.0, 100.0, 100.0)).unwrap();
        assert_eq!(p.points, r(5.0, -895.0, 100.0, 100.0));
        assert_eq!(p.pixels, r(10.0, -1790.0, 200.0, 200.0));
    }

    #[test]
    fn selection_past_the_edge_is_cut_not_shifted() {
        let d = disp(-1920.0, 0.0, 1920.0, 1080.0, 1.25);
        let p = resolve(&d, r(1800.0, 1000.0, 400.0, 400.0)).unwrap();
        assert_eq!(p.points, r(-120.0, 1000.0, 120.0, 80.0));
        assert_eq!(p.pixels.w, 150.0);
        assert_eq!(p.pixels.h, 100.0);
    }

    #[test]
    fn fractional_scale_rounds_edges_not_sizes() {
        let d = disp(0.0, 0.0, 1536.0, 864.0, 1.25);
        let a = resolve(&d, r(0.0, 0.0, 10.3, 10.3)).unwrap();
        let b = resolve(&d, r(10.3, 0.0, 10.3, 10.3)).unwrap();
        assert_eq!(
            a.pixels.x + a.pixels.w,
            b.pixels.x,
            "neighbours share the edge pixel"
        );
    }

    #[test]
    fn too_small_is_refused() {
        let d = disp(0.0, 0.0, 800.0, 600.0, 2.0);
        assert!(resolve(&d, r(10.0, 10.0, 3.0, 100.0)).is_none());
        assert!(resolve(&d, r(10.0, 10.0, 100.0, 3.9)).is_none());
        assert!(resolve(&d, r(10.0, 10.0, 4.0, 4.0)).is_some());
    }

    #[test]
    fn display_under_the_pointer_with_negative_origins() {
        let ds = [
            disp(0.0, 0.0, 1440.0, 900.0, 2.0),
            disp(-1920.0, -200.0, 1920.0, 1080.0, 1.0),
            disp(1440.0, 0.0, 2560.0, 1440.0, 1.0),
        ];
        assert_eq!(display_at(&ds, (-5.0, 10.0), false), Some(1));
        assert_eq!(display_at(&ds, (700.0, 400.0), false), Some(0));
        assert_eq!(display_at(&ds, (3000.0, 1400.0), false), Some(2));
        assert_eq!(
            display_at(&ds, (3000.0, 2000.0), false),
            Some(0),
            "off every display: the first"
        );
        assert_eq!(display_at(&[], (0.0, 0.0), false), None);
        // Windows: the pointer is in physical pixels (the 2x primary spans 2880 px)
        assert_eq!(display_at(&ds, (2000.0, 100.0), true), Some(0));
        assert_eq!(display_at(&ds, (-100.0, 100.0), true), Some(1));
    }
}
