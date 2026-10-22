//! Headless, silent check of the whole audio path, run by `lowk --selfcheck-audio` (the bundle's
//! own binary, so `just qa-bundle` exercises the bundled AEC3 and the bundled STT worker):
//!
//! * owner bug A ("I said something, the app started recognising it and closed"): the Me and
//!   System streams are fed from FILES as raw 48 kHz device signals (hot, clipping mic plus a
//!   strong echo of the System speech) through the real ring -> resampler -> AEC3 -> session ->
//!   STT worker path for `--seconds` (60 in qa-bundle). The process must stay alive, the worker
//!   must not die, both streams must produce text, the call must save.
//! * owner bug B (capture altered playback): a real tap + mic is started and rebuilt several
//!   times; the capture aggregate must have no output stream (it cannot play) and no Lowk
//!   aggregate may be left behind (needs the System Audio permission, else the check is skipped).
//!
//! Nothing is played; nothing here runs unless the flag is passed.
use lowk_audio::{AecLink, PipelineFileSource, PipelineOpts};
use lowk_session::{MemCallStore, Session, SessionDeps};
use lowk_stt::local::NemotronFactory;
use lowk_stt::ModelStore;
use lowk_types::fakes::{InlineMainThread, MemSettingsStore, NoopRetriever};
use lowk_types::*;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn check(ok: bool, name: &str, detail: impl std::fmt::Display) -> bool {
    println!("CHECK {} {name} :: {detail}", if ok { "PASS" } else { "FAIL" });
    ok
}
fn skip(name: &str, why: impl std::fmt::Display) {
    println!("CHECK SKIP {name} :: {why}");
}

struct Opts {
    me: PathBuf,
    system: PathBuf,
    models: PathBuf,
    seconds: f32,
    hot: bool,
    require_aec: bool,
}

fn run_session(o: &Opts, tag: &str, mic_gain: f32, echo_gain: f32) -> bool {
    let mut settings = Settings::default();
    settings.stt.engine = "nemotron-local".into();
    settings.stt.languages = vec!["ru".into(), "en".into()];
    settings.stt.background_correction = false;
    settings.audio.aec = AecMode::On;
    let mut deps = SessionDeps::new(
        Arc::new(InlineMainThread),
        Arc::new(NoopRetriever),
        Arc::new(MemSettingsStore::new(settings)),
    );
    let src = PipelineFileSource::new(
        Some(o.me.clone()),
        Some(o.system.clone()),
        PipelineOpts {
            mic_gain,
            echo_gain,
            seconds: o.seconds,
            speed: 1.0,
        },
    );
    let done = src.done();
    deps.audio = Arc::new(Mutex::new(src));
    let mut reg = lowk_stt::EngineRegistry::default();
    reg.register(Box::new(NemotronFactory::new(ModelStore::new(o.models.clone()))));
    deps.stt = Arc::new(reg);
    deps.corrector = None;
    let store = Arc::new(MemCallStore::default());
    deps.store = store.clone();
    let session = Arc::new(Session::new(deps));
    let rx = session.subscribe();
    let counts: Arc<Mutex<[(u32, u32); 2]>> = Arc::default(); // per stream: (partials, finals)
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let (c2, s2) = (counts.clone(), stop.clone());
    let collector = std::thread::spawn(move || loop {
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok(AppEvent::Stt(e)) => {
                let i = usize::from(e.stream == Stream::System);
                let mut c = c2.lock().unwrap();
                if e.kind == SttKind::Final {
                    c[i].1 += 1;
                } else {
                    c[i].0 += 1;
                }
            }
            Ok(_) => {}
            Err(_) if s2.load(std::sync::atomic::Ordering::Relaxed) => return,
            Err(_) => {}
        }
    });
    let t0 = Instant::now();
    let started = session.start(StartOpts {
        title: Some(format!("audio selfcheck {tag}")),
        folder: None,
        system: Some(Target::WholeSystem),
        mic: Some("file".into()),
        demo: false,
        languages: None,
    });
    if !check(
        started.is_ok(),
        &format!("{tag}: session starts on file audio"),
        format!("{started:?}"),
    ) {
        return false;
    }
    let finished = done.wait(Duration::from_secs_f32(o.seconds * 2.0 + 120.0));
    std::thread::sleep(Duration::from_millis(2500)); // last partials
    let stopped = session.stop();
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    let _ = collector.join();
    let errs = session.stt_errors();
    let c = *counts.lock().unwrap();
    let mut ok = true;
    ok &= check(
        finished,
        &format!("{tag}: audio fed to the end"),
        format!(
            "{:.0}s wall for {:.0}s of audio",
            t0.elapsed().as_secs_f32(),
            o.seconds
        ),
    );
    ok &= check(
        stopped.is_ok(),
        &format!("{tag}: session stops cleanly"),
        format!("{stopped:?}"),
    );
    ok &= check(
        errs.is_empty(),
        &format!("{tag}: STT worker never died"),
        format!("{} error(s) {:?}", errs.len(), errs.first()),
    );
    ok &= check(
        c[1].1 >= 3,
        &format!("{tag}: System speech recognised"),
        format!("{} partials / {} finals", c[1].0, c[1].1),
    );
    ok &= check(
        c[0].0 + c[0].1 >= 1,
        &format!("{tag}: Me speech recognised (own words survive AEC)"),
        format!("{} partials / {} finals", c[0].0, c[0].1),
    );
    ok
}

#[cfg(target_os = "macos")]
fn tap_checks() {
    use lowk_audio::mac::{devices, tap};
    use lowk_audio::pipeline::RingIn;
    if !devices::lowk_aggregates().is_empty() {
        check(
            false,
            "aggregates: none before the test",
            format!("{:?}", devices::lowk_aggregates()),
        );
        return;
    }
    let (ring, _rx) = RingIn::new(Default::default(), 48_000);
    let first = tap::start_tap(&tap::TapTarget::System(vec![]), ring);
    let t = match first {
        Ok(t) => t,
        Err(e) => {
            skip(
                "capture aggregate cannot play / no aggregate left",
                format!("tap unavailable ({e}): grant System Audio Recording"),
            );
            return;
        }
    };
    let mut ok = check(
        t.output_streams() == 0,
        "capture aggregate has no output stream (cannot play, cannot alter the speakers)",
        format!("{} output streams", t.output_streams()),
    );
    ok &= check(
        devices::lowk_aggregates().len() == 1,
        "exactly one capture aggregate while capturing",
        format!("{:?}", devices::lowk_aggregates()),
    );
    drop(t);
    for round in 0..4 {
        let (ring, _rx) = RingIn::new(Default::default(), 48_000);
        if let Ok(t) = tap::start_tap(&tap::TapTarget::System(vec![]), ring) {
            ok &= t.output_streams() == 0;
            std::thread::sleep(Duration::from_millis(250));
            drop(t);
        }
        ok &= check(
            devices::lowk_aggregates().is_empty(),
            &format!("no Lowk aggregate left after rebuild {round}"),
            format!("{:?}", devices::lowk_aggregates()),
        );
    }
    let _ = ok;
}

pub fn main(args: &[String]) -> i32 {
    let arg = |k: &str| {
        args.iter()
            .position(|a| a == k)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let has = |k: &str| args.iter().any(|a| a == k);
    let (Some(me), Some(system), Some(models)) = (arg("--me"), arg("--system"), arg("--models")) else {
        eprintln!("usage: lowk --selfcheck-audio --me <file> --system <file> --models <dir> [--seconds 60] [--hot] [--require-aec] [--no-tap]");
        return 2;
    };
    let o = Opts {
        me: me.into(),
        system: system.into(),
        models: models.into(),
        seconds: arg("--seconds").and_then(|s| s.parse().ok()).unwrap_or(60.0),
        hot: has("--hot"),
        require_aec: has("--require-aec"),
    };
    let mut ok = true;
    if o.require_aec {
        ok &= check(
            AecLink::available(),
            "WebRTC AEC3 is compiled into this binary",
            "feature aec",
        );
    }
    // normal conversation level, mild echo: the everyday case
    ok &= run_session(&o, "mic+system+AEC", 1.0, 0.35);
    if o.hot {
        // hot clipping mic (x10, samples far beyond full scale) with a loud echo of the speakers
        ok &= run_session(&o, "hot mic + loud echo", 10.0, 1.2);
    }
    #[cfg(target_os = "macos")]
    if !has("--no-tap") {
        tap_checks();
    }
    println!("SELFCHECK-AUDIO {}", if ok { "ok" } else { "FAILED" });
    i32::from(!ok)
}
