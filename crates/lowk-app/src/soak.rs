//! Long-session soak, headless and silent: `lowk --selfcheck-soak` (run by `just soak`, never by `just ci`).
//!
//! A looped, concatenated set of the Russian bench recordings (`fixtures/stt`, our own
//! recordings and synthesized clips, nothing downloaded) is fed as the System stream through the
//! real capture ring -> session -> local STT sidecar (+ Whisper correction) -> transcript ->
//! vault `.md` -> search index, for `--hours` of audio, in real time (`--speed 1`) or faster.
//! Nothing is played, no permission is needed. The run measures, every `--sample-secs`:
//!
//! * RSS of the app process and of its worker children (no growth trend),
//! * word-to-screen latency (the estimator of `stt_replay`, windowed over the run: no drift),
//! * ring overruns / dropped audio, worker restarts, engine backlog,
//! * transcript completeness (words fed vs words recognised per window, silent gaps, bus vs `.md`),
//! * the call `.md` size and the time the search index needs to take the growing file in.
//!
//! The report is Markdown (`--report`), the raw series CSV (`--csv`).
use crate::m2::{Env, Services, StaticKey};
use lowk_audio::file::decode_16k;
use lowk_audio::{
    AudioBackend, AudioEvent, AudioEventSink, CaptureConfig, CaptureHandle, FrameSink, PipelineFileSource,
    PipelineOpts,
};
use lowk_session::{CallRecorder, CallStore, Session, SessionDeps};
use lowk_store::{FileSettingsStore, Vault};
use lowk_stt::local::{self, NemotronFactory};
use lowk_stt::sidecar::SidecarSpec;
use lowk_stt::{Corrector, ModelStore, WhisperCorrector};
use lowk_types::fakes::{InlineMainThread, MemSettingsStore, NoopRetriever};
use lowk_types::*;
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const GAP_S: f32 = 2.0;

// ---------------------------------------------------------------------------------------------
// input: concatenated clips + layout
// ---------------------------------------------------------------------------------------------

struct Clip {
    name: String,
    start_s: f32,
    dur_s: f32,
    words: usize,
}

/// Normalised words (lowercase, ё -> е, letters and digits only), as the replay's CER does.
fn norm_words(s: &str) -> Vec<String> {
    s.to_lowercase()
        .replace('ё', "е")
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .map(str::to_owned)
        .collect()
}

fn write_wav16(path: &Path, pcm: &[f32]) -> std::io::Result<()> {
    use std::io::Write;
    let data = (pcm.len() * 2) as u32;
    let mut f = std::io::BufWriter::new(std::fs::File::create(path)?);
    f.write_all(b"RIFF")?;
    f.write_all(&(36 + data).to_le_bytes())?;
    f.write_all(b"WAVEfmt ")?;
    f.write_all(&16u32.to_le_bytes())?;
    f.write_all(&1u16.to_le_bytes())?; // PCM
    f.write_all(&1u16.to_le_bytes())?; // mono
    f.write_all(&16_000u32.to_le_bytes())?;
    f.write_all(&32_000u32.to_le_bytes())?;
    f.write_all(&2u16.to_le_bytes())?;
    f.write_all(&16u16.to_le_bytes())?;
    f.write_all(b"data")?;
    f.write_all(&data.to_le_bytes())?;
    for s in pcm {
        f.write_all(&((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes())?;
    }
    f.flush()
}

/// Every clip with a reference text next to it, in a stable order, joined with short silences.
fn build_base(dir: &Path, out: &Path) -> Result<(Vec<Clip>, f32), String> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| matches!(p.extension().and_then(|e| e.to_str()), Some("m4a" | "wav")))
        .filter(|p| p.with_extension("txt").exists())
        .collect();
    files.sort();
    if files.is_empty() {
        return Err(format!("no clips with a .txt reference in {}", dir.display()));
    }
    let (mut pcm, mut clips) = (Vec::<f32>::new(), vec![]);
    for f in files {
        let a = decode_16k(&f).map_err(|e| format!("{}: {e:?}", f.display()))?;
        let words = norm_words(&std::fs::read_to_string(f.with_extension("txt")).unwrap_or_default()).len();
        clips.push(Clip {
            name: f.file_stem().unwrap().to_string_lossy().into_owned(),
            start_s: pcm.len() as f32 / 16_000.0,
            dur_s: a.len() as f32 / 16_000.0,
            words,
        });
        pcm.extend(a);
        pcm.extend(std::iter::repeat_n(0.0, (GAP_S * 16_000.0) as usize));
    }
    write_wav16(out, &pcm).map_err(|e| e.to_string())?;
    Ok((clips, pcm.len() as f32 / 16_000.0))
}

// ---------------------------------------------------------------------------------------------
// measuring backend: wraps the pipeline source, sees every frame and every audio event
// ---------------------------------------------------------------------------------------------

#[derive(Default)]
struct Tap {
    /// (t_mono_ns, rms) of the newest ~25 min of frames (for the word-timing estimate).
    frames: Mutex<VecDeque<(u64, f32)>>,
    first_ns: AtomicU64,
    n_frames: AtomicU64,
    dropped_samples: AtomicU64,
    drop_events: AtomicU64,
}

struct TapBackend {
    inner: PipelineFileSource,
    tap: Arc<Tap>,
}
impl AudioBackend for TapBackend {
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
        let tap = self.tap.clone();
        let tapped: FrameSink = Arc::new(move |f: Frame16k| {
            let rms = (f.pcm.iter().map(|x| x * x).sum::<f32>() / f.pcm.len().max(1) as f32).sqrt();
            let _ = tap
                .first_ns
                .compare_exchange(0, f.t_mono_ns, Ordering::Relaxed, Ordering::Relaxed);
            tap.n_frames.fetch_add(1, Ordering::Relaxed);
            {
                let mut q = tap.frames.lock().unwrap();
                q.push_back((f.t_mono_ns, rms));
                while q.len() > 75_000 {
                    q.pop_front();
                }
            }
            sink(f)
        });
        let tap = self.tap.clone();
        let evs: AudioEventSink = Arc::new(move |e: AudioEvent| {
            if let AudioEvent::Dropped { samples, .. } = &e {
                tap.dropped_samples.fetch_add(*samples, Ordering::Relaxed);
                tap.drop_events.fetch_add(1, Ordering::Relaxed);
            }
            ev(e)
        });
        self.inner.start(cfg, tapped, evs)
    }
}

// ---------------------------------------------------------------------------------------------
// vault call store that counts what the session wrote
// ---------------------------------------------------------------------------------------------

#[derive(Default)]
struct Written {
    finals: AtomicU64,
    materializations: AtomicU64,
}
struct SoakStore {
    vault: Vault,
    written: Arc<Written>,
}
struct SoakRec {
    w: lowk_store::CallWriter,
    written: Arc<Written>,
}
impl CallRecorder for SoakRec {
    fn push(&mut self, e: CallEntry) -> Result<()> {
        if let CallEntry::Final { event } = &e {
            if event.kind == SttKind::Final && !event.text.trim().is_empty() {
                self.written.finals.fetch_add(1, Ordering::Relaxed);
            }
        }
        self.w.push(e)
    }
    fn materialize(&mut self) -> Result<()> {
        self.written.materializations.fetch_add(1, Ordering::Relaxed);
        self.w.materialize()
    }
    fn finish(self: Box<Self>) -> Result<Option<CallMeta>> {
        Ok(Some(self.w.finish()?))
    }
}
impl CallStore for SoakStore {
    fn begin_call(&self, new: NewCall) -> Result<(CallId, Box<dyn CallRecorder>)> {
        let w = self.vault.begin_call(new)?;
        let id = w.id();
        Ok((
            id,
            Box::new(SoakRec {
                w,
                written: self.written.clone(),
            }),
        ))
    }
}

// ---------------------------------------------------------------------------------------------
// event collector: online word-to-screen lags, per-utterance word counts (bounded memory)
// ---------------------------------------------------------------------------------------------

#[derive(Default)]
struct Stats {
    /// (arrival t rel. s, lag ms) per word
    lags: Vec<(f32, f32)>,
    /// utt -> (t_audio_end rel. s, words of the newest final revision)
    utts: HashMap<u64, (f32, usize)>,
    live_done: HashMap<u64, ()>,
    partials: u64,
    finals: u64,
    rev_finals: u64,
}

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

fn pct(v: &[f32], q: f64) -> f32 {
    if v.is_empty() {
        return f32::NAN;
    }
    let mut s = v.to_vec();
    s.sort_by(|a, b| a.partial_cmp(b).unwrap());
    s[(((s.len() as f64) * q).ceil() as usize).clamp(1, s.len()) - 1]
}

// ---------------------------------------------------------------------------------------------
// process memory
// ---------------------------------------------------------------------------------------------

/// (own RSS MB, RSS MB of all descendants) via `ps`; `None` where `ps` is not available.
fn rss_mb() -> Option<(f64, f64)> {
    let out = std::process::Command::new("ps")
        .args(["-A", "-o", "pid=,ppid=,rss="])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let rows: Vec<(u32, u32, u64)> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| {
            let mut it = l.split_whitespace().map(|x| x.parse::<u64>().ok());
            Some((
                it.next()??.try_into().ok()?,
                it.next()??.try_into().ok()?,
                it.next()??,
            ))
        })
        .collect();
    let me = std::process::id();
    let mut family = vec![me];
    loop {
        let more: Vec<u32> = rows
            .iter()
            .filter(|r| family.contains(&r.1) && !family.contains(&r.0))
            .map(|r| r.0)
            .collect();
        if more.is_empty() {
            break;
        }
        family.extend(more);
    }
    let kb =
        |f: &dyn Fn(u32) -> bool| rows.iter().filter(|r| f(r.0)).map(|r| r.2).sum::<u64>() as f64 / 1024.0;
    Some((kb(&|p| p == me), kb(&|p| p != me && family.contains(&p))))
}

/// 1-minute load average of the machine (contention from other work shows up here); NaN if unknown.
fn load1() -> f64 {
    if let Ok(t) = std::fs::read_to_string("/proc/loadavg") {
        return t
            .split_whitespace()
            .next()
            .and_then(|x| x.parse().ok())
            .unwrap_or(f64::NAN);
    }
    std::process::Command::new("sysctl")
        .args(["-n", "vm.loadavg"])
        .output()
        .ok()
        .and_then(|o| {
            String::from_utf8_lossy(&o.stdout)
                .trim_matches(|c: char| c == '{' || c == '}' || c.is_whitespace())
                .split_whitespace()
                .next()
                .and_then(|x| x.parse().ok())
        })
        .unwrap_or(f64::NAN)
}

fn median(v: &mut [f32]) -> f32 {
    if v.is_empty() {
        return f32::NAN;
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v[v.len() / 2]
}

// ---------------------------------------------------------------------------------------------
// run
// ---------------------------------------------------------------------------------------------

struct Sample {
    t_s: f64,
    audio_s: f64,
    rss_app: f64,
    rss_children: f64,
    md_kb: f64,
    lag_p50: f32,
    lag_p95: f32,
    words_win: usize,
    restarts: u32,
    backlog_ms: u32,
    emit_p95: u32,
    dropped: u64,
    finals: u64,
    load: f64,
}

struct IndexProbe {
    at_s: f64,
    md_kb: f64,
    chunks: u32,
    update_s: f64,
    search_ms: u32,
}

#[derive(Clone)]
struct Opts {
    audio: PathBuf,
    models: PathBuf,
    hours: f32,
    speed: f64,
    correct: bool,
    sample_secs: u64,
    probe_mins: u64,
    report: Option<PathBuf>,
    csv: Option<PathBuf>,
    keep: bool,
}

fn win_stats(lags: &[(f32, f32)], lo: f32, hi: f32) -> (usize, f32, f32) {
    let v: Vec<f32> = lags
        .iter()
        .filter(|l| l.0 >= lo && l.0 < hi)
        .map(|l| l.1)
        .collect();
    (v.len(), pct(&v, 0.5), pct(&v, 0.95))
}

/// Least-squares slope of y over x (per x unit).
fn slope(pts: &[(f64, f64)]) -> f64 {
    let n = pts.len() as f64;
    if pts.len() < 3 {
        return f64::NAN;
    }
    let (sx, sy) = pts.iter().fold((0.0, 0.0), |a, p| (a.0 + p.0, a.1 + p.1));
    let (mx, my) = (sx / n, sy / n);
    let num: f64 = pts.iter().map(|p| (p.0 - mx) * (p.1 - my)).sum();
    let den: f64 = pts.iter().map(|p| (p.0 - mx).powi(2)).sum();
    num / den
}

fn run(o: &Opts) -> Result<bool, String> {
    let tmp = std::env::temp_dir().join(format!("lowk-soak-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
    let result = run_in(o, &tmp);
    if !o.keep {
        let _ = std::fs::remove_dir_all(&tmp);
    } else {
        eprintln!("kept {}", tmp.display());
    }
    result
}

fn run_in(o: &Opts, tmp: &Path) -> Result<bool, String> {
    let wav = tmp.join("soak-base.wav");
    let (clips, base_s) = build_base(&o.audio, &wav)?;
    let total_s = o.hours * 3600.0;
    let ref_words: usize = clips.iter().map(|c| c.words).sum();
    eprintln!(
        "soak: {} clips, base loop {:.0} s ({} reference words); feeding {:.0} s ({:.2} loops) at speed {}",
        clips.len(),
        base_s,
        ref_words,
        total_s,
        total_s / base_s,
        o.speed
    );

    // services: vault + index (the real ones)
    let data = tmp.join("data");
    std::fs::create_dir_all(&data).map_err(|e| e.to_string())?;
    let settings = Arc::new(FileSettingsStore::open(&data).map_err(|e| format!("{e:?}"))?);
    #[derive(Default)]
    struct NoSink;
    impl EventSink for NoSink {
        fn emit(&self, _ev: AppEvent) {}
    }
    let env = Env {
        settings: settings.clone() as Arc<dyn SettingsStore>,
        sink: Arc::new(NoSink),
        keys: Arc::new(StaticKey(None)),
        models: lowk_index::embedder::models::ModelStore::new(tmp.join("embed-models")),
        embed_worker: None,
    };
    let vault = Vault::open(&tmp.join("vault")).map_err(|e| format!("{e:?}"))?;
    let svc = Services::start(env, vault.clone()).map_err(|e| format!("{e:?}"))?;

    // session on the real engines
    let mut s = Settings::default();
    s.stt.engine = local::NEMOTRON_ID.into();
    s.stt.languages = vec!["ru".into(), "en".into()];
    s.stt.background_correction = o.correct;
    s.audio.aec = AecMode::Off;
    let mut deps = SessionDeps::new(
        Arc::new(InlineMainThread),
        Arc::new(NoopRetriever),
        Arc::new(MemSettingsStore::new(s)),
    );
    let tap = Arc::new(Tap::default());
    let src = PipelineFileSource::new(
        None,
        Some(wav.clone()),
        PipelineOpts {
            mic_gain: 1.0,
            echo_gain: 0.0,
            seconds: total_s,
            speed: o.speed,
        },
    );
    let done = src.done();
    deps.audio = Arc::new(Mutex::new(TapBackend {
        inner: src,
        tap: tap.clone(),
    }));
    let worker = local::find_worker().ok_or("lowk-stt-worker not found (set LOWK_STT_WORKER)")?;
    let store = ModelStore::new(o.models.clone());
    let mut reg = lowk_stt::EngineRegistry::default();
    reg.register(Box::new(NemotronFactory::new(store.clone())));
    deps.stt = Arc::new(reg);
    deps.corrector = if o.correct && local::whisper_installed(&store) {
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
        })) as Arc<dyn Corrector>)
    } else {
        None
    };
    let correction = deps.corrector.is_some();
    let written = Arc::new(Written::default());
    deps.store = Arc::new(SoakStore {
        vault: vault.clone(),
        written: written.clone(),
    });
    let session = Arc::new(Session::new(deps));
    let rx = session.subscribe();

    let stats = Arc::new(Mutex::new(Stats::default()));
    let stop = Arc::new(AtomicBool::new(false));
    let t_start = Instant::now();
    let collector = {
        let (stats, stop, tap) = (stats.clone(), stop.clone(), tap.clone());
        std::thread::spawn(move || {
            // per utterance: arrival time of the first partial that had > j words
            let mut seen: HashMap<u64, Vec<u64>> = HashMap::new();
            loop {
                let ev = match rx.recv_timeout(Duration::from_millis(50)) {
                    Ok(AppEvent::Stt(e)) => e,
                    Ok(_) => continue,
                    Err(_) if stop.load(Ordering::Relaxed) => return,
                    Err(_) => continue,
                };
                let t_recv = now_mono_ns();
                let t0 = tap.first_ns.load(Ordering::Relaxed);
                let rel = |ns: u64| (ns.saturating_sub(t0)) as f32 / 1e9;
                let words = norm_words(&ev.text);
                let mut st = stats.lock().unwrap();
                match ev.kind {
                    SttKind::Final => {
                        st.utts.insert(ev.utt, (rel(ev.t_audio_end_ns), words.len()));
                        if st.live_done.insert(ev.utt, ()).is_some() {
                            st.rev_finals += 1;
                            continue; // a corrected revision: counted, not a "live" lag
                        }
                        st.finals += 1;
                        let mut sn = seen.remove(&ev.utt).unwrap_or_default();
                        sn.resize(words.len().max(sn.len()), t_recv);
                        let span: Vec<(u64, f32)> = {
                            let q = tap.frames.lock().unwrap();
                            q.iter()
                                .copied()
                                .filter(|(t, _)| *t >= ev.t_start_ns && *t <= ev.t_audio_end_ns)
                                .collect()
                        };
                        if span.len() < 10 || words.is_empty() {
                            continue;
                        }
                        let total: usize = words.iter().map(|w| w.chars().count() + 1).sum::<usize>().max(1);
                        let mut c = 0usize;
                        for (j, w) in words.iter().enumerate() {
                            c += w.chars().count() + 1;
                            let spoken_end = active_time(&span, c as f64 / total as f64);
                            let lag = (sn[j] as f64 - spoken_end as f64) / 1e6;
                            st.lags.push((rel(sn[j]), lag as f32));
                        }
                    }
                    SttKind::Partial => {
                        st.partials += 1;
                        if st.live_done.contains_key(&ev.utt) {
                            continue;
                        }
                        let sn = seen.entry(ev.utt).or_default();
                        if sn.len() < words.len() {
                            sn.resize(words.len(), t_recv);
                        }
                    }
                }
            }
        })
    };

    let call = session
        .start(StartOpts {
            title: Some("soak".into()),
            folder: None,
            system: Some(Target::WholeSystem),
            mic: None,
            demo: false,
            languages: None,
        })
        .map_err(|e| format!("session start: {e:?}"))?;

    // sampler + index probes on this thread until the audio is fed
    let mut samples: Vec<Sample> = vec![];
    let mut probes: Vec<IndexProbe> = vec![];
    let mut last_probe = Instant::now();
    let mut last_win = 0.0f32;
    let probe_every = Duration::from_secs(o.probe_mins * 60);
    let sample_every = Duration::from_secs(o.sample_secs);
    let deadline = Duration::from_secs_f64(f64::from(total_s) / o.speed.max(0.01) * 2.0 + 600.0);
    let mut next = Instant::now() + sample_every;
    let take = |samples: &mut Vec<Sample>, last_win: &mut f32| {
        let t0 = tap.first_ns.load(Ordering::Relaxed);
        let audio_s = tap.n_frames.load(Ordering::Relaxed) as f64 * 0.02;
        let st = stats.lock().unwrap();
        let now_rel = t_start.elapsed().as_secs_f32();
        let _ = (t0, now_rel);
        let hi = (audio_s as f32).max(*last_win);
        let (n, p50, p95) = win_stats(&st.lags, *last_win, hi + 1.0);
        *last_win = hi;
        let finals = st.finals;
        drop(st);
        let d = session.stt_diagnostics();
        let (restarts, backlog_ms, emit_p95) = d.map_or((0, 0, 0), |d| {
            (
                d.streams.iter().map(|s| s.restarts).sum(),
                d.streams.iter().map(|s| s.backlog_ms).max().unwrap_or(0),
                d.streams.iter().map(|s| s.emit_lag.p95_ms).max().unwrap_or(0),
            )
        });
        let (ra, rc) = rss_mb().unwrap_or((f64::NAN, f64::NAN));
        let md_kb = vault.read_call(&call).map_or(0.0, |c| c.md.len() as f64 / 1024.0);
        let s = Sample {
            t_s: t_start.elapsed().as_secs_f64(),
            audio_s,
            rss_app: ra,
            rss_children: rc,
            md_kb,
            lag_p50: p50,
            lag_p95: p95,
            words_win: n,
            restarts,
            backlog_ms,
            emit_p95,
            dropped: tap.dropped_samples.load(Ordering::Relaxed),
            finals,
            load: load1(),
        };
        eprintln!(
            "[{:>6.0}s audio {:>6.0}s] rss app {:.0} MB workers {:.0} MB | md {:.0} KB | lag p50/p95 {:.0}/{:.0} ms ({} words) | restarts {} backlog {} ms | dropped {} | finals {}",
            s.t_s, s.audio_s, s.rss_app, s.rss_children, s.md_kb, s.lag_p50, s.lag_p95, s.words_win, s.restarts, s.backlog_ms, s.dropped, s.finals
        );
        samples.push(s);
    };
    let index_probe = |probes: &mut Vec<IndexProbe>, audio_s: f64| {
        let before = svc.index.status();
        let t = Instant::now();
        svc.on_store_change(&StoreChange {
            kind: StoreChangeKind::Updated,
            id: call,
            is_folder: false,
        });
        let mut after = before.clone();
        while t.elapsed() < Duration::from_secs(120) {
            std::thread::sleep(Duration::from_millis(100));
            after = svc.index.status();
            if after.pending == 0 && after.chunks != before.chunks {
                break;
            }
        }
        let md = vault.read_call(&call).map(|c| c.md).unwrap_or_default();
        let last_words: String = norm_words(&md)
            .iter()
            .rev()
            .take(3)
            .rev()
            .cloned()
            .collect::<Vec<_>>()
            .join(" ");
        let r = svc.index.search(&SearchQuery {
            text: last_words,
            filters: SearchFilters::default(),
            mode: SearchMode::Keyword,
            limit: 10,
            offset: 0,
        });
        probes.push(IndexProbe {
            at_s: audio_s,
            md_kb: md.len() as f64 / 1024.0,
            chunks: after.chunks,
            update_s: t.elapsed().as_secs_f64(),
            search_ms: r.map_or(u32::MAX, |r| r.took_ms),
        });
        let p = probes.last().unwrap();
        eprintln!(
            "  index probe @ audio {:.0} s: .md {:.0} KB -> {} chunks, update {:.1} s, search {} ms",
            p.at_s, p.md_kb, p.chunks, p.update_s, p.search_ms
        );
    };
    while !done.is_done() && t_start.elapsed() < deadline {
        std::thread::sleep(Duration::from_millis(500));
        if Instant::now() >= next {
            next += sample_every;
            take(&mut samples, &mut last_win);
        }
        if last_probe.elapsed() >= probe_every {
            last_probe = Instant::now();
            let a = tap.n_frames.load(Ordering::Relaxed) as f64 * 0.02;
            index_probe(&mut probes, a);
        }
    }
    let fed = done.is_done();
    std::thread::sleep(Duration::from_secs(10)); // last partials and corrections
    take(&mut samples, &mut last_win);
    let errs = session.stt_errors();
    let diag = session.stt_diagnostics();
    let stopped = session.stop();
    stop.store(true, Ordering::Relaxed);
    let _ = collector.join();
    let wall_s = t_start.elapsed().as_secs_f64();
    let audio_s = tap.n_frames.load(Ordering::Relaxed) as f64 * 0.02;
    index_probe(&mut probes, audio_s);

    // ---- the written file ----
    let md = vault
        .read_call(&call)
        .map(|c| c.md)
        .map_err(|e| format!("{e:?}"))?;
    let doc = lowk_store::call_codec::parse(&md);
    let (mut turns, mut md_words) = (0usize, 0usize);
    for b in doc.blocks() {
        if let lowk_store::call_codec::Block::Turn { text, .. } = b {
            turns += 1;
            md_words += norm_words(text).len();
        }
    }
    let st = stats.lock().unwrap();
    let bus_utts = st.utts.len();
    let bus_words: usize = st.utts.values().map(|u| u.1).sum();
    let nonempty_utts = st.utts.values().filter(|u| u.1 > 0).count();

    // completeness per 10-minute window: words fed (reference) vs words recognised
    let win_s = 600.0f32;
    let n_win = (audio_s as f32 / win_s).ceil() as usize;
    let mut fed_w = vec![0usize; n_win.max(1)];
    let mut got_w = vec![0usize; n_win.max(1)];
    let loops = (total_s / base_s).ceil() as usize;
    'outer: for l in 0..loops {
        for c in &clips {
            let start = l as f32 * base_s + c.start_s;
            if start + c.dur_s > audio_s as f32 - 5.0 {
                break 'outer; // the tail not fed (yet) is not "expected"
            }
            fed_w[((start / win_s) as usize).min(n_win - 1)] += c.words;
        }
    }
    for (end, w) in st.utts.values() {
        got_w[((*end / win_s) as usize).min(n_win - 1)] += w;
    }
    let mut ends: Vec<f32> = st.utts.values().filter(|u| u.1 > 0).map(|u| u.0).collect();
    ends.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let max_gap = ends.windows(2).map(|w| w[1] - w[0]).fold(0.0f32, f32::max);
    let longest_clip = clips.iter().map(|c| c.dur_s).fold(0.0f32, f32::max);

    // ---- verdicts ----
    type Pts = Vec<(f64, f64)>;
    let (rss_pts, lag_pts): (Pts, Pts) = (
        samples
            .iter()
            .filter(|s| s.audio_s > 900.0 && s.rss_app.is_finite())
            .map(|s| (s.audio_s / 3600.0, s.rss_app + s.rss_children))
            .collect(),
        samples
            .iter()
            .filter(|s| s.words_win >= 20)
            .map(|s| (s.audio_s / 3600.0, f64::from(s.lag_p50)))
            .collect(),
    );
    let rss_slope = slope(&rss_pts);
    let lag_slope = slope(&lag_pts);
    let edge = 900.0f32.min(audio_s as f32 / 4.0);
    let first = win_stats(&st.lags, 60.0, 60.0 + edge);
    let last = win_stats(&st.lags, audio_s as f32 - edge - 10.0, audio_s as f32);
    let restarts = diag
        .as_ref()
        .map_or(0, |d| d.streams.iter().map(|s| s.restarts).sum::<u32>());
    let dropped = tap.dropped_samples.load(Ordering::Relaxed);
    // complete windows only (the last one is partial), unless the run is shorter than one window
    let complete = if n_win > 1 { n_win - 1 } else { n_win };
    let min_ratio = (0..complete)
        .filter(|&i| fed_w[i] > 0)
        .map(|i| got_w[i] as f64 / fed_w[i] as f64)
        .fold(f64::INFINITY, f64::min);
    let rss_first = samples.iter().find(|s| s.rss_app.is_finite() && s.t_s > 120.0);
    let rss_last = samples.last();
    let wp50: Vec<f32> = (0..n_win)
        .map(|i| win_stats(&st.lags, i as f32 * win_s, (i + 1) as f32 * win_s))
        .filter(|w| w.0 >= 30)
        .map(|w| w.1)
        .collect();
    let (half_a, half_b) = if wp50.len() >= 4 {
        let h = wp50.len() / 2;
        (median(&mut wp50[..h].to_vec()), median(&mut wp50[h..].to_vec()))
    } else {
        (f32::NAN, f32::NAN)
    };
    let checks: Vec<(String, bool, String)> = vec![
        ("audio fed to the end".into(), fed, format!("{audio_s:.0} s of {total_s:.0} s in {wall_s:.0} s wall")),
        ("session stops cleanly".into(), stopped.is_ok(), format!("{stopped:?}")),
        ("STT worker never died or errored".into(), errs.is_empty() && restarts == 0, format!("{restarts} restarts, {} errors {:?}", errs.len(), errs.first())),
        ("no ring overruns / dropped audio".into(), dropped == 0, format!("{dropped} samples in {} events", tap.drop_events.load(Ordering::Relaxed))),
        ("no transcript gaps (every 10-min window >= 70% of the reference words, no silence > longest clip + 20 s)".into(),
            min_ratio >= 0.7 && max_gap < longest_clip + 20.0, format!("min window ratio {min_ratio:.2}, max gap between finals {max_gap:.1} s (longest clip {longest_clip:.0} s)")),
        ("everything the bus finalised reached the .md".into(), md_words.abs_diff(bus_words) * 100 <= bus_words.max(1), format!("bus {bus_words} words / {bus_utts} utterances vs .md {md_words} words / {turns} lines")),
        ("RSS does not grow (< 25 MB/h trend after 15 min, app + workers)".into(), rss_slope.is_nan() || rss_slope < 25.0, format!("{rss_slope:.1} MB/h")),
        ("latency does not drift (median of the 10-min window p50s, 2nd half <= 1st half + 300 ms; isolated contention spikes do not count)".into(),
            half_b.is_nan() || half_a.is_nan() || half_b <= half_a + 300.0, format!("1st half {half_a:.0} ms, 2nd half {half_b:.0} ms; window p50 trend {lag_slope:.0} ms/h (informational); machine load avg per window in the table")),
    ];

    // ---- report ----
    let mut r = String::new();
    r += &format!(
        "# Soak run\n\n- audio fed: **{audio_s:.0} s ({:.2} h)** as the System stream, looped from {} bench clips ({base_s:.0} s loop, {ref_words} reference words/loop), speed x{}\n- wall time: {wall_s:.0} s; correction (Whisper): {}; engine: {}\n- ring: real capture pipeline (48 kHz ring -> resampler -> frames); AEC off (no Me stream)\n\n",
        audio_s / 3600.0, clips.len(), o.speed, if correction { "on" } else { "off" }, local::NEMOTRON_ID
    );
    r += &format!(
        "- clips: {}\n\n",
        clips
            .iter()
            .map(|c| format!("{} ({:.0} s)", c.name, c.dur_s))
            .collect::<Vec<_>>()
            .join(", ")
    );
    r += "## Checks\n\n| check | result | detail |\n|---|---|---|\n";
    for (n, ok, d) in &checks {
        r += &format!("| {n} | {} | {d} |\n", if *ok { "PASS" } else { "**FAIL**" });
    }
    r += "\n## Memory\n\n";
    if let (Some(a), Some(b)) = (rss_first, rss_last) {
        r += &format!(
            "- app RSS {:.0} -> {:.0} MB; workers {:.0} -> {:.0} MB (first sample after 2 min -> last)\n- trend after 15 min: {rss_slope:.1} MB/h (app + workers)\n",
            a.rss_app, b.rss_app, a.rss_children, b.rss_children
        );
    }
    r += &format!("\n## Word-to-screen latency (estimator, +-0.3 s)\n\n- first {edge:.0} s window (from 1 min): n={} p50 {:.0} ms p95 {:.0} ms\n- last {edge:.0} s window: n={} p50 {:.0} ms p95 {:.0} ms\n- p50 trend: {lag_slope:.0} ms/h\n- whole run: n={} p50 {:.0} ms p95 {:.0} ms\n\n| window (audio min) | words | p50 ms | p95 ms | words fed (ref) | words recognised | ratio | load avg (mean) |\n|---|---|---|---|---|---|---|---|\n",
        first.0, first.1, first.2, last.0, last.1, last.2,
        st.lags.len(), pct(&st.lags.iter().map(|l| l.1).collect::<Vec<_>>(), 0.5), pct(&st.lags.iter().map(|l| l.1).collect::<Vec<_>>(), 0.95));
    for i in 0..n_win {
        let (lo, hi) = (i as f32 * win_s, (i + 1) as f32 * win_s);
        let (n, p50, p95) = win_stats(&st.lags, lo, hi);
        r += &format!(
            "| {:.0}-{:.0} | {n} | {p50:.0} | {p95:.0} | {} | {} | {:.2} | {:.1} |\n",
            lo / 60.0,
            hi / 60.0,
            fed_w[i],
            got_w[i],
            got_w[i] as f64 / fed_w[i].max(1) as f64,
            {
                let l: Vec<f64> = samples
                    .iter()
                    .filter(|s| s.audio_s >= f64::from(lo) && s.audio_s < f64::from(hi) && s.load.is_finite())
                    .map(|s| s.load)
                    .collect();
                l.iter().sum::<f64>() / l.len().max(1) as f64
            }
        );
    }
    r += &format!(
        "\n(last window is partial; ratio compares finalised words with the words of clips fully fed in that window)\n\n## Counts\n\n- utterances on the bus: {bus_utts} ({nonempty_utts} non-empty), live finals {}, corrected revisions {}, partial events {}\n- finals pushed into the call journal: {}; journal materialisations: {}\n- .md: {:.0} KB, {turns} lines, {md_words} words\n- engine: {:?}\n",
        st.finals, st.rev_finals, st.partials,
        written.finals.load(Ordering::Relaxed), written.materializations.load(Ordering::Relaxed),
        md.len() as f64 / 1024.0, diag
    );
    r += "\n## Search index\n\n| audio s | .md KB | chunks | update s | search ms |\n|---|---|---|---|---|\n";
    for p in &probes {
        r += &format!(
            "| {:.0} | {:.0} | {} | {:.1} | {} |\n",
            p.at_s, p.md_kb, p.chunks, p.update_s, p.search_ms
        );
    }
    r += "\n## Series\n\n| wall s | audio s | app MB | workers MB | .md KB | p50 ms | p95 ms | words | restarts | backlog ms | emit p95 ms | dropped | load |\n|---|---|---|---|---|---|---|---|---|---|---|---|---|\n";
    for s in &samples {
        r += &format!(
            "| {:.0} | {:.0} | {:.0} | {:.0} | {:.0} | {:.0} | {:.0} | {} | {} | {} | {} | {} |\n",
            s.t_s,
            s.audio_s,
            s.rss_app,
            s.rss_children,
            s.md_kb,
            s.lag_p50,
            s.lag_p95,
            s.words_win,
            s.restarts,
            s.backlog_ms,
            s.emit_p95,
            s.dropped
        );
    }
    println!("{r}");
    if let Some(p) = &o.report {
        std::fs::write(p, &r).map_err(|e| e.to_string())?;
    }
    if let Some(p) = &o.csv {
        let mut c = String::from("wall_s,audio_s,rss_app_mb,rss_workers_mb,md_kb,lag_p50_ms,lag_p95_ms,words,restarts,backlog_ms,emit_p95_ms,dropped_samples\n");
        for s in &samples {
            c += &format!(
                "{:.0},{:.0},{:.0},{:.0},{:.0},{:.0},{:.0},{},{},{},{},{}\n",
                s.t_s,
                s.audio_s,
                s.rss_app,
                s.rss_children,
                s.md_kb,
                s.lag_p50,
                s.lag_p95,
                s.words_win,
                s.restarts,
                s.backlog_ms,
                s.emit_p95,
                s.dropped
            );
        }
        std::fs::write(p, c).map_err(|e| e.to_string())?;
    }
    for (n, ok, d) in &checks {
        println!("CHECK {} {n} :: {d}", if *ok { "PASS" } else { "FAIL" });
    }
    Ok(checks.iter().all(|c| c.1))
}

/// `lowk --selfcheck-soak --audio <fixtures/stt> --models <dir> [--hours 2] [--speed 1]
/// [--no-correct] [--sample-secs 60] [--probe-mins 20] [--report f.md] [--csv f.csv] [--keep]`.
pub fn main(args: &[String]) -> i32 {
    let arg = |k: &str| {
        args.iter()
            .position(|a| a == k)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let has = |k: &str| args.iter().any(|a| a == k);
    let (Some(audio), Some(models)) = (arg("--audio"), arg("--models")) else {
        eprintln!("usage: lowk --selfcheck-soak --audio <dir with clips + .txt> --models <engine models dir> [--hours 2] [--speed 1] [--no-correct] [--sample-secs 60] [--probe-mins 20] [--report f.md] [--csv f.csv] [--keep]");
        return 2;
    };
    let o = Opts {
        audio: audio.into(),
        models: models.into(),
        hours: arg("--hours").and_then(|s| s.parse().ok()).unwrap_or(2.0),
        speed: arg("--speed").and_then(|s| s.parse().ok()).unwrap_or(1.0),
        correct: !has("--no-correct"),
        sample_secs: arg("--sample-secs").and_then(|s| s.parse().ok()).unwrap_or(60),
        probe_mins: arg("--probe-mins").and_then(|s| s.parse().ok()).unwrap_or(20),
        report: arg("--report").map(PathBuf::from),
        csv: arg("--csv").map(PathBuf::from),
        keep: has("--keep"),
    };
    match run(&o) {
        Ok(ok) => {
            println!("SELFCHECK-SOAK {}", if ok { "ok" } else { "FAILED" });
            i32::from(!ok)
        }
        Err(e) => {
            eprintln!("soak: {e}");
            2
        }
    }
}
