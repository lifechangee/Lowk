//! Global cursor position (top-left origin).
use lowk_types::CursorSource;

/// `CGEventGetLocation` on macOS, `GetCursorPos` on Windows. Safe from any thread.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemCursor;

impl CursorSource for SystemCursor {
    fn position(&self) -> (f64, f64) {
        crate::platform::cursor_position()
    }
}
