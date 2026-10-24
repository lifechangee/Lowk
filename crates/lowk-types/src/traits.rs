//! Cross-crate traits. Feature crates never depend on each other; they meet
//! here. Implementations: see the crate named in each doc comment.
use crate::*;
use std::sync::mpsc::Receiver;
use std::sync::Arc;

/// Runs closures on the OS UI thread (AppKit / Win32). Implemented by
/// `lowk-app` over Tauri's `run_on_main_thread`.
pub trait MainThread: Send + Sync {
    /// Fire-and-forget on the main thread.
    fn run(&self, f: Box<dyn FnOnce() + Send>);
    /// True when called on the main thread (so `run_sync` cannot deadlock).
    fn is_main(&self) -> bool;
}

impl dyn MainThread {
    /// Run on the main thread and wait for the result.
    pub fn run_sync<R: Send + 'static>(&self, f: impl FnOnce() -> R + Send + 'static) -> R {
        if self.is_main() {
            return f();
        }
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        self.run(Box::new(move || {
            let _ = tx.send(f());
        }));
        rx.recv().expect("main thread dropped the job")
    }
}

/// Global cursor position (top-left origin, points).
pub trait CursorSource: Send + Sync {
    fn position(&self) -> (f64, f64);
}

/// Implemented by `lowk-index`.
pub trait Retriever: Send + Sync {
    fn retrieve(&self, q: &RetrievalQuery) -> Result<Vec<ContextChunk>>;

    /// Full context for one Ask: folder instructions, pinned documents, relevant chunks of
    /// documents and previous calls (M2). The default wraps `retrieve` (no folder context).
    fn retrieve_pack(&self, q: &RetrievalQuery) -> Result<ContextPack> {
        Ok(ContextPack {
            relevant: self.retrieve(q)?,
            ..ContextPack::default()
        })
    }
}

/// Implemented by `lowk-stealth`. All methods are safe to call from any
/// thread; the implementation marshals to the main thread.
pub trait OverlayController: Send + Sync {
    fn show(&self) -> Result<()>;
    fn hide(&self);
    fn panic_hide(&self);
    fn set_mode(&self, m: OverlayMode) -> Result<()>;
    fn mode(&self) -> OverlayMode;
    /// The follow loop outlives the call, hence the owned source.
    fn move_hold(&self, phase: Phase, cursor: Arc<dyn CursorSource>);
    fn status(&self) -> StealthStatus;
    fn selftest(&self) -> Result<SelfTestReport>;
}

/// Implemented by `lowk-store`.
pub trait SettingsStore: Send + Sync {
    fn get(&self) -> Settings;
    fn patch(&self, p: SettingsPatch) -> Result<Settings>;
    /// Receives the new settings after every successful patch.
    fn subscribe(&self) -> Receiver<Settings>;
}

/// Implemented by `lowk-input`'s backends.
pub trait InputBackend: Send {
    fn set_binds(&mut self, binds: &[BindSpec]) -> Result<BindReport>;
    fn events(&self) -> Receiver<BindEvent>;
}

/// Receives events from the session bus (the `UiEmitter` in `lowk-app`).
pub trait EventSink: Send + Sync {
    fn emit(&self, ev: AppEvent);
}

/// Hardware-agnostic screen sampler used by the adaptive-tone task
/// (implemented by `lowk-capture`, consumed by `lowk-stealth`).
pub trait RegionSampler: Send + Sync {
    /// RGBA8 bitmap of `rect` scaled to `w` x `h`, excluding our own windows.
    fn grab_region(&self, rect: Rect, w: u32, h: u32) -> Result<Vec<u8>>;

    /// Optional continuous capture of `rect` (output `w` x `h`). `None` = not supported or it failed to start:
    /// the caller falls back to polling [`grab_region`](Self::grab_region). Dropping the stream stops it completely.
    fn open_stream(&self, _rect: Rect, _w: u32, _h: u32) -> Option<Box<dyn RegionStream>> {
        None
    }
}

/// A running continuous capture of the region under the overlay (macOS: one `SCStream`).
pub trait RegionStream: Send {
    /// Follow the overlay (moved / resized / other display). Cheap when nothing changed.
    fn retarget(&mut self, rect: Rect) -> Result<()>;
    /// Wait up to `timeout` for a frame whose content changed since the last one returned (RGBA8, `w` x `h`).
    fn next_frame(&mut self, timeout: std::time::Duration) -> Option<Vec<u8>>;
    /// `(frames delivered by the OS, frames whose content changed and were processed)` since the start.
    fn counters(&self) -> (u64, u64);
    /// Last stream error (the stream then stops delivering; the caller falls back to polling).
    fn error(&self) -> Option<String>;
}

/// Plain screen grab *without* window exclusion (what a normal capturer sees).
/// Used by the stealth self-test pixel layer; implemented by `lowk-capture`.
pub trait ScreenGrab: Send + Sync {
    /// RGBA8 pixels of `rect` (global top-left points) with width and height.
    fn grab(&self, rect: Rect) -> Result<(Vec<u8>, u32, u32)>;
}
