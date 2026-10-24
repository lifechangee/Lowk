//! «Перераспознать запись»: a full-quality OFFLINE pass over saved call audio.
//!
//! The live transcript is a streaming compromise; for lectures the final text matters more. This
//! module reads the saved 16 kHz streams (Me and System separately, so the speaker split is kept),
//! cuts each into speech chunks, and runs every chunk through a [`Transcriber`]:
//!
//! * [`WhisperBatch`]: Whisper large-v3-turbo in the same sidecar worker the live correction uses
//!   (`lowk-stt-worker correct`), at low priority, one chunk at a time;
//! * [`DeepgramBatch`]: Deepgram's pre-recorded API (explicit opt-in, never for local-only folders).
//!
//! Chunking is an energy VAD (speech regions bridged over short pauses, packed up to ~22 s, never
//! above 28 s, cut at the quietest point inside long speech, padded by 300 ms of context on both
//! sides). The pass is paused between chunks while a recording runs, can be cancelled at any chunk
//! boundary, and never writes anything itself: it returns [`Piece`]s the caller merges into a new
//! transcript revision. Anything the engine cannot read (error, damaged text, a letter outside the
//! call's languages, a repetition loop) comes back as `None` and the live text of that stretch stays.
use crate::corrector::{connect, Conn};
use crate::sidecar::{frame_bytes, pcm_i16, SidecarSpec, KIND_CONTROL, KIND_UTTERANCE};
use lowk_types::*;
use std::io::Write;
use std::sync::mpsc::RecvTimeoutError;
use std::time::{Duration, Instant};

const FRAME_MS: u64 = 20;
const SAMPLES_PER_FRAME: usize = 320;
/// Chunks are packed up to this length ...
pub const TARGET_CHUNK_MS: u64 = 22_000;
/// ... and never exceed this (Whisper's window is 30 s).
pub const MAX_CHUNK_MS: u64 = 28_000;
const MIN_CHUNK_MS: u64 = 800;
const MIN_SPEECH_MS: u64 = 200;
const PAD_MS: u64 = 300;
/// Pauses shorter than this do not split a speech region.
const BRIDGE_MS: u64 = 700;
/// Regions closer than this share a chunk (while it stays under the target).
const JOIN_GAP_MS: u64 = 1_500;
/// Consecutive failed chunks after which the pass gives up (a dead worker, a bad key).
const MAX_CONSECUTIVE_ERRORS: u32 = 4;

/// Random access to one stream's 16 kHz mono samples.
pub trait PcmSource {
    fn len_samples(&self) -> u64;
    /// `n` samples from `start`, clipped at the end.
    fn read(&mut self, start: u64, n: usize) -> Result<Vec<f32>>;
}

/// In-memory source (tests, the bench replay).
pub struct MemSource(pub Vec<f32>);
impl PcmSource for MemSource {
    fn len_samples(&self) -> u64 {
        self.0.len() as u64
    }
    fn read(&mut self, start: u64, n: usize) -> Result<Vec<f32>> {
        let s = (start as usize).min(self.0.len());
        Ok(self.0[s..(s + n).min(self.0.len())].to_vec())
    }
}

/// Turns one chunk of audio into text.
pub trait Transcriber {
    /// Written to the transcript frontmatter (`retranscribed_by`).
    fn name(&self) -> String;
    /// `lang` = the call's single language, `None` = detect.
    fn transcribe(&mut self, pcm: &[f32], lang: Option<&str>) -> Result<String>;
    /// Frees what the engine holds (a worker's model memory); it loads again on the next call.
    fn release(&mut self) {}
}

/// One re-transcribed stretch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Piece {
    pub stream: Stream,
    pub start_ms: u64,
    pub end_ms: u64,
    /// `None` = unreadable, keep the live text there.
    pub text: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    Done(Vec<Piece>),
    Cancelled,
}

/// Callbacks of a running pass.
pub struct Hooks<'a> {
    pub cancelled: &'a dyn Fn() -> bool,
    /// True while a recording runs: the pass waits (and frees the engine).
    pub paused: &'a dyn Fn() -> bool,
    /// `(done_ms, total_ms)` of audio processed.
    pub progress: &'a mut dyn FnMut(u64, u64),
    pub pause_changed: &'a mut dyn FnMut(bool),
    /// How long to sleep between checks while paused.
    pub poll: Duration,
}

// ---------------------------------------------------------------------------------------
// Chunking
// ---------------------------------------------------------------------------------------

/// RMS per 20 ms frame of a whole stream (read in blocks; checks `cancelled` between them).
pub fn scan_rms(src: &mut dyn PcmSource, cancelled: &dyn Fn() -> bool) -> Result<Option<Vec<f32>>> {
    let total = src.len_samples();
    let block = SAMPLES_PER_FRAME * 500; // 10 s
    let mut out = Vec::with_capacity((total as usize) / SAMPLES_PER_FRAME + 1);
    let mut pos = 0u64;
    while pos < total {
        if cancelled() {
            return Ok(None);
        }
        let b = src.read(pos, block)?;
        if b.is_empty() {
            break;
        }
        for f in b.chunks(SAMPLES_PER_FRAME) {
            out.push((f.iter().map(|x| x * x).sum::<f32>() / f.len() as f32).sqrt());
        }
        pos += b.len() as u64;
    }
    Ok(Some(out))
}

/// Cuts a stream into `(start_ms, end_ms)` chunks from its per-frame RMS. Pure.
pub fn plan_chunks(rms: &[f32]) -> Vec<(u64, u64)> {
    let total_ms = rms.len() as u64 * FRAME_MS;
    let mut live: Vec<f32> = rms.iter().copied().filter(|x| *x > 1e-5).collect();
    if live.is_empty() {
        return vec![];
    }
    live.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let noise = live[live.len() / 5];
    let p95 = live[(live.len() * 95 / 100).min(live.len() - 1)];
    // the noise floor estimate is the speech level itself when a recording is almost all speech,
    // so the threshold is also capped relative to the loud end
    let thr = (noise * 3.0).max(0.08 * p95).min(0.2 * p95).max(0.002);

    // speech regions, bridged over short pauses
    let bridge = (BRIDGE_MS / FRAME_MS) as usize;
    let mut regions: Vec<(u64, u64)> = vec![];
    let (mut start, mut last_active): (Option<usize>, usize) = (None, 0);
    for (i, r) in rms.iter().enumerate() {
        if *r > thr {
            if start.is_none() {
                start = Some(i);
            }
            last_active = i;
        } else if let Some(s) = start {
            if i - last_active > bridge {
                regions.push((s as u64 * FRAME_MS, (last_active as u64 + 1) * FRAME_MS));
                start = None;
            }
        }
    }
    if let Some(s) = start {
        regions.push((s as u64 * FRAME_MS, (last_active as u64 + 1) * FRAME_MS));
    }
    regions.retain(|(s, e)| e - s >= MIN_SPEECH_MS);

    // pack neighbours into chunks of about TARGET_CHUNK_MS
    let mut packed: Vec<(u64, u64)> = vec![];
    for (s, e) in regions {
        match packed.last_mut() {
            Some((ps, pe)) if s - *pe <= JOIN_GAP_MS && e - *ps <= TARGET_CHUNK_MS => *pe = e,
            _ => packed.push((s, e)),
        }
    }

    // cut over-long speech at the quietest frame of the last 8 s of the window
    let mut cut: Vec<(u64, u64)> = vec![];
    for (s, e) in packed {
        let mut cur = s;
        while e - cur > MAX_CHUNK_MS {
            let lo = ((cur + MAX_CHUNK_MS - 8_000) / FRAME_MS) as usize;
            let hi = ((cur + MAX_CHUNK_MS) / FRAME_MS) as usize;
            let at = (lo..hi.min(rms.len()))
                .min_by(|a, b| rms[*a].partial_cmp(&rms[*b]).unwrap())
                .map_or(cur + MAX_CHUNK_MS, |i| i as u64 * FRAME_MS)
                .max(cur + MIN_CHUNK_MS);
            cut.push((cur, at));
            cur = at;
        }
        cut.push((cur, e));
    }

    // context padding without overlapping the neighbours
    let mut out: Vec<(u64, u64)> = Vec::with_capacity(cut.len());
    let mut prev_end = 0u64;
    for (i, (s, e)) in cut.iter().enumerate() {
        let next_start = cut.get(i + 1).map_or(total_ms, |n| n.0);
        let ps = s.saturating_sub(PAD_MS).max(prev_end);
        let mut pe = (e + PAD_MS).min(total_ms).min(next_start.max(*e));
        if pe - ps < MIN_CHUNK_MS {
            pe = (ps + MIN_CHUNK_MS).min(total_ms);
        }
        out.push((ps, pe));
        prev_end = pe;
    }
    out
}

// ---------------------------------------------------------------------------------------
// Text filter
// ---------------------------------------------------------------------------------------

/// The text of a chunk if it can be trusted: not damaged («�»), no letter outside the call's
/// languages, no Whisper repetition loop, a plausible amount of speech for the audio length.
pub fn filter_text(raw: &str, chunk_ms: u64, allowed: &[String]) -> Option<String> {
    if text::has_replacement(raw) {
        return None;
    }
    let t = text::clean_speech(raw);
    if t.is_empty() {
        return None;
    }
    if !allowed.is_empty() && text::foreign_letter_count(&t, allowed) > 0 {
        return None;
    }
    // more than ~40 characters a second is not speech
    if t.chars().count() as u64 * 1000 > chunk_ms.max(1_000) * 40 {
        return None;
    }
    let words: Vec<&str> = t.split_whitespace().collect();
    let mut run = 1;
    for w in words.windows(2) {
        run = if w[0].eq_ignore_ascii_case(w[1]) {
            run + 1
        } else {
            1
        };
        if run >= 8 {
            return None;
        }
    }
    Some(t)
}

// ---------------------------------------------------------------------------------------
// The pass
// ---------------------------------------------------------------------------------------

/// Runs the pass over `sources`. Returns the pieces in time order, or `Cancelled`.
pub fn run(
    sources: &mut [(Stream, &mut dyn PcmSource)],
    tr: &mut dyn Transcriber,
    langs: &[String],
    h: &mut Hooks<'_>,
) -> Result<Outcome> {
    let mut jobs: Vec<(usize, u64, u64)> = vec![];
    for (i, (_, src)) in sources.iter_mut().enumerate() {
        let Some(rms) = scan_rms(&mut **src, h.cancelled)? else {
            return Ok(Outcome::Cancelled);
        };
        jobs.extend(plan_chunks(&rms).into_iter().map(|(s, e)| (i, s, e)));
    }
    jobs.sort_by_key(|(i, s, _)| (*s, *i));
    let total: u64 = jobs.iter().map(|(_, s, e)| e - s).sum();
    (h.progress)(0, total);
    let lang = match langs {
        [one] => Some(one.as_str()),
        _ => None,
    };
    let (mut done, mut errors) = (0u64, 0u32);
    let mut pieces = Vec::with_capacity(jobs.len());
    let mut was_paused = false;
    for (i, s, e) in jobs {
        loop {
            if (h.cancelled)() {
                return Ok(Outcome::Cancelled);
            }
            if !(h.paused)() {
                break;
            }
            if !was_paused {
                was_paused = true;
                tr.release();
                (h.pause_changed)(true);
            }
            std::thread::sleep(h.poll);
        }
        if was_paused {
            was_paused = false;
            (h.pause_changed)(false);
        }
        let (stream, src) = &mut sources[i];
        let pcm = src.read(s * 16, ((e - s) * 16) as usize)?;
        let text = match tr.transcribe(&pcm, lang) {
            Ok(raw) => {
                errors = 0;
                filter_text(&raw, e - s, langs)
            }
            Err(err) => {
                errors += 1;
                tracing::warn!(code = ?err.code, "re-transcription of a chunk failed; keeping the live text");
                if errors >= MAX_CONSECUTIVE_ERRORS {
                    return Err(err);
                }
                None
            }
        };
        pieces.push(Piece {
            stream: *stream,
            start_ms: s,
            end_ms: e,
            text,
        });
        done += e - s;
        (h.progress)(done, total);
    }
    Ok(Outcome::Done(pieces))
}

// ---------------------------------------------------------------------------------------
// Local engine
// ---------------------------------------------------------------------------------------

/// Whisper in the sidecar worker (`lowk-stt-worker correct`), one chunk at a time.
pub struct WhisperBatch {
    spec: SidecarSpec,
    conn: Option<Conn>,
    next_id: u64,
    timeout: Duration,
}

impl WhisperBatch {
    pub fn new(mut spec: SidecarSpec) -> Self {
        // an offline pass never competes with a live call: lowest priority
        spec.nice = spec.nice.max(19);
        Self {
            spec,
            conn: None,
            next_id: 0,
            timeout: Duration::from_secs(240),
        }
    }

    fn stop_conn(&mut self) {
        if let Some((mut child, mut stdin, _)) = self.conn.take() {
            let _ = stdin.write_all(&frame_bytes(KIND_CONTROL, br#"{"op":"stop"}"#));
            drop(stdin);
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl Transcriber for WhisperBatch {
    fn name(&self) -> String {
        "whisper-large-v3-turbo".into()
    }

    fn transcribe(&mut self, pcm: &[f32], lang: Option<&str>) -> Result<String> {
        for attempt in 0..2 {
            if self.conn.is_none() {
                if !self.spec.exe.exists() {
                    return Err(AppError::new(ErrorCode::NotFound).with_detail("lowk-stt-worker"));
                }
                self.conn = connect(&self.spec);
            }
            let Some((child, stdin, replies)) = self.conn.as_mut() else {
                return Err(
                    AppError::new(ErrorCode::EngineNotReady).with_detail("whisper worker did not start")
                );
            };
            self.next_id += 1;
            let id = self.next_id;
            let lang = lang.unwrap_or("");
            let mut p = Vec::with_capacity(9 + lang.len() + pcm.len() * 2);
            p.extend_from_slice(&id.to_le_bytes());
            p.push(lang.len().min(255) as u8);
            p.extend_from_slice(lang.as_bytes());
            pcm_i16(pcm, &mut p);
            if stdin.write_all(&frame_bytes(KIND_UTTERANCE, &p)).is_ok() {
                let t0 = Instant::now();
                loop {
                    match replies.recv_timeout(self.timeout.saturating_sub(t0.elapsed())) {
                        Ok(r) if r.id == id => return Ok(r.text),
                        Ok(_) => {}
                        Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => break,
                    }
                }
            }
            tracing::warn!(attempt, "whisper worker failed or timed out; restarting it");
            let _ = child.kill();
            let _ = child.wait();
            self.conn = None;
        }
        Err(AppError::new(ErrorCode::SttEngine).with_detail("whisper worker failed twice on a chunk"))
    }

    fn release(&mut self) {
        self.stop_conn();
    }
}

impl Drop for WhisperBatch {
    fn drop(&mut self) {
        self.stop_conn();
    }
}

// ---------------------------------------------------------------------------------------
// Cloud engine
// ---------------------------------------------------------------------------------------

/// Which engine a pass uses. Cloud needs an explicit setting and a folder that allows the cloud:
/// a local-only folder is REFUSED (`LocalOnlyBlocked`) for a manual run; the automatic run after a
/// recording quietly uses the local engine instead (the user never asked for that call to leave
/// the machine, and a local pass is always allowed).
pub fn pick_backend(want_cloud: bool, folder_allows_cloud: bool, automatic: bool) -> Result<RetxBackend> {
    match (want_cloud, folder_allows_cloud) {
        (false, _) => Ok(RetxBackend::Local),
        (true, true) => Ok(RetxBackend::Cloud),
        (true, false) if automatic => Ok(RetxBackend::Local),
        (true, false) => Err(AppError::new(ErrorCode::LocalOnlyBlocked).with_detail(
            "this folder is local-only; turn off cloud re-transcription or allow the cloud for the folder",
        )),
    }
}

const DEEPGRAM_LISTEN: &str = "https://api.deepgram.com/v1/listen";

/// `language=<one>` for a single call language, else `multi` (same rule as the live engine).
pub fn deepgram_query(langs: &[String]) -> String {
    let lang = match langs {
        [one] => one.as_str(),
        _ => "multi",
    };
    format!("model=nova-3&language={lang}&punctuate=true&smart_format=true")
}

fn wav16(pcm: &[f32]) -> Vec<u8> {
    let bytes = (pcm.len() * 2) as u32;
    let mut v = Vec::with_capacity(44 + pcm.len() * 2);
    v.extend_from_slice(b"RIFF");
    v.extend_from_slice(&(36 + bytes).to_le_bytes());
    v.extend_from_slice(b"WAVEfmt ");
    v.extend_from_slice(&16u32.to_le_bytes());
    v.extend_from_slice(&1u16.to_le_bytes());
    v.extend_from_slice(&1u16.to_le_bytes());
    v.extend_from_slice(&16_000u32.to_le_bytes());
    v.extend_from_slice(&32_000u32.to_le_bytes());
    v.extend_from_slice(&2u16.to_le_bytes());
    v.extend_from_slice(&16u16.to_le_bytes());
    v.extend_from_slice(b"data");
    v.extend_from_slice(&bytes.to_le_bytes());
    pcm_i16(pcm, &mut v);
    v
}

/// Deepgram Nova-3 pre-recorded API, one request per chunk. The key is held privately and sent
/// only in the `Authorization` header; it is never logged or put in an error.
pub struct DeepgramBatch {
    key: String,
    endpoint: String,
    query: String,
    agent: ureq::Agent,
}

impl DeepgramBatch {
    pub fn new(key: String, langs: &[String]) -> Self {
        Self::with_endpoint(key, langs, DEEPGRAM_LISTEN.into())
    }

    pub fn with_endpoint(key: String, langs: &[String], endpoint: String) -> Self {
        let tls = ureq::tls::TlsConfig::builder()
            .provider(ureq::tls::TlsProvider::Rustls)
            .unversioned_rustls_crypto_provider(crate::cloud::tls_provider())
            .root_certs(ureq::tls::RootCerts::WebPki)
            .build();
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .tls_config(tls)
            .timeout_connect(Some(Duration::from_secs(20)))
            .timeout_global(Some(Duration::from_secs(120)))
            .build()
            .into();
        Self {
            key,
            endpoint,
            query: deepgram_query(langs),
            agent,
        }
    }
}

impl Transcriber for DeepgramBatch {
    fn name(&self) -> String {
        "deepgram-nova-3".into()
    }

    fn transcribe(&mut self, pcm: &[f32], _lang: Option<&str>) -> Result<String> {
        let url = format!("{}?{}", self.endpoint, self.query);
        let body = wav16(pcm);
        let mut last = AppError::new(ErrorCode::Network);
        for attempt in 0..3 {
            if attempt > 0 {
                std::thread::sleep(Duration::from_millis(500 * attempt));
            }
            let r = self
                .agent
                .post(&url)
                .header("Authorization", format!("Token {}", self.key))
                .header("Content-Type", "audio/wav")
                .send(&body[..]);
            match r {
                Ok(mut resp) => {
                    let s = resp.body_mut().read_to_string().map_err(|_| {
                        AppError::new(ErrorCode::Network).with_detail("deepgram response unreadable")
                    })?;
                    let v: serde_json::Value = serde_json::from_str(&s).map_err(|_| {
                        AppError::new(ErrorCode::LlmBadResponse).with_detail("deepgram response is not JSON")
                    })?;
                    return Ok(v["results"]["channels"][0]["alternatives"][0]["transcript"]
                        .as_str()
                        .unwrap_or("")
                        .to_owned());
                }
                // a rejected key is final; the message never contains the key
                Err(ureq::Error::StatusCode(c @ (401 | 403))) => {
                    return Err(AppError::new(ErrorCode::ApiKeyMissing)
                        .with_detail(format!("deepgram rejected the key (HTTP {c})")));
                }
                Err(ureq::Error::StatusCode(c @ 400..=499)) if c != 429 => {
                    return Err(AppError::new(ErrorCode::Network).with_detail(format!("deepgram HTTP {c}")));
                }
                Err(ureq::Error::StatusCode(c)) => {
                    last = AppError::new(ErrorCode::Network).with_detail(format!("deepgram HTTP {c}"));
                }
                Err(_) => last = AppError::new(ErrorCode::Network).with_detail("deepgram request failed"),
            }
        }
        Err(last)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::io::{BufRead, BufReader, Read};
    use std::net::TcpListener;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    /// 20 ms RMS frames: `(seconds, level)` runs.
    fn rms(runs: &[(f32, f32)]) -> Vec<f32> {
        runs.iter()
            .flat_map(|(s, l)| std::iter::repeat_n(*l, (s * 50.0) as usize))
            .collect()
    }

    #[test]
    fn chunks_follow_speech_pad_it_and_stay_under_the_cap() {
        // 2 s quiet, 5 s speech, 0.4 s pause (bridged), 3 s speech, 6 s quiet, 40 s unbroken speech with one dip
        let r = rms(&[
            (2.0, 0.001),
            (5.0, 0.1),
            (0.4, 0.001),
            (3.0, 0.1),
            (6.0, 0.001),
            (20.0, 0.1),
            (0.1, 0.002),
            (20.0, 0.1),
            (3.0, 0.001),
        ]);
        let c = plan_chunks(&r);
        assert_eq!(c.len(), 3, "{c:?}");
        // first chunk: speech 2.0..10.4 s plus 300 ms of context each side
        assert_eq!(c[0], (1_700, 10_700));
        for (s, e) in &c {
            assert!(e - s <= MAX_CHUNK_MS + 2 * PAD_MS, "{s}..{e}");
        }
        // the long speech is cut at its quiet dip, not mid-word
        let dip_ms = (2.0 + 5.0 + 0.4 + 3.0 + 6.0 + 20.0) * 1000.0;
        assert!(
            ((c[1].1 as f32) - dip_ms).abs() < 700.0,
            "cut near the dip: {c:?}"
        );
        // chunks never overlap
        assert!(c.windows(2).all(|w| w[0].1 <= w[1].0));
    }

    #[test]
    fn silence_and_blips_make_no_chunks() {
        assert!(plan_chunks(&rms(&[(10.0, 0.0)])).is_empty());
        assert!(
            plan_chunks(&rms(&[(5.0, 0.001), (0.1, 0.2), (5.0, 0.001)])).is_empty(),
            "100 ms blip"
        );
        assert!(plan_chunks(&[]).is_empty());
    }

    #[test]
    fn text_filter_drops_damage_foreign_letters_loops_and_gibberish_rates() {
        let ru = vec!["ru".to_string(), "en".to_string()];
        assert_eq!(
            filter_text("  Привет, мир!  ", 5_000, &ru).as_deref(),
            Some("Привет, мир!")
        );
        assert_eq!(
            filter_text("Hello world", 5_000, &ru).as_deref(),
            Some("Hello world")
        );
        assert!(filter_text("при\u{FFFD}вет", 5_000, &ru).is_none());
        assert!(filter_text("", 5_000, &ru).is_none());
        assert!(
            filter_text("zażółć gęślą", 5_000, &ru).is_none(),
            "Polish letters in a ru+en call"
        );
        assert!(
            filter_text(&"да ".repeat(12), 20_000, &ru).is_none(),
            "repetition loop"
        );
        assert!(
            filter_text(&"слово ".repeat(100), 1_000, &ru).is_none(),
            "600 chars in a second"
        );
    }

    #[test]
    fn backend_choice_local_only_folders_never_go_to_the_cloud() {
        assert_eq!(pick_backend(false, true, false).unwrap(), RetxBackend::Local);
        assert_eq!(pick_backend(false, false, false).unwrap(), RetxBackend::Local);
        assert_eq!(pick_backend(true, true, false).unwrap(), RetxBackend::Cloud);
        let e = pick_backend(true, false, false).unwrap_err();
        assert_eq!(e.code, ErrorCode::LocalOnlyBlocked, "manual run is refused");
        assert_eq!(
            pick_backend(true, false, true).unwrap(),
            RetxBackend::Local,
            "automatic run stays local"
        );
    }

    /// Answers `f(len_of_pcm)` per call and records what it saw.
    struct Fake {
        calls: RefCell<Vec<(usize, Option<String>)>>,
        released: usize,
        f: Box<dyn FnMut(usize, usize) -> Result<String>>,
    }
    impl Fake {
        fn new(f: impl FnMut(usize, usize) -> Result<String> + 'static) -> Self {
            Self {
                calls: RefCell::new(vec![]),
                released: 0,
                f: Box::new(f),
            }
        }
    }
    impl Transcriber for Fake {
        fn name(&self) -> String {
            "fake".into()
        }
        fn transcribe(&mut self, pcm: &[f32], lang: Option<&str>) -> Result<String> {
            let n = self.calls.borrow().len();
            self.calls.borrow_mut().push((pcm.len(), lang.map(str::to_owned)));
            (self.f)(n, pcm.len())
        }
        fn release(&mut self) {
            self.released += 1;
        }
    }

    /// 40 s Me stream: speech at 1..6 s and 20..25 s; System: speech at 10..14 s.
    fn sources() -> (MemSource, MemSource) {
        let sig = |spans: &[(usize, usize)]| {
            let mut v = vec![0.0005f32; 40 * 16_000];
            for (a, b) in spans {
                for x in &mut v[a * 16_000..b * 16_000] {
                    *x = 0.2;
                }
            }
            MemSource(v)
        };
        (sig(&[(1, 6), (20, 25)]), sig(&[(10, 14)]))
    }

    fn hooks<'a>(
        cancelled: &'a dyn Fn() -> bool,
        paused: &'a dyn Fn() -> bool,
        progress: &'a mut dyn FnMut(u64, u64),
        pause_changed: &'a mut dyn FnMut(bool),
    ) -> Hooks<'a> {
        Hooks {
            cancelled,
            paused,
            progress,
            pause_changed,
            poll: Duration::from_millis(5),
        }
    }

    #[test]
    fn each_stream_is_transcribed_separately_in_time_order_with_the_call_language() {
        let (mut me, mut sys) = sources();
        let mut tr = Fake::new(|n, _| Ok(format!("текст {n}")));
        let seen = RefCell::new(vec![]);
        let (c, p) = (|| false, || false);
        let mut prog = |d: u64, t: u64| seen.borrow_mut().push((d, t));
        let mut pc = |_: bool| {};
        let mut h = hooks(&c, &p, &mut prog, &mut pc);
        let mut srcs: Vec<(Stream, &mut dyn PcmSource)> =
            vec![(Stream::Me, &mut me), (Stream::System, &mut sys)];
        let out = run(&mut srcs, &mut tr, &["ru".to_string()], &mut h).unwrap();
        let Outcome::Done(pieces) = out else {
            panic!("{out:?}")
        };
        assert_eq!(pieces.len(), 3);
        assert_eq!(
            pieces.iter().map(|p| p.stream).collect::<Vec<_>>(),
            [Stream::Me, Stream::System, Stream::Me],
            "chronological across streams"
        );
        assert_eq!(pieces[0].start_ms, 700);
        assert_eq!(pieces[0].text.as_deref(), Some("текст 0"));
        assert!(tr.calls.borrow().iter().all(|(_, l)| l.as_deref() == Some("ru")));
        let seen = seen.borrow();
        assert_eq!(seen.first().unwrap().0, 0);
        let (d, t) = *seen.last().unwrap();
        assert_eq!(d, t, "progress ends at 100 %");
        assert!(seen.windows(2).all(|w| w[0].0 <= w[1].0), "monotonic");
    }

    #[test]
    fn several_languages_let_the_engine_detect() {
        let (mut me, mut sys) = sources();
        let mut tr = Fake::new(|_, _| Ok("ok".into()));
        let (c, p) = (|| false, || false);
        let (mut prog, mut pc) = (|_: u64, _: u64| {}, |_: bool| {});
        let mut h = hooks(&c, &p, &mut prog, &mut pc);
        let mut srcs: Vec<(Stream, &mut dyn PcmSource)> =
            vec![(Stream::Me, &mut me), (Stream::System, &mut sys)];
        run(&mut srcs, &mut tr, &["ru".to_string(), "en".to_string()], &mut h).unwrap();
        assert!(tr.calls.borrow().iter().all(|(_, l)| l.is_none()));
    }

    #[test]
    fn cancel_stops_at_the_next_chunk_and_returns_nothing() {
        let (mut me, mut sys) = sources();
        let cancel = AtomicBool::new(false);
        let mut tr = Fake::new(|_, _| Ok("ok".into()));
        let c = || cancel.load(Ordering::SeqCst);
        let p = || false;
        let mut prog = |d: u64, _t: u64| {
            if d > 0 {
                cancel.store(true, Ordering::SeqCst); // the user presses Cancel after the first chunk
            }
        };
        let mut pc = |_: bool| {};
        let mut h = hooks(&c, &p, &mut prog, &mut pc);
        let mut srcs: Vec<(Stream, &mut dyn PcmSource)> =
            vec![(Stream::Me, &mut me), (Stream::System, &mut sys)];
        let out = run(&mut srcs, &mut tr, &[], &mut h).unwrap();
        assert_eq!(out, Outcome::Cancelled);
        assert_eq!(tr.calls.borrow().len(), 1, "no chunk after the cancel");
    }

    #[test]
    fn a_new_recording_pauses_the_pass_and_frees_the_engine_then_it_resumes() {
        let (mut me, mut sys) = sources();
        let recording = AtomicBool::new(false);
        let polls = AtomicUsize::new(0);
        let mut tr = Fake::new(|_, _| Ok("ok".into()));
        let c = || false;
        // a recording starts after the first chunk and lasts for 6 polls
        let p = || {
            if recording.load(Ordering::SeqCst) {
                if polls.fetch_add(1, Ordering::SeqCst) >= 6 {
                    recording.store(false, Ordering::SeqCst);
                    return false;
                }
                return true;
            }
            false
        };
        let events = RefCell::new(vec![]);
        let mut prog = |d: u64, _t: u64| {
            if d > 0 && polls.load(Ordering::SeqCst) == 0 {
                recording.store(true, Ordering::SeqCst);
            }
        };
        let mut pc = |on: bool| events.borrow_mut().push(on);
        let mut h = hooks(&c, &p, &mut prog, &mut pc);
        let mut srcs: Vec<(Stream, &mut dyn PcmSource)> =
            vec![(Stream::Me, &mut me), (Stream::System, &mut sys)];
        let out = run(&mut srcs, &mut tr, &[], &mut h).unwrap();
        assert!(matches!(out, Outcome::Done(ref v) if v.len() == 3));
        assert_eq!(*events.borrow(), [true, false], "paused once, resumed once");
        assert_eq!(tr.released, 1, "the engine was released while paused");
        assert_eq!(tr.calls.borrow().len(), 3, "nothing skipped or repeated");
    }

    #[test]
    fn a_failing_chunk_keeps_the_live_text_and_a_dead_engine_aborts() {
        let (mut me, mut sys) = sources();
        let mut tr = Fake::new(|n, _| {
            if n == 1 {
                Err(AppError::new(ErrorCode::SttEngine))
            } else {
                Ok("ok".into())
            }
        });
        let (c, p) = (|| false, || false);
        let (mut prog, mut pc) = (|_: u64, _: u64| {}, |_: bool| {});
        let mut h = hooks(&c, &p, &mut prog, &mut pc);
        let mut srcs: Vec<(Stream, &mut dyn PcmSource)> =
            vec![(Stream::Me, &mut me), (Stream::System, &mut sys)];
        let Outcome::Done(v) = run(&mut srcs, &mut tr, &[], &mut h).unwrap() else {
            panic!()
        };
        assert_eq!(
            v.iter().map(|p| p.text.is_some()).collect::<Vec<_>>(),
            [true, false, true]
        );

        // four failures in a row = the engine is gone
        let (mut me, mut sys) = sources();
        let mut dead = Fake::new(|_, _| Err(AppError::new(ErrorCode::SttEngine)));
        let mut many = MemSource(vec![0.3; 16_000 * 4]);
        let _ = (&mut me, &mut sys);
        let mut srcs: Vec<(Stream, &mut dyn PcmSource)> = vec![(Stream::Me, &mut many)];
        // one 4 s chunk only: below the abort threshold, so it is a piece with no text
        let Outcome::Done(v) =
            run(&mut srcs, &mut dead, &[], &mut hooks(&c, &p, &mut prog, &mut pc)).unwrap()
        else {
            panic!()
        };
        assert_eq!(v[0].text, None);
        let mut sig = vec![0.0005f32; 16_000 * 60];
        for k in 0..5 {
            for x in &mut sig[(k * 11 + 1) * 16_000..(k * 11 + 6) * 16_000] {
                *x = 0.3;
            }
        }
        let mut long = MemSource(sig);
        let mut srcs: Vec<(Stream, &mut dyn PcmSource)> = vec![(Stream::Me, &mut long)];
        let e = run(&mut srcs, &mut dead, &[], &mut hooks(&c, &p, &mut prog, &mut pc)).unwrap_err();
        assert_eq!(e.code, ErrorCode::SttEngine);
    }

    /// One-shot HTTP server: returns the request head (lowercased) and replies with `body`.
    fn serve(status: u16, body: &'static str) -> (String, std::thread::JoinHandle<String>) {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/v1/listen", l.local_addr().unwrap());
        let h = std::thread::spawn(move || {
            let (mut s, _) = l.accept().unwrap();
            let mut r = BufReader::new(s.try_clone().unwrap());
            let mut head = String::new();
            let mut len = 0usize;
            loop {
                let mut line = String::new();
                r.read_line(&mut line).unwrap();
                if line == "\r\n" || line.is_empty() {
                    break;
                }
                if let Some(v) = line.to_lowercase().strip_prefix("content-length:") {
                    len = v.trim().parse().unwrap();
                }
                head.push_str(&line.to_lowercase());
            }
            let mut b = vec![0u8; len];
            r.read_exact(&mut b).unwrap();
            assert_eq!(&b[0..4], b"RIFF", "the chunk goes up as a WAV");
            write!(
                s,
                "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
            head
        });
        (url, h)
    }

    #[test]
    fn deepgram_prerecorded_request_and_response() {
        let (url, srv) = serve(
            200,
            r#"{"results":{"channels":[{"alternatives":[{"transcript":"Привет, мир."}]}]}}"#,
        );
        let mut dg = DeepgramBatch::with_endpoint("SECRET-KEY".into(), &["ru".into()], url);
        let text = dg.transcribe(&vec![0.1; 16_000], Some("ru")).unwrap();
        assert_eq!(text, "Привет, мир.");
        let head = srv.join().unwrap();
        assert!(
            head.starts_with("post /v1/listen?model=nova-3&language=ru"),
            "{head}"
        );
        assert!(head.contains("authorization: token secret-key"));
        assert!(head.contains("content-type: audio/wav"));
        assert_eq!(
            deepgram_query(&["ru".into(), "en".into()]),
            "model=nova-3&language=multi&punctuate=true&smart_format=true"
        );
    }

    #[test]
    fn deepgram_rejected_key_is_final_and_the_error_never_contains_the_key() {
        let (url, srv) = serve(401, r#"{"err_code":"INVALID_AUTH"}"#);
        let mut dg = DeepgramBatch::with_endpoint("SECRET-KEY".into(), &[], url);
        let e = dg.transcribe(&vec![0.1; 1600], None).unwrap_err();
        srv.join().unwrap();
        assert_eq!(e.code, ErrorCode::ApiKeyMissing);
        assert!(!format!("{e:?}").contains("SECRET"));
    }
}
