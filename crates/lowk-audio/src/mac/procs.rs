//! Core Audio process objects, grouped by owning app, plus app metadata (name, bundle id,
//! icon) for the source picker.
use cidre::core_audio as ca;
use lowk_types::{AppError, ErrorCode, Result, SourceInfo, SourceKind};
use std::collections::{BTreeMap, HashMap};
use std::sync::{Mutex, OnceLock};

#[derive(Debug, Clone)]
pub struct ProcInfo {
    /// Core Audio process object id (what `CATapDescription` takes).
    pub obj: u32,
    pub pid: i32,
    pub bundle_id: String,
    pub path: String,
    pub running_output: bool,
    pub running_input: bool,
}

#[derive(Debug, Clone)]
pub struct Group {
    /// Outermost `.app` path, or `pid:N` for a bare process.
    pub key: String,
    pub name: String,
    pub bundle_id: String,
    /// Main process (the app's own executable, else the lowest pid).
    pub main_pid: i32,
    pub members: Vec<ProcInfo>,
}

impl Group {
    pub fn any_output(&self) -> bool {
        self.members.iter().any(|m| m.running_output)
    }
    /// Sorted process object ids: the tap membership signature.
    pub fn objs(&self) -> Vec<u32> {
        let mut v: Vec<u32> = self.members.iter().map(|m| m.obj).collect();
        v.sort_unstable();
        v
    }
}

fn io_err(what: &str, e: impl std::fmt::Debug) -> AppError {
    AppError::new(ErrorCode::AudioDevice).with_detail(format!("{what}: {e:?}"))
}

pub fn pid_path(pid: i32) -> String {
    let mut buf = vec![0u8; 4096];
    // SAFETY: buffer is valid for `buf.len()` bytes; proc_pidpath writes at most that many.
    let n = unsafe { libc::proc_pidpath(pid, buf.as_mut_ptr().cast(), buf.len() as u32) };
    if n <= 0 {
        return String::new();
    }
    String::from_utf8_lossy(&buf[..n as usize]).into_owned()
}

extern "C" {
    // Private but long-stable libsystem symbol (used by Activity Monitor, `launchctl`):
    // the process that "owns" an XPC helper, e.g. com.apple.WebKit.GPU -> Safari.
    fn responsibility_get_pid_responsible_for_pid(pid: libc::pid_t) -> libc::pid_t;
}

fn responsible_pid(pid: i32) -> i32 {
    // SAFETY: plain syscall wrapper taking and returning integers.
    unsafe { responsibility_get_pid_responsible_for_pid(pid) }
}

/// Outermost `.app` bundle containing this executable path.
fn outer_app(path: &str) -> Option<String> {
    let i = path.find(".app/")?;
    Some(path[..i + 4].to_string())
}

struct AppMeta {
    name: String,
    bundle_id: String,
    background_only: bool,
}

fn meta_cache() -> &'static Mutex<HashMap<String, Option<std::sync::Arc<AppMeta>>>> {
    static C: OnceLock<Mutex<HashMap<String, Option<std::sync::Arc<AppMeta>>>>> = OnceLock::new();
    C.get_or_init(Default::default)
}

fn app_meta(app: &str) -> Option<std::sync::Arc<AppMeta>> {
    if let Some(m) = meta_cache().lock().unwrap().get(app) {
        return m.clone();
    }
    let m = plist::Value::from_file(format!("{app}/Contents/Info.plist"))
        .ok()
        .and_then(|v| {
            let d = v.as_dictionary()?;
            let s = |k: &str| d.get(k).and_then(|v| v.as_string()).map(str::to_string);
            let b = |k: &str| d.get(k).and_then(|v| v.as_boolean()).unwrap_or(false);
            let stem = std::path::Path::new(app)
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            Some(std::sync::Arc::new(AppMeta {
                name: s("CFBundleDisplayName")
                    .or_else(|| s("CFBundleName"))
                    .unwrap_or(stem),
                bundle_id: s("CFBundleIdentifier").unwrap_or_default(),
                background_only: b("LSBackgroundOnly") || b("LSUIElement"),
            }))
        });
    meta_cache().lock().unwrap().insert(app.to_string(), m.clone());
    m
}

pub fn list_processes() -> Result<Vec<ProcInfo>> {
    let mut v = vec![];
    for p in ca::System::processes().map_err(|e| io_err("process list", e))? {
        let pid = p.pid().unwrap_or(-1);
        v.push(ProcInfo {
            obj: p.0 .0,
            pid,
            bundle_id: p.bundle_id().map(|s| s.to_string()).unwrap_or_default(),
            path: pid_path(pid),
            running_output: p.is_running_output().unwrap_or(false),
            running_input: p.is_running_input().unwrap_or(false),
        });
    }
    Ok(v)
}

/// Groups helper processes under the owning `.app` (Chrome Helper -> Chrome). Helpers that
/// live outside any app (system XPC services such as `com.apple.WebKit.GPU`) are mapped to
/// their responsible process (Safari).
pub fn groups() -> Result<Vec<Group>> {
    let own = std::process::id() as i32;
    let mut m: BTreeMap<String, Group> = BTreeMap::new();
    for p in list_processes()? {
        let mut app = outer_app(&p.path);
        // Only XPC services (WebKit.GPU, ...) borrow their owner: a plain CLI tool such as
        // `afplay` started from some app is its own source, not part of that app.
        if app.is_none() && p.pid > 0 && p.path.contains(".xpc/") {
            let r = responsible_pid(p.pid);
            if r > 0 && r != p.pid {
                app = outer_app(&pid_path(r));
            }
        }
        let (key, name, bid) = match app {
            Some(app) => match app_meta(&app) {
                Some(meta) => (app.clone(), meta.name.clone(), meta.bundle_id.clone()),
                None => {
                    let n = std::path::Path::new(&app)
                        .file_stem()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    (app, n, p.bundle_id.clone())
                }
            },
            None => {
                let name = std::path::Path::new(&p.path)
                    .file_name()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "?".into());
                (format!("pid:{}", p.pid), name, p.bundle_id.clone())
            }
        };
        let g = m.entry(key.clone()).or_insert(Group {
            key,
            name,
            bundle_id: bid,
            main_pid: p.pid,
            members: vec![],
        });
        if p.pid == own {
            // Never offer (or tap) ourselves.
            continue;
        }
        g.members.push(p);
    }
    let mut all: Vec<Group> = m.into_values().filter(|g| !g.members.is_empty()).collect();
    // Bare processes whose bundle id belongs to an app group (e.g. Chrome running from a
    // code-sign clone outside /Applications) join that app.
    let (mut out, bare): (Vec<Group>, Vec<Group>) = all.drain(..).partition(|g| g.key.ends_with(".app"));
    for b in bare {
        let host = (!b.bundle_id.is_empty())
            .then(|| {
                out.iter_mut().find(|a| {
                    !a.bundle_id.is_empty()
                        && (a.bundle_id == b.bundle_id
                            || b.bundle_id.starts_with(&format!("{}.", a.bundle_id)))
                })
            })
            .flatten();
        match host {
            Some(a) => a.members.extend(b.members),
            None => out.push(b),
        }
    }
    for g in &mut out {
        g.main_pid = g
            .members
            .iter()
            .find(|p| {
                !p.path.contains("Helper")
                    && p.path.contains("/Contents/MacOS/")
                    && !p.path.contains("/Contents/Frameworks/")
            })
            .or_else(|| g.members.iter().min_by_key(|p| p.pid))
            .map(|p| p.pid)
            .unwrap_or(0);
    }
    Ok(out)
}

/// Resolves a `Target::App` id (bundle id; also accepts a pid or app name) to a group.
pub fn resolve_app(spec: &str) -> Result<Group> {
    let gs = groups()?;
    if let Ok(pid) = spec.parse::<i32>() {
        return gs
            .into_iter()
            .find(|g| g.members.iter().any(|m| m.pid == pid))
            .ok_or_else(|| {
                AppError::new(ErrorCode::AudioTargetLost)
                    .with_detail(format!("pid {pid} has no audio process object yet"))
            });
    }
    let s = spec.to_lowercase();
    gs.into_iter()
        .find(|g| {
            g.bundle_id.to_lowercase() == s
                || g.members.iter().any(|m| m.bundle_id.to_lowercase() == s)
                || g.name.to_lowercase() == s
        })
        .ok_or_else(|| {
            AppError::new(ErrorCode::AudioTargetLost).with_detail(format!("no audio process for '{spec}'"))
        })
}

/// What a capture target IS, independent of its pid: survives the app restarting its audio
/// helper (S5). A pid spec such as `afplay`'s is only valid for one process lifetime.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    pub bundle_id: String,
    pub name: String,
    /// Executable path of the main process (distinguishes bare CLI tools by what they run).
    pub exe: String,
}

pub fn identity_of(g: &Group) -> Identity {
    let exe = g
        .members
        .iter()
        .find(|m| m.pid == g.main_pid)
        .or_else(|| g.members.first())
        .map(|m| m.path.clone())
        .unwrap_or_default();
    Identity {
        bundle_id: g.bundle_id.clone(),
        name: g.name.clone(),
        exe,
    }
}

/// The group that is the same app as `id` (pure, testable).
pub fn match_identity<'a>(groups: &'a [Group], id: &Identity) -> Option<&'a Group> {
    groups.iter().find(|g| {
        (!id.bundle_id.is_empty() && g.bundle_id == id.bundle_id)
            || (!id.exe.is_empty() && g.members.iter().any(|m| m.path == id.exe))
    })
}

/// `resolve_app`, falling back to the remembered identity when the exact spec (a dead pid)
/// no longer resolves.
pub fn resolve_app_or_identity(spec: &str, id: Option<&Identity>) -> Result<Group> {
    match resolve_app(spec) {
        Ok(g) => Ok(g),
        Err(e) => {
            if let Some(id) = id {
                let gs = groups()?;
                if let Some(g) = match_identity(&gs, id) {
                    return Ok(g.clone());
                }
            }
            Err(e)
        }
    }
}

/// Re-reads one group's current membership (process set changes while capturing).
pub fn refresh_group(key: &str) -> Option<Group> {
    groups().ok()?.into_iter().find(|g| g.key == key)
}

/// Process objects of our own process tree (the app plus the helpers it is responsible for,
/// e.g. its WebKit processes): excluded from the whole-system tap.
pub fn own_tree_objs() -> Vec<u32> {
    let me = std::process::id() as i32;
    // A bundle started by Finder/launchd is responsible for its helpers. A binary started from a
    // terminal or another app (`just dev`, the QA harness) is not: its WebKit helpers are
    // attributed to that host, so also take the WebKit XPC helpers of the same host created after us.
    let host = responsible_pid(me);
    let from_host = host > 0 && host != me;
    let mut v: Vec<u32> = list_processes()
        .unwrap_or_default()
        .iter()
        .filter(|p| {
            p.pid == me
                || (p.pid > 0 && responsible_pid(p.pid) == me)
                || (from_host
                    && p.pid > me
                    && responsible_pid(p.pid) == host
                    && p.path.contains("/com.apple.WebKit."))
        })
        .map(|p| p.obj)
        .collect();
    v.sort_unstable();
    v
}

/// True when something other than us is currently producing output.
pub fn system_active() -> bool {
    let me = std::process::id() as i32;
    list_processes()
        .map(|v| v.iter().any(|p| p.pid != me && p.running_output))
        .unwrap_or(false)
}

// ---------------------------------------------------------------- source picker

pub fn system_source() -> SourceInfo {
    SourceInfo {
        id: "system".into(),
        name: "System".into(),
        bundle_id: String::new(),
        pid: 0,
        icon_png_b64: None,
        producing_audio: system_active(),
        kind: SourceKind::System,
    }
}

fn source_of(g: &Group, with_icon: bool) -> SourceInfo {
    SourceInfo {
        id: if g.bundle_id.is_empty() {
            g.main_pid.to_string()
        } else {
            g.bundle_id.clone()
        },
        name: g.name.clone(),
        bundle_id: g.bundle_id.clone(),
        pid: g.main_pid.max(0) as u32,
        icon_png_b64: if with_icon && g.key.ends_with(".app") {
            icon_png_b64(&g.key)
        } else {
            None
        },
        producing_audio: g.any_output(),
        kind: SourceKind::App,
    }
}

/// System + apps that have a Core Audio process object, producing ones first.
pub fn list_sources() -> Result<Vec<SourceInfo>> {
    // Daemons (no .app) only show up while they actually play something.
    let mut v: Vec<SourceInfo> = groups()?
        .iter()
        .filter(|g| g.key.ends_with(".app") || g.any_output())
        .map(|g| source_of(g, true))
        .collect();
    v.sort_by(|a, b| {
        b.producing_audio
            .cmp(&a.producing_audio)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    v.insert(0, system_source());
    Ok(v)
}

/// Installed apps (so a call app can be chosen before it starts playing audio).
pub fn list_installed() -> Result<Vec<SourceInfo>> {
    let live = groups()?;
    let mut out: BTreeMap<String, SourceInfo> = BTreeMap::new();
    let home = std::env::var("HOME").unwrap_or_default();
    let roots = [
        "/Applications".to_string(),
        "/Applications/Utilities".to_string(),
        "/System/Applications".to_string(),
        "/System/Applications/Utilities".to_string(),
        format!("{home}/Applications"),
    ];
    for root in roots {
        let Ok(rd) = std::fs::read_dir(&root) else {
            continue;
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().and_then(|s| s.to_str()) != Some("app") {
                continue;
            }
            let path = p.to_string_lossy().into_owned();
            let Some(meta) = app_meta(&path) else { continue };
            if meta.bundle_id.is_empty() || meta.background_only {
                continue;
            }
            let running = live.iter().find(|g| g.bundle_id == meta.bundle_id);
            out.entry(meta.bundle_id.clone()).or_insert_with(|| SourceInfo {
                id: meta.bundle_id.clone(),
                name: meta.name.clone(),
                bundle_id: meta.bundle_id.clone(),
                pid: running.map(|g| g.main_pid.max(0) as u32).unwrap_or(0),
                icon_png_b64: icon_png_b64(&path),
                producing_audio: running.map(|g| g.any_output()).unwrap_or(false),
                kind: SourceKind::App,
            });
        }
    }
    let mut v: Vec<SourceInfo> = out.into_values().collect();
    v.sort_by(|a, b| {
        b.producing_audio
            .cmp(&a.producing_audio)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(v)
}

// ---------------------------------------------------------------- icons

fn icon_cache() -> &'static Mutex<HashMap<String, Option<String>>> {
    static C: OnceLock<Mutex<HashMap<String, Option<String>>>> = OnceLock::new();
    C.get_or_init(Default::default)
}

/// Base64 PNG of the app icon, <= 64 px, read from the bundle's `.icns` (no AppKit).
pub fn icon_png_b64(app: &str) -> Option<String> {
    if let Some(c) = icon_cache().lock().unwrap().get(app) {
        return c.clone();
    }
    let r = icns_png(app).map(|png| {
        use base64::Engine;
        base64::engine::general_purpose::STANDARD.encode(png)
    });
    icon_cache().lock().unwrap().insert(app.to_string(), r.clone());
    r
}

fn icns_png(app: &str) -> Option<Vec<u8>> {
    let v = plist::Value::from_file(format!("{app}/Contents/Info.plist")).ok()?;
    let d = v.as_dictionary()?;
    let mut name = d.get("CFBundleIconFile")?.as_string()?.to_string();
    if !name.ends_with(".icns") {
        name.push_str(".icns");
    }
    let data = std::fs::read(format!("{app}/Contents/Resources/{name}")).ok()?;
    pick_icns_png(&data, 64)
}

/// Smallest embedded PNG whose edge is >= `want` px (else the largest); icns entries are
/// `[type:4][len:4 BE][payload]` after an 8-byte `icns` header.
pub fn pick_icns_png(data: &[u8], want: u32) -> Option<Vec<u8>> {
    if data.len() < 8 || &data[..4] != b"icns" {
        return None;
    }
    let mut best: Option<(u32, &[u8])> = None;
    let mut i = 8;
    while i + 8 <= data.len() {
        let len = u32::from_be_bytes(data[i + 4..i + 8].try_into().ok()?) as usize;
        if len < 8 || i + len > data.len() {
            break;
        }
        let payload = &data[i + 8..i + len];
        if payload.len() > 24 && payload.starts_with(&[0x89, b'P', b'N', b'G']) {
            // IHDR width at byte 16
            let w = u32::from_be_bytes(payload[16..20].try_into().ok()?);
            let better = match best {
                None => true,
                Some((bw, _)) => (w >= want && (bw < want || w < bw)) || (bw < want && w > bw),
            };
            if better {
                best = Some((w, payload));
            }
        }
        i += len;
    }
    best.map(|(_, p)| p.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(w: u32) -> Vec<u8> {
        let mut p = vec![
            0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 13, b'I', b'H', b'D', b'R',
        ];
        p.extend(w.to_be_bytes());
        p.extend([0u8; 16]);
        p
    }
    fn entry(t: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        let mut e = t.to_vec();
        e.extend(((payload.len() + 8) as u32).to_be_bytes());
        e.extend(payload);
        e
    }

    #[test]
    fn icns_picks_smallest_at_least_wanted() {
        let mut f = b"icns".to_vec();
        f.extend([0, 0, 0, 0]);
        for (t, w) in [(b"icp4", 16u32), (b"icp6", 64), (b"ic07", 128), (b"ic10", 1024)] {
            f.extend(entry(t, &png(w)));
        }
        f.extend(entry(b"is32", &[1, 2, 3]));
        let got = pick_icns_png(&f, 64).unwrap();
        assert_eq!(u32::from_be_bytes(got[16..20].try_into().unwrap()), 64);
        let got = pick_icns_png(&f, 4096).unwrap();
        assert_eq!(u32::from_be_bytes(got[16..20].try_into().unwrap()), 1024);
        assert!(pick_icns_png(b"nope", 64).is_none());
    }

    fn grp(bundle: &str, name: &str, pid: i32, path: &str) -> Group {
        Group {
            key: format!("pid:{pid}"),
            name: name.into(),
            bundle_id: bundle.into(),
            main_pid: pid,
            members: vec![ProcInfo {
                obj: pid as u32,
                pid,
                bundle_id: bundle.into(),
                path: path.into(),
                running_output: true,
                running_input: false,
            }],
        }
    }

    #[test]
    fn s5_identity_survives_a_pid_change() {
        let before = grp("", "afplay", 100, "/usr/bin/afplay");
        let id = identity_of(&before);
        // the process restarted under a new pid
        let after = vec![
            grp("", "Safari", 7, "/Applications/Safari.app/x"),
            grp("", "afplay", 205, "/usr/bin/afplay"),
        ];
        assert_eq!(match_identity(&after, &id).unwrap().main_pid, 205);
        // an app is found again by bundle id even when its binary moved
        let app = identity_of(&grp("us.zoom.xos", "zoom.us", 1, "/A/zoom"));
        let gs = vec![grp("us.zoom.xos", "zoom.us", 9, "/B/zoom")];
        assert_eq!(match_identity(&gs, &app).unwrap().main_pid, 9);
        assert!(match_identity(&[grp("", "other", 3, "/usr/bin/other")], &id).is_none());
    }

    #[test]
    fn outer_app_is_outermost() {
        assert_eq!(
            outer_app("/Applications/Google Chrome.app/Contents/Frameworks/X.framework/Helpers/Google Chrome Helper.app/Contents/MacOS/h").as_deref(),
            Some("/Applications/Google Chrome.app")
        );
        assert_eq!(outer_app("/usr/bin/afplay"), None);
    }

    #[test]
    fn live_listing_does_not_panic() {
        // Real Core Audio, no permission needed.
        let v = list_sources().unwrap();
        assert_eq!(v[0].kind, SourceKind::System);
    }
}
