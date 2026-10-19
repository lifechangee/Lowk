//! API keys in the OS keychain (macOS Keychain / Windows Credential Manager).
//! Service `dev.lowk.app`, account = provider id (`openrouter`, `stt-cloud`).
//! Values never appear in logs or error details; `get` is Rust-only (never expose
//! it over IPC).
use keyring::Entry;
use lowk_types::{AppError, ErrorCode, Result};

pub const SERVICE: &str = "dev.lowk.app";

fn entry(provider: &str) -> Result<Entry> {
    if provider.is_empty() {
        return Err(AppError::new(ErrorCode::InvalidArgument));
    }
    Entry::new(SERVICE, provider).map_err(map_err)
}

fn map_err(e: keyring::Error) -> AppError {
    // Display of keyring errors never contains the secret value.
    tracing::warn!(error = %e, "keychain operation failed");
    let code = match e {
        keyring::Error::NoEntry => ErrorCode::ApiKeyMissing,
        _ => ErrorCode::SecretStore,
    };
    AppError::new(code)
}

pub fn set(provider: &str, value: &str) -> Result<()> {
    if value.is_empty() {
        return Err(AppError::new(ErrorCode::InvalidArgument));
    }
    entry(provider)?.set_password(value).map_err(map_err)
}

/// Presence check. Reads the item (macOS may show an ACL prompt for unsigned
/// builds); any error counts as absent.
pub fn present(provider: &str) -> bool {
    entry(provider)
        .and_then(|e| e.get_password().map_err(map_err))
        .is_ok()
}

/// Rust-only: the value never reaches the WebView.
pub fn get(provider: &str) -> Result<String> {
    entry(provider)?.get_password().map_err(map_err)
}

/// Deleting a missing secret is not an error.
pub fn delete(provider: &str) -> Result<()> {
    match entry(provider)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(map_err(e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_empty() {
        assert!(set("", "x").is_err());
        assert!(set("p", "").is_err());
    }

    /// Touches the real keychain; run with `cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn roundtrip() {
        let p = "lowk-test-roundtrip";
        set(p, "s3cret").unwrap();
        assert!(present(p));
        assert_eq!(get(p).unwrap(), "s3cret");
        delete(p).unwrap();
        assert!(!present(p));
        delete(p).unwrap();
    }
}
