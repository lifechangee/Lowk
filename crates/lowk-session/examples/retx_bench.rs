//! Re-transcription accuracy on the bench clips: runs the OFFLINE pass (`lowk_stt::retx`, Whisper
//! large-v3-turbo through the sidecar worker, chunked by the energy VAD) over each clip and reports
//! CER against the reference text next to it. Files only, nothing is played.
//!
//! cargo run --release -p lowk-session --example retx_bench -- \
//!     --audio <dir with NN_me_q.wav ... and the .txt references> --models <dir with whisper-turbo-local/> \
//!     [--langs ru,en] [--out report.md] [--fail-above <live mean CER>]
//! The worker is found like in the app (`LOWK_STT_WORKER`). Live CER for the same clips comes from
//! `stt_replay` (its "CER live" / "CER corrected" columns); `--fail-above <live mean>` exits 1 unless the
//! re-transcription is at least as good as live (the acceptance check).
use lowk_stt::retx::{self, Hooks, MemSource, Outcome, PcmSource, WhisperBatch};
use lowk_stt::sidecar::SidecarSpec;
use lowk_stt::{local, ModelStore};
use lowk_types::Stream;
use std::path::PathBuf;
use std::time::{Duration, Instant};

fn arg(k: &str) -> Option<String> {
    let a: Vec<String> = std::env::args().collect();
    a.iter().position(|x| x == k).and_then(|i| a.get(i + 1)).cloned()
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

fn main() {
    let audio = PathBuf::from(arg("--audio").unwrap_or_else(|| "spikes/stt-bench/audio".into()));
    let models = PathBuf::from(arg("--models").expect("--models <dir>"));
    let langs: Vec<String> = arg("--langs")
        .unwrap_or_else(|| "ru,en".into())
        .split(',')
        .map(str::to_owned)
        .collect();
    let store = ModelStore::new(models);
    assert!(
        local::whisper_installed(&store),
        "whisper model missing in --models"
    );
    let exe = local::find_worker().expect("lowk-stt-worker not found (set LOWK_STT_WORKER)");
    let mut tr = WhisperBatch::new(SidecarSpec {
        exe,
        args: vec![
            "correct".into(),
            "--model".into(),
            store
                .path(local::WHISPER_ID, local::WHISPER_FILE)
                .to_string_lossy()
                .into_owned(),
            "--threads".into(),
            arg("--threads").unwrap_or_else(|| "4".into()),
        ],
        nice: 5,
    });
    let mut wavs: Vec<PathBuf> = std::fs::read_dir(&audio)
        .expect("audio dir")
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "wav") && p.with_extension("txt").exists())
        .collect();
    wavs.sort();
    let mut rows = vec![];
    let mut md = String::from("# Re-transcription bench (offline pass, Whisper large-v3-turbo)\n\n| clip | audio s | chunks | CER | wall s |\n|---|---|---|---|---|\n");
    for w in &wavs {
        let name = w.file_stem().unwrap().to_string_lossy().into_owned();
        let pcm = lowk_audio::file::decode_16k(w).expect("decode");
        let secs = pcm.len() as f64 / 16_000.0;
        let reference = std::fs::read_to_string(w.with_extension("txt")).unwrap();
        let mut src = MemSource(pcm);
        let t0 = Instant::now();
        let (c, p) = (|| false, || false);
        let (mut prog, mut pc) = (|_: u64, _: u64| {}, |_: bool| {});
        let mut h = Hooks {
            cancelled: &c,
            paused: &p,
            progress: &mut prog,
            pause_changed: &mut pc,
            poll: Duration::from_millis(50),
        };
        let mut srcs: Vec<(Stream, &mut dyn PcmSource)> = vec![(Stream::Me, &mut src)];
        let Outcome::Done(pieces) = retx::run(&mut srcs, &mut tr, &langs, &mut h).expect("pass") else {
            panic!("cancelled")
        };
        let text = pieces
            .iter()
            .filter_map(|p| p.text.as_deref())
            .collect::<Vec<_>>()
            .join(" ");
        let c = cer(&text, &reference);
        eprintln!(
            "{name}: {} chunks, CER {c:.3}, {:.1}s",
            pieces.len(),
            t0.elapsed().as_secs_f64()
        );
        md += &format!(
            "| {name} | {secs:.0} | {} | {c:.3} | {:.1} |\n",
            pieces.len(),
            t0.elapsed().as_secs_f64()
        );
        rows.push((name, c, text));
    }
    let mean = rows.iter().map(|r| r.1).sum::<f64>() / rows.len().max(1) as f64;
    md += &format!(
        "\n**Mean CER after re-transcription: {mean:.3}** ({} clips)\n\n## Texts\n",
        rows.len()
    );
    for (n, c, t) in &rows {
        md += &format!("\n### {n} (CER {c:.3})\n{t}\n");
    }
    println!("{md}");
    if let Some(p) = arg("--out") {
        std::fs::write(p, md).unwrap();
    }
    if let Some(limit) = arg("--fail-above").and_then(|v| v.parse::<f64>().ok()) {
        if mean > limit {
            eprintln!("FAIL: mean CER {mean:.3} after re-transcription is worse than live {limit:.3}");
            std::process::exit(1);
        }
        eprintln!("OK: mean CER {mean:.3} <= live {limit:.3}");
    }
}
