//! `MainThread` over Tauri's `run_on_main_thread`.
use lowk_types::MainThread;
use std::thread::ThreadId;
use tauri::AppHandle;

pub struct TauriMainThread {
    app: AppHandle,
    main_id: ThreadId,
}

impl TauriMainThread {
    /// Must be created on the main thread (Tauri `setup` runs there).
    pub fn new(app: AppHandle) -> Self {
        Self {
            app,
            main_id: std::thread::current().id(),
        }
    }
}

impl MainThread for TauriMainThread {
    fn run(&self, f: Box<dyn FnOnce() + Send>) {
        if self.app.run_on_main_thread(f).is_err() {
            tracing::warn!("main thread is gone, job dropped");
        }
    }
    fn is_main(&self) -> bool {
        std::thread::current().id() == self.main_id
    }
}
