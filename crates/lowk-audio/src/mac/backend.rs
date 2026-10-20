//! macOS `AudioBackend`: one supervisor thread owns the mic session and the tap session and
//! keeps them healthy (ARCHITECTURE 6.1):
//!
//! * mic first, then the tap (own-process exclusion needs our process object);
//! * rebuild on default-output change / rate change, on target process-set change, and via
//!   the silent-tap watchdog (>= 2 s exact zeros AND target running output AND >= 5 s since the
//!   last rebuild);
//! * a missing target (app not producing audio yet, or gone) is polled, never fatal;
//! * 10 Hz `Level`, 1 Hz `Dropped`.
use super::devices::{self, OutInfo};
use super::procs::{self, Group};
use super::tap::{self, MicSession, TapSession, TapTarget};
use crate::aec::AecLink;
use crate::pipeline::{spawn_worker, Meter, WorkerHandle};
use crate::{
    AecMode, AudioBackend, AudioEvent, AudioEventSink, CaptureConfig, CaptureHandle, FrameSink, MIC_NONE,
};
use lowk_platform::perm;
use lowk_types::{AppError, AudioApp, DeviceInfo, ErrorCode, PermKind, PermState, Result, Stream, Target};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::channel;
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// Core Audio backend (taps need macOS 14.2+; the app bundle declares 14.4).
#[derive(Default)]
pub struct CoreAudio;

impl AudioBackend for CoreAudio {
    fn list_apps(&self) -> Result<Vec<AudioApp>> {
        Ok(procs::groups()?
            .into_iter()
            .map(|g| AudioApp {
                id: if g.bundle_id.is_empty() {
                    g.main_pid.to_string()
                } else {
                    g.bundle_id.clone()
                },
                name: g.name.clone(),
                producing_audio: g.any_output(),
                pids: g.members.iter().map(|m| m.pid.max(0) as u32).collect(),
            })
            .collect())
    }

    fn list_mics(&self) -> Result<Vec<DeviceInfo>> {
        devices::list_mics()
    }

    fn start(
        &mut self,
        cfg: &CaptureConfig,
        sink: FrameSink,
        ev: AudioEventSink,
    ) -> Result<Box<dyn CaptureHandle>> {
        let stop = Arc::new(AtomicBool::new(false));
        let (ready_tx, ready_rx) = channel::<Result<()>>();
        let (cfg2, stop2) = (cfg.clone(), stop.clone());
        let join = std::thread::Builder::new()
            .name("audio-sup".into())
            .spawn(move || Supervisor::run(cfg2, sink, ev, stop2, ready_tx))
            .map_err(|e| AppError::new(ErrorCode::AudioDevice).with_detail(e.to_string()))?;
        let handle = Handle {
            stop,
            join: Some(join),
        };
        // First attempt includes any TCC prompt the user has to answer.
        match ready_rx.recv_timeout(Duration::from_secs(330)) {
            Ok(Ok(())) => Ok(Box::new(handle)),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(AppError::new(ErrorCode::AudioDevice).with_detail("capture start timed out")),
        }
    }
}

struct Handle {
    stop: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
}
impl CaptureHandle for Handle {}
impl Drop for Handle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

const TICK: Duration = Duration::from_millis(100);
const ZERO_MS_TRIP: u64 = 2_000;
const REBUILD_COOLDOWN: Duration = Duration::from_secs(5);

struct Supervisor {
    cfg: CaptureConfig,
    ev: AudioEventSink,
    me_w: Option<WorkerHandle>,
    sys_w: Option<WorkerHandle>,
    aec: AecLink,
    mic: Option<MicSession>,
    tap: Option<TapSession>,
    /// Process-object set the current tap was built for (App targets / excluded set for System).
    tap_objs: Vec<u32>,
    last_rebuild: Instant,
    last_tap_try: Instant,
    last_mic_try: Instant,
    tap_fail_count: u32,
    watchdog_trips: u32,
    target_gone_since: Option<Instant>,
    target_lost_sent: bool,
    out: Option<OutInfo>,
    mic_zero_checked: bool,
    sys_perm_reported: bool,
}

impl Supervisor {
    fn want_mic(&self) -> bool {
        self.cfg.mic.as_deref() != Some(MIC_NONE)
    }

    fn run(
        cfg: CaptureConfig,
        sink: FrameSink,
        ev: AudioEventSink,
        stop: Arc<AtomicBool>,
        ready: std::sync::mpsc::Sender<Result<()>>,
    ) {
        let aec = AecLink::new(false);
        let want_mic = cfg.mic.as_deref() != Some(MIC_NONE);
        let me_w = want_mic.then(|| spawn_worker(Stream::Me, sink.clone(), ev.clone(), Some(aec.clone())));
        let sys_w = cfg
            .system
            .is_some()
            .then(|| spawn_worker(Stream::System, sink.clone(), ev.clone(), Some(aec.clone())));
        let mut s = Supervisor {
            cfg,
            ev: ev.clone(),
            me_w,
            sys_w,
            aec,
            mic: None,
            tap: None,
            tap_objs: vec![],
            last_rebuild: Instant::now(),
            last_tap_try: Instant::now(),
            last_mic_try: Instant::now(),
            tap_fail_count: 0,
            watchdog_trips: 0,
            target_gone_since: None,
            target_lost_sent: false,
            out: devices::default_output(),
            mic_zero_checked: false,
            sys_perm_reported: false,
        };

        // ---- first attempt: mic, settle, tap
        let mic_res = if s.want_mic() { Some(s.start_mic()) } else { None };
        std::thread::sleep(Duration::from_millis(150));
        let tap_res = if s.cfg.system.is_some() {
            Some(s.start_tap())
        } else {
            None
        };
        s.update_aec();
        let hard_fail =
            |r: &Option<Result<()>>| matches!(r, Some(Err(e)) if e.code != ErrorCode::AudioTargetLost);
        let both_failed = [&mic_res, &tap_res]
            .iter()
            .all(|r| r.as_ref().map(|r| r.is_err()).unwrap_or(true));
        let first_err = mic_res
            .iter()
            .chain(tap_res.iter())
            .find_map(|r| r.as_ref().err().cloned());
        let fatal = both_failed && (hard_fail(&mic_res) || hard_fail(&tap_res)) && first_err.is_some();
        if fatal {
            let _ = ready.send(Err(first_err.unwrap()));
            return; // workers drop with `s`
        }
        let _ = ready.send(Ok(()));

        let mut meter = Meter::new(
            s.me_w.as_ref().map(|w| w.shared.clone()),
            s.sys_w.as_ref().map(|w| w.shared.clone()),
            ev,
        );
        let mut tick = 0u32;
        while !stop.load(Ordering::Relaxed) {
            std::thread::sleep(TICK);
            tick += 1;
            meter.tick();
            if tick % 3 == 0 {
                s.check();
            }
        }
        // Stop the HAL first (blocks until IOProcs returned), then the workers.
        s.tap = None;
        s.mic = None;
    }

    fn emit(&self, e: AudioEvent) {
        (self.ev)(e);
    }

    fn mic_uid(&self) -> Option<&str> {
        self.cfg
            .mic
            .as_deref()
            .filter(|m| !m.is_empty() && *m != "default")
    }

    #[allow(clippy::collapsible_match)]
    fn start_mic(&mut self) -> Result<()> {
        self.last_mic_try = Instant::now();
        match perm::status(PermKind::Microphone) {
            PermState::Denied => {
                self.emit(AudioEvent::Permission(PermKind::Microphone));
                return Err(AppError::new(ErrorCode::PermMicrophone));
            }
            PermState::NotDetermined => {
                if perm::request(PermKind::Microphone).ok() == Some(PermState::Denied) {
                    self.emit(AudioEvent::Permission(PermKind::Microphone));
                    return Err(AppError::new(ErrorCode::PermMicrophone));
                }
            }
            _ => {}
        }
        let Some(w) = self.me_w.as_ref() else {
            return Ok(());
        };
        let ring = w.new_ring(96_000);
        let uid = self.mic_uid().map(str::to_string);
        let ring2;
        let res = match tap::start_mic(uid.as_deref(), ring) {
            Ok(m) => Ok(m),
            Err(e) if uid.is_some() => {
                // Selected device is gone: fall back to the default input.
                tracing::warn!(?e, "selected mic unavailable, using default input");
                ring2 = w.new_ring(96_000);
                tap::start_mic(None, ring2)
            }
            Err(e) => Err(e),
        };
        match res {
            Ok(m) => {
                tracing::info!(name = %m.name, rate = m.rate, "mic started");
                self.mic = Some(m);
                self.mic_zero_checked = false;
                Ok(())
            }
            Err(e) => {
                tracing::warn!(?e, "mic start failed");
                Err(e)
            }
        }
    }

    fn resolve_target(&self) -> Result<(TapTarget, Vec<u32>)> {
        match self.cfg.system.as_ref() {
            Some(Target::App(id)) => {
                let g: Group = procs::resolve_app(id)?;
                let objs = g.objs();
                Ok((TapTarget::Procs(objs.clone()), objs))
            }
            _ => {
                let ex = procs::own_tree_objs();
                Ok((TapTarget::System(ex.clone()), ex))
            }
        }
    }

    fn start_tap(&mut self) -> Result<()> {
        self.last_tap_try = Instant::now();
        let Some(w) = self.sys_w.as_ref() else {
            return Ok(());
        };
        let (target, objs) = self.resolve_target()?;
        let ring = w.new_ring(96_000);
        match tap::start_tap(&target, ring) {
            Ok(t) => {
                self.tap = Some(t);
                self.tap_objs = objs;
                self.last_rebuild = Instant::now();
                self.tap_fail_count = 0;
                self.target_gone_since = None;
                self.target_lost_sent = false;
                self.out = devices::default_output();
                Ok(())
            }
            Err(e) => {
                self.tap_fail_count += 1;
                tracing::warn!(?e, n = self.tap_fail_count, "tap start failed");
                if self.tap_fail_count == 3 {
                    lowk_platform::perm::report_system_audio_probe(
                        lowk_platform::perm::SystemAudioProbe::Failed,
                    );
                    self.emit(AudioEvent::Permission(PermKind::SystemAudio));
                }
                Err(e)
            }
        }
    }

    fn rebuild_tap(&mut self, reason: &str) {
        tracing::info!(reason, "rebuilding tap");
        self.tap = None; // full teardown: stop IOProc, destroy aggregate, destroy tap
        if let Some(w) = &self.sys_w {
            w.shared.zero_ms.store(0, Ordering::Relaxed);
        }
        let _ = self.start_tap();
        self.last_rebuild = Instant::now();
        self.emit(AudioEvent::TapRebuilt {
            reason: reason.to_string(),
        });
    }

    fn update_aec(&mut self) {
        let on = match self.cfg.aec {
            AecMode::On => true,
            AecMode::Off => false,
            AecMode::Auto => !self.out.as_ref().map(|o| o.headphones).unwrap_or(false),
        };
        let active = on && self.me_w.is_some() && self.sys_w.is_some();
        if active != self.aec.enabled() {
            tracing::info!(active, available = AecLink::available(), "echo cancellation");
        }
        self.aec.set_enabled(active);
    }

    /// Runs every 300 ms.
    fn check(&mut self) {
        // ---- mic: lost device / default input changed / retry
        if self.want_mic() {
            match &self.mic {
                Some(m) => {
                    let alive = devices::device_by_uid(&m.uid)
                        .and_then(|d| d.is_alive().ok())
                        .unwrap_or(false);
                    let moved =
                        self.mic_uid().is_none() && devices::default_input_uid().is_some_and(|u| u != m.uid);
                    if !alive || moved {
                        self.mic = None;
                        self.emit(AudioEvent::DeviceChanged);
                        let _ = self.start_mic();
                    } else if !self.mic_zero_checked {
                        if let Some(w) = &self.me_w {
                            if w.shared.zero_ms.load(Ordering::Relaxed) >= 3_000 {
                                self.mic_zero_checked = true;
                                if perm::status(PermKind::Microphone) == PermState::Denied {
                                    self.emit(AudioEvent::Permission(PermKind::Microphone));
                                }
                            } else if w.shared.loud_frames.load(Ordering::Relaxed) > 0 {
                                self.mic_zero_checked = true;
                            }
                        }
                    }
                }
                None if self.last_mic_try.elapsed() > Duration::from_secs(3) => {
                    let _ = self.start_mic();
                }
                None => {}
            }
        }

        let Some(sys) = self.sys_w.as_ref().map(|w| w.shared.clone()) else {
            return;
        };

        // ---- output device (tap sub-device) change / headphones
        let out_now = devices::default_output();
        if let (Some(n), Some(p)) = (&out_now, &self.out) {
            let rate_moved = self
                .tap
                .as_ref()
                .is_some_and(|t| t.current_rate().is_some_and(|r| (r - t.rate).abs() > 1.0));
            if n.uid != p.uid || rate_moved {
                self.out = out_now.clone();
                self.emit(AudioEvent::DeviceChanged);
                self.update_aec();
                self.rebuild_tap("output device or rate changed");
                return;
            }
        }

        // ---- target membership
        match self.cfg.system.clone() {
            Some(Target::App(id)) => match procs::resolve_app(&id) {
                Ok(g) => {
                    self.target_gone_since = None;
                    self.target_lost_sent = false;
                    let objs = g.objs();
                    if self.tap.is_none() {
                        if self.last_tap_try.elapsed() > Duration::from_secs(2) {
                            let _ = self.start_tap();
                        }
                        return;
                    }
                    if objs != self.tap_objs && self.last_rebuild.elapsed() > Duration::from_secs(1) {
                        self.rebuild_tap("process set of target changed");
                        return;
                    }
                }
                Err(_) => {
                    let since = *self.target_gone_since.get_or_insert_with(Instant::now);
                    if since.elapsed() > Duration::from_secs(3) {
                        if self.tap.is_some() {
                            self.tap = None;
                        }
                        if !self.target_lost_sent {
                            self.target_lost_sent = true;
                            self.emit(AudioEvent::TargetLost);
                        }
                    }
                    return;
                }
            },
            _ => {
                if self.tap.is_none() {
                    if self.last_tap_try.elapsed() > Duration::from_secs(2) {
                        let _ = self.start_tap();
                    }
                    return;
                }
                // our own helper processes (webview audio) must stay excluded
                let ex = procs::own_tree_objs();
                if ex != self.tap_objs && self.last_rebuild.elapsed() > Duration::from_secs(1) {
                    self.rebuild_tap("own process set changed");
                    return;
                }
            }
        }

        // ---- permission feedback from real traffic
        if !self.sys_perm_reported && sys.loud_frames.load(Ordering::Relaxed) > 0 {
            self.sys_perm_reported = true;
            lowk_platform::perm::report_system_audio_probe(lowk_platform::perm::SystemAudioProbe::Working);
        }
        if sys.loud_frames.load(Ordering::Relaxed) > 0 {
            self.watchdog_trips = 0;
        }

        // ---- silent-tap watchdog
        let zero = sys.zero_ms.load(Ordering::Relaxed);
        if zero >= ZERO_MS_TRIP && self.last_rebuild.elapsed() > REBUILD_COOLDOWN && self.target_active() {
            self.watchdog_trips += 1;
            tracing::warn!(
                zero_ms = zero,
                trips = self.watchdog_trips,
                "silent tap while target produces output"
            );
            self.rebuild_tap("silent tap");
            if self.watchdog_trips >= 3 {
                self.emit(AudioEvent::SilentTap);
                self.watchdog_trips = 0;
            }
        }
    }

    fn target_active(&self) -> bool {
        match self.cfg.system.as_ref() {
            Some(Target::App(id)) => procs::resolve_app(id).map(|g| g.any_output()).unwrap_or(false),
            _ => procs::system_active(),
        }
    }
}
