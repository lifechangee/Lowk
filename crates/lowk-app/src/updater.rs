//! Auto-update: policy, state and the background loop.
//!
//! Updates come from the GitHub Releases `latest.json` of the project (one stable channel). The updater
//! plugin is only initialised when `plugins.updater.pubkey` in `tauri.conf.json` is a real minisign public
//! key; with a missing or placeholder key nothing here ever touches the network (`Block::NoKey`).
//!
//! Policy: an automatic check runs at most once a day, only when `updates.check_daily` is on and never
//! while a recording is in progress; the package is downloaded only after the user asks (or with
//! `updates.auto_install`); installing and relaunching never happen during a recording, a requested
//! restart simply waits until it has ended. Every package is verified against the public key before it
//! is installed; a bad signature is reported and the package is dropped.
use crate::state::Shared;
use lowk_types::{SessionPhase, SettingsPatch, SettingsStore, UpdateSettings};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, Runtime};
use tauri_plugin_updater::{Update, UpdaterExt};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

/// The single update source: the latest stable GitHub Release.
pub const ENDPOINT: &str = "https://github.com/lifechangee/Lowk/releases/latest/download/latest.json";
/// Automatic checks are at least this far apart.
pub const CHECK_INTERVAL_SECS: i64 = 24 * 60 * 60;
/// Release notes shown in the notice are cut to this many characters.
const NOTES_MAX: usize = 400;
const TICK: std::time::Duration = std::time::Duration::from_secs(45);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum UpdatePhase {
    Idle,
    Checking,
    Available,
    Downloading,
    Ready,
    Error,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum UpdateErrorKind {
    /// Offline, server unreachable, no release found.
    Network,
    /// The package does not match the public key: it is never installed.
    Signature,
    Other,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct UpdateInfo {
    pub version: String,
    /// Excerpt of the release notes.
    pub notes: Option<String>,
    pub date: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct UpdateStatus {
    /// False when this build has no update signing key: the updater is off and makes no requests.
    pub enabled: bool,
    pub current: String,
    pub phase: UpdatePhase,
    pub available: Option<UpdateInfo>,
    pub downloaded: u64,
    pub total: Option<u64>,
    pub error: Option<UpdateErrorKind>,
    /// A restart was requested while recording; it happens as soon as the recording ends.
    pub restart_pending: bool,
    /// The last user action was refused because a recording is running.
    pub blocked_by_recording: bool,
    pub last_checked: Option<String>,
}

/// Why a check does not happen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Block {
    NoKey,
    Disabled,
    Recording,
    TooSoon,
    /// A download or a ready package is already there.
    Busy,
}

pub struct CheckCtx<'a> {
    pub settings: &'a UpdateSettings,
    pub recording: bool,
    pub key_ok: bool,
    /// The user pressed «Проверить сейчас» (skips the daily throttle and the toggle).
    pub manual: bool,
    pub now: OffsetDateTime,
}

/// May a check run now? Pure; the single place the policy lives.
pub fn gate(c: &CheckCtx) -> Result<(), Block> {
    if !c.key_ok {
        return Err(Block::NoKey);
    }
    if c.recording {
        return Err(Block::Recording);
    }
    if !c.manual {
        if !c.settings.check_daily {
            return Err(Block::Disabled);
        }
        if let Some(last) = c.settings.last_checked.as_deref().and_then(parse_time) {
            let age = (c.now - last).whole_seconds();
            if (0..CHECK_INTERVAL_SECS).contains(&age) {
                return Err(Block::TooSoon);
            }
        }
    }
    Ok(())
}

fn parse_time(s: &str) -> Option<OffsetDateTime> {
    OffsetDateTime::parse(s, &Rfc3339).ok()
}

/// Is a found version shown? The version the user skipped stays hidden from automatic checks;
/// a manual check always shows it.
pub fn offered(version: &str, skipped: Option<&str>, manual: bool) -> bool {
    manual || skipped != Some(version)
}

/// Installing and relaunching are allowed only outside a recording (any non-idle session phase).
pub fn may_install(phase: SessionPhase) -> bool {
    phase == SessionPhase::Idle
}

/// True for a real minisign public key in Tauri's format (base64 of the `.pub` file). Empty and
/// placeholder values are not usable: the updater then stays off.
pub fn key_usable(pubkey: &str) -> bool {
    use base64::Engine;
    let Ok(raw) = base64::engine::general_purpose::STANDARD.decode(pubkey.trim()) else {
        return false;
    };
    let Ok(text) = String::from_utf8(raw) else {
        return false;
    };
    minisign_verify::PublicKey::decode(&text).is_ok()
}

/// `plugins.updater.pubkey` from the Tauri config, if any.
pub fn configured_pubkey(cfg: &tauri::Config) -> Option<String> {
    cfg.plugins
        .0
        .get("updater")?
        .get("pubkey")?
        .as_str()
        .map(str::to_string)
}

pub fn classify(e: &tauri_plugin_updater::Error) -> UpdateErrorKind {
    use tauri_plugin_updater::Error as E;
    match e {
        E::Minisign(_)
        | E::Base64(_)
        | E::SignatureUtf8(_)
        | E::SignedVersionMismatch { .. }
        | E::MissingSignedVersion => UpdateErrorKind::Signature,
        E::Reqwest(_) | E::Network(_) | E::ReleaseNotFound | E::Io(_) => UpdateErrorKind::Network,
        _ => UpdateErrorKind::Other,
    }
}

fn excerpt(notes: &str) -> String {
    let t = notes.trim();
    if t.chars().count() <= NOTES_MAX {
        return t.to_string();
    }
    let cut: String = t.chars().take(NOTES_MAX).collect();
    format!("{}…", cut.trim_end())
}

struct Inner {
    phase: UpdatePhase,
    info: Option<UpdateInfo>,
    update: Option<Update>,
    bytes: Option<Vec<u8>>,
    downloaded: u64,
    total: Option<u64>,
    error: Option<UpdateErrorKind>,
    restart_pending: bool,
    blocked_by_recording: bool,
}

/// The updater's runtime state (managed by Tauri).
pub struct Updates {
    key_ok: bool,
    inner: Mutex<Inner>,
}

impl Updates {
    pub fn new(key_ok: bool) -> Self {
        Self {
            key_ok,
            inner: Mutex::new(Inner {
                phase: UpdatePhase::Idle,
                info: None,
                update: None,
                bytes: None,
                downloaded: 0,
                total: None,
                error: None,
                restart_pending: false,
                blocked_by_recording: false,
            }),
        }
    }

    pub fn key_ok(&self) -> bool {
        self.key_ok
    }

    pub fn status(&self, last_checked: Option<String>) -> UpdateStatus {
        let i = self.inner.lock();
        UpdateStatus {
            enabled: self.key_ok,
            current: env!("CARGO_PKG_VERSION").into(),
            phase: i.phase,
            available: i.info.clone(),
            downloaded: i.downloaded,
            total: i.total,
            error: i.error,
            restart_pending: i.restart_pending,
            blocked_by_recording: i.blocked_by_recording,
            last_checked,
        }
    }

    pub fn phase(&self) -> UpdatePhase {
        self.inner.lock().phase
    }

    pub fn set_blocked(&self, b: bool) {
        self.inner.lock().blocked_by_recording = b;
    }

    fn fail(&self, kind: UpdateErrorKind) {
        let mut i = self.inner.lock();
        i.phase = UpdatePhase::Error;
        i.error = Some(kind);
        i.bytes = None;
    }

    /// Forget the offered update (skipped, or a later check found none).
    pub fn clear(&self) {
        let mut i = self.inner.lock();
        i.phase = UpdatePhase::Idle;
        i.info = None;
        i.update = None;
        i.bytes = None;
        i.error = None;
        i.restart_pending = false;
        i.downloaded = 0;
        i.total = None;
    }

    pub fn offered_version(&self) -> Option<String> {
        self.inner.lock().info.as_ref().map(|i| i.version.clone())
    }

    pub fn request_restart(&self) {
        self.inner.lock().restart_pending = true;
    }

    pub fn restart_pending(&self) -> bool {
        self.inner.lock().restart_pending
    }
}

/// Result of [`check`].
#[derive(Debug, PartialEq, Eq)]
pub enum CheckOutcome {
    Blocked(Block),
    /// A request was made (whatever it returned); the caller records the time.
    Checked,
}

/// The policy gate, then one request to `endpoint`. Needs the updater plugin on `app`.
pub async fn check<R: Runtime>(
    app: &AppHandle<R>,
    ups: &Updates,
    c: CheckCtx<'_>,
    endpoint: &url::Url,
) -> CheckOutcome {
    if let Err(b) = gate(&c) {
        ups.set_blocked(b == Block::Recording && c.manual);
        return CheckOutcome::Blocked(b);
    }
    {
        let mut i = ups.inner.lock();
        i.blocked_by_recording = false;
        if matches!(
            i.phase,
            UpdatePhase::Downloading | UpdatePhase::Ready | UpdatePhase::Checking
        ) {
            return CheckOutcome::Blocked(Block::Busy);
        }
        i.phase = UpdatePhase::Checking;
        i.error = None;
    }
    let found = async {
        let mut b = app
            .updater_builder()
            .endpoints(vec![endpoint.clone()])?
            .timeout(std::time::Duration::from_secs(30));
        // honour the system / manual proxy (re-read on every check; the download reuses it)
        if let Some(p) =
            lowk_platform::proxy::for_url(endpoint.as_str()).and_then(|p| p.parse::<url::Url>().ok())
        {
            b = b.proxy(p);
        }
        b.build()?.check().await
    }
    .await;
    match found {
        Ok(Some(u)) if offered(&u.version, c.settings.skipped_version.as_deref(), c.manual) => {
            let mut i = ups.inner.lock();
            i.info = Some(UpdateInfo {
                version: u.version.clone(),
                notes: u.body.as_deref().map(excerpt).filter(|n| !n.is_empty()),
                date: u.date.and_then(|d| d.format(&Rfc3339).ok()),
            });
            i.update = Some(u);
            i.phase = UpdatePhase::Available;
        }
        Ok(_) => ups.clear(),
        Err(e) => {
            tracing::warn!(%e, "update check failed");
            ups.fail(classify(&e));
        }
    }
    CheckOutcome::Checked
}

/// Downloads and verifies the offered package (kept in memory; nothing is installed yet).
pub async fn download(ups: &Updates) {
    let update = {
        let mut i = ups.inner.lock();
        if i.phase != UpdatePhase::Available {
            return;
        }
        let Some(u) = i.update.clone() else { return };
        i.phase = UpdatePhase::Downloading;
        i.downloaded = 0;
        i.total = None;
        i.error = None;
        u
    };
    let progress = |chunk: usize, total: Option<u64>| {
        let mut i = ups.inner.lock();
        i.downloaded += chunk as u64;
        i.total = total;
    };
    match update.download(progress, || {}).await {
        Ok(bytes) => {
            let mut i = ups.inner.lock();
            i.bytes = Some(bytes);
            i.phase = UpdatePhase::Ready;
        }
        Err(e) => {
            tracing::warn!(%e, "update download failed");
            ups.fail(classify(&e));
        }
    }
}

/// Installs a ready package and relaunches, unless a recording is running (then the restart waits).
pub fn apply(st: &Shared, ups: &Updates) {
    let (update, bytes) = {
        let mut i = ups.inner.lock();
        if i.phase != UpdatePhase::Ready {
            return;
        }
        if !may_install(st.session.state().phase) {
            i.restart_pending = true;
            return;
        }
        match (i.update.clone(), i.bytes.take()) {
            (Some(u), Some(b)) => (u, b),
            _ => return,
        }
    };
    // `install` verifies the signature again, so a package that does not match is never installed.
    match update.install(&bytes) {
        Ok(()) => {
            tracing::info!("update installed; relaunching");
            crate::commands::settings::relaunch(&st.app);
        }
        Err(e) => {
            tracing::warn!(%e, "update install failed");
            ups.fail(classify(&e));
        }
    }
}

fn recording(st: &Shared) -> bool {
    !may_install(st.session.state().phase)
}

pub fn now_rfc3339() -> String {
    OffsetDateTime::now_utc().format(&Rfc3339).unwrap_or_default()
}

/// One check for the commands and the loop: gate, request, record the time, optionally download.
pub async fn run_check(st: &Shared, ups: &Updates, manual: bool) -> CheckOutcome {
    let settings = st.settings.get().updates;
    let Ok(endpoint) = ENDPOINT.parse::<url::Url>() else {
        return CheckOutcome::Blocked(Block::NoKey);
    };
    let out = check(
        &st.app,
        ups,
        CheckCtx {
            settings: &settings,
            recording: recording(st),
            key_ok: ups.key_ok(),
            manual,
            now: OffsetDateTime::now_utc(),
        },
        &endpoint,
    )
    .await;
    if out == CheckOutcome::Checked {
        let _ = st.settings.patch(SettingsPatch(
            serde_json::json!({ "updates": { "last_checked": now_rfc3339() } }),
        ));
        if !manual && settings.auto_install && ups.phase() == UpdatePhase::Available {
            ups.request_restart();
            download(ups).await;
        }
    }
    out
}

/// Background loop: the daily check and a waiting restart. Not started without a usable key.
pub fn spawn(st: Shared, ups: std::sync::Arc<Updates>) {
    if !ups.key_ok() {
        tracing::info!("auto-update is off: this build has no update signing key");
        return;
    }
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(60)).await;
        loop {
            run_check(&st, &ups, false).await;
            if ups.restart_pending() && ups.phase() == UpdatePhase::Ready {
                apply(&st, &ups);
            }
            tokio::time::sleep(TICK).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use tauri::test::MockRuntime;

    const PUB: &str = "untrusted comment: minisign public key E7620F1842B4E81F\nRWQf6LRCGA9i53mlYecO4IzT51TGPpvWucNSCh1CBM0QTaLn73Y7GFO3\n";

    fn b64(s: &str) -> String {
        use base64::Engine;
        base64::engine::general_purpose::STANDARD.encode(s)
    }

    fn now() -> OffsetDateTime {
        OffsetDateTime::from_unix_timestamp(1_800_000_000).unwrap()
    }

    fn ago(secs: i64) -> Option<String> {
        Some((now() - time::Duration::seconds(secs)).format(&Rfc3339).unwrap())
    }

    fn ctx<'a>(s: &'a UpdateSettings, recording: bool, key_ok: bool, manual: bool) -> CheckCtx<'a> {
        CheckCtx {
            settings: s,
            recording,
            key_ok,
            manual,
            now: now(),
        }
    }

    // ---- pure policy ----

    #[test]
    fn policy_disabled_without_key_even_for_a_manual_check() {
        let s = UpdateSettings::default();
        assert_eq!(gate(&ctx(&s, false, false, false)), Err(Block::NoKey));
        assert_eq!(gate(&ctx(&s, false, false, true)), Err(Block::NoKey));
    }

    #[test]
    fn policy_never_checks_during_a_recording() {
        let s = UpdateSettings::default();
        assert_eq!(gate(&ctx(&s, true, true, false)), Err(Block::Recording));
        assert_eq!(gate(&ctx(&s, true, true, true)), Err(Block::Recording));
        assert_eq!(gate(&ctx(&s, false, true, false)), Ok(()));
    }

    #[test]
    fn policy_automatic_check_needs_the_toggle() {
        let s = UpdateSettings {
            check_daily: false,
            ..Default::default()
        };
        assert_eq!(gate(&ctx(&s, false, true, false)), Err(Block::Disabled));
        assert_eq!(
            gate(&ctx(&s, false, true, true)),
            Ok(()),
            "«Проверить сейчас» still works"
        );
    }

    #[test]
    fn policy_at_most_once_a_day() {
        let mut s = UpdateSettings {
            last_checked: ago(23 * 3600),
            ..Default::default()
        };
        assert_eq!(gate(&ctx(&s, false, true, false)), Err(Block::TooSoon));
        assert_eq!(
            gate(&ctx(&s, false, true, true)),
            Ok(()),
            "manual ignores the throttle"
        );
        s.last_checked = ago(24 * 3600 + 1);
        assert_eq!(gate(&ctx(&s, false, true, false)), Ok(()));
        s.last_checked = ago(-3600); // clock moved back: do not lock the check out
        assert_eq!(gate(&ctx(&s, false, true, false)), Ok(()));
        s.last_checked = Some("garbage".into());
        assert_eq!(gate(&ctx(&s, false, true, false)), Ok(()));
    }

    #[test]
    fn policy_skipped_version_is_hidden_from_automatic_checks_only() {
        assert!(!offered("0.2.0", Some("0.2.0"), false));
        assert!(offered("0.2.0", Some("0.2.0"), true));
        assert!(offered("0.3.0", Some("0.2.0"), false));
        assert!(offered("0.2.0", None, false));
    }

    #[test]
    fn policy_installs_only_outside_a_session() {
        assert!(may_install(SessionPhase::Idle));
        for p in [
            SessionPhase::Starting,
            SessionPhase::Recording,
            SessionPhase::Paused,
            SessionPhase::Stopping,
        ] {
            assert!(!may_install(p), "{p:?}");
        }
    }

    #[test]
    fn placeholder_and_empty_keys_keep_the_updater_off() {
        assert!(!key_usable(""));
        assert!(!key_usable("REPLACE_WITH_UPDATER_PUBLIC_KEY"));
        assert!(!key_usable(&b64("untrusted comment: placeholder\nnot-a-key\n")));
        assert!(key_usable(&b64(PUB)));
        let mut c = tauri::test::mock_context::<MockRuntime, _>(tauri::test::noop_assets());
        assert_eq!(configured_pubkey(c.config()), None);
        c.config_mut()
            .plugins
            .0
            .insert("updater".into(), serde_json::json!({ "pubkey": "x" }));
        assert_eq!(configured_pubkey(c.config()).as_deref(), Some("x"));
    }

    // ---- mock endpoint ----

    struct Mock {
        port: u16,
        hits: Arc<AtomicUsize>,
    }

    fn platform_key() -> String {
        let os = match std::env::consts::OS {
            "macos" => "darwin",
            o => o,
        };
        format!("{os}-{}", std::env::consts::ARCH)
    }

    /// Serves `/latest.json` (a fake manifest announcing 9.9.9) and `/pkg` (a fake package whose
    /// signature is garbage). Counts every request.
    fn serve(signature: &str) -> Mock {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let hits = Arc::new(AtomicUsize::new(0));
        let h2 = hits.clone();
        let manifest = serde_json::json!({
            "version": "9.9.9",
            "notes": "Fixed things. ".repeat(60),
            "pub_date": "2030-01-01T00:00:00Z",
            "platforms": { platform_key(): { "url": format!("http://127.0.0.1:{port}/pkg"), "signature": signature } },
        })
        .to_string();
        std::thread::spawn(move || {
            for s in l.incoming().flatten() {
                let mut s = s;
                let mut buf = [0u8; 4096];
                let n = s.read(&mut buf).unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]).to_string();
                h2.fetch_add(1, Ordering::SeqCst);
                let (ctype, body) = if req.starts_with("GET /latest.json") {
                    ("application/json", manifest.clone().into_bytes())
                } else {
                    ("application/octet-stream", b"not a real package".to_vec())
                };
                let _ = write!(
                    s,
                    "HTTP/1.1 200 OK\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = s.write_all(&body);
            }
        });
        Mock { port, hits }
    }

    impl Mock {
        fn url(&self) -> url::Url {
            format!("http://127.0.0.1:{}/latest.json", self.port)
                .parse()
                .unwrap()
        }
        fn hits(&self) -> usize {
            self.hits.load(Ordering::SeqCst)
        }
    }

    fn app() -> tauri::App<MockRuntime> {
        let mut c = tauri::test::mock_context(tauri::test::noop_assets());
        c.config_mut().plugins.0.insert(
            "updater".into(),
            serde_json::json!({ "pubkey": b64(PUB), "endpoints": [], "dangerousInsecureTransportProtocol": true }),
        );
        tauri::test::mock_builder()
            .plugin(tauri_plugin_updater::Builder::new().build())
            .build(c)
            .unwrap()
    }

    fn run(
        app: &tauri::App<MockRuntime>,
        ups: &Updates,
        s: &UpdateSettings,
        recording: bool,
        manual: bool,
        url: &url::Url,
    ) -> CheckOutcome {
        tauri::async_runtime::block_on(check(
            app.handle(),
            ups,
            ctx(s, recording, ups.key_ok(), manual),
            url,
        ))
    }

    #[test]
    fn auto_check_off_makes_no_request() {
        let m = serve("sig");
        let app = app();
        let ups = Updates::new(true);
        let s = UpdateSettings {
            check_daily: false,
            ..Default::default()
        };
        assert_eq!(
            run(&app, &ups, &s, false, false, &m.url()),
            CheckOutcome::Blocked(Block::Disabled)
        );
        assert_eq!(m.hits(), 0);
        assert_eq!(ups.phase(), UpdatePhase::Idle);
    }

    #[test]
    fn no_request_without_key_during_recording_or_inside_the_day() {
        let m = serve("sig");
        let app = app();
        let s = UpdateSettings::default();
        let no_key = Updates::new(false);
        assert_eq!(
            run(&app, &no_key, &s, false, true, &m.url()),
            CheckOutcome::Blocked(Block::NoKey)
        );
        assert!(!no_key.status(None).enabled);
        let ups = Updates::new(true);
        assert_eq!(
            run(&app, &ups, &s, true, true, &m.url()),
            CheckOutcome::Blocked(Block::Recording)
        );
        assert!(ups.status(None).blocked_by_recording);
        let recent = UpdateSettings {
            last_checked: Some(now_rfc3339()),
            ..Default::default()
        };
        let mut c = ctx(&recent, false, true, false);
        c.now = OffsetDateTime::now_utc();
        let out = tauri::async_runtime::block_on(check(app.handle(), &ups, c, &m.url()));
        assert_eq!(out, CheckOutcome::Blocked(Block::TooSoon));
        assert_eq!(m.hits(), 0);
    }

    #[test]
    fn daily_check_finds_the_fake_release_and_honours_skip() {
        let m = serve("sig");
        let app = app();
        let ups = Updates::new(true);
        let mut s = UpdateSettings::default();
        assert_eq!(run(&app, &ups, &s, false, false, &m.url()), CheckOutcome::Checked);
        assert_eq!(m.hits(), 1);
        let st = ups.status(None);
        assert_eq!(st.phase, UpdatePhase::Available);
        let info = st.available.unwrap();
        assert_eq!(info.version, "9.9.9");
        assert!(
            info.notes.unwrap().chars().count() <= NOTES_MAX + 1,
            "notes are an excerpt"
        );

        // the user skips this version: automatic checks no longer offer it, a manual one does
        ups.clear();
        s.skipped_version = Some("9.9.9".into());
        run(&app, &ups, &s, false, false, &m.url());
        assert_eq!(ups.phase(), UpdatePhase::Idle);
        run(&app, &ups, &s, false, true, &m.url());
        assert_eq!(ups.phase(), UpdatePhase::Available);
    }

    #[test]
    fn network_down_is_a_network_error() {
        let app = app();
        let ups = Updates::new(true);
        let dead = {
            let l = TcpListener::bind("127.0.0.1:0").unwrap();
            format!("http://127.0.0.1:{}/latest.json", l.local_addr().unwrap().port())
                .parse()
                .unwrap()
        };
        let s = UpdateSettings::default();
        assert_eq!(run(&app, &ups, &s, false, true, &dead), CheckOutcome::Checked);
        let st = ups.status(None);
        assert_eq!(
            (st.phase, st.error),
            (UpdatePhase::Error, Some(UpdateErrorKind::Network))
        );
    }

    #[test]
    fn signature_failure_is_reported_and_the_package_is_never_installed() {
        let m = serve(&b64("untrusted comment: forged\nRUQ-not-a-valid-signature\n"));
        let app = app();
        let ups = Updates::new(true);
        let s = UpdateSettings::default();
        run(&app, &ups, &s, false, true, &m.url());
        assert_eq!(ups.phase(), UpdatePhase::Available);
        tauri::async_runtime::block_on(download(&ups));
        let st = ups.status(None);
        assert_eq!(
            (st.phase, st.error),
            (UpdatePhase::Error, Some(UpdateErrorKind::Signature))
        );
        assert!(
            ups.inner.lock().bytes.is_none(),
            "an unverified package is dropped"
        );
        // nothing is Ready, so there is nothing `apply` could install
        assert_ne!(ups.phase(), UpdatePhase::Ready);
    }

    #[test]
    fn errors_are_classified_for_the_ui() {
        let bad = minisign_verify::PublicKey::decode("nope").unwrap_err();
        assert_eq!(
            classify(&tauri_plugin_updater::Error::Minisign(bad)),
            UpdateErrorKind::Signature
        );
        assert_eq!(
            classify(&tauri_plugin_updater::Error::ReleaseNotFound),
            UpdateErrorKind::Network
        );
        assert_eq!(
            classify(&tauri_plugin_updater::Error::EmptyEndpoints),
            UpdateErrorKind::Other
        );
    }

    #[test]
    fn notes_excerpt_is_cut_on_a_char_boundary() {
        let long = "й".repeat(NOTES_MAX + 50);
        let e = excerpt(&long);
        assert_eq!(e.chars().count(), NOTES_MAX + 1);
        assert!(e.ends_with('…'));
        assert_eq!(excerpt("  short \n"), "short");
    }
}
