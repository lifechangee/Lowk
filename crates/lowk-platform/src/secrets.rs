//! API keys in the OS keychain (macOS Keychain / Windows Credential Manager).
//! Service `dev.lowk.app`, account = provider id (`openrouter`, `stt-cloud`).
//! Values never appear in logs or error details; `get` is Rust-only (never expose
//! it over IPC).
use keyring::Entry;
use lowk_types::{AppError, ErrorCode, Result};
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use zeroize::Zeroizing;

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

/// Per-run cache: each secret is read from the keychain at most once (every read can raise a macOS
/// ACL prompt). Zeroized on drop/invalidation; invalidated by `set` and `delete`.
static CACHE: Mutex<Option<HashMap<String, Zeroizing<String>>>> = Mutex::new(None);
/// Number of real keychain reads (tests / diagnostics).
static READS: AtomicUsize = AtomicUsize::new(0);

fn cache() -> std::sync::MutexGuard<'static, Option<HashMap<String, Zeroizing<String>>>> {
    CACHE.lock().unwrap_or_else(|p| p.into_inner())
}

/// Values already held in memory (read or saved in this run). Never touches the keychain, so it
/// cannot raise a prompt: the diagnostics export uses it to scrub exact secret values from the logs.
pub fn cached_values() -> Vec<String> {
    cache()
        .as_ref()
        .map(|m| m.values().map(|v| v.to_string()).collect())
        .unwrap_or_default()
}

pub fn keychain_reads() -> usize {
    READS.load(Ordering::Relaxed)
}

fn cached_get(provider: &str, load: impl FnOnce() -> Result<String>) -> Result<String> {
    let mut g = cache();
    let map = g.get_or_insert_with(HashMap::new);
    if let Some(v) = map.get(provider) {
        return Ok(v.to_string());
    }
    READS.fetch_add(1, Ordering::Relaxed);
    let v = load()?;
    map.insert(provider.to_owned(), Zeroizing::new(v.clone()));
    Ok(v)
}

fn invalidate(provider: &str) {
    if let Some(m) = cache().as_mut() {
        m.remove(provider);
    }
}

pub fn set(provider: &str, value: &str) -> Result<()> {
    if value.is_empty() {
        return Err(AppError::new(ErrorCode::InvalidArgument));
    }
    invalidate(provider);
    entry(provider)?.set_password(value).map_err(map_err)?;
    // We just wrote it: keep it in memory so the next use does not touch the keychain.
    cache()
        .get_or_insert_with(HashMap::new)
        .insert(provider.to_owned(), Zeroizing::new(value.to_owned()));
    Ok(())
}

/// Presence check (served from the cache once the secret was read or saved in this run).
pub fn present(provider: &str) -> bool {
    get(provider).is_ok()
}

/// Rust-only: the value never reaches the WebView. Call from the main app process only.
pub fn get(provider: &str) -> Result<String> {
    // Debug/QA builds only: automated runs must never touch (or prompt for) the user's keychain.
    #[cfg(debug_assertions)]
    if std::env::var_os("LOWK_NO_KEYCHAIN").is_some() {
        return Err(AppError::new(ErrorCode::ApiKeyMissing));
    }
    cached_get(provider, || entry(provider)?.get_password().map_err(map_err))
}

/// Deleting a missing secret is not an error.
pub fn delete(provider: &str) -> Result<()> {
    invalidate(provider);
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

    #[test]
    fn reads_keychain_once_per_run() {
        let before = keychain_reads();
        let loads = std::cell::Cell::new(0);
        let load = || {
            loads.set(loads.get() + 1);
            Ok("v".to_owned())
        };
        for _ in 0..5 {
            assert_eq!(cached_get("cache-test", load).unwrap(), "v");
        }
        assert_eq!(loads.get(), 1);
        assert_eq!(keychain_reads() - before, 1);
        invalidate("cache-test");
        cached_get("cache-test", load).unwrap();
        assert_eq!(loads.get(), 2);
        invalidate("cache-test");
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
