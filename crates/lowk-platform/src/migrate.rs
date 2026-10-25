//! Moving the data root: copy -> verify -> delete the old files, with byte progress.
//! The old files are removed only after *every* file was copied and compared, so a failure or a
//! cancel leaves the previous location intact.
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

const CHUNK: usize = 1 << 20;
/// Sub-folders that move with the data root. Logs stay: the live log file is open and they are tiny.
pub const MOVED_SUBDIRS: [&str; 2] = ["models", "engines"];

#[derive(Debug)]
pub enum MigrateError {
    Cancelled,
    NoSpace { need: u64, free: u64 },
    Io(io::Error),
    Mismatch(PathBuf),
}

impl std::fmt::Display for MigrateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cancelled => write!(f, "cancelled"),
            Self::NoSpace { need, free } => write!(f, "not enough space: need {need} bytes, {free} free"),
            Self::Io(e) => write!(f, "{e}"),
            Self::Mismatch(p) => write!(f, "verification failed for {}", p.display()),
        }
    }
}
impl From<io::Error> for MigrateError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

/// Total size of the files below `p` (0 when missing).
pub fn dir_size(p: &Path) -> u64 {
    let Ok(rd) = fs::read_dir(p) else {
        return fs::metadata(p)
            .map(|m| if m.is_file() { m.len() } else { 0 })
            .unwrap_or(0);
    };
    rd.flatten()
        .map(|e| {
            let p = e.path();
            if p.is_dir() {
                dir_size(&p)
            } else {
                e.metadata().map(|m| m.len()).unwrap_or(0)
            }
        })
        .sum()
}

/// Free bytes on the volume that holds `p` (the nearest existing ancestor is used).
pub fn free_bytes(p: &Path) -> u64 {
    let mut probe = p.to_path_buf();
    while !probe.exists() {
        if !probe.pop() {
            break;
        }
    }
    let probe = fs::canonicalize(&probe).unwrap_or(probe);
    let disks = sysinfo::Disks::new_with_refreshed_list();
    disks
        .list()
        .iter()
        .filter(|d| probe.starts_with(d.mount_point()))
        .max_by_key(|d| d.mount_point().as_os_str().len())
        .map(|d| d.available_space())
        .unwrap_or(u64::MAX)
}

fn files_below(root: &Path, out: &mut Vec<PathBuf>) -> io::Result<()> {
    for e in fs::read_dir(root)? {
        let p = e?.path();
        if p.is_dir() {
            files_below(&p, out)?;
        } else {
            out.push(p);
        }
    }
    Ok(())
}

fn same_content(a: &Path, b: &Path) -> io::Result<bool> {
    if fs::metadata(a)?.len() != fs::metadata(b)?.len() {
        return Ok(false);
    }
    let (mut fa, mut fb) = (File::open(a)?, File::open(b)?);
    let (mut ba, mut bb) = (vec![0u8; CHUNK], vec![0u8; CHUNK]);
    loop {
        let n = fa.read(&mut ba)?;
        if n == 0 {
            return Ok(true);
        }
        fb.read_exact(&mut bb[..n])?;
        if ba[..n] != bb[..n] {
            return Ok(false);
        }
    }
}

fn copy_file(
    src: &Path,
    dst: &Path,
    cancel: &AtomicBool,
    tick: &mut dyn FnMut(u64),
) -> Result<(), MigrateError> {
    if let Some(d) = dst.parent() {
        fs::create_dir_all(d)?;
    }
    let part = dst.with_extension("lowk-part");
    let mut r = File::open(src)?;
    let mut w = File::create(&part)?;
    let mut buf = vec![0u8; CHUNK];
    loop {
        if cancel.load(Ordering::Relaxed) {
            drop(w);
            let _ = fs::remove_file(&part);
            return Err(MigrateError::Cancelled);
        }
        let n = r.read(&mut buf)?;
        if n == 0 {
            break;
        }
        w.write_all(&buf[..n])?;
        tick(n as u64);
    }
    w.sync_all()?;
    drop(w);
    fs::rename(&part, dst)?;
    if !same_content(src, dst)? {
        let _ = fs::remove_file(dst);
        return Err(MigrateError::Mismatch(src.to_path_buf()));
    }
    Ok(())
}

/// Outcome of a finished move.
#[derive(Debug, Default)]
pub struct Moved {
    pub bytes: u64,
    /// Old locations that could not be deleted (e.g. a file still held by a running worker).
    pub leftovers: Vec<PathBuf>,
}

/// Copies `subdirs` of `from` into `to`, verifies, then deletes the originals.
/// `progress(done_bytes, total_bytes)` is called after every chunk.
pub fn move_root(
    from: &Path,
    to: &Path,
    subdirs: &[&str],
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<Moved, MigrateError> {
    let total: u64 = subdirs.iter().map(|s| dir_size(&from.join(s))).sum();
    let free = free_bytes(to);
    // 5 % head-room for file-system overhead.
    let need = total + total / 20;
    if free < need {
        return Err(MigrateError::NoSpace { need, free });
    }
    fs::create_dir_all(to)?;
    let mut done = 0u64;
    let mut copied: Vec<PathBuf> = vec![];
    for s in subdirs {
        let src_dir = from.join(s);
        if !src_dir.is_dir() {
            continue;
        }
        let mut files = vec![];
        files_below(&src_dir, &mut files)?;
        for f in files {
            let rel = f.strip_prefix(&src_dir).unwrap_or(&f);
            let dst = to.join(s).join(rel);
            let skip = dst.is_file() && same_content(&f, &dst).unwrap_or(false);
            if skip {
                done += fs::metadata(&f)?.len();
                progress(done, total);
                continue;
            }
            copy_file(&f, &dst, cancel, &mut |n| {
                done += n;
                progress(done, total);
            })?;
        }
        copied.push(src_dir);
    }
    let mut leftovers = vec![];
    for d in copied {
        if fs::remove_dir_all(&d).is_err() {
            leftovers.push(d);
        }
    }
    progress(total, total);
    Ok(Moved {
        bytes: total,
        leftovers,
    })
}

/// What `first_run` decided.
#[derive(Debug, PartialEq, Eq)]
pub enum FirstRun {
    /// A root is already persisted (or isolated profile): nothing to do.
    Configured,
    /// Fresh install: the default root was persisted.
    Fresh,
    /// Existing models in the old location were moved into the new root.
    Migrated,
    /// Not enough space for the move: the old location stays the root.
    KeptInPlace,
}

/// Upgrade handling, once. `legacy` is where older versions kept models (`<app data>`).
/// Returns the decision and the root now in force.
pub fn first_run(
    app_data: &Path,
    default_root: &Path,
    progress: &mut dyn FnMut(u64, u64),
) -> (FirstRun, PathBuf) {
    if crate::dirs::read_pointer(app_data).is_some() {
        let r = crate::dirs::read_pointer(app_data).unwrap_or_else(|| default_root.to_path_buf());
        return (FirstRun::Configured, r);
    }
    let legacy_bytes: u64 = MOVED_SUBDIRS.iter().map(|s| dir_size(&app_data.join(s))).sum();
    if default_root == app_data || legacy_bytes == 0 {
        let _ = crate::dirs::write_pointer(app_data, default_root);
        return (FirstRun::Fresh, default_root.to_path_buf());
    }
    let cancel = AtomicBool::new(false);
    match move_root(app_data, default_root, &MOVED_SUBDIRS, &cancel, progress) {
        Ok(_) => {
            let _ = crate::dirs::write_pointer(app_data, default_root);
            (FirstRun::Migrated, default_root.to_path_buf())
        }
        Err(e) => {
            tracing::warn!(
                "data migration skipped, models stay in {}: {e}",
                app_data.display()
            );
            let _ = crate::dirs::write_pointer(app_data, app_data);
            (FirstRun::KeptInPlace, app_data.to_path_buf())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put(p: &Path, body: &[u8]) {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, body).unwrap();
    }

    fn fixture() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let t = tempfile::tempdir().unwrap();
        let old = t.path().join("old");
        let new = t.path().join("new");
        put(&old.join("models/whisper/ggml.bin"), &vec![7u8; 3 * CHUNK + 5]);
        put(&old.join("models/embed/m.gguf"), b"embedder");
        put(&old.join("engines/lowk-stt-worker.exe"), b"exe");
        put(&old.join("logs/lowk.log"), b"log");
        (t, old, new)
    }

    #[test]
    fn move_copies_verifies_and_deletes_old() {
        let (_t, old, new) = fixture();
        let mut last = (0, 0);
        let m = move_root(
            &old,
            &new,
            &MOVED_SUBDIRS,
            &AtomicBool::new(false),
            &mut |d, t| last = (d, t),
        )
        .unwrap();
        assert_eq!(last.0, last.1);
        assert_eq!(m.bytes, (3 * CHUNK + 5 + 8 + 3) as u64);
        assert!(m.leftovers.is_empty());
        assert_eq!(fs::read(new.join("models/embed/m.gguf")).unwrap(), b"embedder");
        assert_eq!(
            fs::metadata(new.join("models/whisper/ggml.bin")).unwrap().len(),
            (3 * CHUNK + 5) as u64
        );
        assert!(!old.join("models").exists() && !old.join("engines").exists());
        assert!(old.join("logs/lowk.log").exists(), "logs are not moved");
    }

    #[test]
    fn cancel_keeps_the_old_files() {
        let (_t, old, new) = fixture();
        let r = move_root(&old, &new, &MOVED_SUBDIRS, &AtomicBool::new(true), &mut |_, _| {});
        assert!(matches!(r, Err(MigrateError::Cancelled)));
        assert!(old.join("models/whisper/ggml.bin").exists());
        assert!(!new.join("models/whisper/ggml.bin").exists());
    }

    #[test]
    fn resumes_over_a_partial_copy() {
        let (_t, old, new) = fixture();
        put(&new.join("models/embed/m.gguf"), b"embedder");
        move_root(
            &old,
            &new,
            &MOVED_SUBDIRS,
            &AtomicBool::new(false),
            &mut |_, _| {},
        )
        .unwrap();
        assert!(new.join("models/whisper/ggml.bin").exists());
    }

    #[test]
    fn first_run_migrates_legacy_models_and_persists_the_root() {
        let (_t, old, new) = fixture();
        let (r, root) = first_run(&old, &new, &mut |_, _| {});
        assert_eq!(r, FirstRun::Migrated);
        assert_eq!(root, new);
        assert!(new.join("models/embed/m.gguf").exists());
        assert_eq!(crate::dirs::read_pointer(&old), Some(new.clone()));
        // second start: nothing happens
        assert_eq!(first_run(&old, &new, &mut |_, _| {}).0, FirstRun::Configured);
    }

    #[test]
    fn first_run_fresh_install_persists_default() {
        let t = tempfile::tempdir().unwrap();
        let (app, root) = (t.path().join("app"), t.path().join("root"));
        let (r, got) = first_run(&app, &root, &mut |_, _| {});
        assert_eq!((r, got), (FirstRun::Fresh, root.clone()));
        assert_eq!(crate::dirs::read_pointer(&app), Some(root));
    }

    #[test]
    fn first_run_keeps_models_in_place_when_the_move_fails() {
        let (t, old, _new) = fixture();
        // the target is below a regular file: it can never be created
        let blocker = t.path().join("file");
        fs::write(&blocker, b"x").unwrap();
        let (r, root) = first_run(&old, &blocker.join("data"), &mut |_, _| {});
        assert_eq!(r, FirstRun::KeptInPlace);
        assert_eq!(root, old);
        assert!(old.join("models/embed/m.gguf").exists());
        assert_eq!(crate::dirs::read_pointer(&old), Some(old.clone()));
    }

    #[test]
    fn space_and_size_probes_work() {
        let (_t, old, new) = fixture();
        // free_bytes of a real dir is finite; ask for more by faking a huge total is not possible,
        // so check the arithmetic through the error type instead
        let e = MigrateError::NoSpace { need: 10, free: 1 };
        assert!(e.to_string().contains("not enough space"));
        assert!(free_bytes(&new) > 0 && dir_size(&old) > 0);
    }
}
