//! macOS capture over ScreenCaptureKit. One-shot `SCScreenshotManager` grabs for screenshots and the polled
//! fallback of the tone sampler; for the adaptive scrim also a continuous `SCStream` of the region under the overlay
//! ([`SckStream`], alive only while the scrim is on and the overlay visible; dropping it stops it).
//! Needs Screen Recording; callers gate on `lowk-platform::perm` first.
use lowk_types::{AppError, ErrorCode, Rect, RegionSampler, RegionStream, Result, ScreenGrab};
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
        // Same aspect as the source rect, else SCK letterboxes with black and the bottom tone rows come out dimmed.
        let (rw, rh) = crate::geometry::request_px(plan.src.w, plan.src.h, cw, ch, ss);
        diag.requested_px = (rw, rh);
        let cfg = SCStreamConfiguration::new()
            .with_width(rw)
            .with_height(rh)
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
        // The tone sampler (cached) builds partly / fully off-screen cells from on-screen pixels only.
        Some(_) if cached => Ok((
            crate::geometry::compose_cells(&plan, &px, crate::frames::TONE_COLS, crate::frames::TONE_ROWS),
            plan.out_w,
            plan.out_h,
        )),
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

    fn open_stream(&self, rect: Rect, w: u32, h: u32) -> Option<Box<dyn RegionStream>> {
        match SckStream::start(rect, w, h) {
            Ok(s) => Some(Box::new(s)),
            Err(e) => {
                tracing::warn!("tone: SCStream did not start ({e:?}); polling instead");
                None
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Continuous capture (thin glue: all pixel work is `frames::FrameProcessor`, unit-tested without an OS)
// ---------------------------------------------------------------------------------------------------------------

use crate::frames::{FrameProcessor, StreamGeometry};
use screencapturekit::cm::{CMSampleBufferExt, CMSampleBufferSCExt, SCFrameStatus};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

struct StreamShared {
    geo: Mutex<StreamGeometry>,
    proc: Mutex<FrameProcessor>,
    slot: Mutex<Option<Vec<u8>>>,
    cv: Condvar,
    delivered: AtomicU64,
    processed: AtomicU64,
    err: Mutex<Option<String>>,
}

impl StreamShared {
    /// Called on an SCK dispatch queue for every sample buffer.
    fn on_sample(&self, sb: &CMSampleBuffer) {
        // Idle / blank / suspended frames carry no new pixels.
        if matches!(
            sb.frame_status(),
            Some(
                SCFrameStatus::Idle
                    | SCFrameStatus::Blank
                    | SCFrameStatus::Suspended
                    | SCFrameStatus::Stopped
            )
        ) {
            return;
        }
        let Some(pb) = sb.pixel_buffer() else { return };
        let Ok(guard) = pb.lock_read_only() else { return };
        let (iw, ih, stride) = (guard.width(), guard.height(), guard.bytes_per_row());
        let base = guard.base_address();
        if base.is_null() || iw == 0 || ih == 0 || stride < iw * 4 {
            return;
        }
        self.delivered.fetch_add(1, Ordering::Relaxed);
        // SAFETY: the buffer is locked for reading for the lifetime of `guard`; it holds `ih` rows of `stride` bytes.
        let bytes = unsafe { std::slice::from_raw_parts(base, stride * ih) };
        let geo = self.geo.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let bmp = self.proc.lock().unwrap_or_else(|e| e.into_inner()).process(
            &geo,
            bytes,
            stride,
            iw as u32,
            ih as u32,
            crate::frames::TONE_COLS,
            crate::frames::TONE_ROWS,
        );
        drop(guard);
        if let Some(b) = bmp {
            self.processed.fetch_add(1, Ordering::Relaxed);
            *self.slot.lock().unwrap_or_else(|e| e.into_inner()) = Some(b);
            self.cv.notify_one();
        }
    }
}

/// One `SCStream` over the region under the overlay: own app excluded, small output at the source aspect,
/// `minimumFrameInterval` 40 ms, only changed frames reach the sampler. Follows the overlay with
/// `updateConfiguration` (sourceRect / size) and `updateContentFilter` (other display); never recreated.
pub struct SckStream {
    stream: SCStream,
    shared: Arc<StreamShared>,
    geo: StreamGeometry,
    out: (u32, u32),
    content: SCShareableContent,
    content_at: Instant,
    filter_at: Instant,
}

const CONTENT_REFRESH: Duration = Duration::from_secs(5);
/// QA only (a kept own window can appear later): the filter is rebuilt this often while `KEEP_TITLE` is set.
const FILTER_REFRESH: Duration = Duration::from_millis(1500);

fn display_frames(c: &SCShareableContent) -> Vec<Rect> {
    c.displays()
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
        .collect()
}

/// Display `display` without any window of this app (windows created later are excluded too). QA: the window
/// titled `KEEP_TITLE` stays visible.
fn stream_filter(c: &SCShareableContent, display: usize) -> Result<SCContentFilter> {
    let displays = c.displays();
    let d = displays
        .get(display)
        .ok_or_else(|| AppError::new(ErrorCode::NotFound).with_detail("no display"))?;
    let own = std::process::id() as i32;
    let apps = c.applications();
    let own_apps: Vec<&SCRunningApplication> = apps.iter().filter(|a| a.process_id() == own).collect();
    let keep = KEEP_TITLE.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let windows = c.windows();
    let kept: Vec<&SCWindow> = match keep.as_deref() {
        Some(k) => windows
            .iter()
            .filter(|w| w.owning_application().is_some_and(|a| a.process_id() == own))
            .filter(|w| w.title().as_deref() == Some(k))
            .collect(),
        None => vec![],
    };
    SCContentFilter::create()
        .with_display(d)
        .with_excluding_applications(&own_apps, &kept)
        .build()
        .map_err(|e| fail("stream filter", e))
}

fn stream_config(g: &StreamGeometry) -> SCStreamConfiguration {
    let s = g.plan.src;
    SCStreamConfiguration::new()
        .with_width(g.req.0)
        .with_height(g.req.1)
        .with_source_rect(CGRect::new(s.x, s.y, s.w, s.h))
        .with_shows_cursor(false)
        .with_queue_depth(3)
        .with_minimum_frame_interval(&CMTime::new(crate::frames::MIN_FRAME_INTERVAL_MS, 1000))
}

impl SckStream {
    /// Creates and starts the stream for `rect` (global top-left points), output bitmap `w` x `h`.
    pub fn start(rect: Rect, w: u32, h: u32) -> Result<Self> {
        let content = SCShareableContent::get().map_err(|e| fail("shareable content", e))?;
        let geo = StreamGeometry::new(&rect, &display_frames(&content), w, h)
            .ok_or_else(|| AppError::new(ErrorCode::NotFound).with_detail("no display"))?;
        let shared = Arc::new(StreamShared {
            geo: Mutex::new(geo.clone()),
            proc: Mutex::new(FrameProcessor::default()),
            slot: Mutex::new(None),
            cv: Condvar::new(),
            delivered: AtomicU64::new(0),
            processed: AtomicU64::new(0),
            err: Mutex::new(None),
        });
        let filter = stream_filter(&content, geo.plan.display)?;
        let sh_err = shared.clone();
        let delegate = ErrorHandler::new(move |e: SCError| {
            *sh_err.err.lock().unwrap_or_else(|x| x.into_inner()) = Some(format!("{e}"));
        });
        let mut stream = SCStream::new_with_delegate(&filter, &stream_config(&geo), delegate)
            .map_err(|e| fail("stream", e))?;
        let sh = shared.clone();
        stream
            .add_output_handler(
                move |sb: CMSampleBuffer, ty: SCStreamOutputType| {
                    if matches!(ty, SCStreamOutputType::Screen) {
                        sh.on_sample(&sb);
                    }
                },
                SCStreamOutputType::Screen,
            )
            .map_err(|e| fail("stream output", e))?;
        stream.start_capture().map_err(|e| fail("stream start", e))?;
        Ok(Self {
            stream,
            shared,
            geo,
            out: (w, h),
            content,
            content_at: Instant::now(),
            filter_at: Instant::now(),
        })
    }
}

impl RegionStream for SckStream {
    fn retarget(&mut self, rect: Rect) -> Result<()> {
        if self.content_at.elapsed() > CONTENT_REFRESH {
            self.content = SCShareableContent::get().map_err(|e| fail("shareable content", e))?;
            self.content_at = Instant::now();
        }
        let new = StreamGeometry::new(&rect, &display_frames(&self.content), self.out.0, self.out.1)
            .ok_or_else(|| AppError::new(ErrorCode::NotFound).with_detail("no display"))?;
        let qa_keep = KEEP_TITLE.lock().unwrap_or_else(|e| e.into_inner()).is_some();
        let refresh_filter = qa_keep && self.filter_at.elapsed() > FILTER_REFRESH;
        if new == self.geo && !refresh_filter {
            return Ok(());
        }
        // The pixels in flight were rendered for the old geometry: publish the new one first, forget old content.
        *self.shared.geo.lock().unwrap_or_else(|e| e.into_inner()) = new.clone();
        self.shared.proc.lock().unwrap_or_else(|e| e.into_inner()).reset();
        if new.plan.display != self.geo.plan.display || refresh_filter {
            let f = stream_filter(&self.content, new.plan.display)?;
            self.stream
                .update_content_filter(&f)
                .map_err(|e| fail("stream filter update", e))?;
            self.filter_at = Instant::now();
        }
        if !new.same_config(&self.geo) {
            self.stream
                .update_configuration(&stream_config(&new))
                .map_err(|e| fail("stream config update", e))?;
        }
        self.geo = new;
        Ok(())
    }

    fn next_frame(&mut self, timeout: Duration) -> Option<Vec<u8>> {
        let mut g = self.shared.slot.lock().unwrap_or_else(|e| e.into_inner());
        if g.is_none() {
            g = self
                .shared
                .cv
                .wait_timeout(g, timeout)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
        g.take()
    }

    fn counters(&self) -> (u64, u64) {
        (
            self.shared.delivered.load(Ordering::Relaxed),
            self.shared.processed.load(Ordering::Relaxed),
        )
    }

    fn error(&self) -> Option<String> {
        self.shared.err.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

impl Drop for SckStream {
    fn drop(&mut self) {
        // Completely stopped: no frames, no recording indicator.
        let _ = self.stream.stop_capture();
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
