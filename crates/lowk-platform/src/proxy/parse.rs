//! Pure parsing of proxy settings: URLs, environment variables, the Windows `ProxyServer` /
//! `ProxyOverride` strings and the macOS SystemConfiguration dictionary. No I/O.
use super::{Config, Source};
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scheme {
    Http,
    Https,
    Socks5,
}

impl Scheme {
    pub fn as_str(self) -> &'static str {
        match self {
            Scheme::Http => "http",
            Scheme::Https => "https",
            Scheme::Socks5 => "socks5",
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct ProxyUrl {
    pub scheme: Scheme,
    pub host: String,
    pub port: u16,
    pub user: Option<String>,
    pub pass: Option<String>,
}

impl std::fmt::Debug for ProxyUrl {
    // never print credentials
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.display())
    }
}

fn pct_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Some(Ok(v)) = s.get(i + 1..i + 3).map(|h| u8::from_str_radix(h, 16)) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn pct_encode(s: &str) -> String {
    let mut o = String::new();
    for c in s.chars() {
        if c.is_ascii_alphanumeric() || "-._~".contains(c) {
            o.push(c);
        } else {
            let mut buf = [0u8; 4];
            for b in c.encode_utf8(&mut buf).bytes() {
                o.push_str(&format!("%{b:02X}"));
            }
        }
    }
    o
}

impl ProxyUrl {
    /// `[scheme://][user:pass@]host[:port]`. A missing scheme means `http`. `socks`, `socks5` and
    /// `socks5h` are SOCKS5 (the name is always resolved by the proxy); SOCKS4 is rejected.
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim();
        let (scheme, rest) = match s.split_once("://") {
            Some((sc, r)) => (sc.to_ascii_lowercase(), r),
            None => ("http".to_string(), s),
        };
        let scheme = match scheme.as_str() {
            "http" => Scheme::Http,
            "https" => Scheme::Https,
            "socks" | "socks5" | "socks5h" => Scheme::Socks5,
            _ => return None,
        };
        let auth = rest.split(['/', '?', '#']).next()?;
        let (cred, hostport) = match auth.rsplit_once('@') {
            Some((c, h)) => (Some(c), h),
            None => (None, auth),
        };
        let (host, port) = if let Some(r) = hostport.strip_prefix('[') {
            let (h, after) = r.split_once(']')?;
            (h.to_string(), after.strip_prefix(':'))
        } else {
            match hostport.rsplit_once(':') {
                Some((h, p)) => (h.to_string(), Some(p)),
                None => (hostport.to_string(), None),
            }
        };
        if host.is_empty() || host.contains(char::is_whitespace) {
            return None;
        }
        let port = match port {
            Some(p) => p.parse::<u16>().ok().filter(|p| *p != 0)?,
            None => match scheme {
                Scheme::Http => 80,
                Scheme::Https => 443,
                Scheme::Socks5 => 1080,
            },
        };
        let (user, pass) = match cred {
            Some(c) => match c.split_once(':') {
                Some((u, p)) => (Some(pct_decode(u)), Some(pct_decode(p))),
                None => (Some(pct_decode(c)), None),
            },
            None => (None, None),
        };
        Some(Self {
            scheme,
            host,
            port,
            user,
            pass,
        })
    }

    fn host_for_url(&self) -> String {
        if self.host.contains(':') {
            format!("[{}]", self.host)
        } else {
            self.host.clone()
        }
    }

    /// Full URL for the HTTP client (credentials included). SOCKS5 uses `socks5h`.
    pub fn to_url(&self) -> String {
        let sc = match self.scheme {
            Scheme::Socks5 => "socks5h",
            s => s.as_str(),
        };
        let cred = match (&self.user, &self.pass) {
            (Some(u), Some(p)) => format!("{}:{}@", pct_encode(u), pct_encode(p)),
            (Some(u), None) => format!("{}@", pct_encode(u)),
            _ => String::new(),
        };
        format!("{sc}://{cred}{}:{}", self.host_for_url(), self.port)
    }

    /// For the UI and logs: no credentials.
    pub fn display(&self) -> String {
        format!("{}://{}:{}", self.scheme.as_str(), self.host_for_url(), self.port)
    }
}

/// Hosts that never go through a proxy.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Bypass {
    entries: Vec<String>,
    /// Windows `<local>` / macOS `ExcludeSimpleHostnames`: names without a dot.
    local: bool,
}

fn parse_cidr(e: &str) -> Option<(IpAddr, u8)> {
    let (ip, bits) = e.split_once('/')?;
    let bits: u8 = bits.parse().ok()?;
    let addr = match ip.parse::<IpAddr>() {
        Ok(a) => a,
        Err(_) => {
            // macOS writes "169.254/16"
            let parts: Vec<&str> = ip.split('.').collect();
            if parts.is_empty() || parts.len() > 4 {
                return None;
            }
            let mut o = [0u8; 4];
            for (i, p) in parts.iter().enumerate() {
                o[i] = p.parse().ok()?;
            }
            IpAddr::V4(Ipv4Addr::from(o))
        }
    };
    let max = if addr.is_ipv4() { 32 } else { 128 };
    (bits <= max).then_some((addr, bits))
}

fn in_cidr(host: IpAddr, net: IpAddr, bits: u8) -> bool {
    match (host, net) {
        (IpAddr::V4(h), IpAddr::V4(n)) => {
            let mask = if bits == 0 {
                0
            } else {
                u32::MAX << (32 - bits as u32)
            };
            u32::from(h) & mask == u32::from(n) & mask
        }
        (IpAddr::V6(h), IpAddr::V6(n)) => {
            let mask = if bits == 0 {
                0
            } else {
                u128::MAX << (128 - bits as u32)
            };
            u128::from(h) & mask == u128::from(n) & mask
        }
        _ => false,
    }
}

fn glob(pat: &str, s: &str) -> bool {
    let (p, t) = (pat.as_bytes(), s.as_bytes());
    let (mut pi, mut ti, mut star, mut mark) = (0, 0, None, 0);
    while ti < t.len() {
        if pi < p.len() && p[pi] == b'*' {
            star = Some(pi);
            mark = ti;
            pi += 1;
        } else if pi < p.len() && p[pi] == t[ti] {
            pi += 1;
            ti += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == b'*' {
        pi += 1;
    }
    pi == p.len()
}

/// `host:8080` -> `host` (not for bare IPv6).
fn strip_port(e: &str) -> &str {
    match e.rsplit_once(':') {
        Some((h, p)) if !h.contains(':') && p.chars().all(|c| c.is_ascii_digit()) => h,
        _ => e,
    }
}

impl Bypass {
    pub fn new<I: IntoIterator<Item = S>, S: AsRef<str>>(entries: I, local: bool) -> Self {
        Self {
            entries: entries
                .into_iter()
                .map(|e| e.as_ref().trim().to_ascii_lowercase())
                .filter(|e| !e.is_empty())
                .collect(),
            local,
        }
    }

    /// Loopback names/addresses bypass unconditionally, whatever the entries say.
    pub fn is_loopback(host: &str) -> bool {
        let h = host
            .trim_matches(['[', ']'])
            .trim_end_matches('.')
            .to_ascii_lowercase();
        h == "localhost" || h.ends_with(".localhost") || h.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback())
    }

    pub fn matches(&self, host: &str) -> bool {
        if Self::is_loopback(host) {
            return true;
        }
        let h = host
            .trim_matches(['[', ']'])
            .trim_end_matches('.')
            .to_ascii_lowercase();
        let ip = h.parse::<IpAddr>().ok();
        if self.local && ip.is_none() && !h.contains('.') {
            return true;
        }
        self.entries.iter().any(|e| {
            if e == "*" {
                return true;
            }
            if e.contains('/') {
                return match (parse_cidr(e), ip) {
                    (Some((net, bits)), Some(ip)) => in_cidr(ip, net, bits),
                    _ => false,
                };
            }
            let e = strip_port(e);
            if e.contains('*') {
                return glob(e, &h);
            }
            if let Some(dom) = e.strip_prefix('.') {
                return h == dom || h.ends_with(e);
            }
            h == e || h.ends_with(&format!(".{e}"))
        })
    }
}

/// Environment: `HTTP_PROXY`, `HTTPS_PROXY`, `ALL_PROXY`, `NO_PROXY`. `get` resolves a variable name
/// case-insensitively (the real one tries upper then lower case).
pub fn parse_env(get: &dyn Fn(&str) -> Option<String>) -> Option<Config> {
    let var = |n: &str| get(n).and_then(|v| ProxyUrl::parse(&v));
    let all = var("ALL_PROXY");
    let http = var("HTTP_PROXY").or_else(|| all.clone());
    let https = var("HTTPS_PROXY").or(all);
    if http.is_none() && https.is_none() {
        return None;
    }
    let bypass = get("NO_PROXY")
        .map(|v| Bypass::new(v.split([',', ' ']), false))
        .unwrap_or_default();
    Some(Config {
        http,
        https,
        bypass,
        source: Source::Env,
    })
}

/// Windows `ProxyServer`: `host:port` (all protocols) or `http=h:p;https=h:p;socks=h:p`.
/// Returns `(http, https)`; a `socks=` entry fills in whatever the http/https entries do not.
pub fn parse_proxy_server(s: &str) -> (Option<ProxyUrl>, Option<ProxyUrl>) {
    let (mut http, mut https, mut socks, mut plain) = (None, None, None, None);
    for part in s.split([';', ' ']).map(str::trim).filter(|p| !p.is_empty()) {
        match part.split_once('=') {
            Some((k, v)) => match k.to_ascii_lowercase().as_str() {
                "http" => http = ProxyUrl::parse(v),
                "https" => https = ProxyUrl::parse(v),
                "socks" => {
                    let v = if v.contains("://") {
                        v.to_string()
                    } else {
                        format!("socks5://{v}")
                    };
                    socks = ProxyUrl::parse(&v)
                }
                _ => {}
            },
            None => plain = plain.or_else(|| ProxyUrl::parse(part)),
        }
    }
    let fallback = plain.or(socks);
    (http.or_else(|| fallback.clone()), https.or(fallback))
}

/// Windows `ProxyOverride`: `;`-separated, wildcards, and `<local>`.
pub fn parse_windows_override(s: &str) -> Bypass {
    let mut local = false;
    let mut entries = Vec::new();
    for e in s.split([';', ',']).map(str::trim).filter(|e| !e.is_empty()) {
        if e.eq_ignore_ascii_case("<local>") {
            local = true;
        } else {
            entries.push(e.to_string());
        }
    }
    Bypass::new(entries, local)
}

/// `HKCU\...\Internet Settings`: `ProxyEnable` (DWORD), `ProxyServer`, `ProxyOverride`.
pub fn parse_windows(enable: bool, server: &str, over: &str) -> Option<Config> {
    if !enable || server.trim().is_empty() {
        return None;
    }
    let (http, https) = parse_proxy_server(server);
    if http.is_none() && https.is_none() {
        return None;
    }
    Some(Config {
        http,
        https,
        bypass: parse_windows_override(over),
        source: Source::Windows,
    })
}

/// A value of the SystemConfiguration proxies dictionary.
#[derive(Clone, Debug, PartialEq)]
pub enum MacVal {
    Num(i64),
    Str(String),
    List(Vec<String>),
}

/// macOS `SCDynamicStoreCopyProxies` dictionary (`HTTPEnable`, `HTTPProxy`, `HTTPPort`, `HTTPS*`,
/// `SOCKS*`, `ExceptionsList`, `ExcludeSimpleHostnames`).
pub fn parse_macos(d: &HashMap<String, MacVal>) -> Option<Config> {
    let num = |k: &str| match d.get(k) {
        Some(MacVal::Num(n)) => *n,
        Some(MacVal::Str(s)) => s.trim().parse().unwrap_or(0),
        _ => 0,
    };
    let one = |prefix: &str, scheme: Scheme| -> Option<ProxyUrl> {
        if num(&format!("{prefix}Enable")) != 1 {
            return None;
        }
        let host = match d.get(&format!("{prefix}Proxy")) {
            Some(MacVal::Str(h)) if !h.trim().is_empty() => h.trim().to_string(),
            _ => return None,
        };
        let port = u16::try_from(num(&format!("{prefix}Port")))
            .ok()
            .filter(|p| *p != 0)?;
        Some(ProxyUrl {
            scheme,
            host,
            port,
            user: None,
            pass: None,
        })
    };
    let (http, https, socks) = (
        one("HTTP", Scheme::Http),
        one("HTTPS", Scheme::Http),
        one("SOCKS", Scheme::Socks5),
    );
    // http(s) preferred; SOCKS only when no http(s) proxy exists
    let (http, https) = if http.is_none() && https.is_none() {
        (socks.clone(), socks)
    } else {
        (http, https)
    };
    if http.is_none() && https.is_none() {
        return None;
    }
    let list = match d.get("ExceptionsList") {
        Some(MacVal::List(l)) => l.clone(),
        _ => Vec::new(),
    };
    Some(Config {
        http,
        https,
        bypass: Bypass::new(list, num("ExcludeSimpleHostnames") == 1),
        source: Source::Macos,
    })
}
