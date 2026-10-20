//! macOS capture over ScreenCaptureKit (`SCScreenshotManager`, one-shot; never a
//! persistent `SCStream`, so no recording indicator stays alive).
//! Needs Screen Recording; callers gate on `lowk-platform::perm` first.
use lowk_types::{AppError, ErrorCode, Rect, RegionSampler, Result, ScreenGrab};
use screencapturekit::prelude::*;
use screencapturekit::screenshot_manager::{CGImageExt, SCScreenshotManager};

fn fail(what: &str, e: impl std::fmt::Display) -> AppError {
    AppError::new(ErrorCode::PermScreenRecording).with_detail(format!("{what}: {e}"))
}

/// Grab `rect` (global top-left points) as RGBA8, `out` pixels when given
/// (nearest-neighbour fit), else at native size. `exclude_own` removes every
/// window of this process from the picture.
fn grab(rect: Rect, out: Option<(u32, u32)>, exclude_own: bool) -> Result<(Vec<u8>, u32, u32)> {
    let content = SCShareableContent::get().map_err(|e| fail("shareable content", e))?;
    let displays = content.displays();
    let (cx, cy) = (rect.x + rect.w / 2.0, rect.y + rect.h / 2.0);
    let display = displays
        .iter()
        .find(|d| {
            let f = d.frame();
            cx >= f.origin.x
                && cx < f.origin.x + f.size.width
                && cy >= f.origin.y
                && cy < f.origin.y + f.size.height
        })
        .or_else(|| displays.first())
        .ok_or_else(|| AppError::new(ErrorCode::NotFound).with_detail("no display"))?;
    let own = std::process::id() as i32;
    let windows = content.windows();
    let skip: Vec<&SCWindow> = if exclude_own {
        windows
            .iter()
            .filter(|w| w.owning_application().is_some_and(|a| a.process_id() == own))
            .collect()
    } else {
        vec![]
    };
    let filter = SCContentFilter::create()
        .with_display(display)
        .with_excluding_windows(&skip)
        .build()
        .map_err(|e| fail("filter", e))?;
    let f = display.frame();
    let (w, h) = out.unwrap_or((rect.w.max(1.0) as u32, rect.h.max(1.0) as u32));
    let src = CGRect::new(
        rect.x - f.origin.x,
        rect.y - f.origin.y,
        rect.w.max(1.0),
        rect.h.max(1.0),
    );
    let cfg = SCStreamConfiguration::new()
        .with_width(w)
        .with_height(h)
        .with_source_rect(src)
        .with_shows_cursor(false);
    let img = SCScreenshotManager::capture_image(&filter, &cfg).map_err(|e| fail("capture", e))?;
    let (iw, ih) = (img.width() as u32, img.height() as u32);
    let px = img.rgba_data().map_err(|e| fail("pixels", e))?;
    if px.len() != (iw * ih * 4) as usize {
        return Err(AppError::new(ErrorCode::Unknown).with_detail("unexpected pixel buffer size"));
    }
    match out {
        Some((ow, oh)) if (iw, ih) != (ow, oh) => Ok((fit(&px, iw, ih, ow, oh), ow, oh)),
        _ => Ok((px, iw, ih)),
    }
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

/// Adaptive-tone sampler: our own windows are excluded explicitly (not relying
/// on `sharingType`), so the overlay never samples itself.
#[derive(Default, Clone, Copy)]
pub struct SckSampler;

impl RegionSampler for SckSampler {
    fn grab_region(&self, rect: Rect, w: u32, h: u32) -> Result<Vec<u8>> {
        grab(rect, Some((w, h)), true).map(|(px, ..)| px)
    }
}

/// What a normal capturer sees (no exclusion): the stealth self-test pixel layer.
#[derive(Default, Clone, Copy)]
pub struct SckGrab;

impl ScreenGrab for SckGrab {
    fn grab(&self, rect: Rect) -> Result<(Vec<u8>, u32, u32)> {
        grab(rect, None, false)
    }
}

/// G7: every display, our own windows (main, overlay, pill) excluded explicitly by `grab`.
/// The long edge is capped at `max_edge` pixels (never upscaled).
pub fn capture_displays(max_edge: u32) -> Result<Vec<crate::DisplayShot>> {
    let content = SCShareableContent::get().map_err(|e| fail("shareable content", e))?;
    let frames: Vec<Rect> = content
        .displays()
        .iter()
        .map(|d| {
            let f = d.frame();
            Rect {
                x: f.origin.x,
                y: f.origin.y,
                w: f.size.width,
                h: f.size.height,
            }
        })
        .collect();
    let mut out = Vec::with_capacity(frames.len());
    for r in frames {
        let long = r.w.max(r.h).max(1.0);
        let k = (f64::from(max_edge) / long).min(1.0);
        let (w, h) = (((r.w * k) as u32).max(1), ((r.h * k) as u32).max(1));
        let (rgba, w, h) = grab(r, Some((w, h)), true)?;
        out.push(crate::DisplayShot {
            primary: r.x == 0.0 && r.y == 0.0,
            rgba,
            width: w,
            height: h,
        });
    }
    Ok(out)
}
