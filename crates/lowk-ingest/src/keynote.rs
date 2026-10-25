//! Keynote (`.key`): Apple publishes no open parser, so on macOS Keynote itself exports the deck
//! to PDF (AppleScript, `osascript`), and the PDF path takes over. Everywhere else, and when
//! Keynote is not installed or refuses, the file fails with `DocUnsupported` and the hint
//! `export_to=pdf|pptx`; it is never converted to an empty document.
use crate::ocr::Ocr;
#[cfg(target_os = "macos")]
use crate::pdf;
use crate::pdf::PdfOut;
use crate::{Limits, Progress};
use lowk_types::{AppError, ErrorCode, IngestRequest, Result};
use std::path::Path;

fn unsupported(reason: &str, detail: impl Into<String>) -> AppError {
    AppError::new(ErrorCode::DocUnsupported)
        .with_param("reason", reason)
        .with_param("export_to", "pdf|pptx")
        .with_detail(detail)
}

#[cfg(not(target_os = "macos"))]
pub fn convert(
    _path: &Path,
    _req: &IngestRequest,
    _ocr: &dyn Ocr,
    _lim: &Limits,
    _p: &mut Progress<'_>,
) -> Result<PdfOut> {
    Err(unsupported(
        "keynote_needs_macos",
        "Keynote files can only be read on a Mac with Keynote; export to PDF or PPTX",
    ))
}

#[cfg(target_os = "macos")]
const SCRIPT: [&str; 9] = [
    "on run argv",
    "set src to POSIX file (item 1 of argv)",
    "set dst to POSIX file (item 2 of argv)",
    "tell application \"Keynote\"",
    "set theDoc to open src",
    "export theDoc to dst as PDF",
    "close theDoc saving no",
    "end tell",
    "end run",
];

#[cfg(target_os = "macos")]
pub fn convert(
    path: &Path,
    req: &IngestRequest,
    ocr: &dyn Ocr,
    lim: &Limits,
    progress: &mut Progress<'_>,
) -> Result<PdfOut> {
    use std::process::Stdio;
    use std::time::{Duration, Instant};

    if !Path::new("/Applications/Keynote.app").exists()
        && !Path::new("/System/Applications/Keynote.app").exists()
    {
        return Err(unsupported(
            "keynote_not_installed",
            "Keynote is not installed; export the deck to PDF or PPTX",
        ));
    }
    let dir = std::env::temp_dir().join(format!("lowk-keynote-{}", lowk_types::Ulid::new()));
    std::fs::create_dir_all(&dir)?;
    let out = dir.join("export.pdf");
    let res = (|| -> Result<PdfOut> {
        let mut cmd = lowk_platform::process::command("/usr/bin/osascript");
        for l in SCRIPT {
            cmd.arg("-e").arg(l);
        }
        let mut child = cmd
            .arg(path)
            .arg(&out)
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| unsupported("osascript_failed", e.to_string()))?;
        let started = Instant::now();
        loop {
            if let Err(e) = progress.check() {
                let _ = child.kill();
                let _ = child.wait();
                return Err(e);
            }
            match child.try_wait()? {
                Some(st) if st.success() => break,
                Some(_) => {
                    let mut err = String::new();
                    if let Some(mut s) = child.stderr.take() {
                        use std::io::Read;
                        let _ = s.read_to_string(&mut err);
                    }
                    return Err(unsupported(
                        "keynote_export_failed",
                        format!("Keynote could not export the file (automation permission denied, or a damaged deck): {}", err.trim()),
                    ));
                }
                None if started.elapsed() > Duration::from_secs(300) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(unsupported(
                        "keynote_timeout",
                        "Keynote did not finish exporting within 5 minutes",
                    ));
                }
                None => std::thread::sleep(Duration::from_millis(200)),
            }
        }
        if !out.exists() {
            return Err(unsupported(
                "keynote_export_failed",
                "Keynote reported success but wrote no PDF",
            ));
        }
        pdf::convert(&out, req, ocr, lim, progress)
    })();
    let _ = std::fs::remove_dir_all(&dir);
    res
}
