//! Manual / scripted capture tool (replaces the spike CLI).
//!   list [--installed]
//!   mics
//!   probe                          system-audio permission probe (plays a very quiet tone)
//!   capture (--app ID|PID|NAME | --system) [--duration S] [--out DIR] [--no-mic] [--aec auto|on|off]
//!   gen-tones DIR                  tone_440.wav / tone_880.wav (mono 48 kHz, 40 s) for isolation tests
//!   analyze FILE.wav               level + Goertzel at 440/880 Hz
use lowk_audio::{default_backend, AudioEvent, CaptureConfig, MIC_NONE};
use lowk_types::AecMode;
use lowk_types::{now_mono_ns, Frame16k, Stream, Target};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn main() {
    tracing_init();
    let a: Vec<String> = std::env::args().skip(1).collect();
    match a.first().map(String::as_str) {
        Some("list") => {
            let v = if a.iter().any(|x| x == "--installed") {
                lowk_audio::list_installed_sources()
            } else {
                lowk_audio::list_sources()
            };
            for s in v.expect("list") {
                println!(
                    "{:<9} {:<42} {:<26} pid={:<6} producing={:<5} icon={}B",
                    format!("{:?}", s.kind),
                    s.bundle_id,
                    s.name,
                    s.pid,
                    s.producing_audio,
                    s.icon_png_b64.as_ref().map(|i| i.len()).unwrap_or(0)
                );
            }
        }
        #[cfg(target_os = "macos")]
        Some("procs") => {
            for p in lowk_audio::mac::procs::list_processes().expect("procs") {
                println!("obj={} pid={} out={} in={} bundle={} path={}", p.obj, p.pid, p.running_output, p.running_input, p.bundle_id, p.path);
            }
        }
        Some("mics") => {
            for m in default_backend().list_mics().expect("mics") {
                println!("{} {} {}", if m.is_default { "*" } else { " " }, m.id, m.name);
            }
        }
        Some("probe") => {
            let t = Instant::now();
            let r = lowk_audio::probe_system_audio(Duration::from_secs(120));
            println!("probe: {r:?} in {:.1}s", t.elapsed().as_secs_f32());
        }
        Some("gen-tones") => gen_tones(a.get(1).map(String::as_str).unwrap_or(".")),
        Some("analyze") => analyze(a.get(1).expect("analyze FILE.wav")),
        Some("capture") => capture(&a[1..]),
        _ => eprintln!("usage: capture list [--installed] | mics | probe | capture (--app X | --system) [--duration S] [--out DIR] [--no-mic] [--aec auto|on|off] | gen-tones DIR | analyze FILE"),
    }
}

fn tracing_init() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .try_init();
}

fn arg(a: &[String], k: &str) -> Option<String> {
    a.iter().position(|x| x == k).and_then(|i| a.get(i + 1).cloned())
}

fn capture(a: &[String]) {
    let dur: f64 = arg(a, "--duration").and_then(|d| d.parse().ok()).unwrap_or(10.0);
    let out = arg(a, "--out").unwrap_or_else(|| "out".into());
    std::fs::create_dir_all(&out).unwrap();
    let system = if a.iter().any(|x| x == "--system") {
        Some(Target::WholeSystem)
    } else {
        arg(a, "--app").map(Target::App)
    };
    let mic = if a.iter().any(|x| x == "--no-mic") {
        Some(MIC_NONE.to_string())
    } else {
        arg(a, "--mic")
    };
    let aec = match arg(a, "--aec").as_deref() {
        Some("on") => AecMode::On,
        Some("off") => AecMode::Off,
        _ => AecMode::Auto,
    };
    #[derive(Default)]
    struct Rec {
        pcm: [Vec<f32>; 2],
        age_ms: [Vec<f64>; 2],
        first_t: [Option<u64>; 2],
    }
    let rec: Arc<Mutex<Rec>> = Arc::default();
    let r2 = rec.clone();
    let sink = Arc::new(move |f: Frame16k| {
        let now = now_mono_ns();
        let i = (f.stream == Stream::System) as usize;
        let mut r = r2.lock().unwrap();
        // age of the newest sample of this frame when it reaches the sink
        r.age_ms[i].push((now as f64 - (f.t_mono_ns as f64 + 20e6)) / 1e6);
        r.first_t[i].get_or_insert(f.t_mono_ns);
        r.pcm[i].extend_from_slice(&f.pcm);
    });
    let last = Arc::new(Mutex::new((0f32, 0f32)));
    let l2 = last.clone();
    let ev = Arc::new(move |e: AudioEvent| match e {
        AudioEvent::Level { me, sys } => *l2.lock().unwrap() = (me, sys),
        other => eprintln!("[event] {other:?}"),
    });
    let mut be = default_backend();
    let cfg = CaptureConfig {
        system,
        mic,
        aec,
        mic_on: true,
    };
    let t0 = Instant::now();
    let h = match be.start(&cfg, sink, ev) {
        Ok(h) => h,
        Err(e) => {
            eprintln!("start failed: {e:?}");
            std::process::exit(2);
        }
    };
    eprintln!("[start] took {:.2}s", t0.elapsed().as_secs_f32());
    let t = Instant::now();
    while t.elapsed().as_secs_f64() < dur {
        std::thread::sleep(Duration::from_secs(1));
        let (m, s) = *last.lock().unwrap();
        eprintln!(
            "[t={:4.1}s] level me={m:.2} sys={s:.2}",
            t.elapsed().as_secs_f32()
        );
    }
    drop(h);
    let mut r = rec.lock().unwrap();
    for (i, name) in ["me", "them"].iter().enumerate() {
        if r.pcm[i].is_empty() {
            eprintln!("[{name}] no audio");
            continue;
        }
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 16_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut w = hound::WavWriter::create(format!("{out}/{name}.wav"), spec).unwrap();
        for s in &r.pcm[i] {
            w.write_sample((s.clamp(-1.0, 1.0) * 32767.0) as i16).unwrap();
        }
        w.finalize().unwrap();
        let mut v = std::mem::take(&mut r.age_ms[i]);
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let p = |q: f64| v[((v.len() - 1) as f64 * q) as usize];
        eprintln!(
            "[{name}] {:.2}s of audio, {} frames; frame age at sink (ms; newest sample -> delivery, incl. ring+resampler+worker): p50={:.1} p95={:.1} max={:.1}",
            r.pcm[i].len() as f64 / 16000.0,
            v.len(),
            p(0.5),
            p(0.95),
            p(1.0)
        );
    }
}

fn gen_tones(dir: &str) {
    std::fs::create_dir_all(dir).unwrap();
    for f in [440.0f32, 880.0] {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 48_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut w = hound::WavWriter::create(format!("{dir}/tone_{}.wav", f as u32), spec).unwrap();
        for i in 0..48_000 * 40 {
            let s = (2.0 * std::f32::consts::PI * f * i as f32 / 48_000.0).sin() * 0.3;
            w.write_sample((s * 32767.0) as i16).unwrap();
        }
        w.finalize().unwrap();
    }
}

fn goertzel(x: &[f32], freq: f64, sr: f64) -> f64 {
    let w = 2.0 * std::f64::consts::PI * freq / sr;
    let c = 2.0 * w.cos();
    let (mut s1, mut s2) = (0f64, 0f64);
    for v in x {
        let s = *v as f64 + c * s1 - s2;
        s2 = s1;
        s1 = s;
    }
    (s1 * s1 + s2 * s2 - c * s1 * s2).max(0.0).sqrt() * 2.0 / x.len() as f64
}

fn analyze(path: &str) {
    let mut r = hound::WavReader::open(path).unwrap();
    let sr = r.spec().sample_rate as f64;
    let x: Vec<f32> = r.samples::<i16>().map(|s| s.unwrap() as f32 / 32768.0).collect();
    let skip = (sr * 0.5) as usize;
    let x = if x.len() > skip { &x[skip..] } else { &x[..] };
    println!(
        "{path}: {:.1}s tone440={:.4} tone880={:.4}",
        x.len() as f64 / sr,
        goertzel(x, 440.0, sr),
        goertzel(x, 880.0, sr)
    );
}
