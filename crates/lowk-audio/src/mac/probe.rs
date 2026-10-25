//! System-audio permission probe (macOS has no public API for "System Audio Recording").
//!
//! We play a very quiet tone (cut as soon as the verdict is known) through a child `afplay` and tap exactly that process.
//! Creating the tap is what makes macOS show the real TCC prompt. Non-silent samples while
//! the child reports `IsRunningOutput` => granted; all-zero buffers while it plays (macOS
//! hands out silence, not an error, when the permission is missing) or a tap error => denied.
use super::tap::{start_tap, TapTarget};
use crate::pipeline::{RingIn, StreamShared};
use cidre::core_audio as ca;
use lowk_platform::perm::{report_system_audio_probe, set_system_audio_reason, SystemAudioProbe};
use ringbuf::traits::Consumer;
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};

const THRESH: f32 = 1e-4;

fn write_tone(path: &std::path::Path) -> std::io::Result<()> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 48_000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut w = hound::WavWriter::create(path, spec).map_err(std::io::Error::other)?;
    let n = 48_000 * 5 / 2;
    for i in 0..n {
        let env = ((i.min(n - i)) as f32 / 2400.0).min(1.0);
        let s = (2.0 * std::f32::consts::PI * 440.0 * i as f32 / 48_000.0).sin() * 0.05 * env;
        w.write_sample((s * 32767.0) as i16)
            .map_err(std::io::Error::other)?;
    }
    w.finalize().map_err(std::io::Error::other)
}

fn run_probe() -> SystemAudioProbe {
    let dir = std::env::temp_dir().join(format!("lowk-probe-{}", std::process::id()));
    if std::fs::create_dir_all(&dir).is_err() {
        return SystemAudioProbe::Silent;
    }
    let wav = dir.join("probe.wav");
    if write_tone(&wav).is_err() {
        return SystemAudioProbe::Silent;
    }
    let result = probe_with(&wav);
    let _ = std::fs::remove_dir_all(&dir);
    result
}

fn probe_with(wav: &std::path::Path) -> SystemAudioProbe {
    // Long enough to still be playing while a permission prompt is on screen.
    let Ok(mut child) = lowk_platform::process::command("/usr/bin/afplay")
        .args(["-v", "0.5"])
        .arg(wav)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return SystemAudioProbe::Silent;
    };
    let pid = child.id() as i32;
    let kill = |c: &mut std::process::Child| {
        let _ = c.kill();
        let _ = c.wait();
    };
    // The process object only exists once the child touched Core Audio.
    let t0 = Instant::now();
    let obj = loop {
        // Ok(0) = "no audio object yet" (success status, kAudioObjectUnknown): keep waiting.
        if let Ok(p) = ca::Process::with_pid(pid) {
            if p.0 .0 != 0 {
                break Some(p);
            }
        }
        if t0.elapsed() > Duration::from_secs(3) {
            break None;
        }
        std::thread::sleep(Duration::from_millis(30));
    };
    let Some(obj) = obj else {
        kill(&mut child);
        return SystemAudioProbe::Silent;
    };
    let shared = Arc::new(StreamShared::default());
    let (ring, mut cons) = RingIn::new(shared, 48_000);
    // May block while the TCC prompt is shown.
    let tap = match start_tap(&TapTarget::Procs(vec![obj.0 .0]), ring) {
        Ok(t) => t,
        Err(e) => {
            tracing::warn!(?e, "system-audio probe: tap failed");
            kill(&mut child);
            set_system_audio_reason(format!("{e:?}"));
            return SystemAudioProbe::TapError;
        }
    };
    let mut buf = vec![0f32; 4096];
    let mut zeros_while_playing = Duration::ZERO;
    let mut last = Instant::now();
    let deadline = Instant::now() + Duration::from_secs(4);
    let mut verdict = SystemAudioProbe::Silent;
    while Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
        let n = cons.pop_slice(&mut buf);
        let dt = last.elapsed();
        last = Instant::now();
        if n > 0 && buf[..n].iter().any(|v| v.abs() > THRESH) {
            verdict = SystemAudioProbe::Working;
            break;
        }
        let playing = obj.is_running_output().unwrap_or(false);
        if playing && n > 0 {
            zeros_while_playing += dt;
            if zeros_while_playing > Duration::from_millis(1200) {
                verdict = SystemAudioProbe::Failed;
                break;
            }
        }
        if !playing && child.try_wait().map(|s| s.is_some()).unwrap_or(true) {
            break;
        }
    }
    drop(tap);
    kill(&mut child);
    verdict
}

/// Runs the probe (blocking up to `timeout`; the user may be answering a prompt), updates the
/// shared permission state and returns the verdict.
pub fn probe_system_audio(timeout: Duration) -> SystemAudioProbe {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("audio-probe".into())
        .spawn(move || {
            let _ = tx.send(run_probe());
        })
        .expect("spawn probe");
    let r = rx.recv_timeout(timeout).unwrap_or(SystemAudioProbe::Silent);
    report_system_audio_probe(r);
    r
}
