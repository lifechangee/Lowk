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
use crate::aec::{aec_should_run, AecLink};
use crate::pipeline::{spawn_worker, Meter, RingIn, WorkerHandle};
use crate::{AudioBackend, AudioEventSink, CaptureConfig, CaptureHandle, FrameSink, MIC_NONE};
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

/// Root of the app's process tree (walk parents while the exe name matches).
fn tree_root(sys: &System, pid: u32) -> u32 {
    crate::proc_tree::tree_root(pid, |p| {
        sys.process(Pid::from_u32(p)).map(|pr| {
            (
                pr.parent().map(|pp| pp.as_u32()),
                pr.name().to_string_lossy().into_owned(),
            )
        })
    })
}

/// Human name of an exe: the version-resource FileDescription / ProductName ("Kontur Talk"),
/// falling back to the file stem. Never fails.
fn display_name(exe: &str, fallback: &str) -> String {
    use windows::core::{w, HSTRING, PCWSTR};
    use windows::Win32::Storage::FileSystem::{GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW};
    if exe.is_empty() {
        return fallback.to_string();
    }
    let path = HSTRING::from(exe);
    let mut buf: Vec<u8>;
    unsafe {
        let n = GetFileVersionInfoSizeW(&path, None);
        if n == 0 {
            return fallback.to_string();
        }
        buf = vec![0u8; n as usize];
        if GetFileVersionInfoW(&path, None, n, buf.as_mut_ptr().cast()).is_err() {
            return fallback.to_string();
        }
        let mut tr: *mut core::ffi::c_void = std::ptr::null_mut();
        let mut len = 0u32;
        let lang = if VerQueryValueW(
            buf.as_ptr().cast(),
            w!("\\VarFileInfo\\Translation"),
            &mut tr,
            &mut len,
        )
        .as_bool()
            && len >= 4
        {
            let t = std::slice::from_raw_parts(tr as *const u16, 2);
            format!("{:04x}{:04x}", t[0], t[1])
        } else {
            "040904b0".to_string()
        };
        for key in ["FileDescription", "ProductName"] {
            let q = HSTRING::from(format!("\\StringFileInfo\\{lang}\\{key}"));
            let mut v: *mut core::ffi::c_void = std::ptr::null_mut();
            let mut vl = 0u32;
            if VerQueryValueW(buf.as_ptr().cast(), PCWSTR(q.as_ptr()), &mut v, &mut vl).as_bool() && vl > 1 {
                let sl = std::slice::from_raw_parts(v as *const u16, vl as usize);
                let txt = String::from_utf16_lossy(sl).trim_matches('\0').trim().to_string();
                if !txt.is_empty() {
                    return txt;
                }
            }
        }
    }
    fallback.to_string()
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
                name: {
                    let stem = p
                        .map(|p| p.name().to_string_lossy().trim_end_matches(".exe").to_string())
                        .unwrap_or_else(|| format!("pid{pid}"));
                    let exe = p
                        .and_then(|p| p.exe())
                        .map(|e| e.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    display_name(&exe, &stem)
                },
                producing: false,
            });
            // Active is also true for muted calls; a non-zero peak means real sound.
            e.producing |= active && peak > 0.0001;
        }
    }
    Ok(apps.into_values().collect())
}

/// «Microphone: auto»: every process (with its parent) plus whether it has an Active WASAPI capture
/// session on any active capture endpoint (`IAudioSessionManager2`). `None` when the audio
/// enumeration itself failed (the caller keeps its last state).
pub(crate) fn capture_session_rows() -> Option<Vec<crate::mic_use::ProcRow>> {
    let _ = initialize_mta();
    let en = DeviceEnumerator::new().ok()?;
    let devices = en.get_device_collection(&Direction::Capture).ok()?;
    let mut active: std::collections::BTreeSet<u32> = Default::default();
    for dev in &devices {
        let Ok(dev) = dev else { continue };
        let Ok(mgr) = dev.get_iaudiosessionmanager() else {
            continue;
        };
        let Ok(list) = mgr.get_audiosessionenumerator() else {
            continue;
        };
        for i in 0..list.get_count().unwrap_or(0) {
            let Ok(c) = list.get_session(i) else { continue };
            let pid = c.get_process_id().unwrap_or(0);
            if pid != 0 && matches!(c.get_state(), Ok(SessionState::Active)) {
                active.insert(pid);
            }
        }
    }
    let mut sys = System::new();
    sys.refresh_processes(ProcessesToUpdate::All, true);
    Some(
        sys.processes()
            .values()
            .map(|p| {
                let name = p.name().to_string_lossy().to_lowercase();
                let mut keys = vec![name.trim_end_matches(".exe").to_string(), name];
                if let Some(e) = p.exe() {
                    keys.push(e.to_string_lossy().to_lowercase());
                }
                crate::mic_use::ProcRow {
                    pid: p.pid().as_u32(),
                    parent: p.parent().map(|pp| pp.as_u32()),
                    keys,
                    holds_input: active.contains(&p.pid().as_u32()),
                }
            })
            .collect(),
    )
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

/// Picker part 2: apps that own a visible, titled top-level window right now (a call app can be
/// chosen before it plays anything). Start-Menu enumeration is not implemented; this covers
/// "app is open, call not started yet". Apps that already have an audio session are left to
/// `list_sources`, but are included here too (the UI merges by id).
pub fn list_installed() -> Result<Vec<SourceInfo>> {
    use windows::core::BOOL;
    use windows::Win32::Foundation::{HWND, LPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowTextLengthW, GetWindowThreadProcessId, IsWindowVisible,
    };
    unsafe extern "system" fn cb(h: HWND, l: LPARAM) -> BOOL {
        // SAFETY: `l` is the address of the Vec passed to EnumWindows below.
        let pids = unsafe { &mut *(l.0 as *mut Vec<u32>) };
        if unsafe { IsWindowVisible(h).as_bool() && GetWindowTextLengthW(h) > 0 } {
            let mut pid = 0u32;
            unsafe { GetWindowThreadProcessId(h, Some(&mut pid)) };
            if pid != 0 {
                pids.push(pid);
            }
        }
        BOOL::from(true)
    }
    let mut pids: Vec<u32> = vec![];
    // SAFETY: callback only touches `pids`, which outlives the call.
    unsafe {
        let _ = EnumWindows(Some(cb), LPARAM(&mut pids as *mut Vec<u32> as isize));
    }
    let mut sys = System::new();
    sys.refresh_processes(ProcessesToUpdate::All, true);
    let win_dir = std::env::var("SystemRoot")
        .unwrap_or_else(|_| "C:\\Windows".into())
        .to_lowercase();
    let me = std::process::id();
    let mut out: BTreeMap<u32, SourceInfo> = BTreeMap::new();
    for pid in pids {
        let root = tree_root(&sys, pid);
        if root == me || out.contains_key(&root) {
            continue;
        }
        let Some(p) = sys.process(Pid::from_u32(root)) else {
            continue;
        };
        let exe = p
            .exe()
            .map(|e| e.to_string_lossy().into_owned())
            .unwrap_or_default();
        if exe.is_empty() || exe.to_lowercase().starts_with(&win_dir) {
            continue; // shell, system UI, inaccessible processes
        }
        let stem = p.name().to_string_lossy().trim_end_matches(".exe").to_string();
        out.insert(
            root,
            SourceInfo {
                id: exe.clone(),
                name: display_name(&exe, &stem),
                bundle_id: String::new(),
                pid: root,
                icon_png_b64: None,
                producing_audio: false,
                kind: SourceKind::App,
            },
        );
    }
    let mut v: Vec<SourceInfo> = out.into_values().collect();
    v.sort_by_key(|s| s.name.to_lowercase());
    Ok(v)
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
        // The mic gate: while off, the WASAPI capture client is not even created (no tray mic icon).
        let mic_on = Arc::new(AtomicBool::new(cfg.mic_on));
        let (has_me, has_sys) = (me_w.is_some(), sys_w.is_some());
        let aec_mode = cfg.aec;
        let aec_for = move |on: bool| aec_should_run(aec_mode, false, on, has_me, has_sys);
        aec.set_enabled(aec_for(cfg.mic_on));

        let mut joins: Vec<JoinHandle<()>> = vec![];
        let (ready_tx, ready_rx) = channel::<Result<()>>();
        let mut expected = 0;
        if let (Some(w), Some(t)) = (sys_w.as_ref(), cfg.system.clone()) {
            expected += 1;
            joins.push(spawn_capture(
                "audio-win-sys",
                w,
                stop.clone(),
                None,
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
                Some(mic_on.clone()),
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
            mic_on,
            aec,
            aec_for: Box::new(aec_for),
            joins,
            _workers: (me_w, sys_w),
        }))
    }
}

struct Handle {
    stop: Arc<AtomicBool>,
    /// Gate read by the mic capture thread: false = release the WASAPI client, true = open a new one.
    mic_on: Arc<AtomicBool>,
    aec: AecLink,
    aec_for: Box<dyn Fn(bool) -> bool + Send>,
    joins: Vec<JoinHandle<()>>,
    // dropped after the capture threads have been joined
    _workers: (Option<WorkerHandle>, Option<WorkerHandle>),
}
impl CaptureHandle for Handle {
    fn set_mic(&self, on: bool) -> Result<()> {
        if self.mic_on.swap(on, Ordering::SeqCst) == on {
            return Ok(());
        }
        // fresh AEC state for the reopened device; bypassed (and reference dropped) while off
        self.aec.set_enabled((self.aec_for)(on));
        if on {
            self.aec.restart();
        }
        Ok(())
    }
}
impl Drop for Handle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        for j in self.joins.drain(..) {
            let _ = j.join();
        }
    }
}

/// The client plus (for an app target) the root pid it is attached to, so a restart of the app is noticed.
fn open_system(t: &Target) -> Result<(AudioClient, Option<u32>)> {
    match t {
        Target::App(id) => {
            let mut sys = System::new();
            sys.refresh_processes(ProcessesToUpdate::All, true);
            let pid = find_pid(&sys, id)
                .ok_or_else(|| AppError::new(ErrorCode::AudioTargetLost).with_detail(id.clone()))?;
            let root = tree_root(&sys, pid);
            AudioClient::new_application_loopback_client(root, true)
                .map(|c| (c, Some(root)))
                .map_err(|e| err("app loopback", e))
        }
        Target::WholeSystem => {
            match AudioClient::new_application_loopback_client(std::process::id(), false) {
                Ok(c) => Ok((c, None)),
                Err(e) => {
                    tracing::warn!(?e, "process loopback unavailable, using endpoint loopback");
                    let en = DeviceEnumerator::new().map_err(|e| err("enumerator", e))?;
                    let dev = en
                        .get_default_device(&Direction::Render)
                        .map_err(|e| err("default render", e))?;
                    dev.get_iaudioclient()
                        .map(|c| (c, None))
                        .map_err(|e| err("render client", e))
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

fn open_mic(id: Option<&str>) -> Result<(AudioClient, Option<u32>)> {
    let en = DeviceEnumerator::new().map_err(|e| err("enumerator", e))?;
    let dev = match id {
        Some(i) => en.get_device(i).map_err(|e| err("input device", e))?,
        None => en
            .get_default_device(&Direction::Capture)
            .map_err(|e| err("default input", e))?,
    };
    dev.get_iaudioclient()
        .map(|c| (c, None))
        .map_err(|e| err("capture client", e))
}

fn spawn_capture(
    name: &str,
    w: &WorkerHandle,
    stop: Arc<AtomicBool>,
    gate: Option<Arc<AtomicBool>>,
    ready: std::sync::mpsc::Sender<Result<()>>,
    open: impl Fn() -> Result<(AudioClient, Option<u32>)> + Send + 'static,
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
            let allowed = || gate.as_ref().is_none_or(|g| g.load(Ordering::SeqCst));
            while !stop.load(Ordering::Relaxed) {
                if !allowed() {
                    // Mic switched off: nothing is open. Report ready so `start` does not wait for us.
                    if first {
                        first = false;
                        let _ = ready.send(Ok(()));
                    }
                    std::thread::sleep(Duration::from_millis(50));
                    continue;
                }
                let res = open().and_then(|(c, watch)| {
                    // `c` (and its capture client) is dropped when the loop returns: the endpoint is released.
                    // An app target whose root process died (app restarted) ends the loop with an error;
                    // the retry below then re-resolves the app by its exe and re-attaches.
                    let checked = std::cell::Cell::new(Instant::now());
                    let alive = std::cell::Cell::new(true);
                    let keep = || {
                        if let Some(pid) = watch {
                            if alive.get() && checked.get().elapsed() >= Duration::from_secs(1) {
                                checked.set(Instant::now());
                                let mut sys = System::new();
                                sys.refresh_processes(ProcessesToUpdate::Some(&[Pid::from_u32(pid)]), true);
                                alive.set(sys.process(Pid::from_u32(pid)).is_some());
                            }
                        }
                        allowed() && alive.get()
                    };
                    let r = capture_loop(c, attach(), &stop, &keep, || {
                        if first {
                            first = false;
                            let _ = ready.send(Ok(()));
                        }
                    });
                    if !alive.get() {
                        tracing::info!(pid = ?watch, "captured app exited, waiting for it to come back");
                        return r.and(Err(AppError::new(ErrorCode::AudioTargetLost)));
                    }
                    r
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
    keep: &dyn Fn() -> bool,
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
    while !stop.load(Ordering::Relaxed) && keep() {
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
