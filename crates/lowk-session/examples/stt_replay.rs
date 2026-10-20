#![allow(clippy::type_complexity, clippy::too_many_arguments)]
//! Replays recordings through the full session path (file audio source -> frame sink ->
//! engine sidecar -> transcript -> bus -> call journal) and reports word-to-screen latency
//! and CER against the reference text. No audio is ever played: file sources only.
//!
//! cargo run --release -p lowk-session --example stt_replay -- \
//!     --audio spikes/stt-bench/audio --models <dir with the engine's model folders> \
//!     [--engine nemotron-local|soniox|deepgram] [--scenarios 01,02] [--noise q,n] [--no-correct]
//!     [--right 6] [--chunk-ms 160] [--hang-ms 480] [--langs ru,en] [--out report.md]
use lowk_audio::{AudioBackend, AudioEventSink, CaptureConfig, CaptureHandle, FileAudioSource, FrameSink};
use lowk_session::{MemCallStore, Session, SessionDeps};
use lowk_stt::cloud::{CloudFactory, Vendor};
use lowk_stt::local::{self, LocalTuning, NemotronFactory};
use lowk_stt::sidecar::SidecarSpec;
use lowk_stt::{ModelStore, WhisperCorrector};
use lowk_types::fakes::{InlineMainThread, MemSettingsStore, NoopRetriever};
use lowk_types::*;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn arg(k: &str) -> Option<String> {
    let a: Vec<String> = std::env::args().collect();
    a.iter().position(|x| x == k).and_then(|i| a.get(i + 1)).cloned()
}
fn flag(k: &str) -> bool {
    std::env::args().any(|x| x == k)
}

/// Wraps a backend to record when the first frame of each stream was captured and the
/// frame energies (to estimate when words were spoken).
struct Tap {
    inner: FileAudioSource,
    frames: Arc<Mutex<HashMap<Stream, Vec<(u64, f32)>>>>,
}
impl AudioBackend for Tap {
    fn list_apps(&self) -> Result<Vec<AudioApp>> {
        self.inner.list_apps()
    }
    fn list_mics(&self) -> Result<Vec<DeviceInfo>> {
        self.inner.list_mics()
    }
    fn start(
        &mut self,
        cfg: &CaptureConfig,
        sink: FrameSink,
        ev: AudioEventSink,
    ) -> Result<Box<dyn CaptureHandle>> {
        let frames = self.frames.clone();
        let tapped: FrameSink = Arc::new(move |f: Frame16k| {
            let rms = (f.pcm.iter().map(|x| x * x).sum::<f32>() / f.pcm.len().max(1) as f32).sqrt();
            frames
                .lock()
                .unwrap()
                .entry(f.stream)
                .or_default()
                .push((f.t_mono_ns, rms));
            sink(f)
        });
        self.inner.start(cfg, tapped, ev)
    }
}

fn norm(s: &str) -> Vec<char> {
    s.to_lowercase()
        .replace('ё', "е")
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .collect()
}

fn lev<T: PartialEq>(a: &[T], b: &[T]) -> usize {
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for i in 1..=a.len() {
        let mut cur = vec![i; b.len() + 1];
        for j in 1..=b.len() {
            cur[j] = (prev[j] + 1)
                .min(cur[j - 1] + 1)
                .min(prev[j - 1] + usize::from(a[i - 1] != b[j - 1]));
        }
        prev = cur;
    }
    prev[b.len()]
}

fn cer(hyp: &str, reference: &str) -> f64 {
    let (h, r) = (norm(hyp), norm(reference));
    lev(&h, &r) as f64 / r.len().max(1) as f64
}

fn pct(v: &mut [f64], q: f64) -> f64 {
    if v.is_empty() {
        return f64::NAN;
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v[(((v.len() as f64) * q).ceil() as usize).clamp(1, v.len()) - 1]
}

/// Time (ns) at which `frac` (0..1) of the speech-active duration has elapsed.
fn active_time(frames: &[(u64, f32)], frac: f64) -> u64 {
    let mut rms: Vec<f32> = frames.iter().map(|f| f.1).collect();
    rms.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let p90 = rms[(rms.len() * 9 / 10).min(rms.len() - 1)];
    let thr = (0.25 * p90).max(0.006);
    let act: Vec<u64> = frames.iter().filter(|f| f.1 > thr).map(|f| f.0).collect();
    if act.is_empty() {
        return frames[0].0;
    }
    let i = ((act.len() as f64 * frac).ceil() as usize).clamp(1, act.len()) - 1;
    act[i] + 20_000_000
}

struct ClipResult {
    name: String,
    live_cer: f64,
    final_cer: f64,
    lags_ms: Vec<f64>,
    live: String,
    fin: String,
    diag: Option<SttDiagnostics>,
}

fn run_pair(
    reg_builder: &dyn Fn() -> lowk_stt::EngineRegistry,
    engine: &str,
    corrector: Option<Arc<dyn lowk_stt::Corrector>>,
    langs: Vec<String>,
    correct: bool,
    me: Option<PathBuf>,
    them: Option<PathBuf>,
    refs: [Option<String>; 2],
    tag: &str,
) -> Vec<ClipResult> {
    let frames = Arc::new(Mutex::new(HashMap::new()));
    let mut settings = Settings::default();
    settings.stt.engine = engine.to_owned();
    settings.stt.languages = langs;
    settings.stt.background_correction = correct;
    let mut deps = SessionDeps::new(
        Arc::new(InlineMainThread),
        Arc::new(NoopRetriever),
        Arc::new(MemSettingsStore::new(settings)),
    );
    let src = FileAudioSource::new(me.clone(), them.clone());
    let done = src.done();
    deps.audio = Arc::new(Mutex::new(Tap {
        inner: src,
        frames: frames.clone(),
    }));
    deps.stt = Arc::new(reg_builder());
    deps.corrector = if correct { corrector } else { None };
    deps.store = Arc::new(MemCallStore::default());
    let session = Session::new(deps);
    let rx = session.subscribe();

    // collect bus events with their arrival time (stand-in for "on screen")
    let evs: Arc<Mutex<Vec<(u64, SttEvent)>>> = Arc::default();
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let (e2, s2) = (evs.clone(), stop.clone());
    let collector = std::thread::spawn(move || loop {
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok(AppEvent::Stt(e)) => e2.lock().unwrap().push((now_mono_ns(), e)),
            Ok(_) => {}
            Err(_) if s2.load(std::sync::atomic::Ordering::Relaxed) => return,
            Err(_) => {}
        }
    });

    let t0 = Instant::now();
    session
        .start(StartOpts {
            title: Some(tag.into()),
            folder: None,
            system: them.as_ref().map(|_| Target::WholeSystem),
            mic: me.as_ref().map(|_| "file".to_string()),
            demo: false,
        })
        .expect("session start");
    done.wait(Duration::from_secs(600));
    std::thread::sleep(Duration::from_millis(1500)); // let the last partials surface
    session.stop().expect("stop");
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    collector.join().unwrap();
    eprintln!("[{tag}] session took {:.0}s", t0.elapsed().as_secs_f64());
    for e in session.stt_errors() {
        eprintln!("[{tag}] stt error: {e:?}");
    }

    let diag = session.stt_diagnostics();
    let evs = evs.lock().unwrap().clone();
    let frames = frames.lock().unwrap().clone();
    let mut out = vec![];
    for (stream, path, r) in [(Stream::Me, &me, &refs[0]), (Stream::System, &them, &refs[1])] {
        let (Some(path), Some(r)) = (path, r) else {
            continue;
        };
        let mine: Vec<&(u64, SttEvent)> = evs.iter().filter(|(_, e)| e.stream == stream).collect();
        // live text = engine finals (rev as emitted); final text = latest final per utterance
        let mut live: HashMap<u64, (u32, String)> = HashMap::new();
        let mut fin: HashMap<u64, (u32, String)> = HashMap::new();
        for (_, e) in &mine {
            if e.kind == SttKind::Final {
                let slot = fin.entry(e.utt).or_default();
                if e.rev >= slot.0 || slot.1.is_empty() {
                    *slot = (e.rev, e.text.clone());
                }
                let l = live.entry(e.utt).or_insert((e.rev, e.text.clone()));
                if e.rev < l.0 {
                    *l = (e.rev, e.text.clone());
                }
            }
        }
        let join = |m: &HashMap<u64, (u32, String)>| {
            let mut k: Vec<_> = m.keys().copied().collect();
            k.sort_unstable();
            k.iter().map(|u| m[u].1.clone()).collect::<Vec<_>>().join(" ")
        };
        let (live_t, fin_t) = (join(&live), join(&fin));

        // word-to-screen: first time word j of an utterance was on the bus vs its estimated
        // spoken end (words spread over the speech-active frames of that utterance's span,
        // by character count; +-0.3 s, no forced aligner)
        let fr = frames.get(&stream).cloned().unwrap_or_default();
        let mut utts: Vec<u64> = live.keys().copied().collect();
        utts.sort_unstable();
        let mut lags = vec![];
        for u in utts {
            let Some((_, fe)) = mine
                .iter()
                .find(|(_, e)| e.utt == u && e.kind == SttKind::Final && e.rev == live[&u].0)
            else {
                continue;
            };
            let span: Vec<(u64, f32)> = fr
                .iter()
                .copied()
                .filter(|(t, _)| *t >= fe.t_start_ns && *t <= fe.t_audio_end_ns)
                .collect();
            if span.len() < 10 {
                continue;
            }
            let final_words: Vec<String> = norm(&live[&u].1)
                .iter()
                .collect::<String>()
                .split(' ')
                .map(str::to_owned)
                .collect();
            let total_chars: usize = final_words
                .iter()
                .map(|w| w.chars().count() + 1)
                .sum::<usize>()
                .max(1);
            let mut seen: Vec<Option<u64>> = vec![None; final_words.len()];
            for (t_recv, e) in mine.iter().filter(|(_, e)| e.utt == u && e.rev <= live[&u].0) {
                let n = norm(&e.text)
                    .iter()
                    .collect::<String>()
                    .split_whitespace()
                    .count();
                for s in seen.iter_mut().take(n.min(final_words.len())) {
                    s.get_or_insert(*t_recv);
                }
            }
            let mut c = 0usize;
            for (j, w) in final_words.iter().enumerate() {
                c += w.chars().count() + 1;
                if let Some(t) = seen[j] {
                    let spoken_end = active_time(&span, c as f64 / total_chars as f64);
                    lags.push((t as f64 - spoken_end as f64) / 1e6);
                }
            }
        }
        out.push(ClipResult {
            name: path.file_stem().unwrap().to_string_lossy().into_owned(),
            live_cer: cer(&live_t, r),
            final_cer: cer(&fin_t, r),
            lags_ms: lags,
            live: live_t,
            fin: fin_t,
            diag: diag.clone(),
        });
    }
    out
}

fn main() {
    let audio = PathBuf::from(arg("--audio").unwrap_or_else(|| "spikes/stt-bench/audio".into()));
    let models = PathBuf::from(arg("--models").expect("--models <dir>"));
    let engine = arg("--engine").unwrap_or_else(|| local::NEMOTRON_ID.into());
    let scen: Vec<String> = arg("--scenarios")
        .unwrap_or_else(|| "01,02,03,04,05".into())
        .split(',')
        .map(str::to_owned)
        .collect();
    let noise: Vec<String> = arg("--noise")
        .unwrap_or_else(|| "q,n".into())
        .split(',')
        .map(str::to_owned)
        .collect();
    let langs: Vec<String> = arg("--langs")
        .unwrap_or_else(|| "ru,en".into())
        .split(',')
        .map(str::to_owned)
        .collect();
    let tuning = LocalTuning {
        right: arg("--right").and_then(|s| s.parse().ok()).unwrap_or(6),
        chunk_ms: arg("--chunk-ms").and_then(|s| s.parse().ok()).unwrap_or(160),
        hang_ms: arg("--hang-ms").and_then(|s| s.parse().ok()).unwrap_or(480),
    };
    let correct = !flag("--no-correct");
    let worker = local::find_worker().expect("lowk-stt-worker not found (set LOWK_STT_WORKER)");
    let store = ModelStore::new(models.clone());
    if flag("--install") {
        // exercises the real downloader: resume, SHA-256 pin, progress
        let nf = NemotronFactory {
            store: store.clone(),
            worker: Some(worker.clone()),
            tuning: tuning.clone(),
        };
        let cancel = std::sync::atomic::AtomicBool::new(false);
        let mut last = Instant::now();
        let mut p = |f: &str, ph: InstallPhase, d: u64, t: u64| {
            if ph != InstallPhase::Downloading || last.elapsed() > Duration::from_secs(5) {
                last = Instant::now();
                eprintln!("install {f}: {ph:?} {}/{} MB", d >> 20, t >> 20);
            }
        };
        use lowk_stt::SttEngineFactory;
        nf.install(&cancel, &mut p).expect("install nemotron");
        local::install_whisper(&store, &cancel, &mut p).expect("install whisper");
    }
    let t2 = tuning.clone();
    let (st2, w2) = (store.clone(), worker.clone());
    let reg = move || {
        let mut r = lowk_stt::EngineRegistry::default();
        r.register(Box::new(NemotronFactory {
            store: st2.clone(),
            worker: Some(w2.clone()),
            tuning: t2.clone(),
        }));
        r.register(Box::new(CloudFactory(Vendor::Soniox)));
        r.register(Box::new(CloudFactory(Vendor::Deepgram)));
        r
    };
    let corrector: Option<Arc<dyn lowk_stt::Corrector>> = if correct && local::whisper_installed(&store) {
        Some(Arc::new(WhisperCorrector::new(SidecarSpec {
            exe: worker,
            args: vec![
                "correct".into(),
                "--model".into(),
                store
                    .path(local::WHISPER_ID, local::WHISPER_FILE)
                    .to_string_lossy()
                    .into_owned(),
                "--threads".into(),
                "2".into(),
            ],
            nice: 15,
        })))
    } else {
        None
    };
    eprintln!(
        "engine={engine} tuning={tuning:?} correction={}",
        corrector.is_some()
    );

    let mut results = vec![];
    for n in &scen {
        for q in &noise {
            let me = audio.join(format!("{n}_me_{q}.m4a"));
            let them = audio.join(format!("{n}_them_{q}.m4a"));
            let rd = |p: &PathBuf| std::fs::read_to_string(p.with_extension("txt")).ok();
            if !me.exists() || !them.exists() {
                continue;
            }
            let only = arg("--only");
            let (me, them) = match only.as_deref() {
                Some("me") => (Some(me), None),
                Some("them") => (None, Some(them)),
                _ => (Some(me), Some(them)),
            };
            let r = run_pair(
                &reg,
                &engine,
                corrector.clone(),
                langs.clone(),
                correct,
                me.clone(),
                them.clone(),
                [me.as_ref().and_then(rd), them.as_ref().and_then(rd)],
                &format!("{n}_{q}"),
            );
            results.extend(r);
        }
    }

    let mut md = String::new();
    md += &format!(
        "# Session replay: {engine} ({tuning:?}, correction={})\n\n",
        corrector.is_some()
    );
    md += "| clip | CER live | CER corrected | lag p50 ms | lag p95 ms | n words |\n|---|---|---|---|---|---|\n";
    let (mut all_lags, mut live_c, mut fin_c) = (vec![], vec![], vec![]);
    for r in &results {
        let mut l = r.lags_ms.clone();
        md += &format!(
            "| {} | {:.3} | {:.3} | {:.0} | {:.0} | {} |\n",
            r.name,
            r.live_cer,
            r.final_cer,
            pct(&mut l, 0.5),
            pct(&mut l, 0.95),
            l.len()
        );
        all_lags.extend(r.lags_ms.iter().copied());
        live_c.push(r.live_cer);
        fin_c.push(r.final_cer);
    }
    let mean = |v: &[f64]| v.iter().sum::<f64>() / v.len().max(1) as f64;
    md += &format!(
        "\n**All clips**: word-to-screen p50 {:.0} ms, p95 {:.0} ms ({} words); mean CER live {:.3}, corrected {:.3}\n",
        pct(&mut all_lags.clone(), 0.5),
        pct(&mut all_lags, 0.95),
        results.iter().map(|r| r.lags_ms.len()).sum::<usize>(),
        mean(&live_c),
        mean(&fin_c)
    );
    if let Some(d) = results.last().and_then(|r| r.diag.clone()) {
        md += &format!("\nEngine diagnostics (last session): {d:#?}\n");
    }
    md += "\n## Texts\n";
    for r in &results {
        md += &format!("\n### {}\nlive: {}\n\ncorrected: {}\n", r.name, r.live, r.fin);
    }
    println!("{md}");
    if let Some(p) = arg("--out") {
        std::fs::write(p, md).unwrap();
    }
}
