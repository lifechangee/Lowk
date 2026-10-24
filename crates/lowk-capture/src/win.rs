//! Windows screenshots: GDI `BitBlt` per monitor. Our own windows are excluded by flipping
//! their display affinity to `WDA_EXCLUDEFROMCAPTURE` for the duration of the grab (the
//! overlay and pill already carry it permanently). Compile-checked from other hosts; the
//! runtime is verified on a Windows machine later.
use crate::DisplayShot;
use lowk_types::{AppError, CaptureMonitors, ErrorCode, Rect, Result};
use windows::core::BOOL;
use windows::Win32::Foundation::{HWND, LPARAM, RECT};
use windows::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, EnumDisplayMonitors, GetDC,
    GetDIBits, ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, CAPTUREBLT, DIB_RGB_COLORS,
    HDC, HMONITOR, SRCCOPY,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowDisplayAffinity, GetWindowRect, SetWindowDisplayAffinity,
    WDA_EXCLUDEFROMCAPTURE, WINDOW_DISPLAY_AFFINITY,
};

fn fail(what: &str) -> AppError {
    AppError::new(ErrorCode::Unknown).with_detail(format!("screenshot: {what}"))
}

unsafe extern "system" fn collect(_m: HMONITOR, _dc: HDC, r: *mut RECT, data: LPARAM) -> BOOL {
    // SAFETY: `data` is the `&mut Vec<RECT>` passed below; `r` is valid for the call.
    let v = unsafe { &mut *(data.0 as *mut Vec<RECT>) };
    v.push(unsafe { *r });
    BOOL(1)
}

/// Excludes `own` windows (raw HWND values) from the grab, restores their affinity afterwards.
pub fn capture_displays(
    max_edge: u32,
    own: &[isize],
    mode: CaptureMonitors,
    mouse: Option<(f64, f64)>,
) -> Result<Vec<DisplayShot>> {
    with_own_excluded(own, || grab_all(max_edge, mode, mouse, own))
}

/// A rectangle of the virtual desktop (physical pixels).
pub fn capture_area(rect: Rect, max_edge: u32, own: &[isize]) -> Result<DisplayShot> {
    let r = RECT {
        left: rect.x.round() as i32,
        top: rect.y.round() as i32,
        right: (rect.x + rect.w).round() as i32,
        bottom: (rect.y + rect.h).round() as i32,
    };
    with_own_excluded(own, || grab_one(r, max_edge))
}

fn with_own_excluded<T>(own: &[isize], f: impl FnOnce() -> Result<T>) -> Result<T> {
    let mut saved: Vec<(HWND, u32)> = vec![];
    for &raw in own {
        let h = HWND(raw as *mut _);
        let mut cur = 0u32;
        // SAFETY: HWND values come from live Tauri windows; the out pointer is valid.
        unsafe {
            if GetWindowDisplayAffinity(h, &mut cur).is_ok() {
                saved.push((h, cur));
                let _ = SetWindowDisplayAffinity(h, WDA_EXCLUDEFROMCAPTURE);
            }
        }
    }
    let r = f();
    for (h, a) in saved {
        // SAFETY: as above.
        unsafe {
            let _ = SetWindowDisplayAffinity(h, WINDOW_DISPLAY_AFFINITY(a));
        }
    }
    r
}

/// The foreground window of another app (not one of ours), as a rect.
fn focused_window(own: &[isize]) -> Option<Rect> {
    // SAFETY: plain queries; the out pointer is valid.
    unsafe {
        let h = GetForegroundWindow();
        if h.0.is_null() || own.contains(&(h.0 as isize)) {
            return None;
        }
        let mut r = RECT::default();
        GetWindowRect(h, &mut r).ok()?;
        Some(Rect {
            x: f64::from(r.left),
            y: f64::from(r.top),
            w: f64::from(r.right - r.left),
            h: f64::from(r.bottom - r.top),
        })
    }
}

fn grab_all(
    max_edge: u32,
    mode: CaptureMonitors,
    mouse: Option<(f64, f64)>,
    own: &[isize],
) -> Result<Vec<DisplayShot>> {
    let mut rects: Vec<RECT> = vec![];
    // SAFETY: the callback only pushes into `rects`, which outlives the call.
    unsafe {
        let _ = EnumDisplayMonitors(None, None, Some(collect), LPARAM(&mut rects as *mut _ as isize));
    }
    if rects.is_empty() {
        return Err(AppError::new(ErrorCode::NotFound).with_detail("no display"));
    }
    let frames: Vec<Rect> = rects
        .iter()
        .map(|r| Rect {
            x: f64::from(r.left),
            y: f64::from(r.top),
            w: f64::from(r.right - r.left),
            h: f64::from(r.bottom - r.top),
        })
        .collect();
    let focus = if mode == CaptureMonitors::Active {
        focused_window(own)
    } else {
        None
    };
    crate::select_displays(&frames, mode, focus, mouse)
        .into_iter()
        .map(|i| grab_one(rects[i], max_edge))
        .collect()
}

fn grab_one(r: RECT, max_edge: u32) -> Result<DisplayShot> {
    let (sw, sh) = ((r.right - r.left).max(1), (r.bottom - r.top).max(1));
    let k = (f64::from(max_edge) / f64::from(sw.max(sh))).min(1.0);
    let (w, h) = (
        ((f64::from(sw) * k) as i32).max(1),
        ((f64::from(sh) * k) as i32).max(1),
    );
    // SAFETY: plain GDI; every handle is released on all paths below.
    unsafe {
        let screen = GetDC(None);
        let mem = CreateCompatibleDC(Some(screen));
        let bmp = CreateCompatibleBitmap(screen, sw, sh);
        let old = SelectObject(mem, bmp.into());
        let ok = BitBlt(
            mem,
            0,
            0,
            sw,
            sh,
            Some(screen),
            r.left,
            r.top,
            SRCCOPY | CAPTUREBLT,
        )
        .is_ok();
        let mut bi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: sw,
                biHeight: -sh,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bgra = vec![0u8; (sw * sh * 4) as usize];
        let lines = if ok {
            GetDIBits(
                mem,
                bmp,
                0,
                sh as u32,
                Some(bgra.as_mut_ptr().cast()),
                &mut bi,
                DIB_RGB_COLORS,
            )
        } else {
            0
        };
        SelectObject(mem, old);
        let _ = DeleteObject(bmp.into());
        let _ = DeleteDC(mem);
        ReleaseDC(None, screen);
        if lines == 0 {
            return Err(fail("BitBlt/GetDIBits failed"));
        }
        let mut rgba = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            let sy = (i64::from(y) * i64::from(sh) / i64::from(h)) as usize;
            for x in 0..w {
                let sx = (i64::from(x) * i64::from(sw) / i64::from(w)) as usize;
                let i = (sy * sw as usize + sx) * 4;
                rgba.extend_from_slice(&[bgra[i + 2], bgra[i + 1], bgra[i], 255]);
            }
        }
        Ok(DisplayShot {
            primary: r.left == 0 && r.top == 0,
            rgba,
            width: w as u32,
            height: h as u32,
        })
    }
}
