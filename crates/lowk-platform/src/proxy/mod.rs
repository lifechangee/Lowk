//! Outbound proxy resolution: one place every HTTP(S)/WebSocket client asks.
//!
//! Order: manual override (Settings -> Network) -> environment (`HTTPS_PROXY`, `HTTP_PROXY`,
//! `ALL_PROXY`, `NO_PROXY`, upper or lower case) -> OS settings (Windows: per-user Internet Settings
//! registry key; macOS: SystemConfiguration proxies). Loopback hosts always bypass.
//!
//! The OS settings are re-read on every call (a registry read / one SystemConfiguration call), so
//! toggling a VPN in "system proxy" mode takes effect on the next connection without a restart.
//!
//! Out of scope: PAC scripts (`AutoConfigURL`, "Proxy auto-configuration"); WPAD. Those users set the
//! proxy by hand in Settings -> Network. `https://` proxies are dialled as plain `http://` (no TLS to
//! the proxy itself); SOCKS4 is not supported.
mod dial;
#[doc(hidden)]
pub mod mock;
mod os;
mod parse;

pub use dial::{connect, connect_via};
pub use parse::{
    parse_env, parse_macos, parse_proxy_server, parse_windows, parse_windows_override, Bypass, MacVal,
    ProxyUrl, Scheme,
};

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Mutex, RwLock};

/// Where the active proxy came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Manual,
    Env,
    Windows,
    Macos,
}

/// A resolved proxy configuration (not yet specific to a target).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    pub http: Option<ProxyUrl>,
    pub https: Option<ProxyUrl>,
    pub bypass: Bypass,
    pub source: Source,
}

impl Config {
    /// The proxy to use for `host`, or `None` for a direct connection.
    pub fn for_target(&self, host: &str, https: bool) -> Option<ProxyUrl> {
        if self.bypass.matches(host) {
            return None;
        }
        let (first, second) = if https {
            (&self.https, &self.http)
        } else {
            (&self.http, &self.https)
        };
        first.clone().or_else(|| second.clone())
    }

    /// The proxy shown in Settings (https one preferred: that is what API calls use).
    pub fn primary(&self) -> Option<&ProxyUrl> {
        self.https.as_ref().or(self.http.as_ref())
    }

    fn all(p: ProxyUrl, source: Source, bypass: Bypass) -> Self {
        Self {
            http: Some(p.clone()),
            https: Some(p),
            bypass,
            source,
        }
    }
}

static MANUAL: RwLock<Option<ProxyUrl>> = RwLock::new(None);

/// Sets the manual override from the settings string. Empty = use the system. An unparsable value
/// is ignored (returns false) so a typo can never silently break all traffic.
pub fn set_manual(value: &str) -> bool {
    let v = value.trim();
    let parsed = if v.is_empty() { None } else { ProxyUrl::parse(v) };
    if !v.is_empty() && parsed.is_none() {
        return false;
    }
    *MANUAL.write().unwrap_or_else(|e| e.into_inner()) = parsed;
    true
}

fn manual() -> Option<ProxyUrl> {
    MANUAL.read().unwrap_or_else(|e| e.into_inner()).clone()
}

/// Env var, either case.
fn env_get(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .or_else(|| std::env::var(name.to_ascii_lowercase()).ok())
        .filter(|v| !v.trim().is_empty())
}

/// The system (non-manual) configuration: environment first, then the OS.
pub fn system() -> Option<Config> {
    parse_env(&env_get).or_else(os::read)
}

/// The full resolution including the manual override.
pub fn current() -> Option<Config> {
    match manual() {
        Some(p) => Some(Config::all(p, Source::Manual, Bypass::default())),
        None => system(),
    }
}

/// The proxy URL (with credentials, for the HTTP client) a request to `url` must use; `None` = direct.
pub fn for_url(url: &str) -> Option<String> {
    let (host, https) = split_url(url)?;
    current()?.for_target(&host, https).map(|p| p.to_url())
}

/// `host` and whether the scheme is TLS, from `scheme://host[:port]/...`.
pub fn split_url(url: &str) -> Option<(String, bool)> {
    let (scheme, rest) = url.split_once("://")?;
    let https = matches!(scheme.to_ascii_lowercase().as_str(), "https" | "wss");
    let auth = rest.split(['/', '?', '#']).next()?;
    let auth = auth.rsplit('@').next()?;
    let host = if let Some(r) = auth.strip_prefix('[') {
        r.split(']').next()?.to_string()
    } else {
        auth.split(':').next()?.to_string()
    };
    (!host.is_empty()).then_some((host, https))
}

/// What Settings -> Network shows. Credentials are never included.
pub fn describe_system() -> Option<(String, Source)> {
    let c = system()?;
    Some((c.primary()?.display(), c.source))
}

/// Per-key cache of an HTTP client: rebuilt only when the resolved proxy for the URL changes.
pub struct ProxyCache<T: Clone>(Mutex<Option<(Option<String>, T)>>);

impl<T: Clone> Default for ProxyCache<T> {
    fn default() -> Self {
        Self(Mutex::new(None))
    }
}

impl<T: Clone> ProxyCache<T> {
    pub fn get(&self, url: &str, build: impl FnOnce(Option<&str>) -> T) -> T {
        let key = for_url(url);
        let mut g = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((k, v)) = g.as_ref() {
            if *k == key {
                return v.clone();
            }
        }
        let v = build(key.as_deref());
        *g = Some((key, v.clone()));
        v
    }
}

/// Convenience for tests / diagnostics: the parsed environment from an explicit map.
pub fn env_from(map: &HashMap<String, String>) -> Option<Config> {
    parse_env(&|k: &str| map.get(k).or_else(|| map.get(&k.to_ascii_lowercase())).cloned())
}

#[cfg(test)]
mod tests;
