//! Lowk STT worker: local Nemotron streaming (with Silero VAD segmentation) and Whisper
//! utterance correction on the ggml runtime (transcribe.cpp). Speaks the sidecar protocol
//! of `lowk-stt::sidecar` over stdio.
//!
//! stdin frames : `[kind: u8][len: u32 LE][payload]`
//!   kind 1 audio     : `t_mono_ns: u64 LE` + i16 LE mono 16 kHz samples
//!   kind 2 control   : JSON `{"op":"flush"}` | `{"op":"stop"}`
//!   kind 3 utterance : `id: u64 LE`, `lang_len: u8`, lang bytes, i16 LE samples (correct mode)
//! stdout lines : JSON objects with an `ev` field (`ready`, `stt`, `ack`, `corr`, `error`).
//!
//! usage: lowk-stt-worker stream  --model X.gguf --vad V.gguf [--lang auto|ru-RU] [--right 6]
//!                                [--chunk-ms 160] [--hang-ms 480] [--max-utt-s 25]
//!        lowk-stt-worker correct --model W.gguf [--threads N]
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::io::{BufReader, Read, Write};
use std::sync::mpsc::{channel, Receiver};
use transcribe_cpp::{
    Backend, Model, ModelOptions, ParakeetStreamOptions, RunOptions, SessionOptions, StreamExtension,
    StreamOptions, WhisperRunOptions,
};

type BoxErr = Box<dyn std::error::Error>;

enum Msg {
    Audio { t_ns: u64, pcm: Vec<f32> },
    Flush,
    Stop,
    Utt { id: u64, lang: String, pcm: Vec<f32> },
}

fn emit(v: Value) {
    let mut o = std::io::stdout().lock();
    let _ = writeln!(o, "{v}");
    let _ = o.flush();
}

fn arg(a: &[String], key: &str) -> Option<String> {
    a.iter().position(|x| x == key).and_then(|i| a.get(i + 1)).cloned()
}

fn i16_to_f32(b: &[u8]) -> Vec<f32> {
    b.chunks_exact(2)
        .map(|c| i16::from_le_bytes([c[0], c[1]]) as f32 / 32768.0)
        .collect()
}

fn spawn_reader() -> Receiver<Msg> {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let mut r = BufReader::with_capacity(1 << 16, std::io::stdin().lock());
        loop {
            let mut h = [0u8; 5];
            if r.read_exact(&mut h).is_err() {
                let _ = tx.send(Msg::Stop);
                return;
            }
            let len = u32::from_le_bytes([h[1], h[2], h[3], h[4]]) as usize;
            let mut p = vec![0u8; len];
            if r.read_exact(&mut p).is_err() {
                let _ = tx.send(Msg::Stop);
                return;
            }
            let m = match h[0] {
                1 if len >= 8 => Msg::Audio {
                    t_ns: u64::from_le_bytes(p[..8].try_into().unwrap()),
                    pcm: i16_to_f32(&p[8..]),
                },
                2 => match serde_json::from_slice::<Value>(&p).ok().and_then(|v| v["op"].as_str().map(str::to_owned)) {
                    Some(op) if op == "flush" => Msg::Flush,
                    Some(op) if op == "stop" => Msg::Stop,
                    _ => continue,
                },
                3 if len >= 9 => {
                    let ll = p[8] as usize;
                    Msg::Utt {
                        id: u64::from_le_bytes(p[..8].try_into().unwrap()),
                        lang: String::from_utf8_lossy(&p[9..9 + ll]).into_owned(),
                        pcm: i16_to_f32(&p[9 + ll..]),
                    }
                }
                _ => continue,
            };
            let stop = matches!(m, Msg::Stop);
            if tx.send(m).is_err() || stop {
                return;
            }
        }
    });
    rx
}

struct Chunk {
    t0: u64,
    t1: u64,
    pcm: Vec<f32>,
}

enum Ev {
    Chunk(Chunk),
    Flush,
    Stop,
}

/// Re-chunks 20 ms frames into `n` sample blocks.
struct Chunker {
    n: usize,
    buf: Vec<f32>,
    t0: u64,
}
impl Chunker {
    fn take(&mut self, all: bool) -> Option<Chunk> {
        if self.buf.len() >= self.n || (all && !self.buf.is_empty()) {
            let k = self.n.min(self.buf.len());
            let pcm: Vec<f32> = self.buf.drain(..k).collect();
            let t0 = self.t0;
            let t1 = t0 + (pcm.len() as u64) * 1_000_000_000 / 16_000;
            self.t0 = t1;
            return Some(Chunk { t0, t1, pcm });
        }
        None
    }
}

fn next_ev(rx: &Receiver<Msg>, ch: &mut Chunker, pending: &mut VecDeque<Ev>) -> Ev {
    loop {
        if let Some(e) = pending.pop_front() {
            return e;
        }
        match rx.recv() {
            Ok(Msg::Audio { t_ns, pcm }) => {
                if ch.buf.is_empty() {
                    ch.t0 = t_ns;
                }
                ch.buf.extend_from_slice(&pcm);
                while let Some(c) = ch.take(false) {
                    pending.push_back(Ev::Chunk(c));
                }
            }
            Ok(Msg::Flush) => {
                while let Some(c) = ch.take(true) {
                    pending.push_back(Ev::Chunk(c));
                }
                pending.push_back(Ev::Flush);
            }
            Ok(Msg::Stop) | Err(_) => return Ev::Stop,
            Ok(Msg::Utt { .. }) => {}
        }
    }
}

fn run_stream(a: &[String]) -> Result<(), BoxErr> {
    let model = arg(a, "--model").ok_or("--model")?;
    let vad_path = arg(a, "--vad").ok_or("--vad")?;
    let lang = arg(a, "--lang").unwrap_or_else(|| "auto".into());
    let right: i32 = arg(a, "--right").and_then(|s| s.parse().ok()).unwrap_or(6);
    let chunk_ms: usize = arg(a, "--chunk-ms").and_then(|s| s.parse().ok()).unwrap_or(160);
    let hang: u32 = arg(a, "--hang-ms").and_then(|s| s.parse::<u32>().ok()).unwrap_or(480) / chunk_ms as u32;
    let max_utt_ns: u64 = arg(a, "--max-utt-s").and_then(|s| s.parse::<u64>().ok()).unwrap_or(25) * 1_000_000_000;
    let thr: f32 = arg(a, "--vad-threshold").and_then(|s| s.parse().ok()).unwrap_or(0.5);

    let asr = Model::load(&model)?;
    let vad = Model::load(&vad_path)?;
    let mut asr_s = asr.session()?;
    let mut vad_s = vad.vad_session()?;
    let run = RunOptions {
        // no hint = the model's auto language slot
        language: if lang == "auto" { None } else { Some(lang) },
        ..Default::default()
    };
    let so = StreamOptions {
        family: Some(StreamExtension::ParakeetStream(ParakeetStreamOptions {
            att_context_right: Some(right),
        })),
        ..Default::default()
    };
    // Warm-up: compiles the Metal pipelines so the first utterance is not billed for it.
    {
        let mut s = asr_s.stream(&run, &so)?;
        let z = vec![0.0f32; 2560];
        for _ in 0..6 {
            s.feed(&z)?;
        }
        s.finalize()?;
    }
    emit(json!({"ev":"ready","backend":asr.backend()}));

    let rx = spawn_reader();
    let mut ch = Chunker { n: chunk_ms * 16, buf: vec![], t0: 0 };
    let mut pending = VecDeque::new();
    let mut preroll: VecDeque<Chunk> = VecDeque::new();
    let mut utt: u64 = 0;
    let mut last_ack = 0u64;
    let (mut vad_t, mut asr_t) = (0f64, 0f64);

    'outer: loop {
        let c = match next_ev(&rx, &mut ch, &mut pending) {
            Ev::Chunk(c) => c,
            Ev::Flush => continue,
            Ev::Stop => break,
        };
        let tv = std::time::Instant::now();
        let probs = vad_s.stream_feed(&c.pcm)?.probs;
        vad_t += tv.elapsed().as_secs_f64();
        if c.t1 - last_ack >= 1_000_000_000 {
            last_ack = c.t1;
            emit(json!({"ev":"ack","t_ns":c.t1,"vad_s":vad_t,"asr_s":asr_t}));
        }
        if !probs.iter().any(|p| *p >= thr) {
            preroll.push_back(c);
            while preroll.len() > 2 {
                preroll.pop_front();
            }
            continue;
        }
        // ---- an utterance starts ----
        utt += 1;
        let mut rev = 0u32;
        let t_start = preroll.front().map_or(c.t0, |p| p.t0);
        let mut t_end = c.t1;
        let mut last = String::new();
        let mut stream = asr_s.stream(&run, &so)?;
        let mut first = vec![];
        first.extend(preroll.drain(..));
        first.push(c);
        let mut silent = 0u32;
        let mut stop_all = false;
        let mut todo: VecDeque<Chunk> = first.into();
        loop {
            while let Some(c) = todo.pop_front() {
                t_end = c.t1;
                let ta = std::time::Instant::now();
                let u = stream.feed(&c.pcm)?;
                asr_t += ta.elapsed().as_secs_f64();
                if u.result_changed {
                    let t = stream.text().display().trim().to_owned();
                    if !t.is_empty() && t != last {
                        last = t.clone();
                        emit(json!({"ev":"stt","kind":"partial","utt":utt,"rev":rev,"text":t,
                                    "t_start_ns":t_start,"t_audio_end_ns":t_end}));
                        rev += 1;
                    }
                }
            }
            if t_end - t_start >= max_utt_ns {
                break;
            }
            match next_ev(&rx, &mut ch, &mut pending) {
                Ev::Chunk(c) => {
                    let tv = std::time::Instant::now();
                    let p = vad_s.stream_feed(&c.pcm)?.probs;
                    vad_t += tv.elapsed().as_secs_f64();
                    if p.iter().any(|x| *x >= thr) {
                        silent = 0;
                    } else {
                        silent += 1;
                    }
                    if c.t1 - last_ack >= 1_000_000_000 {
                        last_ack = c.t1;
                        emit(json!({"ev":"ack","t_ns":c.t1,"vad_s":vad_t,"asr_s":asr_t}));
                    }
                    todo.push_back(c);
                    if silent >= hang {
                        // feed the trailing silence too, then close
                        while let Some(c) = todo.pop_front() {
                            t_end = c.t1;
                            stream.feed(&c.pcm)?;
                        }
                        break;
                    }
                }
                Ev::Flush => break,
                Ev::Stop => {
                    stop_all = true;
                    break;
                }
            }
        }
        stream.finalize()?;
        let t = stream.text().full.trim().to_owned();
        drop(stream);
        if !t.is_empty() {
            emit(json!({"ev":"stt","kind":"final","utt":utt,"rev":rev,"text":t,
                        "t_start_ns":t_start,"t_audio_end_ns":t_end}));
        }
        if stop_all {
            break 'outer;
        }
    }
    Ok(())
}

fn run_correct(a: &[String]) -> Result<(), BoxErr> {
    let model = arg(a, "--model").ok_or("--model")?;
    let backend = match arg(a, "--backend").as_deref() {
        Some("cpu") => Backend::Cpu,
        Some("cpu-accel") => Backend::CpuAccel,
        _ => Backend::Auto,
    };
    let m = Model::load_with(&model, &ModelOptions { backend, ..Default::default() })?;
    let threads: i32 = arg(a, "--threads").and_then(|s| s.parse().ok()).unwrap_or(0);
    let mut s = m.session_with(&SessionOptions { n_threads: threads, ..Default::default() })?;
    emit(json!({"ev":"ready","backend":m.backend()}));
    let rx = spawn_reader();
    let prompt = arg(a, "--prompt");
    while let Ok(msg) = rx.recv() {
        match msg {
            Msg::Utt { id, lang, pcm } => {
                let t = std::time::Instant::now();
                let opts = RunOptions {
                    language: if lang.is_empty() || lang == "auto" { None } else { Some(lang) },
                    family: prompt.as_ref().map(|p| {
                        transcribe_cpp::RunExtension::Whisper(WhisperRunOptions {
                            initial_prompt: Some(p.clone()),
                            ..Default::default()
                        })
                    }),
                    ..Default::default()
                };
                match s.run(&pcm, &opts) {
                    Ok(r) => emit(json!({"ev":"corr","id":id,"text":r.text.trim(),
                                         "ms":t.elapsed().as_millis() as u64,
                                         "lang":r.language})),
                    Err(e) => emit(json!({"ev":"error","id":id,"message":e.to_string()})),
                }
            }
            Msg::Stop => break,
            _ => {}
        }
    }
    Ok(())
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let r = match a.get(1).map(String::as_str) {
        Some("stream") => run_stream(&a),
        Some("correct") => run_correct(&a),
        _ => Err("usage: lowk-stt-worker <stream|correct> --model ...".into()),
    };
    if let Err(e) = r {
        emit(json!({"ev":"error","message":e.to_string(),"fatal":true}));
        std::process::exit(1);
    }
}
