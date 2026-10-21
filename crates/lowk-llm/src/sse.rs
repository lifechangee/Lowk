//! Tolerant Server-Sent Events parser: any chunking of the byte stream, `\n` / `\r\n` / `\r`
//! line ends, comments (`: keep-alive`), multi-line `data:`, a leading BOM, invalid UTF-8
//! (replaced, never a panic).

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SseEvent {
    pub event: Option<String>,
    pub data: String,
}

#[derive(Debug, Default)]
pub struct SseParser {
    buf: Vec<u8>,
    data: String,
    has_data: bool,
    event: Option<String>,
    started: bool,
}

impl SseParser {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feeds bytes, returns the events completed by them.
    pub fn push(&mut self, bytes: &[u8]) -> Vec<SseEvent> {
        self.buf.extend_from_slice(bytes);
        if !self.started && self.buf.len() >= 3 {
            self.started = true;
            if self.buf.starts_with(&[0xEF, 0xBB, 0xBF]) {
                self.buf.drain(..3);
            }
        }
        let mut out = Vec::new();
        let mut start = 0;
        let mut i = 0;
        while i < self.buf.len() {
            let b = self.buf[i];
            if b == b'\n' || b == b'\r' {
                if b == b'\r' && i + 1 == self.buf.len() {
                    break; // maybe "\r\n" split across chunks: wait for the next byte
                }
                let line = String::from_utf8_lossy(&self.buf[start..i]).into_owned();
                i += if b == b'\r' && self.buf.get(i + 1) == Some(&b'\n') {
                    2
                } else {
                    1
                };
                start = i;
                if let Some(ev) = self.line(&line) {
                    out.push(ev);
                }
            } else {
                i += 1;
            }
        }
        self.buf.drain(..start);
        out
    }

    /// End of stream: flushes a trailing line and a pending event without its blank line.
    pub fn finish(&mut self) -> Vec<SseEvent> {
        let mut out = Vec::new();
        if !self.buf.is_empty() {
            let rest = std::mem::take(&mut self.buf);
            let line = String::from_utf8_lossy(&rest).trim_end_matches('\r').to_string();
            if let Some(ev) = self.line(&line) {
                out.push(ev);
            }
        }
        if let Some(ev) = self.dispatch() {
            out.push(ev);
        }
        out
    }

    fn line(&mut self, line: &str) -> Option<SseEvent> {
        if line.is_empty() {
            return self.dispatch();
        }
        if line.starts_with(':') {
            return None;
        }
        let (field, value) = match line.find(':') {
            Some(p) => {
                let v = &line[p + 1..];
                (&line[..p], v.strip_prefix(' ').unwrap_or(v))
            }
            None => (line, ""),
        };
        match field {
            "data" => {
                if self.has_data {
                    self.data.push('\n');
                }
                self.data.push_str(value);
                self.has_data = true;
            }
            "event" => self.event = Some(value.to_string()),
            _ => {} // id, retry and unknown fields are irrelevant here
        }
        None
    }

    fn dispatch(&mut self) -> Option<SseEvent> {
        let event = self.event.take();
        if !self.has_data {
            return None;
        }
        self.has_data = false;
        Some(SseEvent {
            event,
            data: std::mem::take(&mut self.data),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all(chunks: &[&[u8]]) -> Vec<SseEvent> {
        let mut p = SseParser::new();
        let mut v = Vec::new();
        for c in chunks {
            v.extend(p.push(c));
        }
        v.extend(p.finish());
        v
    }

    #[test]
    fn basic_and_crlf() {
        let e = all(&[b"data: a\n\ndata: b\r\n\r\n"]);
        assert_eq!(e.iter().map(|x| x.data.as_str()).collect::<Vec<_>>(), ["a", "b"]);
    }

    #[test]
    fn comments_multiline_and_event_name() {
        let e = all(&[b": OPENROUTER PROCESSING\n\nevent: x\ndata: 1\ndata: 2\n\n"]);
        assert_eq!(e.len(), 1);
        assert_eq!(e[0].data, "1\n2");
        assert_eq!(e[0].event.as_deref(), Some("x"));
    }

    #[test]
    fn split_everywhere_and_cr_only() {
        let raw = "data: привет\r\rdata: x\r\n\r\n".as_bytes();
        for n in 1..raw.len() {
            let (a, b) = raw.split_at(n);
            let e = all(&[a, b]);
            assert_eq!(e.len(), 2, "split at {n}");
            assert_eq!(e[0].data, "привет");
        }
        let chunks: Vec<&[u8]> = raw.chunks(1).collect();
        assert_eq!(all(&chunks).len(), 2);
    }

    #[test]
    fn bom_garbage_and_missing_blank_line() {
        let e = all(&[b"\xEF\xBB\xBFdata: a\n\n\xff\xfe\ndata: tail"]);
        assert_eq!(e.len(), 2);
        assert_eq!(e[1].data, "tail");
    }

    #[test]
    fn random_bytes_never_panic() {
        let mut x: u64 = 0x9E37_79B9_7F4A_7C15;
        for _ in 0..200 {
            let mut buf = Vec::new();
            for _ in 0..300 {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                let alphabet = b"data: \r\n:event\xff{}[]\"x";
                buf.push(alphabet[(x % alphabet.len() as u64) as usize]);
            }
            let mut p = SseParser::new();
            let mut k = 0;
            while k < buf.len() {
                let n = 1 + (x as usize + k) % 17;
                let end = (k + n).min(buf.len());
                p.push(&buf[k..end]);
                k = end;
            }
            p.finish();
        }
    }
}
