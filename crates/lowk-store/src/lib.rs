//! Vault (Markdown files), call/folder codec, journal, settings.toml, templates.
//! M0 stub (WP4): method signatures from section 10; bodies not implemented.
use lowk_types::*;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct Vault {
    root: PathBuf,
}

#[derive(Debug)]
pub struct Call {
    pub meta: CallMeta,
    pub md: String,
}

impl Vault {
    pub fn open(p: &Path) -> Result<Self> {
        Ok(Self {
            root: p.to_path_buf(),
        })
    }
    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn folders(&self) -> Result<Vec<FolderMeta>> {
        Err(AppError::not_implemented())
    }
    pub fn create_folder(&self, _f: NewFolder) -> Result<FolderMeta> {
        Err(AppError::not_implemented())
    }
    pub fn calls(&self, _folder: Option<&Ulid>) -> Result<Vec<CallMeta>> {
        Err(AppError::not_implemented())
    }
    pub fn read_call(&self, _id: &Ulid) -> Result<Call> {
        Err(AppError::not_implemented())
    }
    pub fn begin_call(&self, _m: NewCall) -> Result<CallWriter> {
        Err(AppError::not_implemented())
    }
    pub fn save_md(&self, _id: &Ulid, _md: &str) -> Result<()> {
        Err(AppError::not_implemented())
    }
    /// Moves to the app-level Trash (restorable); never hard-deletes.
    pub fn delete(&self, _id: &Ulid) -> Result<()> {
        Err(AppError::not_implemented())
    }
    pub fn restore(&self, _id: &Ulid) -> Result<()> {
        Err(AppError::not_implemented())
    }
}

#[derive(Debug)]
pub struct CallWriter {
    entries: Vec<CallEntry>,
}
impl CallWriter {
    pub fn push(&mut self, e: CallEntry) -> Result<()> {
        self.entries.push(e);
        Ok(())
    }
    pub fn materialize(&mut self) -> Result<()> {
        Err(AppError::not_implemented())
    }
    pub fn finish(self) -> Result<CallMeta> {
        Err(AppError::not_implemented())
    }
}
