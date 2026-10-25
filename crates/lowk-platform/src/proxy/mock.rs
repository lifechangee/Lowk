//! Local mock HTTP-CONNECT / SOCKS5 proxies for tests (no real network). Whatever target the client
//! asks for, the tunnel goes to the fixed `backend` address, and the requested `host:port` is recorded
//! so a test can assert that traffic really went through the proxy.
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    HttpConnect,
    Socks5,
}

pub struct MockProxy {
    pub addr: SocketAddr,
    kind: Kind,
    seen: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
    th: Option<JoinHandle<()>>,
}

impl MockProxy {
    pub fn start(kind: Kind, backend: SocketAddr) -> Self {
        let l = TcpListener::bind("127.0.0.1:0").expect("bind mock proxy");
        let addr = l.local_addr().unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let (s2, st2) = (seen.clone(), stop.clone());
        let th = std::thread::spawn(move || {
            for c in l.incoming() {
                if st2.load(Ordering::SeqCst) {
                    break;
                }
                let Ok(c) = c else { continue };
                let s3 = s2.clone();
                std::thread::spawn(move || {
                    let _ = serve(kind, c, backend, &s3);
                });
            }
        });
        Self {
            addr,
            kind,
            seen,
            stop,
            th: Some(th),
        }
    }

    /// `http://127.0.0.1:port` or `socks5://127.0.0.1:port`.
    pub fn url(&self) -> String {
        match self.kind {
            Kind::HttpConnect => format!("http://{}", self.addr),
            Kind::Socks5 => format!("socks5://{}", self.addr),
        }
    }

    /// Targets (`host:port`) the clients asked this proxy to reach.
    pub fn seen(&self) -> Vec<String> {
        self.seen.lock().unwrap().clone()
    }
}

impl Drop for MockProxy {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(self.addr);
        if let Some(t) = self.th.take() {
            let _ = t.join();
        }
    }
}

fn rd(s: &mut TcpStream, n: usize) -> std::io::Result<Vec<u8>> {
    let mut v = vec![0; n];
    s.read_exact(&mut v)?;
    Ok(v)
}

fn serve(
    kind: Kind,
    mut c: TcpStream,
    backend: SocketAddr,
    seen: &Mutex<Vec<String>>,
) -> std::io::Result<()> {
    let target = match kind {
        Kind::HttpConnect => {
            let mut head = Vec::new();
            let mut b = [0u8; 1];
            while !head.ends_with(b"\r\n\r\n") {
                if c.read(&mut b)? == 0 {
                    return Ok(());
                }
                head.push(b[0]);
            }
            let line = String::from_utf8_lossy(&head)
                .lines()
                .next()
                .unwrap_or("")
                .to_string();
            let mut it = line.split_whitespace();
            if it.next() != Some("CONNECT") {
                c.write_all(b"HTTP/1.1 405 Method Not Allowed\r\n\r\n")?;
                return Ok(());
            }
            let t = it.next().unwrap_or("").to_string();
            c.write_all(b"HTTP/1.1 200 Connection established\r\n\r\n")?;
            t
        }
        Kind::Socks5 => {
            let h = rd(&mut c, 2)?;
            rd(&mut c, h[1] as usize)?;
            c.write_all(&[5, 0])?;
            let r = rd(&mut c, 4)?;
            let host = match r[3] {
                1 => rd(&mut c, 4)?
                    .iter()
                    .map(|b| b.to_string())
                    .collect::<Vec<_>>()
                    .join("."),
                4 => "ipv6".into(),
                _ => {
                    let n = rd(&mut c, 1)?[0] as usize;
                    String::from_utf8_lossy(&rd(&mut c, n)?).into_owned()
                }
            };
            let p = rd(&mut c, 2)?;
            c.write_all(&[5, 0, 0, 1, 0, 0, 0, 0, 0, 0])?;
            format!("{host}:{}", u16::from_be_bytes([p[0], p[1]]))
        }
    };
    seen.lock().unwrap().push(target);
    let up = TcpStream::connect(backend)?;
    let (mut c2, mut up2) = (c.try_clone()?, up.try_clone()?);
    let t = std::thread::spawn(move || {
        let _ = std::io::copy(&mut c2, &mut up2);
        let _ = up2.shutdown(std::net::Shutdown::Write);
    });
    let (mut up, mut c) = (up, c);
    let _ = std::io::copy(&mut up, &mut c);
    let _ = c.shutdown(std::net::Shutdown::Write);
    let _ = t.join();
    Ok(())
}
