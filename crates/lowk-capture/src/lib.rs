//! Screenshots and region grabs. `capture_all` (G7) grabs every monitor with our own windows
//! excluded and encodes PNG + a small thumbnail; the region grabs feed the adaptive tone.
use lowk_types::{AppError, CaptureMonitors, ErrorCode, Rect, RegionSampler, Result};

pub mod frames;
pub mod geometry;
#[cfg(target_os = "macos")]
mod mac;
pub mod prep;
#[cfg(target_os = "macos")]
pub use mac::{grab_scaled, keep_window_titled, last_grab_json, SckGrab, SckSampler};
#[cfg(windows)]
mod win;

/// Raw RGBA of one display (platform backends produce these).
pub struct DisplayShot {
    pub primary: bool,
    pub rgba: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

/// One captured monitor, encoded.
#[derive(Debug, Clone)]
pub struct CapturedShot {
    /// Unique per process (assigned by the queue); `0` until queued.
    pub id: u64,
    pub index: u32,
    pub primary: bool,
    pub width: u32,
    pub height: u32,
    /// PNG bytes (long edge <= [`MAX_EDGE`]).
    pub png: std::sync::Arc<Vec<u8>>,
    /// `data:image/png;base64,...`, <= [`THUMB_W`] px wide.
    pub thumb: String,
}

pub const MAX_EDGE: u32 = 2048;
pub const THUMB_W: u32 = 160;

/// Which displays a grab covers (pure; unit-tested). `displays` are the monitor frames in global
/// top-left coordinates, `focus` the frame of the focused window (not ours), `mouse` the pointer.
/// * `All`: every display. * `Primary`: the one at the origin (else the first).
/// * `Active`: the display holding most of the focused window, else the one under the pointer, else the primary.
pub fn select_displays(
    displays: &[Rect],
    mode: CaptureMonitors,
    focus: Option<Rect>,
    mouse: Option<(f64, f64)>,
) -> Vec<usize> {
    if displays.is_empty() {
        return vec![];
    }
    let primary = displays
        .iter()
        .position(|d| d.x == 0.0 && d.y == 0.0)
        .unwrap_or(0);
    match mode {
        CaptureMonitors::All => (0..displays.len()).collect(),
        CaptureMonitors::Primary => vec![primary],
        CaptureMonitors::Active => {
            if let Some(i) = focus.and_then(|f| geometry::pick_display(&f, displays)) {
                return vec![i];
            }
            if let Some((mx, my)) = mouse {
                if let Some(i) = displays
                    .iter()
                    .position(|d| mx >= d.x && mx < d.x + d.w && my >= d.y && my < d.y + d.h)
                {
                    return vec![i];
                }
            }
            vec![primary]
        }
    }
}

/// Screenshots of the displays picked by `mode`, PrtScr style for `All`, with our own windows
/// excluded. `own_windows` are raw native handles of this app's windows (HWND values) that a
/// Windows grab must exclude; ignored on macOS, where every window of this process is excluded
/// through ScreenCaptureKit. `mouse` is the pointer in global top-left coordinates.
pub fn capture_all(
    own_windows: &[isize],
    mode: CaptureMonitors,
    mouse: Option<(f64, f64)>,
) -> Result<Vec<CapturedShot>> {
    #[cfg(target_os = "macos")]
    let raw = {
        let _ = own_windows;
        mac::capture_displays(MAX_EDGE, mode, mouse)?
    };
    #[cfg(windows)]
    let raw = win::capture_displays(MAX_EDGE, own_windows, mode, mouse)?;
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        let _ = (own_windows, mode, mouse);
        Err(AppError::new(ErrorCode::Unsupported))
    }
    #[cfg(any(target_os = "macos", windows))]
    {
        raw.into_iter()
            .enumerate()
            .map(|(i, d)| encode_shot(i as u32, d))
            .collect()
    }
}

/// One rectangle of the screen (global top-left points / pixels), e.g. from the area-selection overlay.
pub fn capture_area(own_windows: &[isize], rect: Rect) -> Result<CapturedShot> {
    if rect.w < 4.0 || rect.h < 4.0 {
        return Err(AppError::new(ErrorCode::InvalidArgument).with_detail("area too small"));
    }
    #[cfg(target_os = "macos")]
    let raw = {
        let _ = own_windows;
        mac::capture_rect(rect, MAX_EDGE)?
    };
    #[cfg(windows)]
    let raw = win::capture_area(rect, MAX_EDGE, own_windows)?;
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        let _ = own_windows;
        Err(AppError::new(ErrorCode::Unsupported))
    }
    #[cfg(any(target_os = "macos", windows))]
    {
        encode_shot(0, raw)
    }
}

pub fn encode_shot(index: u32, d: DisplayShot) -> Result<CapturedShot> {
    let png = encode_png(&d.rgba, d.width, d.height)?;
    let tw = THUMB_W.min(d.width).max(1);
    let th = ((u64::from(d.height) * u64::from(tw) / u64::from(d.width.max(1))) as u32).max(1);
    let small = fit(&d.rgba, d.width, d.height, tw, th);
    let thumb = format!("data:image/png;base64,{}", base64(&encode_png(&small, tw, th)?));
    Ok(CapturedShot {
        id: 0,
        index,
        primary: d.primary,
        width: d.width,
        height: d.height,
        png: std::sync::Arc::new(png),
        thumb,
    })
}

fn fit(px: &[u8], iw: u32, ih: u32, ow: u32, oh: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity((ow * oh * 4) as usize);
    for y in 0..oh {
        for x in 0..ow {
            let (sx, sy) = (x * iw / ow, y * ih / oh);
            let i = ((sy * iw + sx) * 4) as usize;
            out.extend_from_slice(&px[i..i + 4]);
        }
    }
    out
}

pub fn encode_png(rgba: &[u8], w: u32, h: u32) -> Result<Vec<u8>> {
    let bad = |e: png::EncodingError| AppError::new(ErrorCode::Unknown).with_detail(format!("png: {e}"));
    let mut out = Vec::new();
    let mut enc = png::Encoder::new(&mut out, w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.set_compression(png::Compression::Fast);
    let mut wr = enc.write_header().map_err(bad)?;
    wr.write_image_data(rgba).map_err(bad)?;
    wr.finish().map_err(bad)?;
    Ok(out)
}

/// Standard base64 (RFC 4648) with padding.
pub fn base64(b: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::with_capacity(b.len().div_ceil(3) * 4);
    for c in b.chunks(3) {
        let n = (u32::from(c[0]) << 16)
            | (u32::from(*c.get(1).unwrap_or(&0)) << 8)
            | u32::from(*c.get(2).unwrap_or(&0));
        s.push(T[(n >> 18) as usize & 63] as char);
        s.push(T[(n >> 12) as usize & 63] as char);
        s.push(if c.len() > 1 {
            T[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        s.push(if c.len() > 2 {
            T[n as usize & 63] as char
        } else {
            '='
        });
    }
    s
}

#[derive(Debug, Clone)]
pub struct MonitorShot {
    pub index: u32,
    pub primary: bool,
    pub width: u32,
    pub height: u32,
    /// Encoded JPEG/PNG bytes.
    pub data: Vec<u8>,
}

pub trait Screenshots: Send + Sync {
    fn screenshot_all(&self) -> Result<Vec<MonitorShot>>;
    fn screenshot_area(&self, rect: Rect) -> Result<MonitorShot>;
}

#[derive(Default)]
pub struct NoopCapture;
impl Screenshots for NoopCapture {
    fn screenshot_all(&self) -> Result<Vec<MonitorShot>> {
        Ok(vec![])
    }
    fn screenshot_area(&self, _rect: Rect) -> Result<MonitorShot> {
        Err(AppError::not_implemented())
    }
}
impl RegionSampler for NoopCapture {
    fn grab_region(&self, _rect: Rect, w: u32, h: u32) -> Result<Vec<u8>> {
        Ok(vec![0; (w * h * 4) as usize])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(x: f64, y: f64, w: f64, h: f64) -> Rect {
        Rect { x, y, w, h }
    }

    #[test]
    fn display_selection() {
        // primary at the origin, a second monitor to the right, a third to the left (negative x)
        let d = [
            r(1920.0, 0.0, 1920.0, 1080.0),
            r(0.0, 0.0, 1920.0, 1080.0),
            r(-1280.0, 0.0, 1280.0, 1024.0),
        ];
        let m = CaptureMonitors::Active;
        assert_eq!(
            select_displays(&d, CaptureMonitors::All, None, None),
            vec![0, 1, 2]
        );
        assert_eq!(select_displays(&d, CaptureMonitors::Primary, None, None), vec![1]);
        // focused window mostly on the left monitor (straddles the border)
        assert_eq!(
            select_displays(&d, m, Some(r(-900.0, 100.0, 1000.0, 600.0)), Some((1000.0, 10.0))),
            vec![2]
        );
        // no focused window: the pointer decides
        assert_eq!(select_displays(&d, m, None, Some((2500.0, 500.0))), vec![0]);
        // focus off every monitor falls through to the pointer, then to the primary
        assert_eq!(
            select_displays(&d, m, Some(r(9000.0, 9000.0, 10.0, 10.0)), Some((5.0, 5.0))),
            vec![1]
        );
        assert_eq!(select_displays(&d, m, None, None), vec![1]);
        assert_eq!(select_displays(&d, m, None, Some((-5000.0, 0.0))), vec![1]);
        assert!(select_displays(&[], m, None, None).is_empty());
        // single monitor not at the origin: primary falls back to the first
        assert_eq!(
            select_displays(
                &[r(100.0, 0.0, 800.0, 600.0)],
                CaptureMonitors::Primary,
                None,
                None
            ),
            vec![0]
        );
    }

    #[test]
    fn base64_rfc_vectors() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn shot_encodes_png_and_thumbnail() {
        let rgba = vec![200u8; 640 * 400 * 4];
        let s = encode_shot(
            0,
            DisplayShot {
                primary: true,
                rgba,
                width: 640,
                height: 400,
            },
        )
        .unwrap();
        assert_eq!(&s.png[1..4], b"PNG");
        assert!(s.thumb.starts_with("data:image/png;base64,"));
        assert_eq!((s.width, s.height), (640, 400));
    }
}
