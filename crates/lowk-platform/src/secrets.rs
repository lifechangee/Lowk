//! API keys in the OS keychain (macOS Keychain / Windows Credential Manager).
//! Service `dev.lowk.app`, account = provider id (`openrouter`, `stt-cloud`).
//! Values never appear in logs or error details; `get` is Rust-only (never expose
//! it over IPC).
use keyring::Entry;
use lowk_types::{AppError, ErrorCode, Result};
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};
use zeroize::Zeroizing;

pub const SERVICE: &str = "dev.lowk.app";

/// Where secrets physically live. The real one is the OS keychain; tests plug in fakes that model the
/// Windows Credential Manager (UTF-16 blob, size cap) so the whole save -> status -> get chain is covered off-Windows.
pub trait SecretBackend: Send + Sync {
    fn set(&self, provider: &str, value: &str) -> Result<()>;
    fn get(&self, provider: &str) -> Result<String>;
    /// Deleting a missing secret is not an error.
    fn delete(&self, provider: &str) -> Result<()>;
}

struct Keyring;

fn entry(provider: &str) -> Result<Entry> {
    if provider.is_empty() {
        return Err(AppError::new(ErrorCode::InvalidArgument));
    }
    Entry::new(SERVICE, provider).map_err(map_err)
}

impl SecretBackend for Keyring {
    fn set(&self, provider: &str, value: &str) -> Result<()> {
        entry(provider)?.set_password(value).map_err(map_err)
    }
    fn get(&self, provider: &str) -> Result<String> {
        entry(provider)?.get_password().map_err(map_err)
    }
    fn delete(&self, provider: &str) -> Result<()> {
        match entry(provider)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(map_err(e)),
        }
    }
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

/// What a pasted key may look like once cleaned: no surrounding whitespace, quotes or `Bearer `, and plain
/// ASCII without inner whitespace (a key typed under the wrong keyboard layout or with a stray invisible
/// character is rejected here instead of failing later as «key not set»).
pub fn clean_key(raw: &str) -> Result<String> {
    let mut v = raw
        .trim()
        .trim_matches(|c| c == '"' || c == '\'' || c == '`')
        .trim();
    if let Some(rest) = v.strip_prefix("Bearer ").or_else(|| v.strip_prefix("bearer ")) {
        v = rest.trim();
    }
    if v.is_empty() {
        return Err(AppError::new(ErrorCode::InvalidArgument));
    }
    if !v.chars().all(|c| c.is_ascii_graphic()) {
        return Err(
            AppError::new(ErrorCode::InvalidArgument).with_detail("key has spaces or non-ASCII characters")
        );
    }
    Ok(v.to_owned())
}

/// Per-run cache around a backend: each secret is read from the keychain at most once (every read can raise a
/// macOS ACL prompt). Zeroized on drop/invalidation; updated by `set`, invalidated by `delete`.
pub struct Secrets {
    backend: Box<dyn SecretBackend>,
    cache: Mutex<HashMap<String, Zeroizing<String>>>,
    reads: AtomicUsize,
    /// Read the value back through the backend after writing it: a store that accepts a write but cannot
    /// return it (Windows Credential Manager quirks) must fail the save, never report «saved».
    verify_write: bool,
}

impl Secrets {
    pub fn new(backend: Box<dyn SecretBackend>, verify_write: bool) -> Self {
        Self {
            backend,
            cache: Mutex::new(HashMap::new()),
            reads: AtomicUsize::new(0),
            verify_write,
        }
    }

    fn cache(&self) -> std::sync::MutexGuard<'_, HashMap<String, Zeroizing<String>>> {
        self.cache.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub fn cached_values(&self) -> Vec<String> {
        self.cache().values().map(|v| v.to_string()).collect()
    }

    pub fn keychain_reads(&self) -> usize {
        self.reads.load(Ordering::Relaxed)
    }

    fn invalidate(&self, provider: &str) {
        self.cache().remove(provider);
    }

    pub fn set(&self, provider: &str, value: &str) -> Result<()> {
        if provider.is_empty() || value.is_empty() {
            return Err(AppError::new(ErrorCode::InvalidArgument));
        }
        self.invalidate(provider);
        self.backend.set(provider, value)?;
        if self.verify_write {
            self.reads.fetch_add(1, Ordering::Relaxed);
            match self.backend.get(provider) {
                Ok(back) if back == value => {}
                other => {
                    tracing::warn!(readable = other.is_ok(), "keychain write could not be read back");
                    let _ = self.backend.delete(provider);
                    return Err(AppError::new(ErrorCode::SecretStore)
                        .with_detail("saved value could not be read back"));
                }
            }
        }
        // We just wrote it: keep it in memory so the next use does not touch the keychain.
        self.cache()
            .insert(provider.to_owned(), Zeroizing::new(value.to_owned()));
        Ok(())
    }

    pub fn get(&self, provider: &str) -> Result<String> {
        let mut g = self.cache();
        if let Some(v) = g.get(provider) {
            return Ok(v.to_string());
        }
        self.reads.fetch_add(1, Ordering::Relaxed);
        let v = self.backend.get(provider)?;
        g.insert(provider.to_owned(), Zeroizing::new(v.clone()));
        Ok(v)
    }

    pub fn present(&self, provider: &str) -> bool {
        self.get(provider).is_ok()
    }

    pub fn delete(&self, provider: &str) -> Result<()> {
        self.invalidate(provider);
        self.backend.delete(provider)
    }
}

fn global() -> &'static Secrets {
    static G: OnceLock<Secrets> = OnceLock::new();
    G.get_or_init(|| Secrets::new(Box::new(Keyring), cfg!(windows)))
}

/// Values already held in memory (read or saved in this run). Never touches the keychain, so it
/// cannot raise a prompt: the diagnostics export uses it to scrub exact secret values from the logs.
pub fn cached_values() -> Vec<String> {
    global().cached_values()
}

pub fn keychain_reads() -> usize {
    global().keychain_reads()
}

/// Stores a cleaned key (see [`clean_key`]).
pub fn set(provider: &str, value: &str) -> Result<()> {
    global().set(provider, &clean_key(value)?)
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
    global().get(provider)
}

/// Deleting a missing secret is not an error.
pub fn delete(provider: &str) -> Result<()> {
    global().delete(provider)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    /// Models the Windows Credential Manager backend: target `user.service`, the secret is a UTF-16 blob
    /// capped at 2560 bytes (a bigger write is refused), a missing credential reads as `NoEntry`.
    #[derive(Default, Clone)]
    struct WinStore {
        blobs: Arc<Mutex<HashMap<String, Vec<u8>>>>,
        /// Accepts writes but never stores them (a broken persistence class).
        drop_writes: bool,
        reads: Arc<AtomicUsize>,
    }
    impl WinStore {
        fn target(provider: &str) -> String {
            format!("{provider}.{SERVICE}")
        }
    }
    impl SecretBackend for WinStore {
        fn set(&self, provider: &str, value: &str) -> Result<()> {
            let blob: Vec<u8> = value.encode_utf16().flat_map(u16::to_le_bytes).collect();
            if blob.len() > 2560 {
                return Err(AppError::new(ErrorCode::SecretStore));
            }
            if !self.drop_writes {
                self.blobs.lock().unwrap().insert(Self::target(provider), blob);
            }
            Ok(())
        }
        fn get(&self, provider: &str) -> Result<String> {
            self.reads.fetch_add(1, Ordering::Relaxed);
            let g = self.blobs.lock().unwrap();
            let blob = g
                .get(&Self::target(provider))
                .ok_or_else(|| AppError::new(ErrorCode::ApiKeyMissing))?;
            let u16s: Vec<u16> = blob
                .chunks_exact(2)
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .collect();
            String::from_utf16(&u16s).map_err(|_| AppError::new(ErrorCode::SecretStore))
        }
        fn delete(&self, provider: &str) -> Result<()> {
            self.blobs.lock().unwrap().remove(&Self::target(provider));
            Ok(())
        }
    }

    fn secrets(store: &WinStore, verify: bool) -> Secrets {
        Secrets::new(Box::new(store.clone()), verify)
    }

    #[test]
    fn rejects_empty() {
        let s = secrets(&WinStore::default(), true);
        assert!(s.set("", "x").is_err());
        assert!(s.set("p", "").is_err());
    }

    #[test]
    fn save_status_get_roundtrip_on_a_windows_like_store() {
        let store = WinStore::default();
        let s = secrets(&store, true);
        assert!(!s.present("openrouter"));
        s.set("openrouter", "sk-or-v1-abcdef").unwrap();
        assert!(s.present("openrouter"));
        assert_eq!(s.get("openrouter").unwrap(), "sk-or-v1-abcdef");
        // a second process / a restart reads the same value from the store itself
        let fresh = secrets(&store, true);
        assert_eq!(fresh.get("openrouter").unwrap(), "sk-or-v1-abcdef");
        assert_eq!(fresh.get("stt-cloud").unwrap_err().code, ErrorCode::ApiKeyMissing);
    }

    #[test]
    fn save_replaces_a_cached_value_and_delete_clears_it() {
        let store = WinStore::default();
        let s = secrets(&store, false);
        s.set("p", "old").unwrap();
        assert_eq!(s.get("p").unwrap(), "old");
        s.set("p", "new").unwrap();
        assert_eq!(
            s.get("p").unwrap(),
            "new",
            "save must not leave the stale cached key"
        );
        s.delete("p").unwrap();
        assert!(!s.present("p"), "delete must drop the cached key");
        s.delete("p").unwrap();
        assert!(s.cached_values().is_empty());
    }

    #[test]
    fn a_store_that_cannot_return_the_write_fails_the_save() {
        let store = WinStore {
            drop_writes: true,
            ..WinStore::default()
        };
        let s = secrets(&store, true);
        assert_eq!(
            s.set("openrouter", "sk-or-v1-abc").unwrap_err().code,
            ErrorCode::SecretStore
        );
        assert!(
            !s.present("openrouter"),
            "no «saved» for a key that is not readable"
        );
    }

    #[test]
    fn oversized_blob_is_refused_not_silently_truncated() {
        let s = secrets(&WinStore::default(), true);
        assert!(s.set("p", &"k".repeat(1281)).is_err());
        assert!(s.set("p", &"k".repeat(1280)).is_ok());
    }

    #[test]
    fn reads_the_store_once_per_run() {
        let store = WinStore::default();
        let s = secrets(&store, false);
        store.set("p", "v").unwrap();
        for _ in 0..5 {
            assert_eq!(s.get("p").unwrap(), "v");
        }
        assert_eq!(store.reads.load(Ordering::Relaxed), 1);
        assert_eq!(s.keychain_reads(), 1);
    }

    #[test]
    fn pasted_keys_are_cleaned_or_refused() {
        assert_eq!(clean_key("  sk-or-v1-abc\r\n").unwrap(), "sk-or-v1-abc");
        assert_eq!(clean_key("\"sk-or-v1-abc\"").unwrap(), "sk-or-v1-abc");
        assert_eq!(clean_key("Bearer sk-or-v1-abc").unwrap(), "sk-or-v1-abc");
        assert!(clean_key("sk-or-v1-аbc").is_err(), "a Cyrillic look-alike letter");
        assert!(clean_key("sk-or v1").is_err());
        assert!(clean_key("sk-or-v1-abc\u{200b}").is_err());
        assert!(clean_key("   ").is_err());
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
