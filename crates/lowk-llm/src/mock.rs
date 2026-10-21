//! Offline OpenAI-compatible test server (std only). Used by this crate's tests and, through
//! the `lowk-llm-stub` binary and a base-URL override, by `just qa-bundle`. Never part of a
//! user-facing code path: nothing in the app constructs it.
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Clone, Debug, Default)]
pub struct RecordedRequest {
    pub method: String,
    pub path: String,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

impl RecordedRequest {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
    pub fn json(&self) -> serde_json::Value {
        serde_json::from_str(&self.body).unwrap_or(serde_json::Value::Null)
    }
}

#[derive(Clone, Debug)]
pub enum Reply {
    /// `text/event-stream`; each piece is written (and flushed) separately, so pieces may cut
    /// SSE events anywhere. After the last piece: close, or hang (never finishes).
    Sse {
        pieces: Vec<String>,
        piece_delay: Duration,
        hang_after: bool,
    },
    /// A plain response with a body.
    Json {
        status: u16,
        body: String,
        headers: Vec<(String, String)>,
    },
    /// Accept the request and never answer.
    Hang,
}

impl Reply {
    pub fn json(status: u16, body: impl Into<String>) -> Self {
        Reply::Json {
            status,
            body: body.into(),
            headers: vec![],
        }
    }

    /// A normal streamed answer: one SSE event per text piece, a finish chunk with usage, `[DONE]`.
    pub fn answer(pieces: &[&str]) -> Self {
        Reply::Sse {
            pieces: sse_answer(pieces),
            piece_delay: Duration::ZERO,
            hang_after: false,
        }
    }

    /// Streams the pieces and then stalls forever (idle-timeout and cancellation tests).
    pub fn stall_after(pieces: &[&str]) -> Self {
        let mut p = vec![": OPENROUTER PROCESSING\n\n".to_string()];
        p.extend(pieces.iter().map(|t| content_event(t)));
        Reply::Sse {
            pieces: p,
            piece_delay: Duration::ZERO,
            hang_after: true,
        }
    }
}

pub fn content_event(text: &str) -> String {
    format!(
        "data: {}\n\n",
        serde_json::json!({"id":"gen-1","model":"mock-model","choices":[{"index":0,"delta":{"content":text}}]})
    )
}

pub fn sse_answer(pieces: &[&str]) -> Vec<String> {
    let mut v = vec![": OPENROUTER PROCESSING\n\n".to_string()];
    for p in pieces {
        v.push(content_event(p));
    }
    v.push(format!(
        "data: {}\n\n",
        serde_json::json!({"id":"gen-1","model":"mock-model","choices":[{"index":0,"delta":{},"finish_reason":"stop"}],
            "usage":{"prompt_tokens":120,"completion_tokens":30,"prompt_tokens_details":{"cached_tokens":64}}})
    ));
    v.push("data: [DONE]\n\n".into());
    v
}

type Responder = dyn Fn(&RecordedRequest, usize) -> Reply + Send + Sync;

pub struct MockServer {
    addr: std::net::SocketAddr,
    requests: Arc<Mutex<Vec<RecordedRequest>>>,
    stop: Arc<AtomicBool>,
}

impl MockServer {
    /// `responder(request, index of this request)`.
    pub fn start(responder: impl Fn(&RecordedRequest, usize) -> Reply + Send + Sync + 'static) -> Self {
        Self::start_on(0, responder)
    }

    pub fn start_on(
        port: u16,
        responder: impl Fn(&RecordedRequest, usize) -> Reply + Send + Sync + 'static,
    ) -> Self {
        let listener = TcpListener::bind(("127.0.0.1", port)).expect("bind mock server");
        let addr = listener.local_addr().expect("addr");
        let requests = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let responder: Arc<Responder> = Arc::new(responder);
        let counter = Arc::new(AtomicUsize::new(0));
        {
            let (requests, stop) = (requests.clone(), stop.clone());
            std::thread::spawn(move || {
                for conn in listener.incoming() {
                    if stop.load(Ordering::SeqCst) {
                        break;
                    }
                    let Ok(conn) = conn else { continue };
                    let (requests, stop, responder, counter) =
                        (requests.clone(), stop.clone(), responder.clone(), counter.clone());
                    std::thread::spawn(move || {
                        let _ = serve(conn, &requests, &stop, &*responder, &counter);
                    });
                }
            });
        }
        Self { addr, requests, stop }
    }

    /// The nth request gets the nth reply; the last reply repeats.
    pub fn scripted(replies: Vec<Reply>) -> Self {
        assert!(!replies.is_empty());
        Self::start(move |_, i| replies[i.min(replies.len() - 1)].clone())
    }

    /// `http://127.0.0.1:PORT/api/v1`
    pub fn base_url(&self) -> String {
        format!("http://{}/api/v1", self.addr)
    }

    pub fn port(&self) -> u16 {
        self.addr.port()
    }

    pub fn requests(&self) -> Vec<RecordedRequest> {
        self.requests.lock().map(|g| g.clone()).unwrap_or_default()
    }

    /// Requests to the chat endpoint only (probe GETs excluded).
    pub fn chat_requests(&self) -> Vec<RecordedRequest> {
        self.requests()
            .into_iter()
            .filter(|r| r.path.ends_with("/chat/completions"))
            .collect()
    }
}

impl Drop for MockServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(self.addr); // unblock accept
    }
}

fn read_request(conn: &mut TcpStream) -> std::io::Result<RecordedRequest> {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 4096];
    let head_end = loop {
        if let Some(p) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break p;
        }
        let n = conn.read(&mut tmp)?;
        if n == 0 {
            return Err(std::io::ErrorKind::UnexpectedEof.into());
        }
        buf.extend_from_slice(&tmp[..n]);
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).into_owned();
    let mut lines = head.lines();
    let first = lines.next().unwrap_or_default();
    let mut it = first.split_whitespace();
    let method = it.next().unwrap_or_default().to_string();
    let path = it.next().unwrap_or_default().to_string();
    let headers: Vec<(String, String)> = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
        .collect();
    let len: usize = headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, v)| v.parse().ok())
        .unwrap_or(0);
    let mut body = buf[head_end + 4..].to_vec();
    while body.len() < len {
        let n = conn.read(&mut tmp)?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&tmp[..n]);
    }
    Ok(RecordedRequest {
        method,
        path,
        headers,
        body: String::from_utf8_lossy(&body).into_owned(),
    })
}

fn serve(
    mut conn: TcpStream,
    requests: &Mutex<Vec<RecordedRequest>>,
    stop: &AtomicBool,
    responder: &Responder,
    counter: &AtomicUsize,
) -> std::io::Result<()> {
    let req = read_request(&mut conn)?;
    let idx = counter.fetch_add(1, Ordering::SeqCst);
    if let Ok(mut g) = requests.lock() {
        g.push(req.clone());
    }
    match responder(&req, idx) {
        Reply::Json {
            status,
            body,
            headers,
        } => {
            let mut head = format!(
                "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n",
                body.len()
            );
            for (k, v) in headers {
                head.push_str(&format!("{k}: {v}\r\n"));
            }
            head.push_str("\r\n");
            conn.write_all(head.as_bytes())?;
            conn.write_all(body.as_bytes())?;
            conn.flush()
        }
        Reply::Sse {
            pieces,
            piece_delay,
            hang_after,
        } => {
            conn.write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-cache\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n",
            )?;
            for p in pieces {
                if !piece_delay.is_zero() {
                    std::thread::sleep(piece_delay);
                }
                conn.write_all(format!("{:x}\r\n", p.len()).as_bytes())?;
                conn.write_all(p.as_bytes())?;
                conn.write_all(b"\r\n")?;
                conn.flush()?;
            }
            if hang_after {
                while !stop.load(Ordering::SeqCst) {
                    std::thread::sleep(Duration::from_millis(50));
                }
                return Ok(());
            }
            conn.write_all(b"0\r\n\r\n")?;
            conn.flush()
        }
        Reply::Hang => {
            while !stop.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(50));
            }
            Ok(())
        }
    }
}
