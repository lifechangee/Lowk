//! The data root (models, engines, logs): startup migration, «Папка моделей» in Settings -> Storage and the
//! `--purge-user-data` command used by the uninstaller. Path logic lives in `lowk_platform::{dirs, migrate, purge}`.
use lowk_platform::{dirs, migrate, purge};
use lowk_types::{AppError, ErrorCode, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Mutex;

const MB: u64 = 1024 * 1024;

#[derive(Clone, Debug, Default, Serialize, Deserialize, specta::Type)]
pub struct MoveProgress {
    pub done_mb: u32,
    pub total_mb: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
pub struct DataDirInfo {
    pub path: String,
    pub default_path: String,
    pub is_default: bool,
    /// Size of the models and engines in the current folder.
    pub used_mb: u32,
    /// Set while a move (user-initiated or the upgrade migration) is running.
    pub moving: Option<MoveProgress>,
    /// Last move failed: short reason (not localised, shown as detail).
    pub error: Option<String>,
    /// Old files that could not be deleted after a successful move.
    pub leftover: Option<String>,
}

#[derive(Default)]
struct Status {
    moving: Option<MoveProgress>,
    error: Option<String>,
    leftover: Option<String>,
}

static STATUS: Mutex<Status> = Mutex::new(Status {
    moving: None,
    error: None,
    leftover: None,
});

fn status() -> std::sync::MutexGuard<'static, Status> {
    STATUS.lock().unwrap_or_else(|e| e.into_inner())
}

fn mb(b: u64) -> u32 {
    (b / MB).min(u64::from(u32::MAX)) as u32
}

fn set_progress(done: u64, total: u64) {
    status().moving = Some(MoveProgress {
        done_mb: mb(done),
        total_mb: mb(total),
    });
}

/// Call first thing in `run()`: decides the data root on the first start after an install or upgrade.
/// A migration of existing models runs on a background thread; until it finishes the old place stays the root.
pub fn startup() {
    let (Some(app), Some(default)) = (dirs::app_data(), dirs::default_data_root()) else {
        return;
    };
    if std::env::var_os("LOWK_DATA_ROOT").is_some() || dirs::read_pointer(&app).is_some() {
        return;
    }
    let legacy: u64 = migrate::MOVED_SUBDIRS
        .iter()
        .map(|s| migrate::dir_size(&app.join(s)))
        .sum();
    if legacy == 0 || default == app {
        let (_, root) = migrate::first_run(&app, &default, &mut |_, _| {});
        dirs::use_root_for_now(&root);
        return;
    }
    dirs::use_root_for_now(&app);
    set_progress(0, legacy);
    std::thread::spawn(move || {
        let (outcome, root) = migrate::first_run(&app, &default, &mut set_progress);
        dirs::use_root_for_now(&root);
        status().moving = None;
        tracing::info!(?outcome, root = %root.display(), "data root decided");
    });
}

fn io(detail: impl std::fmt::Display) -> AppError {
    AppError::new(ErrorCode::Io).with_detail(detail.to_string())
}

pub fn info() -> Result<DataDirInfo> {
    let l = dirs::layout().ok_or_else(|| io("no data dir"))?;
    let default = dirs::default_data_root().unwrap_or_else(|| l.data_root.clone());
    let used = migrate::MOVED_SUBDIRS
        .iter()
        .map(|s| migrate::dir_size(&l.data_root.join(s)))
        .sum();
    let st = status();
    Ok(DataDirInfo {
        path: l.data_root.display().to_string(),
        default_path: default.display().to_string(),
        is_default: l.data_root == default,
        used_mb: mb(used),
        moving: st.moving.clone(),
        error: st.error.clone(),
        leftover: st.leftover.clone(),
    })
}

/// Validates a new root: absolute, writable, not inside the current one.
fn check_target(from: &Path, to: &Path) -> Result<()> {
    if !to.is_absolute() {
        return Err(AppError::new(ErrorCode::InvalidArgument).with_detail("not an absolute path"));
    }
    if to == from || to.starts_with(from) {
        return Err(
            AppError::new(ErrorCode::InvalidArgument).with_detail("target is inside the current folder")
        );
    }
    if !dirs::dir_writable(to) {
        return Err(AppError::new(ErrorCode::Io).with_detail("folder is not writable"));
    }
    Ok(())
}

/// Moves models and engines to `path` (`None` = the default root) on a background thread; poll [`info`].
pub fn start_move(path: Option<String>) -> Result<()> {
    let from = dirs::data_root().ok_or_else(|| io("no data dir"))?;
    let to = match path {
        Some(p) => PathBuf::from(p),
        None => dirs::default_data_root().ok_or_else(|| io("no default"))?,
    };
    if to == from {
        return Ok(());
    }
    check_target(&from, &to)?;
    {
        let mut st = status();
        if st.moving.is_some() {
            return Err(AppError::new(ErrorCode::InvalidArgument).with_detail("a move is already running"));
        }
        st.moving = Some(MoveProgress::default());
        st.error = None;
        st.leftover = None;
    }
    std::thread::spawn(move || {
        let cancel = AtomicBool::new(false);
        let r = migrate::move_root(&from, &to, &migrate::MOVED_SUBDIRS, &cancel, &mut set_progress);
        let mut st = status();
        st.moving = None;
        match r {
            Ok(m) => {
                if let Err(e) = dirs::set_data_root(&to) {
                    st.error = Some(e.to_string());
                    return;
                }
                if !m.leftovers.is_empty() {
                    st.leftover = Some(
                        m.leftovers
                            .iter()
                            .map(|p| p.display().to_string())
                            .collect::<Vec<_>>()
                            .join(", "),
                    );
                }
                tracing::info!(to = %to.display(), bytes = m.bytes, "data root moved");
            }
            Err(e) => {
                tracing::warn!("moving the data root failed: {e}");
                st.error = Some(e.to_string());
            }
        }
    });
    Ok(())
}

/// `lowk.exe --purge-user-data`: used by the uninstaller («delete settings, models and logs»). Headless; exit 0
/// when everything was removed (missing things are fine), 1 otherwise. Never touches the vault.
pub fn purge_cli() -> i32 {
    let Some(layout) = dirs::layout() else {
        return 1;
    };
    let mut protected = vec![];
    if let Ok(store) = lowk_store::settings::FileSettingsStore::open(&layout.app_data) {
        use lowk_types::SettingsStore;
        if let Ok(v) = crate::state::resolve_vault_path(&store.get().vault.path) {
            protected.push(v);
        }
    }
    let extras = purge::Extras::from_system(protected);
    let report = purge::purge(&layout, &extras, &mut |p| {
        lowk_platform::secrets::delete(p).map_err(|e| e.to_string())
    });
    for (p, e) in &report.failed {
        eprintln!("purge: {}: {e}", p.display());
    }
    i32::from(!report.failed.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_must_be_absolute_writable_and_outside() {
        let t = tempfile::tempdir().unwrap();
        let from = t.path().join("root");
        assert!(check_target(&from, Path::new("relative")).is_err());
        assert!(check_target(&from, &from).is_err());
        assert!(check_target(&from, &from.join("sub")).is_err());
        assert!(check_target(&from, &t.path().join("other")).is_ok());
    }

    #[test]
    fn mb_saturates() {
        assert_eq!(mb(3 * MB + 5), 3);
        assert_eq!(mb(u64::MAX), u32::MAX);
    }
}
