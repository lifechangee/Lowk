//! Lowk audio capture spike. CLI only.
//!   list [--all]
//!   capture (--app <bundle-id|pid|name> | --system) [--duration S] [--out DIR] [--no-mic] [--force-rebuild-after S]
//!   gen-tones DIR          writes tone_440.wav / tone_880.wav (mono 48k, 40 s)
//!   analyze FILE.wav       prints RMS + Goertzel levels at 440/880 Hz
mod clock;
mod dsp;
#[cfg(target_os = "macos")]
mod mac;
#[cfg(windows)]
mod win;

use anyhow::{bail, Result};

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(|s| s.as_str()) {
        Some("list") => cmd_list(args.iter().any(|a| a == "--all")),
        Some("capture") => cmd_capture(&args[1..]),
        Some("gen-tones") => gen_tones(args.get(1).map(|s| s.as_str()).unwrap_or(".")),
        Some("analyze") => analyze(args.get(1).expect("analyze FILE.wav")),
        _ => {
            eprintln!("usage: lowk-audio-spike list [--all] | capture (--app X | --system) [--duration S] [--out DIR] [--no-mic] [--force-rebuild-after S] | gen-tones DIR | analyze FILE");
            Ok(())
        }
    }
}

fn cmd_list(all: bool) -> Result<()> {
    #[cfg(target_os = "macos")]
    return mac::cmd_list(all);
    #[cfg(windows)]
    return win::cmd_list(all);
    #[allow(unreachable_code)]
    { let _ = all; bail!("unsupported OS") }
}

struct Opts { app: Option<String>, system: bool, duration: f64, out: String, mic: bool, force_rebuild: Option<f64> }

fn parse(a: &[String]) -> Result<Opts> {
    let mut o = Opts { app: None, system: false, duration: 10.0, out: "out".into(), mic: true, force_rebuild: None };
    let mut i = 0;
    while i < a.len() {
        match a[i].as_str() {
            "--app" => { i += 1; o.app = Some(a[i].clone()); }
            "--system" => o.system = true,
            "--duration" => { i += 1; o.duration = a[i].parse()?; }
            "--out" => { i += 1; o.out = a[i].clone(); }
            "--no-mic" => o.mic = false,
            "--force-rebuild-after" => { i += 1; o.force_rebuild = Some(a[i].parse()?); }
            x => bail!("unknown arg {x}"),
        }
        i += 1;
    }
    if o.app.is_none() == !o.system { bail!("give exactly one of --app <id> or --system"); }
    Ok(o)
}

#[cfg(target_os = "macos")]
fn cmd_capture(a: &[String]) -> Result<()> {
    use std::sync::{mpsc, Arc};
    use std::sync::atomic::Ordering;
    use std::time::{Duration, Instant};
    let o = parse(a)?;
    std::fs::create_dir_all(&o.out)?;
    let target = if o.system { mac::Target::System } else { mac::Target::Group(mac::resolve_app(o.app.as_ref().unwrap())?) };
    if let mac::Target::Group(g) = &target {
        eprintln!("[target] {} ({}) key={} members={:?}", g.name, g.bundle_id, g.key, g.members.iter().map(|m| m.pid).collect::<Vec<_>>());
    }

    // sinks
    let (mtx, mrx) = mpsc::channel();
    let (ttx, trx) = mpsc::channel();
    let ms = Arc::new(dsp::Shared::default());
    let ts = Arc::new(dsp::Shared::default());
    let (mo, to) = (format!("{}/me.wav", o.out), format!("{}/them.wav", o.out));
    let (mc, tc) = (format!("{}/me_latency.csv", o.out), format!("{}/them_latency.csv", o.out));
    let msc = ms.clone();
    let mh = std::thread::spawn(move || dsp::run_sink("me", mrx, msc, &mo, &mc));
    let tsc = ts.clone();
    let th = std::thread::spawn(move || dsp::run_sink("them", trx, tsc, &to, &tc));

    // mic first: makes our own process an audio client so its process object exists for exclusion
    let mic = if o.mic {
        match mac::start_mic(mtx.clone()) {
            Ok(m) => { eprintln!("[mic] '{}' @ {} Hz", m.device_name, m.rate); Some(m) }
            Err(e) => { eprintln!("[mic] FAILED: {e:#}"); None }
        }
    } else { None };
    drop(mtx);
    std::thread::sleep(Duration::from_millis(200));
    if o.system { eprintln!("[tap] own process object exists: {}", mac::own_process_obj_exists()); }

    let members_of = |t: &mac::Target| -> Vec<u32> {
        match t { mac::Target::Group(g) => mac::refresh_group(&g.key).map(|g| { let mut v: Vec<u32> = g.members.iter().map(|m| m.obj).collect(); v.sort(); v }).unwrap_or_default(), _ => vec![] }
    };
    let mut members = members_of(&target);
    let mut tap = Some(mac::start_tap(&target, &members, ttx.clone())?);
    let t0 = Instant::now();
    let mut last_rebuild = Instant::now();
    let mut forced = false;
    let mut rebuilds = 0;
    let rebuild = |reason: &str, members: &Vec<u32>, tap: &mut Option<mac::TapSession>, n: &mut i32| {
        eprintln!("[watchdog] REBUILD TAP: {reason}");
        *tap = None; // full teardown: stop IOProc, destroy aggregate, destroy tap
        match mac::start_tap(&target, members, ttx.clone()) { Ok(t) => *tap = Some(t), Err(e) => eprintln!("[watchdog] rebuild failed: {e:#}") }
        *n += 1;
    };
    while t0.elapsed().as_secs_f64() < o.duration {
        std::thread::sleep(Duration::from_millis(250));
        let since = last_rebuild.elapsed();
        if tap.is_none() && since > Duration::from_secs(2) {
            members = members_of(&target);
            rebuild("tap missing (previous rebuild failed / target not yet present)", &members, &mut tap, &mut rebuilds);
            last_rebuild = Instant::now();
            continue;
        }
        if let Some(f) = o.force_rebuild { if !forced && t0.elapsed().as_secs_f64() >= f {
            forced = true; members = members_of(&target);
            rebuild("forced by --force-rebuild-after (exercise path)", &members, &mut tap, &mut rebuilds); last_rebuild = Instant::now(); continue;
        } }
        // group membership changed (helper spawned / died)
        let now_members = members_of(&target);
        if matches!(target, mac::Target::Group(_)) && now_members != members && !now_members.is_empty() && since > Duration::from_secs(1) {
            members = now_members;
            rebuild("process set of target changed", &members, &mut tap, &mut rebuilds); last_rebuild = Instant::now(); continue;
        }
        // silent-tap watchdog: >=2 s of exact zeros while target reports output
        let z = ts.zero_ms.load(Ordering::Relaxed);
        if z >= 2000 && since > Duration::from_secs(5) && mac::target_active(&target) {
            eprintln!("[watchdog] {z} ms of all-zero buffers while target is producing audio");
            members = members_of(&target);
            ts.zero_ms.store(0, Ordering::Relaxed);
            rebuild("silent tap", &members, &mut tap, &mut rebuilds); last_rebuild = Instant::now();
        }
    }
    eprintln!("[main] stopping; rebuilds={rebuilds}");
    drop(tap);
    drop(mic);
    drop(ttx);
    mh.join().unwrap()?;
    th.join().unwrap()?;
    Ok(())
}

#[cfg(windows)]
fn cmd_capture(a: &[String]) -> Result<()> {
    let o = parse(a)?;
    win::capture(o.app.as_deref(), o.system, o.duration, &o.out, o.mic)
}

#[cfg(not(any(target_os = "macos", windows)))]
fn cmd_capture(_a: &[String]) -> Result<()> { bail!("unsupported OS") }

fn gen_tones(dir: &str) -> Result<()> {
    std::fs::create_dir_all(dir)?;
    for f in [440.0f32, 880.0] {
        let spec = hound::WavSpec { channels: 1, sample_rate: 48000, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
        let mut w = hound::WavWriter::create(format!("{dir}/tone_{}.wav", f as u32), spec)?;
        for i in 0..48000 * 40 {
            let s = (2.0 * std::f32::consts::PI * f * i as f32 / 48000.0).sin() * 0.3;
            w.write_sample((s * 32767.0) as i16)?;
        }
        w.finalize()?;
    }
    Ok(())
}

fn analyze(path: &str) -> Result<()> {
    let mut r = hound::WavReader::open(path)?;
    let sr = r.spec().sample_rate as f64;
    let x: Vec<f32> = r.samples::<i16>().map(|s| s.unwrap() as f32 / 32768.0).collect();
    // skip first 0.5 s
    let x = &x[(sr * 0.5) as usize..x.len().max((sr * 0.5) as usize)];
    let a440 = dsp::goertzel(x, 440.0, sr);
    let a880 = dsp::goertzel(x, 880.0, sr);
    println!("{path}: {:.1}s rms={:.1} dBFS  tone440={:.4} tone880={:.4}", x.len() as f64 / sr, dsp::rms_dbfs(x), a440, a880);
    Ok(())
}
