//! Screenshots and region grabs. `capture_all` (G7) grabs every monitor with our own windows
//! excluded and encodes PNG + a small thumbnail; the region grabs feed the adaptive tone.
use lowk_types::{AppError, ErrorCode, Rect, RegionSampler, Result};

pub mod geometry;
#[cfg(target_os = "macos")]
mod mac;
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
    pub index: u32,
    pub primary: bool,
    pub width: u32,
    pub height: u32,
    /// PNG bytes (long edge <= [`MAX_EDGE`]).
    pub png: Vec<u8>,
    /// `data:image/png;base64,...`, <= [`THUMB_W`] px wide.
    pub thumb: String,
}

pub const MAX_EDGE: u32 = 2048;
pub const THUMB_W: u32 = 160;

/// All monitors, PrtScr style. `own_windows` are raw native handles of this app's windows
/// (HWND values) that a Windows grab must exclude; ignored on macOS, where every window of
/// this process is excluded through ScreenCaptureKit.
pub fn capture_all(own_windows: &[isize]) -> Result<Vec<CapturedShot>> {
    #[cfg(target_os = "macos")]
    let raw = {
        let _ = own_windows;
        mac::capture_displays(MAX_EDGE)?
    };
    #[cfg(windows)]
    let raw = win::capture_displays(MAX_EDGE, own_windows)?;
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        let _ = own_windows;
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

pub fn encode_shot(index: u32, d: DisplayShot) -> Result<CapturedShot> {
    let png = encode_png(&d.rgba, d.width, d.height)?;
    let tw = THUMB_W.min(d.width).max(1);
    let th = ((u64::from(d.height) * u64::from(tw) / u64::from(d.width.max(1))) as u32).max(1);
    let small = fit(&d.rgba, d.width, d.height, tw, th);
    let thumb = format!("data:image/png;base64,{}", base64(&encode_png(&small, tw, th)?));
    Ok(CapturedShot {
        index,
        primary: d.primary,
        width: d.width,
        height: d.height,
        png,
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
