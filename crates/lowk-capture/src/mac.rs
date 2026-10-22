//! macOS capture over ScreenCaptureKit (`SCScreenshotManager`, one-shot; never a
//! persistent `SCStream`, so no recording indicator stays alive).
//! Needs Screen Recording; callers gate on `lowk-platform::perm` first.
use lowk_types::{AppError, ErrorCode, Rect, RegionSampler, Result, ScreenGrab};
use screencapturekit::prelude::*;
use screencapturekit::screenshot_manager::{CGImageExt, SCScreenshotManager};

/// QA only: the title of one of OUR windows that the tone sampler must still see (a backdrop window drawn by the qa
/// build itself). Never set outside the `qa` feature, so normal builds exclude every window of the process.
static KEEP_TITLE: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

/// Trace of the last tone-sampler grab (the sampler loop swallows errors; QA reads this).
static LAST: std::sync::Mutex<Option<crate::geometry::GrabDiag>> = std::sync::Mutex::new(None);

/// JSON of the last sampler grab, `null` before the first.
pub fn last_grab_json() -> String {
    LAST.lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .map_or("null".into(), crate::geometry::GrabDiag::json)
}

pub fn keep_window_titled(title: Option<String>) {
    *KEEP_TITLE.lock().unwrap_or_else(|e| e.into_inner()) = title;
}

fn fail(what: &str, e: impl std::fmt::Display) -> AppError {
    AppError::new(ErrorCode::PermScreenRecording).with_detail(format!("{what}: {e}"))
}

/// Grab `rect` (global top-left points) as RGBA8, `out` pixels when given
/// (nearest-neighbour fit), else at native size. `exclude_own` removes every
/// window of this process from the picture.
fn grab(rect: Rect, out: Option<(u32, u32)>, exclude_own: bool) -> Result<(Vec<u8>, u32, u32)> {
    grab_with(rect, out, exclude_own, false)
}

thread_local! {
    /// The tone sampler runs at ~4 Hz; enumerating shareable content on every frame costs more than the
    /// capture itself, so the sampler thread reuses it for `CONTENT_TTL` (windows of ours come and go).
    static CONTENT: std::cell::RefCell<Option<(std::time::Instant, SCShareableContent)>> = const { std::cell::RefCell::new(None) };
}
const CONTENT_TTL: std::time::Duration = std::time::Duration::from_millis(1500);

fn content(cached: bool) -> Result<SCShareableContent> {
    if cached {
        let hit = CONTENT.with(|c| {
            c.borrow()
                .as_ref()
                .filter(|(t, _)| t.elapsed() < CONTENT_TTL)
                .map(|(_, c)| c.clone())
        });
        if let Some(c) = hit {
            return Ok(c);
        }
    }
    let c = SCShareableContent::get().map_err(|e| fail("shareable content", e))?;
    if cached {
        CONTENT.with(|slot| *slot.borrow_mut() = Some((std::time::Instant::now(), c.clone())));
    }
    Ok(c)
}

/// The tone sampler asks ScreenCaptureKit for this many times the output size and box-averages down: a 24x18 request
/// for a 600x384 pt window is a ~25x reduction that SCK may answer with garbage/black, and nearest-neighbour picking
/// of 432 source pixels is noisy.
const OVERSAMPLE: u32 = 8;

fn grab_with(
    rect: Rect,
    out: Option<(u32, u32)>,
    exclude_own: bool,
    cached: bool,
) -> Result<(Vec<u8>, u32, u32)> {
    let mut diag = crate::geometry::GrabDiag {
        path: if cached {
            "sampler(cached,oversampled)"
        } else {
            "grab"
        },
        ..Default::default()
    };
    let r = grab_inner(rect, out, exclude_own, cached, &mut diag);
    if cached {
        if let Err(e) = &r {
            diag.error = Some(format!("{e:?}"));
        }
        *LAST.lock().unwrap_or_else(|e| e.into_inner()) = Some(diag);
    }
    r
}

fn grab_inner(
    rect: Rect,
    out: Option<(u32, u32)>,
    exclude_own: bool,
    cached: bool,
    diag: &mut crate::geometry::GrabDiag,
) -> Result<(Vec<u8>, u32, u32)> {
    let content = content(cached)?;
    let displays = content.displays();
    let frames: Vec<Rect> = displays
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
    // Pure geometry (unit-tested): display with most of the window, source rect relative to it, proportional placement.
    let (nat_w, nat_h) = out.unwrap_or((rect.w.max(1.0) as u32, rect.h.max(1.0) as u32));
    let plan = crate::geometry::plan(&rect, &frames, nat_w, nat_h)
        .ok_or_else(|| AppError::new(ErrorCode::NotFound).with_detail("no display"))?;
    let display = &displays[plan.display];
    let own = std::process::id() as i32;
    let windows = content.windows();
    let keep = KEEP_TITLE.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let skip: Vec<&SCWindow> = if exclude_own {
        windows
            .iter()
            .filter(|w| w.owning_application().is_some_and(|a| a.process_id() == own))
            .filter(|w| keep.as_deref().is_none_or(|k| w.title().as_deref() != Some(k)))
            .collect()
    } else {
        vec![]
    };
    diag.display = plan.display;
    diag.src_pts = Some(plan.src);
    diag.own_excluded = skip.len();
    diag.own_kept = windows
        .iter()
        .filter(|w| w.owning_application().is_some_and(|a| a.process_id() == own))
        .count()
        - skip.len();
    let filter = SCContentFilter::create()
        .with_display(display)
        .with_excluding_windows(&skip)
        .build()
        .map_err(|e| fail("filter", e))?;
    let src = CGRect::new(plan.src.x, plan.src.y, plan.src.w, plan.src.h);
    let (cw, ch) = (plan.cap_w, plan.cap_h);
    // The sampler first asks for an oversampled image; if that comes back entirely black (SCK answering a tiny/odd
    // request with an empty frame) it asks once more at the plain size before believing it.
    let scales: &[u32] = if cached { &[OVERSAMPLE, 1] } else { &[1] };
    let mut px = Vec::new();
    for (n, &ss) in scales.iter().enumerate() {
        diag.requested_px = (cw * ss, ch * ss);
        let cfg = SCStreamConfiguration::new()
            .with_width(cw * ss)
            .with_height(ch * ss)
            .with_source_rect(src)
            .with_shows_cursor(false);
        let img = SCScreenshotManager::capture_image(&filter, &cfg).map_err(|e| fail("capture", e))?;
        let (iw, ih) = (img.width() as u32, img.height() as u32);
        let mut raw = img.rgba_data().map_err(|e| fail("pixels", e))?;
        if raw.len() != (iw * ih * 4) as usize {
            // Row-padded buffer: the stride is whatever divides evenly.
            let bpr = if ih > 0 { raw.len() / ih as usize } else { 0 };
            raw = crate::geometry::unpad_rows(&raw, iw as usize, ih as usize, bpr).ok_or_else(|| {
                AppError::new(ErrorCode::Unknown).with_detail(format!(
                    "unexpected pixel buffer size {} for {iw}x{ih}",
                    raw.len()
                ))
            })?;
        }
        diag.image_px = (iw, ih);
        diag.attempts = n as u32 + 1;
        px = if (iw, ih) == (cw, ch) {
            raw
        } else {
            crate::geometry::box_down(&raw, iw, ih, cw, ch)
        };
        if px.chunks_exact(4).any(|p| p[..3] != [0, 0, 0]) {
            break;
        }
    }
    diag.out_px = (plan.out_w, plan.out_h);
    diag.mean_rgb = crate::geometry::mean_rgb(&px);
    match out {
        Some(_) => Ok((crate::geometry::compose(&plan, &px), plan.out_w, plan.out_h)),
        None => Ok((px, cw, ch)),
    }
}

/// Adaptive-tone sampler: our own windows are excluded explicitly (not relying
/// on `sharingType`), so the overlay never samples itself.
#[derive(Default, Clone, Copy)]
pub struct SckSampler;

impl RegionSampler for SckSampler {
    fn grab_region(&self, rect: Rect, w: u32, h: u32) -> Result<Vec<u8>> {
        grab_with(rect, Some((w, h)), true, true).map(|(px, ..)| px)
    }
}

/// QA: `rect` (global top-left points) as the person sees it (nothing excluded) at `scale` pixels per point.
pub fn grab_scaled(rect: Rect, scale: f64) -> Result<(Vec<u8>, u32, u32)> {
    let (w, h) = (
        ((rect.w * scale).round() as u32).max(1),
        ((rect.h * scale).round() as u32).max(1),
    );
    grab(rect, Some((w, h)), false)
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
