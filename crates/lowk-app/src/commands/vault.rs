//! vault, folders, calls (thin glue over `lowk-store`).
use crate::state::{AppState, Shared};
use lowk_store::call_codec::{self, Block, Speaker};
use lowk_store::{FolderPatch, Vault};
use lowk_types::*;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::State;

/// The open vault, opening the configured one on first use.
fn vault(st: &AppState) -> Result<Vault> {
    if let Some(v) = st.vault.read().clone() {
        return Ok(v);
    }
    st.open_vault(None)?;
    st.vault()
}

fn changed(st: &AppState, kind: StoreChangeKind, id: Ulid, is_folder: bool) {
    st.emit(AppEvent::Store(StoreChange { kind, id, is_folder }));
}

#[tauri::command(async)]
#[specta::specta]
pub fn vault_open(st: State<'_, Shared>, path: Option<String>) -> Result<VaultInfo> {
    st.open_vault(path)
}

#[tauri::command(async)]
#[specta::specta]
pub fn vault_info(st: State<'_, Shared>) -> Option<VaultInfo> {
    st.vault.read().as_ref().map(|v| VaultInfo {
        path: v.root().display().to_string(),
    })
}

#[tauri::command(async)]
#[specta::specta]
pub fn folder_list(st: State<'_, Shared>) -> Result<Vec<FolderMeta>> {
    vault(&st)?.folders()
}

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
pub struct FolderDetail {
    pub meta: FolderMeta,
    pub instructions: String,
}

#[tauri::command(async)]
#[specta::specta]
pub fn folder_get(st: State<'_, Shared>, id: FolderRef) -> Result<FolderDetail> {
    let v = vault(&st)?;
    let meta = v
        .folders()?
        .into_iter()
        .find(|f| f.id == id)
        .ok_or_else(|| AppError::new(ErrorCode::NotFound))?;
    let instructions = match &id {
        FolderRef::User(u) => v.folder_doc(u)?.instructions().to_string(),
        _ => String::new(),
    };
    Ok(FolderDetail { meta, instructions })
}

#[tauri::command(async)]
#[specta::specta]
pub fn folder_create(st: State<'_, Shared>, folder: NewFolder) -> Result<FolderMeta> {
    let meta = vault(&st)?.create_folder(folder)?;
    if let FolderRef::User(id) = &meta.id {
        changed(&st, StoreChangeKind::Created, *id, true);
    }
    Ok(meta)
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, specta::Type)]
#[serde(default)]
pub struct FolderUpdate {
    pub title: Option<String>,
    pub tags: Option<Vec<String>>,
    pub allow_cloud: Option<bool>,
    /// Empty string clears the folder's default template.
    pub default_template: Option<String>,
    /// «Record microphone» preset of the folder.
    pub mic: Option<MicPreset>,
    /// «Re-transcribe after recording» preset of the folder.
    pub retx: Option<RetxPreset>,
    pub instructions: Option<String>,
}

#[tauri::command(async)]
#[specta::specta]
pub fn folder_update(st: State<'_, Shared>, id: Ulid, patch: FolderUpdate) -> Result<FolderMeta> {
    let meta = vault(&st)?.update_folder(
        &id,
        FolderPatch {
            title: patch.title,
            tags: patch.tags,
            allow_cloud: patch.allow_cloud,
            default_template: patch
                .default_template
                .map(|t| if t.is_empty() { None } else { Some(t) }),
            mic: patch.mic,
            retx: patch.retx,
            instructions: patch.instructions,
        },
    )?;
    changed(&st, StoreChangeKind::Updated, id, true);
    Ok(meta)
}

/// Moves the folder to the Trash (restorable); never hard-deletes.
#[tauri::command(async)]
#[specta::specta]
pub fn folder_delete(st: State<'_, Shared>, id: Ulid) -> Result<()> {
    vault(&st)?.delete(&id)?;
    changed(&st, StoreChangeKind::Trashed, id, true);
    Ok(())
}

#[tauri::command(async)]
#[specta::specta]
pub fn call_list(
    st: State<'_, Shared>,
    folder: Option<FolderRef>,
    include_deleted: bool,
) -> Result<Vec<CallMeta>> {
    let v = vault(&st)?;
    match folder {
        Some(f) => v.calls_in(&f),
        None => {
            let mut all = v.calls(None)?;
            if include_deleted {
                all.extend(v.calls_in(&FolderRef::Trash)?);
                all.sort_by(|a, b| b.started.cmp(&a.started).then(a.id.cmp(&b.id)));
            }
            Ok(all)
        }
    }
}

/// Ids of calls that already have a non-empty summary.
#[tauri::command(async)]
#[specta::specta]
pub fn call_summarized(st: State<'_, Shared>) -> Result<Vec<Ulid>> {
    let v = vault(&st)?;
    Ok(v.calls(None)?
        .into_iter()
        .filter(|c| {
            v.read_call_doc(&c.id)
                .is_ok_and(|d| !d.summary().trim().is_empty())
        })
        .map(|c| c.id)
        .collect())
}

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
pub struct Utterance {
    pub at_s: u32,
    pub speaker: Stream,
    pub text: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
pub struct QaItem {
    pub id: String,
    pub at_s: u32,
    pub question: String,
    pub question_stream: Stream,
    pub answer: String,
    /// Display name of the prompt template.
    pub template: String,
    pub my_answer: Option<String>,
    pub my_answer_at_s: Option<u32>,
}

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
pub struct ShotItem {
    pub id: String,
    pub at_s: u32,
    pub rel_path: String,
    pub title: String,
    pub caption: String,
    /// `manual` | `auto`.
    pub source: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
pub struct CallDetail {
    pub meta: CallMeta,
    pub summary: Option<String>,
    pub transcript: Vec<Utterance>,
    pub qa: Vec<QaItem>,
    pub shots: Vec<ShotItem>,
    pub keep_audio: bool,
    pub model: String,
}

fn detail(meta: CallMeta, md: &str, templates: &[Template], lang_ru: bool) -> CallDetail {
    let doc = call_codec::parse(md);
    let mut d = CallDetail {
        summary: Some(doc.summary().trim().to_string()).filter(|s| !s.is_empty()),
        keep_audio: doc.fm.audio != "none",
        model: String::new(),
        transcript: vec![],
        qa: vec![],
        shots: vec![],
        meta,
    };
    for b in doc.blocks() {
        match b {
            Block::Turn { at_s, speaker, text } => d.transcript.push(Utterance {
                at_s: *at_s,
                speaker: if matches!(speaker, Speaker::Me) {
                    Stream::Me
                } else {
                    Stream::System
                },
                text: text.clone(),
            }),
            Block::Qa(q) => {
                if d.model.is_empty() {
                    d.model = q.model.clone();
                }
                let name = templates
                    .iter()
                    .find(|t| t.id == q.template)
                    .map(|t| {
                        if lang_ru {
                            t.names.ru.clone()
                        } else {
                            t.names.en.clone()
                        }
                    })
                    .unwrap_or_else(|| q.template.clone());
                d.qa.push(QaItem {
                    id: q.id.clone(),
                    at_s: q.at_s,
                    question: q.question.clone(),
                    question_stream: Stream::System,
                    answer: q.answer.clone(),
                    template: name,
                    my_answer: q.my_answer.as_ref().map(|m| m.1.clone()),
                    my_answer_at_s: q.my_answer.as_ref().map(|m| m.0),
                });
            }
            Block::Image(i) => d.shots.push(ShotItem {
                id: i.rel_path.clone(),
                at_s: i.at_s,
                rel_path: i.rel_path.clone(),
                title: i.rel_path.rsplit('/').next().unwrap_or(&i.rel_path).to_string(),
                caption: i.caption.clone(),
                source: if i.source == "auto" {
                    "auto".into()
                } else {
                    "manual".into()
                },
            }),
            Block::Raw(_) | Block::Marker { .. } => {}
        }
    }
    d
}

#[tauri::command(async)]
#[specta::specta]
pub fn call_get(st: State<'_, Shared>, id: Ulid) -> Result<CallDetail> {
    let v = vault(&st)?;
    let call = v.read_call(&id)?;
    let s = st.settings.get();
    let ru = !matches!(s.ui.language, UiLanguage::En);
    Ok(detail(call.meta, &call.md, &s.templates, ru))
}

/// The raw `.md` (editor and export).
#[tauri::command(async)]
#[specta::specta]
pub fn call_get_md(st: State<'_, Shared>, id: Ulid) -> Result<String> {
    Ok(vault(&st)?.read_call(&id)?.md)
}

#[tauri::command(async)]
#[specta::specta]
pub fn call_save_md(st: State<'_, Shared>, id: Ulid, md: String) -> Result<()> {
    vault(&st)?.save_md(&id, &md)?;
    changed(&st, StoreChangeKind::Updated, id, false);
    Ok(())
}

#[tauri::command(async)]
#[specta::specta]
pub fn call_move(st: State<'_, Shared>, id: Ulid, to: FolderRef) -> Result<CallMeta> {
    let m = vault(&st)?.move_call(&id, &to)?;
    changed(&st, StoreChangeKind::Moved, id, false);
    Ok(m)
}

/// Moves to the Trash (restorable).
#[tauri::command(async)]
#[specta::specta]
pub fn call_delete(st: State<'_, Shared>, id: Ulid) -> Result<()> {
    vault(&st)?.delete(&id)?;
    changed(&st, StoreChangeKind::Trashed, id, false);
    Ok(())
}

#[tauri::command(async)]
#[specta::specta]
pub fn call_restore(st: State<'_, Shared>, id: Ulid) -> Result<()> {
    vault(&st)?.restore(&id)?;
    changed(&st, StoreChangeKind::Restored, id, false);
    Ok(())
}

#[tauri::command(async)]
#[specta::specta]
pub fn call_rename(st: State<'_, Shared>, id: Ulid, title: String) -> Result<CallMeta> {
    let title = title.trim().to_string();
    if title.is_empty() {
        return Err(AppError::new(ErrorCode::InvalidArgument).with_detail("empty title"));
    }
    let m = vault(&st)?.update_call(&id, |d| {
        d.fm.title = title.clone();
        d.heading = title;
    })?;
    changed(&st, StoreChangeKind::Updated, id, false);
    Ok(m)
}

#[tauri::command(async)]
#[specta::specta]
pub fn call_set_tags(st: State<'_, Shared>, id: Ulid, tags: Vec<String>) -> Result<CallMeta> {
    let m = vault(&st)?.set_call_tags(&id, tags)?;
    changed(&st, StoreChangeKind::Updated, id, false);
    Ok(m)
}

/// The only hard delete («Очистить корзину»). Returns the number of items removed.
#[tauri::command(async)]
#[specta::specta]
pub fn trash_empty(st: State<'_, Shared>) -> Result<u32> {
    Ok(vault(&st)?.empty_trash()? as u32)
}

/// `*.zip` writes a zip (Obsidian opens the unzipped result as is); anything else copies the vault to
/// a new directory.
#[tauri::command(async)]
#[specta::specta]
pub fn export_vault(st: State<'_, Shared>, dest: String, include_service: bool) -> Result<u32> {
    let v = vault(&st)?;
    let dest = PathBuf::from(dest);
    let n = if dest.extension().is_some_and(|e| e.eq_ignore_ascii_case("zip")) {
        v.export_vault_zip(&dest, include_service)?
    } else {
        v.export_vault(&dest, include_service)?
    };
    Ok(n as u32)
}

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
pub struct ImportSummary {
    pub files_copied: u32,
    pub skipped: u32,
    pub ids_reassigned: u32,
    pub folders_merged: u32,
}

/// A zip (folder or vault export) or a vault directory is merged into the open vault; the index
/// catches up by itself (imports are not reported by the watcher, so a reconcile is requested).
pub(crate) fn import_any(st: &AppState, src: &std::path::Path) -> Result<ImportSummary> {
    let v = vault(st)?;
    let r = if src.is_file() {
        v.import_zip(src)?
    } else {
        v.import_vault(src)?
    };
    if let Ok(svc) = st.svc() {
        svc.request_reconcile();
    }
    // tell the UI to reload its lists
    st.emit(AppEvent::Store(StoreChange {
        kind: StoreChangeKind::Updated,
        id: Ulid::new(),
        is_folder: true,
    }));
    Ok(ImportSummary {
        files_copied: r.files_copied as u32,
        skipped: r.skipped as u32,
        ids_reassigned: r.ids_reassigned as u32,
        folders_merged: r.folders_merged as u32,
    })
}

#[tauri::command(async)]
#[specta::specta]
pub fn import_vault(st: State<'_, Shared>, src: String) -> Result<ImportSummary> {
    import_any(&st, &PathBuf::from(src))
}
