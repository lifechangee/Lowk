//! Windows backend (UNTESTED on real hardware: written on macOS, only type-checked via
//! `cargo check --target x86_64-pc-windows-msvc`).
//! - app capture: WASAPI process loopback, INCLUDE_TARGET_PROCESS_TREE on the tree root
//! - system capture: process loopback with EXCLUDE_TARGET_PROCESS_TREE on our own PID
//!   (Win10 build 20348+/Win11; on older Win10 fall back to endpoint loopback — not in spike)
//! - mic: shared-mode event capture on default capture endpoint.
use crate::clock::now_ns;
use crate::dsp::{self, Chunk};
use anyhow::{anyhow, Result};
use std::collections::{BTreeMap, VecDeque};
use std::sync::mpsc::{self, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};
use sysinfo::{Pid, ProcessesToUpdate, System};
use wasapi::*;

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

pub fn cmd_list(all: bool) -> Result<()> {
    initialize_mta().ok().map_err(|e| anyhow!("{e:?}"))?;
    let mut sys = System::new();
    sys.refresh_processes(ProcessesToUpdate::All, true);
    let en = DeviceEnumerator::new().map_err(|e| anyhow!("{e:?}"))?;
    // exe name -> (root pid, peak, active)
    let mut apps: BTreeMap<String, (u32, f32, bool)> = BTreeMap::new();
    for dev in &en.get_device_collection(&Direction::Render).map_err(|e| anyhow!("{e:?}"))? {
        let dev = dev.map_err(|e| anyhow!("{e:?}"))?;
        let mgr = dev.get_iaudiosessionmanager().map_err(|e| anyhow!("{e:?}"))?;
        let sessions = mgr.get_audiosessionenumerator().map_err(|e| anyhow!("{e:?}"))?;
        for i in 0..sessions.get_count().map_err(|e| anyhow!("{e:?}"))? {
            let c = sessions.get_session(i).map_err(|e| anyhow!("{e:?}"))?;
            let active = matches!(c.get_state(), Ok(SessionState::Active));
            let pid = c.get_process_id().unwrap_or(0);
            if pid == 0 { continue; }
            let peak = c.get_audiometerinformation().and_then(|m| m.get_peak_value()).unwrap_or(0.0);
            let root = tree_root(&sys, pid);
            let name = sys.process(Pid::from_u32(root)).map(|p| p.name().to_string_lossy().into_owned()).unwrap_or_else(|| format!("pid{pid}"));
            let e = apps.entry(name).or_insert((root, 0.0, false));
            e.1 = e.1.max(peak);
            e.2 |= active && peak > 0.0001; // peak>0 = actually making noise (Active is true for muted calls too)
        }
    }
    println!("{:<32} {:>8} {:>8}  {}", "EXE", "ROOT PID", "PEAK", "PRODUCING");
    for (n, (pid, peak, on)) in apps.iter().filter(|(_, v)| all || v.2) {
        println!("{n:<32} {pid:>8} {peak:>8.3}  {on}");
    }
    Ok(())
}

fn loopback_thread(client: AudioClient, tx: Sender<Chunk>, label: &'static str, stop: Arc<std::sync::atomic::AtomicBool>) -> Result<()> {
    let fmt = WaveFormat::new(32, 32, &SampleType::Float, 48000, 2, None);
    let ba = fmt.get_blockalign() as usize;
    let mut client = client;
    let mode = StreamMode::EventsShared { autoconvert: true, buffer_duration_hns: 0 };
    client.initialize_client(&fmt, &Direction::Capture, &mode).map_err(|e| anyhow!("{label} init: {e:?}"))?;
    let ev = client.set_get_eventhandle().map_err(|e| anyhow!("{e:?}"))?;
    let cap = client.get_audiocaptureclient().map_err(|e| anyhow!("{e:?}"))?;
    client.start_stream().map_err(|e| anyhow!("{e:?}"))?;
    let mut q: VecDeque<u8> = VecDeque::new();
    while !stop.load(std::sync::atomic::Ordering::Relaxed) {
        let n = cap.get_next_packet_size().map_err(|e| anyhow!("{e:?}"))?.unwrap_or(0);
        if n > 0 {
            q.clear();
            cap.read_from_device_to_deque(&mut q).map_err(|e| anyhow!("{e:?}"))?;
            let deliver = now_ns();
            let bytes: Vec<u8> = q.iter().copied().collect();
            let mono: Vec<f32> = bytes.chunks_exact(ba).map(|f| {
                let l = f32::from_le_bytes(f[0..4].try_into().unwrap());
                let r = f32::from_le_bytes(f[4..8].try_into().unwrap());
                (l + r) * 0.5
            }).collect();
            // BufferInfo.timestamp is QPC-based; mapping to our clock is TODO, so capture time
            // is approximated as "end of buffer == now" (cb latency therefore reads ~0 on Windows).
            let cap_ns = deliver.saturating_sub((mono.len() as f64 / 48000.0 * 1e9) as u64);
            let _ = tx.send(Chunk { samples: mono, sr: 48000.0, cap_ns, deliver_ns: deliver });
        }
        let _ = ev.wait_for_event(200);
    }
    let _ = client.stop_stream();
    Ok(())
}

pub fn capture(app: Option<&str>, _system: bool, duration: f64, out: &str, mic: bool) -> Result<()> {
    std::fs::create_dir_all(out)?;
    initialize_mta().ok().map_err(|e| anyhow!("{e:?}"))?;
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let (ttx, trx) = mpsc::channel();
    let (mtx, mrx) = mpsc::channel();
    let (to, tc, mo, mc) = (format!("{out}/them.wav"), format!("{out}/them_latency.csv"), format!("{out}/me.wav"), format!("{out}/me_latency.csv"));
    let th = std::thread::spawn(move || dsp::run_sink("them", trx, Arc::new(dsp::Shared::default()), &to, &tc));
    let mh = std::thread::spawn(move || dsp::run_sink("me", mrx, Arc::new(dsp::Shared::default()), &mo, &mc));

    // them
    let (pid, include) = match app {
        Some(spec) => {
            let mut sys = System::new();
            sys.refresh_processes(ProcessesToUpdate::All, true);
            let pid: u32 = match spec.parse() {
                Ok(p) => p,
                Err(_) => sys.processes().values().find(|p| p.name().to_string_lossy().eq_ignore_ascii_case(spec)).map(|p| p.pid().as_u32()).ok_or_else(|| anyhow!("no process named {spec}"))?,
            };
            (tree_root(&sys, pid), true)
        }
        None => (std::process::id(), false), // exclude our own tree
    };
    let stop2 = stop.clone();
    let tt = std::thread::spawn(move || -> Result<()> {
        initialize_mta().ok().map_err(|e| anyhow!("{e:?}"))?;
        let c = AudioClient::new_application_loopback_client(pid, include).map_err(|e| anyhow!("{e:?}"))?;
        loopback_thread(c, ttx, "them", stop2)
    });
    let mt = if mic {
        let stop3 = stop.clone();
        Some(std::thread::spawn(move || -> Result<()> {
            initialize_mta().ok().map_err(|e| anyhow!("{e:?}"))?;
            let en = DeviceEnumerator::new().map_err(|e| anyhow!("{e:?}"))?;
            let dev = en.get_default_device(&Direction::Capture).map_err(|e| anyhow!("{e:?}"))?;
            let c = dev.get_iaudioclient().map_err(|e| anyhow!("{e:?}"))?;
            loopback_thread(c, mtx, "me", stop3)
        }))
    } else { drop(mtx); None };

    let t0 = Instant::now();
    while t0.elapsed().as_secs_f64() < duration { std::thread::sleep(Duration::from_millis(100)); }
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    if let Err(e) = tt.join().unwrap() { eprintln!("them: {e:#}"); }
    if let Some(m) = mt { if let Err(e) = m.join().unwrap() { eprintln!("me: {e:#}"); } }
    th.join().unwrap()?; mh.join().unwrap()?;
    // TODO(windows): silent-tap watchdog (same logic as mac: zero_ms in dsp::Shared) — not wired here.
    Ok(())
}
