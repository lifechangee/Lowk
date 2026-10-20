//! Hold-to-move magnet geometry: pure functions, global top-left coordinates.
//!
//! On hold-down the grab offset `origin - cursor` is recorded (no jump); on
//! every tick `origin = cursor + grab`, clamped to the visible frame of the
//! display under the cursor. No click, no cursor change: the window stays
//! click-through, so the cursor shape always comes from whatever is below it.

use lowk_types::Rect;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Magnet {
    grab: (f64, f64),
    size: (f64, f64),
}

impl Magnet {
    pub fn begin(origin: (f64, f64), size: (f64, f64), cursor: (f64, f64)) -> Self {
        Self {
            grab: (origin.0 - cursor.0, origin.1 - cursor.1),
            size,
        }
    }

    /// New window origin for the cursor position, clamped to `displays`.
    pub fn target(&self, cursor: (f64, f64), displays: &[Rect]) -> (f64, f64) {
        clamp_origin(
            (cursor.0 + self.grab.0, cursor.1 + self.grab.1),
            self.size,
            cursor,
            displays,
        )
    }
}

fn contains(r: &Rect, p: (f64, f64)) -> bool {
    p.0 >= r.x && p.0 < r.x + r.w && p.1 >= r.y && p.1 < r.y + r.h
}

fn dist2(r: &Rect, p: (f64, f64)) -> f64 {
    let dx = (r.x - p.0).max(0.0).max(p.0 - (r.x + r.w));
    let dy = (r.y - p.1).max(0.0).max(p.1 - (r.y + r.h));
    dx * dx + dy * dy
}

/// The display containing `p`, else the nearest one.
pub fn display_for(p: (f64, f64), displays: &[Rect]) -> Option<Rect> {
    displays
        .iter()
        .find(|d| contains(d, p))
        .or_else(|| displays.iter().min_by(|a, b| dist2(a, p).total_cmp(&dist2(b, p))))
        .copied()
}

/// Stable per-display key (`x,y,WxH` of the visible frame) for remembered positions.
pub fn display_key(d: &Rect) -> String {
    format!("{:.0},{:.0},{:.0}x{:.0}", d.x, d.y, d.w, d.h)
}

/// Keep a `size` window fully inside the visible frame of the display under
/// `cursor`. With no displays the origin is returned unchanged. A window larger
/// than the display is pinned to the display's top-left.
pub fn clamp_origin(
    origin: (f64, f64),
    size: (f64, f64),
    cursor: (f64, f64),
    displays: &[Rect],
) -> (f64, f64) {
    let Some(d) = display_for(cursor, displays) else {
        return origin;
    };
    let max_x = (d.x + d.w - size.0).max(d.x);
    let max_y = (d.y + d.h - size.1).max(d.y);
    (origin.0.clamp(d.x, max_x), origin.1.clamp(d.y, max_y))
}

/// Convert a rect from a bottom-left-origin space (AppKit) to top-left, given
/// the height of the primary display. The single place where the flip lives.
pub fn flip_y(r: Rect, primary_height: f64) -> Rect {
    Rect {
        x: r.x,
        y: primary_height - (r.y + r.h),
        w: r.w,
        h: r.h,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(x: f64, y: f64, w: f64, h: f64) -> Rect {
        Rect { x, y, w, h }
    }

    #[test]
    fn no_jump_on_grab() {
        let m = Magnet::begin((100.0, 200.0), (360.0, 160.0), (150.0, 230.0));
        let d = [r(0.0, 0.0, 1920.0, 1080.0)];
        assert_eq!(m.target((150.0, 230.0), &d), (100.0, 200.0));
        assert_eq!(m.target((200.0, 300.0), &d), (150.0, 270.0));
    }

    #[test]
    fn clamps_to_display() {
        let m = Magnet::begin((100.0, 100.0), (360.0, 160.0), (100.0, 100.0));
        let d = [r(0.0, 0.0, 1000.0, 800.0)];
        assert_eq!(m.target((-500.0, -500.0), &d), (0.0, 0.0));
        assert_eq!(m.target((999.0, 799.0), &d), (640.0, 640.0));
    }

    #[test]
    fn follows_cursor_across_displays() {
        let m = Magnet::begin((10.0, 10.0), (100.0, 50.0), (10.0, 10.0));
        let d = [r(0.0, 0.0, 1000.0, 800.0), r(1000.0, 0.0, 1000.0, 800.0)];
        let t = m.target((1500.0, 100.0), &d);
        assert_eq!(t, (1500.0, 100.0));
        // cursor on the right display: the window cannot straddle back onto the left one
        let t = m.target((1005.0, 100.0), &d);
        assert!(t.0 >= 1000.0);
    }

    #[test]
    fn nearest_display_when_cursor_is_outside() {
        let d = [r(0.0, 0.0, 100.0, 100.0), r(500.0, 0.0, 100.0, 100.0)];
        assert_eq!(display_for((450.0, 50.0), &d).unwrap().x, 500.0);
        assert_eq!(display_for((-10.0, 50.0), &d).unwrap().x, 0.0);
        assert!(display_for((0.0, 0.0), &[]).is_none());
    }

    #[test]
    fn oversize_window_pins_top_left_and_no_displays_passthrough() {
        let d = [r(0.0, 0.0, 100.0, 100.0)];
        assert_eq!(
            clamp_origin((50.0, 50.0), (200.0, 200.0), (10.0, 10.0), &d),
            (0.0, 0.0)
        );
        assert_eq!(
            clamp_origin((50.0, 60.0), (10.0, 10.0), (10.0, 10.0), &[]),
            (50.0, 60.0)
        );
    }

    #[test]
    fn display_key_is_stable_and_distinct() {
        let a = r(0.0, 25.0, 1512.0, 957.0);
        assert_eq!(display_key(&a), "0,25,1512x957");
        assert_ne!(display_key(&a), display_key(&r(1512.0, 0.0, 1920.0, 1080.0)));
    }

    #[test]
    fn flip_is_involutive() {
        let a = r(10.0, 20.0, 300.0, 100.0);
        let f = flip_y(a, 900.0);
        assert_eq!(f, r(10.0, 780.0, 300.0, 100.0));
        assert_eq!(flip_y(f, 900.0), a);
    }
}
