//! Unsupported OS (Linux CI): permissions do not exist.
use lowk_types::{PermKind, PermState, Result};

pub fn status(_k: PermKind) -> PermState {
    PermState::NotApplicable
}

pub fn request(_k: PermKind) -> Result<PermState> {
    Ok(PermState::NotApplicable)
}

pub fn open_url(_url: &str) {}
