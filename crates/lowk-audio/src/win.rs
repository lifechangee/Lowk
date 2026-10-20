//! Windows backend (WASAPI). Compile-checked from macOS (`just check-windows`); NOT run on real
//! hardware yet, so treat timing, device-change handling and Win10 fallbacks as unverified.
//!
//! * App target: process loopback with `INCLUDE_TARGET_PROCESS_TREE` on the root of the app's
//!   process tree (walk parents while the exe name matches).
//! * Whole system: process loopback with `EXCLUDE_TARGET_PROCESS_TREE` on our own PID
//!   (Windows 11 / Win10 build 20348+); older Windows falls back to classic endpoint loopback
//!   on the default render device (includes our own sounds; we make none).
//! * Mic: shared-mode event capture on the selected / default capture endpoint.
//! * Loopback delivers nothing while the app is silent, so the capture loop zero-fills by the
//!   wall clock to keep downstream time continuous.
//! * Capture timestamps are approximated by the read time (QPC mapping TODO); latency numbers
//!   measured on Windows are therefore lower bounds.
use crate::aec::AecLink;
use crate::pipeline::{spawn_worker, Meter, RingIn, WorkerHandle};
use crate::{AecMode, AudioBackend, AudioEventSink, CaptureConfig, CaptureHandle, FrameSink, MIC_NONE};
use lowk_types::{
    now_mono_ns, AppError, AudioApp, DeviceInfo, ErrorCode, Result, SourceInfo, SourceKind, Stream, Target,
};
use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::channel;
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use sysinfo::{Pid, ProcessesToUpdate, System};
use wasapi::{
    initialize_mta, AudioClient, DeviceEnumerator, Direction, SampleType, SessionState, StreamMode,
    WaveFormat,
};

const RATE: u32 = 48_000;

fn err(what: &str, e: impl std::fmt::Debug) -> AppError {
    AppError::new(ErrorCode::AudioDevice).with_detail(format!("{what}: {e:?}"))
}

/// Walk up while the parent runs the same exe: root of the app's process tree.
fn tree_root(sys: &System, pid: u32) -> u32 {
    let mut cur = Pid::from_u32(pid);
    while let Some(p) = sys.process(cur) {
        match p.parent().and_then(|pp| sys.process(pp)) {
            Some(parent) if parent.name() == p.name() => cur = parent.pid(),
            _ => break,
        }
    }
    cur.as_u32()
}

struct AppRow {
    root: u32,
    exe: String,
    name: String,
    producing: bool,
}

/// Audio sessions grouped by app (root pid of the process tree).
fn sessions() -> Result<Vec<AppRow>> {
    let _ = initialize_mta();
    let mut sys = System::new();
    sys.refresh_processes(ProcessesToUpdate::All, true);
    let en = DeviceEnumerator::new().map_err(|e| err("enumerator", e))?;
    let mut apps: BTreeMap<u32, AppRow> = BTreeMap::new();
    for dev in &en
        .get_device_collection(&Direction::Render)
        .map_err(|e| err("devices", e))?
    {
        let dev = dev.map_err(|e| err("device", e))?;
        let Ok(mgr) = dev.get_iaudiosessionmanager() else {
            continue;
        };
        let Ok(list) = mgr.get_audiosessionenumerator() else {
            continue;
        };
        for i in 0..list.get_count().unwrap_or(0) {
            let Ok(c) = list.get_session(i) else { continue };
            let pid = c.get_process_id().unwrap_or(0);
            if pid == 0 {
                continue;
            }
            let active = matches!(c.get_state(), Ok(SessionState::Active));
            let peak = c
                .get_audiometerinformation()
                .and_then(|m| m.get_peak_value())
                .unwrap_or(0.0);
            let root = tree_root(&sys, pid);
            let p = sys.process(Pid::from_u32(root));
            let e = apps.entry(root).or_insert_with(|| AppRow {
                root,
                exe: p
                    .and_then(|p| p.exe())
                    .map(|e| e.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                name: p
                    .map(|p| p.name().to_string_lossy().trim_end_matches(".exe").to_string())
                    .unwrap_or_else(|| format!("pid{pid}")),
                producing: false,
            });
            // Active is also true for muted calls; a non-zero peak means real sound.
            e.producing |= active && peak > 0.0001;
        }
    }
    Ok(apps.into_values().collect())
}

pub fn list_sources() -> Result<Vec<SourceInfo>> {
    let mut v: Vec<SourceInfo> = sessions()?
        .into_iter()
        .map(|a| SourceInfo {
            id: if a.exe.is_empty() {
                a.root.to_string()
            } else {
                a.exe.clone()
            },
            name: a.name,
            bundle_id: String::new(),
            pid: a.root,
            icon_png_b64: None,
            producing_audio: a.producing,
            kind: SourceKind::App,
        })
        .collect();
    v.sort_by(|a, b| {
        b.producing_audio
            .cmp(&a.producing_audio)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    v.insert(
        0,
        SourceInfo {
            id: "system".into(),
            name: "System".into(),
            bundle_id: String::new(),
            pid: 0,
            icon_png_b64: None,
            producing_audio: v.iter().any(|s| s.producing_audio),
            kind: SourceKind::System,
        },
    );
    Ok(v)
}

/// Installed-app enumeration (Start Menu / registry) is not implemented on Windows yet; the
/// picker shows apps that currently have an audio session.
pub fn list_installed() -> Result<Vec<SourceInfo>> {
    Ok(vec![])
}

/// Windows backend.
#[derive(Default)]
pub struct Wasapi;

impl AudioBackend for Wasapi {
    fn list_apps(&self) -> Result<Vec<AudioApp>> {
        Ok(sessions()?
            .into_iter()
            .map(|a| AudioApp {
                id: if a.exe.is_empty() {
                    a.root.to_string()
                } else {
                    a.exe
                },
                name: a.name,
                producing_audio: a.producing,
                pids: vec![a.root],
            })
            .collect())
    }

    fn list_mics(&self) -> Result<Vec<DeviceInfo>> {
        let _ = initialize_mta();
        let en = DeviceEnumerator::new().map_err(|e| err("enumerator", e))?;
        let def = en
            .get_default_device(&Direction::Capture)
            .and_then(|d| d.get_id())
            .ok();
        let mut out = vec![];
        for d in &en
            .get_device_collection(&Direction::Capture)
            .map_err(|e| err("devices", e))?
        {
            let Ok(d) = d else { continue };
            let (Ok(id), Ok(name)) = (d.get_id(), d.get_friendlyname()) else {
                continue;
            };
            out.push(DeviceInfo {
                is_default: def.as_deref() == Some(id.as_str()),
                id,
                name,
            });
        }
        Ok(out)
    }

    fn start(
        &mut self,
        cfg: &CaptureConfig,
        sink: FrameSink,
        ev: AudioEventSink,
    ) -> Result<Box<dyn CaptureHandle>> {
        let stop = Arc::new(AtomicBool::new(false));
        let want_mic = cfg.mic.as_deref() != Some(MIC_NONE);
        let aec = AecLink::new(false);
        let me_w = want_mic.then(|| spawn_worker(Stream::Me, sink.clone(), ev.clone(), Some(aec.clone())));
        let sys_w = cfg
            .system
            .is_some()
            .then(|| spawn_worker(Stream::System, sink.clone(), ev.clone(), Some(aec.clone())));
        // Headphone detection is not implemented on Windows: Auto means "on" when built in.
        aec.set_enabled(cfg.aec != AecMode::Off && me_w.is_some() && sys_w.is_some());

        let mut joins: Vec<JoinHandle<()>> = vec![];
        let (ready_tx, ready_rx) = channel::<Result<()>>();
        let mut expected = 0;
        if let (Some(w), Some(t)) = (sys_w.as_ref(), cfg.system.clone()) {
            expected += 1;
            joins.push(spawn_capture(
                "audio-win-sys",
                w,
                stop.clone(),
                ready_tx.clone(),
                move || open_system(&t),
            ));
        }
        if let Some(w) = me_w.as_ref() {
            expected += 1;
            let dev = cfg.mic.clone().filter(|m| !m.is_empty() && m != "default");
            joins.push(spawn_capture(
                "audio-win-mic",
                w,
                stop.clone(),
                ready_tx.clone(),
                move || open_mic(dev.as_deref()),
            ));
        }
        drop(ready_tx);
        let mut errs = vec![];
        for _ in 0..expected {
            if let Ok(Err(e)) = ready_rx.recv_timeout(Duration::from_secs(20)) {
                errs.push(e);
            }
        }
        if expected > 0 && errs.len() == expected {
            stop.store(true, Ordering::Relaxed);
            for j in joins {
                let _ = j.join();
            }
            return Err(errs.remove(0));
        }
        // 10 Hz meter.
        let (m_me, m_sys) = (
            me_w.as_ref().map(|w| w.shared.clone()),
            sys_w.as_ref().map(|w| w.shared.clone()),
        );
        let (stop2, ev2) = (stop.clone(), ev);
        joins.push(
            std::thread::Builder::new()
                .name("audio-win-meter".into())
                .spawn(move || {
                    let mut m = Meter::new(m_me, m_sys, ev2);
                    while !stop2.load(Ordering::Relaxed) {
                        std::thread::sleep(Duration::from_millis(100));
                        m.tick();
                    }
                })
                .map_err(|e| err("spawn meter", e))?,
        );
        Ok(Box::new(Handle {
            stop,
            joins,
            _workers: (me_w, sys_w),
        }))
    }
}

struct Handle {
    stop: Arc<AtomicBool>,
    joins: Vec<JoinHandle<()>>,
    // dropped after the capture threads have been joined
    _workers: (Option<WorkerHandle>, Option<WorkerHandle>),
}
impl CaptureHandle for Handle {}
impl Drop for Handle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        for j in self.joins.drain(..) {
            let _ = j.join();
        }
    }
}

fn open_system(t: &Target) -> Result<AudioClient> {
    match t {
        Target::App(id) => {
            let mut sys = System::new();
            sys.refresh_processes(ProcessesToUpdate::All, true);
            let pid = find_pid(&sys, id)
                .ok_or_else(|| AppError::new(ErrorCode::AudioTargetLost).with_detail(id.clone()))?;
            AudioClient::new_application_loopback_client(tree_root(&sys, pid), true)
                .map_err(|e| err("app loopback", e))
        }
        Target::WholeSystem => {
            match AudioClient::new_application_loopback_client(std::process::id(), false) {
                Ok(c) => Ok(c),
                Err(e) => {
                    tracing::warn!(?e, "process loopback unavailable, using endpoint loopback");
                    let en = DeviceEnumerator::new().map_err(|e| err("enumerator", e))?;
                    let dev = en
                        .get_default_device(&Direction::Render)
                        .map_err(|e| err("default render", e))?;
                    dev.get_iaudioclient().map_err(|e| err("render client", e))
                }
            }
        }
    }
}

fn find_pid(sys: &System, id: &str) -> Option<u32> {
    if let Ok(p) = id.parse::<u32>() {
        return Some(p);
    }
    let want = id.to_lowercase();
    sys.processes().values().find_map(|p| {
        let exe = p
            .exe()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        let name = p.name().to_string_lossy().to_lowercase();
        (exe == want || name == want || name.trim_end_matches(".exe") == want).then(|| p.pid().as_u32())
    })
}

fn open_mic(id: Option<&str>) -> Result<AudioClient> {
    let en = DeviceEnumerator::new().map_err(|e| err("enumerator", e))?;
    let dev = match id {
        Some(i) => en.get_device(i).map_err(|e| err("input device", e))?,
        None => en
            .get_default_device(&Direction::Capture)
            .map_err(|e| err("default input", e))?,
    };
    dev.get_iaudioclient().map_err(|e| err("capture client", e))
}

fn spawn_capture(
    name: &str,
    w: &WorkerHandle,
    stop: Arc<AtomicBool>,
    ready: std::sync::mpsc::Sender<Result<()>>,
    open: impl Fn() -> Result<AudioClient> + Send + 'static,
) -> JoinHandle<()> {
    let shared = w.shared.clone();
    // The worker hands out a fresh ring per (re)start of the WASAPI client.
    let attach = {
        let w2 = RingFactory::new(w);
        move || w2.new_ring()
    };
    std::thread::Builder::new()
        .name(name.into())
        .spawn(move || {
            let _ = initialize_mta();
            let _ = &shared;
            let mut first = true;
            while !stop.load(Ordering::Relaxed) {
                let res = open().and_then(|c| {
                    capture_loop(c, attach(), &stop, || {
                        if first {
                            first = false;
                            let _ = ready.send(Ok(()));
                        }
                    })
                });
                if let Err(e) = res {
                    if first {
                        first = false;
                        let _ = ready.send(Err(e.clone()));
                    }
                    tracing::warn!(?e, "wasapi capture ended, retrying");
                    // 2 s back-off, interruptible
                    for _ in 0..20 {
                        if stop.load(Ordering::Relaxed) {
                            return;
                        }
                        std::thread::sleep(Duration::from_millis(100));
                    }
                }
            }
        })
        .expect("spawn wasapi capture")
}

/// Clonable way to get fresh rings for one worker from another thread.
struct RingFactory {
    shared: Arc<crate::pipeline::StreamShared>,
    attach: std::sync::mpsc::Sender<ringbuf::HeapCons<f32>>,
}
impl RingFactory {
    fn new(w: &WorkerHandle) -> Self {
        Self {
            shared: w.shared.clone(),
            attach: w.attach_sender(),
        }
    }
    fn new_ring(&self) -> RingIn {
        let (p, c) = RingIn::new(self.shared.clone(), RATE);
        let _ = self.attach.send(c);
        p
    }
}

fn capture_loop(
    mut client: AudioClient,
    mut ring: RingIn,
    stop: &AtomicBool,
    on_ready: impl FnOnce(),
) -> Result<()> {
    let fmt = WaveFormat::new(32, 32, &SampleType::Float, RATE as usize, 2, None);
    let ba = fmt.get_blockalign() as usize;
    let mode = StreamMode::EventsShared {
        autoconvert: true,
        buffer_duration_hns: 0,
    };
    client
        .initialize_client(&fmt, &Direction::Capture, &mode)
        .map_err(|e| err("initialize", e))?;
    let ev = client.set_get_eventhandle().map_err(|e| err("event handle", e))?;
    let cap = client
        .get_audiocaptureclient()
        .map_err(|e| err("capture client", e))?;
    client.start_stream().map_err(|e| err("start", e))?;
    on_ready();
    let mut q: VecDeque<u8> = VecDeque::new();
    let mut mono: Vec<f32> = Vec::with_capacity(4800);
    let mut last_push = Instant::now();
    let mut result = Ok(());
    while !stop.load(Ordering::Relaxed) {
        let n = match cap.get_next_packet_size() {
            Ok(n) => n.unwrap_or(0),
            Err(e) => {
                result = Err(err("packet size", e)); // device removed / format change: caller retries
                break;
            }
        };
        if n > 0 {
            q.clear();
            if let Err(e) = cap.read_from_device_to_deque(&mut q) {
                result = Err(err("read", e));
                break;
            }
            let bytes: Vec<u8> = q.iter().copied().collect();
            mono.clear();
            mono.extend(bytes.chunks_exact(ba).map(|f| {
                let l = f32::from_le_bytes(f[0..4].try_into().unwrap());
                let r = f32::from_le_bytes(f[4..8].try_into().unwrap());
                (l + r) * 0.5
            }));
            ring.push(&mono, RATE, now_mono_ns());
            last_push = Instant::now();
        } else if last_push.elapsed() >= Duration::from_millis(20) {
            // Loopback sends nothing during silence: fill with zeros by the clock.
            let ms = last_push.elapsed().as_millis().min(200) as usize;
            mono.clear();
            mono.resize(ms * RATE as usize / 1000, 0.0);
            ring.push(&mono, RATE, now_mono_ns());
            last_push += Duration::from_millis(ms as u64);
        }
        let _ = ev.wait_for_event(20);
    }
    let _ = client.stop_stream();
    result
}
