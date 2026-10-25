//! The only place that builds child-process commands. On Windows every child must be created with
//! `CREATE_NO_WINDOW`, otherwise a console window flashes for each console-subsystem child (the speech
//! worker, the embedder, `explorer`, `cmd`). `tests/no_raw_command.rs` forbids `Command::new` elsewhere.
use std::ffi::OsStr;
use std::process::Command;

/// `CREATE_NO_WINDOW` process-creation flag.
pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;
/// `BELOW_NORMAL_PRIORITY_CLASS`.
pub const BELOW_NORMAL_PRIORITY: u32 = 0x0000_4000;

/// A command that never opens a console window (a plain `Command` on other systems).
pub fn command(program: impl AsRef<OsStr>) -> Command {
    #[allow(unused_mut)]
    let mut c = Command::new(program);
    hide_window(&mut c);
    c
}

/// Same as [`command`] plus the lower priority class used for background work (Windows only).
#[allow(unused_mut)]
pub fn background_command(program: impl AsRef<OsStr>) -> Command {
    let mut c = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(CREATE_NO_WINDOW | BELOW_NORMAL_PRIORITY);
    }
    c
}

/// Applies the no-console flag to an existing command.
pub fn hide_window(c: &mut Command) -> &mut Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(CREATE_NO_WINDOW);
    }
    c
}

/// Windows `STATUS_ILLEGAL_INSTRUCTION`.
const STATUS_ILLEGAL_INSTRUCTION: u32 = 0xC000_001D;

/// Names a crash that points at the machine, not at the model: an illegal-instruction fault (Windows
/// `0xc000001d`, unix SIGILL) means the build needs a CPU feature this processor does not have.
pub fn classify_exit(code: Option<i32>, signal: Option<i32>) -> Option<lowk_types::ErrorCode> {
    if code.is_some_and(|c| c as u32 == STATUS_ILLEGAL_INSTRUCTION) || signal == Some(4) {
        Some(lowk_types::ErrorCode::CpuUnsupported)
    } else {
        None
    }
}

/// [`classify_exit`] for a real exit status.
pub fn exit_cause(st: &std::process::ExitStatus) -> Option<lowk_types::ErrorCode> {
    #[cfg(unix)]
    let signal = {
        use std::os::unix::process::ExitStatusExt;
        st.signal()
    };
    #[cfg(not(unix))]
    let signal = None;
    classify_exit(st.code(), signal)
}

/// Last lines of a child's stderr, kept for the log when it dies or never becomes ready.
pub type StderrTail = std::sync::Arc<std::sync::Mutex<std::collections::VecDeque<String>>>;

/// Drains `stderr` on a thread (so the child never blocks on a full pipe), keeping the last `keep` lines.
pub fn tail_stderr(stderr: impl std::io::Read + Send + 'static, keep: usize) -> StderrTail {
    use std::io::BufRead;
    let tail: StderrTail = Default::default();
    let t = tail.clone();
    std::thread::spawn(move || {
        for l in std::io::BufReader::new(stderr).lines().map_while(Result::ok) {
            let mut g = t.lock().unwrap_or_else(|p| p.into_inner());
            if g.len() >= keep {
                g.pop_front();
            }
            g.push_back(l);
        }
    });
    tail
}

pub fn tail_text(t: &StderrTail) -> String {
    t.lock()
        .unwrap_or_else(|p| p.into_inner())
        .iter()
        .cloned()
        .collect::<Vec<_>>()
        .join(" | ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn illegal_instruction_is_named() {
        use lowk_types::ErrorCode;
        assert_eq!(
            classify_exit(Some(0xC000_001Du32 as i32), None),
            Some(ErrorCode::CpuUnsupported)
        );
        assert_eq!(classify_exit(None, Some(4)), Some(ErrorCode::CpuUnsupported));
        assert_eq!(classify_exit(Some(3), None), None);
        assert_eq!(classify_exit(Some(0), None), None);
    }

    #[test]
    fn stderr_tail_keeps_the_last_lines() {
        let t = tail_stderr(std::io::Cursor::new(b"a\nb\nc\nd\n".to_vec()), 2);
        for _ in 0..100 {
            if tail_text(&t) == "c | d" {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(tail_text(&t), "c | d");
    }

    #[test]
    fn flag_values_match_win32() {
        assert_eq!(CREATE_NO_WINDOW, 0x0800_0000);
        assert_eq!(BELOW_NORMAL_PRIORITY, 0x4000);
    }

    #[cfg(unix)]
    #[test]
    fn runs_programs() {
        let o = command("echo").arg("hi").output().unwrap();
        assert_eq!(String::from_utf8_lossy(&o.stdout).trim(), "hi");
    }
}
