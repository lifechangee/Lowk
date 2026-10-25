//! WebSocket runtime of the cloud engines (rustls + the pure-Rust graviola crypto provider).
use super::{deepgram_url, descriptor, resolve_key, Parser, Vendor, SONIOX_MODEL, SONIOX_URL};
use crate::diag::StreamStats;
use crate::sidecar::pcm_i16;
use crate::{SttConfig, SttEngine};
use lowk_types::*;
use serde_json::{json, Value};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::atomic::Ordering;
use std::sync::mpsc::{sync_channel, Receiver, RecvTimeoutError, SyncSender};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use tungstenite::client::IntoClientRequest;
use tungstenite::{stream::MaybeTlsStream, Message, WebSocket};

const POLL: Duration = Duration::from_millis(20);
const MAX_RECONNECTS: u32 = 3;
// ---------------------------------------------------------------------------------------
// Engine
// ---------------------------------------------------------------------------------------

enum Out {
    Audio(MonoNs, Vec<f32>),
    Flush,
    Stop,
}

pub struct CloudEngine {
    desc: EngineDescriptor,
    vendor: Vendor,
    stream: Stream,
    stats: Arc<StreamStats>,
    tx: Option<SyncSender<Out>>,
    th: Option<JoinHandle<()>>,
}

type Ws = WebSocket<MaybeTlsStream<TcpStream>>;

fn set_timeout(ws: &mut Ws, d: Option<Duration>) {
    match ws.get_mut() {
        MaybeTlsStream::Plain(s) => {
            let _ = s.set_read_timeout(d);
        }
        MaybeTlsStream::Rustls(s) => {
            let _ = s.get_mut().set_read_timeout(d);
        }
        _ => {}
    }
}

/// rustls with the pure-Rust graviola provider and the bundled Mozilla roots.
pub(crate) fn tls_provider() -> Arc<rustls::crypto::CryptoProvider> {
    Arc::new(rustls_graviola::default_provider())
}

fn tls_config() -> Arc<rustls::ClientConfig> {
    static CFG: std::sync::OnceLock<Arc<rustls::ClientConfig>> = std::sync::OnceLock::new();
    CFG.get_or_init(|| {
        let roots = rustls::RootCertStore {
            roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
        };
        Arc::new(
            rustls::ClientConfig::builder_with_provider(tls_provider())
                .with_safe_default_protocol_versions()
                .expect("tls versions")
                .with_root_certificates(roots)
                .with_no_client_auth(),
        )
    })
    .clone()
}

fn connect(url: &str, auth: Option<(&str, String)>) -> std::result::Result<Ws, String> {
    let mut req = url.into_client_request().map_err(|e| e.to_string())?;
    if let Some((k, v)) = auth {
        req.headers_mut().insert(
            tungstenite::http::HeaderName::from_bytes(k.as_bytes()).map_err(|e| e.to_string())?,
            v.parse().map_err(|_| "bad header".to_string())?,
        );
    }
    let host = req.uri().host().ok_or("no host")?.to_owned();
    let addr = (host.as_str(), 443)
        .to_socket_addrs()
        .map_err(|e| e.to_string())?
        .next()
        .ok_or("dns")?;
    let tcp = TcpStream::connect_timeout(&addr, Duration::from_secs(10)).map_err(|e| e.to_string())?;
    let _ = tcp.set_nodelay(true);
    let _ = tcp.set_read_timeout(Some(Duration::from_secs(10)));
    let (mut ws, _) = tungstenite::client_tls_with_config(
        req,
        tcp,
        None,
        Some(tungstenite::Connector::Rustls(tls_config())),
    )
    .map_err(|e| e.to_string())?;
    set_timeout(&mut ws, Some(POLL));
    Ok(ws)
}

impl CloudEngine {
    pub fn new(vendor: Vendor, stream: Stream) -> Self {
        Self {
            desc: descriptor(vendor),
            vendor,
            stream,
            stats: Arc::new(StreamStats::default()),
            tx: None,
            th: None,
        }
    }
}

/// Soniox start message: the call's languages become `language_hints`.
pub(crate) fn soniox_config(key: &str, langs: &[LangTag]) -> serde_json::Value {
    let hints: Vec<&str> = langs.iter().map(String::as_str).collect();
    json!({
        "api_key": key, "model": SONIOX_MODEL, "audio_format": "pcm_s16le",
        "sample_rate": 16000, "num_channels": 1, "language_hints": hints,
        "enable_language_identification": true, "enable_endpoint_detection": true,
    })
}

struct Conn {
    ws: Ws,
    parser: Parser,
    sent_first: bool,
    last_tx: Instant,
}

fn open(
    vendor: Vendor,
    key: &str,
    langs: &[LangTag],
    stream: Stream,
    utt_base: u64,
) -> std::result::Result<Conn, String> {
    let (mut ws, first) = match vendor {
        Vendor::Soniox => {
            let mut ws = connect(SONIOX_URL, None)?;
            let cfg = soniox_config(key, langs);
            ws.send(Message::text(cfg.to_string()))
                .map_err(|e| e.to_string())?;
            (ws, false)
        }
        Vendor::Deepgram => (
            connect(
                &deepgram_url(langs),
                Some(("Authorization", format!("Token {key}"))),
            )?,
            false,
        ),
    };
    set_timeout(&mut ws, Some(POLL));
    Ok(Conn {
        ws,
        parser: Parser::new(vendor, stream, utt_base),
        sent_first: first,
        last_tx: Instant::now(),
    })
}

fn is_timeout(e: &tungstenite::Error) -> bool {
    matches!(e, tungstenite::Error::Io(io) if matches!(io.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut))
}

/// Reads whatever is available; returns false when the socket is closed.
fn pump(c: &mut Conn, sink: &Arc<dyn SttSink>, stats: &StreamStats, err_msg: &mut Option<String>) -> bool {
    loop {
        match c.ws.read() {
            Ok(Message::Text(t)) => {
                if let Some(v) = serde_json::from_str::<Value>(&t)
                    .ok()
                    .filter(|v| v["error_code"].is_number())
                {
                    *err_msg = Some(format!(
                        "{} {}",
                        v["error_code"],
                        v["error_message"].as_str().unwrap_or("")
                    ));
                    return false;
                }
                for ev in c.parser.on_message(&t, now_mono_ns()) {
                    stats.note_event(&ev);
                    sink.emit(ev);
                }
                if c.parser.finished {
                    return false;
                }
            }
            Ok(Message::Close(_)) => return false,
            Ok(_) => {}
            Err(e) if is_timeout(&e) => return true,
            Err(e) => {
                *err_msg = Some(e.to_string());
                return false;
            }
        }
    }
}

fn run(
    vendor: Vendor,
    stream: Stream,
    langs: Vec<LangTag>,
    key: String,
    sink: Arc<dyn SttSink>,
    rx: Receiver<Out>,
    stats: Arc<StreamStats>,
) {
    let mut reconnects = 0u32;
    let mut utt_base = 0u64;
    'conn: loop {
        let mut conn = match open(vendor, &key, &langs, stream, utt_base) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(vendor = vendor.id(), error = %e.replace(&key, "***"), "cloud stt connect failed");
                reconnects += 1;
                sink.error(SttError {
                    stream,
                    message: format!("{}: connect failed", vendor.id()),
                    recoverable: reconnects <= MAX_RECONNECTS,
                    code: None,
                });
                if reconnects > MAX_RECONNECTS {
                    while let Ok(m) = rx.recv() {
                        if matches!(m, Out::Stop) {
                            return;
                        }
                    }
                    return;
                }
                std::thread::sleep(Duration::from_secs(1));
                while let Ok(m) = rx.try_recv() {
                    if matches!(m, Out::Stop) {
                        return;
                    }
                }
                continue;
            }
        };
        let mut buf = Vec::new();
        let mut err_msg = None;
        loop {
            match rx.recv_timeout(POLL) {
                Ok(Out::Audio(t, pcm)) => {
                    if conn.parser.t0.is_none() {
                        conn.parser.t0 = Some(t);
                    }
                    buf.clear();
                    pcm_i16(&pcm, &mut buf);
                    // coalesce whatever is already queued
                    while let Ok(Out::Audio(_, more)) = rx.try_recv() {
                        pcm_i16(&more, &mut buf);
                    }
                    conn.sent_first = true;
                    if conn.ws.send(Message::binary(buf.clone())).is_err() {
                        break;
                    }
                    conn.last_tx = Instant::now();
                }
                Ok(Out::Flush) => {
                    let m = match vendor {
                        Vendor::Soniox => json!({"type":"finalize"}),
                        Vendor::Deepgram => json!({"type":"Finalize"}),
                    };
                    let _ = conn.ws.send(Message::text(m.to_string()));
                }
                Ok(Out::Stop) | Err(RecvTimeoutError::Disconnected) => {
                    // end of audio, then collect the trailing results
                    let _ = match vendor {
                        Vendor::Soniox => conn.ws.send(Message::text("")),
                        Vendor::Deepgram => conn
                            .ws
                            .send(Message::text(json!({"type":"CloseStream"}).to_string())),
                    };
                    let t0 = Instant::now();
                    while t0.elapsed() < Duration::from_secs(6)
                        && pump(&mut conn, &sink, &stats, &mut err_msg)
                    {}
                    for ev in conn.parser.flush(now_mono_ns()) {
                        stats.note_event(&ev);
                        sink.emit(ev);
                    }
                    let _ = conn.ws.close(None);
                    return;
                }
                Err(RecvTimeoutError::Timeout) => {
                    if vendor == Vendor::Deepgram && conn.last_tx.elapsed() > Duration::from_secs(5) {
                        let _ = conn
                            .ws
                            .send(Message::text(json!({"type":"KeepAlive"}).to_string()));
                        conn.last_tx = Instant::now();
                    }
                }
            }
            if !pump(&mut conn, &sink, &stats, &mut err_msg) {
                break;
            }
        }
        // connection lost (or vendor error)
        utt_base += 1_000_000;
        for ev in conn.parser.flush(now_mono_ns()) {
            sink.emit(ev);
        }
        reconnects += 1;
        stats.restarts.store(reconnects, Ordering::Relaxed);
        // 4xx from the vendor (bad key, no balance, bad request) will not fix itself
        let fatal = err_msg
            .as_deref()
            .is_some_and(|m| m.starts_with("40") || m.contains("HTTP error: 40"));
        tracing::warn!(vendor = vendor.id(), reason = %err_msg.clone().unwrap_or_default().replace(&key, "***"), "cloud stt connection ended");
        sink.error(SttError {
            stream,
            message: format!(
                "{}: {}",
                vendor.id(),
                err_msg.unwrap_or_else(|| "connection closed".into())
            ),
            recoverable: !fatal && reconnects <= MAX_RECONNECTS,
            code: None,
        });
        if fatal || reconnects > MAX_RECONNECTS {
            while let Ok(m) = rx.recv() {
                if matches!(m, Out::Stop) {
                    break;
                }
            }
            return;
        }
        std::thread::sleep(Duration::from_millis(500));
        while let Ok(m) = rx.try_recv() {
            if matches!(m, Out::Stop) {
                return;
            }
        }
        continue 'conn;
    }
}

impl SttEngine for CloudEngine {
    fn descriptor(&self) -> &EngineDescriptor {
        &self.desc
    }

    fn start(&mut self, cfg: &SttConfig, sink: Arc<dyn SttSink>) -> Result<()> {
        let key = resolve_key(self.vendor)?;
        let (tx, rx) = sync_channel(1000);
        self.tx = Some(tx);
        let (v, s, langs, st) = (self.vendor, self.stream, cfg.langs.clone(), self.stats.clone());
        self.th = Some(
            std::thread::Builder::new()
                .name(format!("lowk-stt-{}-{s:?}", v.id()))
                .spawn(move || run(v, s, langs, key, sink, rx, st))
                .map_err(|e| AppError::new(ErrorCode::SttEngine).with_detail(e.to_string()))?,
        );
        Ok(())
    }

    fn push(&mut self, f: &Frame16k) -> Result<()> {
        let Some(tx) = &self.tx else { return Ok(()) };
        self.stats
            .note_pushed(f.t_mono_ns + (f.pcm.len() as u64) * 1_000_000_000 / 16_000);
        tx.try_send(Out::Audio(f.t_mono_ns, f.pcm.clone())).map_err(|_| {
            self.stats.dropped_frames.fetch_add(1, Ordering::Relaxed);
            AppError::new(ErrorCode::SttBacklog)
        })
    }

    fn flush(&mut self) -> Result<()> {
        if let Some(tx) = &self.tx {
            let _ = tx.send(Out::Flush);
        }
        Ok(())
    }

    fn stop(&mut self) -> Result<()> {
        if let Some(tx) = self.tx.take() {
            let _ = tx.send(Out::Stop);
        }
        if let Some(h) = self.th.take() {
            let _ = h.join();
        }
        Ok(())
    }

    fn backlog_ms(&self) -> u32 {
        self.stats.backlog_ms()
    }

    fn stats(&self) -> Option<StreamDiag> {
        Some(self.stats.diag(self.stream))
    }
}

impl Drop for CloudEngine {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

#[cfg(test)]
mod lang_tests {
    use super::*;

    #[test]
    fn soniox_gets_the_call_languages_as_hints() {
        let l = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert_eq!(
            soniox_config("k", &l(&["ru", "en"]))["language_hints"],
            json!(["ru", "en"])
        );
        assert_eq!(soniox_config("k", &l(&["pl"]))["language_hints"], json!(["pl"]));
    }

    #[test]
    fn deepgram_single_language_else_multi() {
        assert!(deepgram_url(&["pl".into()]).contains("language=pl"));
        assert!(deepgram_url(&["en".into()]).contains("language=en"));
        assert!(deepgram_url(&["ru".into(), "en".into()]).contains("language=multi"));
    }
}
