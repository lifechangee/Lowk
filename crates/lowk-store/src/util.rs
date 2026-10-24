//! Small shared helpers: errors, atomic writes, names, hashing, time.
use lowk_types::{AppError, ErrorCode};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use time::format_description::well_known::Rfc3339;
use time::{Duration, OffsetDateTime, UtcOffset};

pub(crate) fn err(code: ErrorCode, detail: impl Into<String>) -> AppError {
    AppError::new(code).with_detail(detail)
}

pub(crate) fn hash_bytes(b: &[u8]) -> String {
    blake3::hash(b).to_hex().to_string()
}

/// Write `data` to `path` atomically: temp file in the same dir, fsync, rename.
pub fn atomic_write(path: &Path, data: &[u8]) -> lowk_types::Result<()> {
    let dir = path
        .parent()
        .ok_or_else(|| err(ErrorCode::InvalidArgument, "path has no parent"))?;
    fs::create_dir_all(dir)?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let tmp = dir.join(format!(".{name}.{}.tmp", lowk_types::Ulid::new()));
    let res = (|| -> std::io::Result<()> {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(data)?;
        f.sync_all()?;
        fs::rename(&tmp, path)
    })();
    if let Err(e) = res {
        let _ = fs::remove_file(&tmp);
        return Err(e.into());
    }
    Ok(())
}

const RESERVED: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9",
    "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Make `s` a safe file/dir name on every OS (Windows rules are the strictest).
pub fn sanitize_name(s: &str) -> String {
    let mut out: String = s
        .chars()
        .map(|c| {
            if c.is_control() || "<>:\"/\\|?*".contains(c) {
                ' '
            } else {
                c
            }
        })
        .collect();
    out = out.split_whitespace().collect::<Vec<_>>().join(" ");
    out = out.trim_matches(|c: char| c == '.' || c == ' ').to_string();
    if out.chars().count() > 120 {
        out = out.chars().take(120).collect::<String>().trim_end().to_string();
    }
    let stem = out.split('.').next().unwrap_or("").to_ascii_uppercase();
    if out.is_empty() {
        out = "Untitled".into();
    } else if RESERVED.contains(&stem.as_str()) {
        out.push('_');
    }
    out
}

/// First free `base`, `base (2)`, `base (3)` ... under `dir` (with `ext`).
pub(crate) fn unique_path(dir: &Path, base: &str, ext: &str) -> PathBuf {
    let mk = |n: u32| {
        let name = if n == 1 {
            base.to_string()
        } else {
            format!("{base} ({n})")
        };
        if ext.is_empty() {
            dir.join(name)
        } else {
            dir.join(format!("{name}.{ext}"))
        }
    };
    (1..).map(mk).find(|p| !p.exists()).expect("unbounded range")
}

pub(crate) fn rel_string(root: &Path, p: &Path) -> String {
    p.strip_prefix(root)
        .unwrap_or(p)
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

pub(crate) fn parse_rfc3339(s: &str) -> Option<OffsetDateTime> {
    OffsetDateTime::parse(s, &Rfc3339).ok()
}

pub(crate) fn fmt_rfc3339(t: OffsetDateTime) -> String {
    t.format(&Rfc3339).unwrap_or_default()
}

/// Current wall time expressed in the UTC offset of `like` (an RFC 3339 string).
pub fn now_in_offset_of(like: &str) -> String {
    let off = parse_rfc3339(like).map(|t| t.offset()).unwrap_or(UtcOffset::UTC);
    fmt_rfc3339(OffsetDateTime::now_utc().to_offset(off))
}

pub(crate) fn add_secs(t: &str, secs: u64) -> String {
    match parse_rfc3339(t) {
        Some(d) => fmt_rfc3339(d + Duration::seconds(secs as i64)),
        None => t.to_string(),
    }
}

pub(crate) fn diff_secs(a: &str, b: &str) -> Option<u64> {
    let (a, b) = (parse_rfc3339(a)?, parse_rfc3339(b)?);
    u64::try_from((b - a).whole_seconds()).ok()
}

/// `YYYY-MM-DD HHMM` prefix for call file names, taken literally from the
/// RFC 3339 string (the user's local time as recorded).
pub(crate) fn file_stamp(started: &str) -> String {
    let b = started.as_bytes();
    if b.len() >= 16 && b[10] == b'T' && started.is_char_boundary(16) {
        format!("{} {}{}", &started[..10], &started[11..13], &started[14..16])
    } else {
        "0000-00-00 0000".into()
    }
}
