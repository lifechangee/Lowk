//! TCP dialer that honours the proxy configuration: direct, HTTP `CONNECT` tunnel or SOCKS5.
//! Used for WebSockets (tungstenite wants a connected stream, TLS is layered on top by the caller).
use super::{ProxyUrl, Scheme};
use std::io::{self, Read, Write};
use std::net::{IpAddr, TcpStream, ToSocketAddrs};
use std::time::Duration;

fn err(msg: impl Into<String>) -> io::Error {
    io::Error::other(msg.into())
}

fn direct(host: &str, port: u16, timeout: Duration) -> io::Result<TcpStream> {
    let mut last = err("dns: no address");
    for a in (host, port).to_socket_addrs()? {
        match TcpStream::connect_timeout(&a, timeout) {
            Ok(s) => return Ok(s),
            Err(e) => last = e,
        }
    }
    Err(last)
}

/// Connects to `host:port` (a TLS target) honouring the resolved proxy for it.
pub fn connect(host: &str, port: u16, timeout: Duration) -> io::Result<TcpStream> {
    let proxy = super::current().and_then(|c| c.for_target(host, true));
    match proxy {
        Some(p) => connect_via(&p, host, port, timeout),
        None => direct(host, port, timeout),
    }
}

/// Connects to `host:port` through `proxy`. The socket has read/write timeouts only during the
/// handshake; they are cleared before returning.
pub fn connect_via(proxy: &ProxyUrl, host: &str, port: u16, timeout: Duration) -> io::Result<TcpStream> {
    let mut s = direct(&proxy.host, proxy.port, timeout)
        .map_err(|e| err(format!("proxy {}: {e}", proxy.display())))?;
    s.set_read_timeout(Some(timeout))?;
    s.set_write_timeout(Some(timeout))?;
    let _ = s.set_nodelay(true);
    match proxy.scheme {
        Scheme::Http | Scheme::Https => http_connect(&mut s, proxy, host, port)?,
        Scheme::Socks5 => socks5(&mut s, proxy, host, port)?,
    }
    s.set_read_timeout(None)?;
    s.set_write_timeout(None)?;
    Ok(s)
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn base64(b: &[u8]) -> String {
    let mut o = String::new();
    for c in b.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        o.push(B64[(n >> 18) as usize & 63] as char);
        o.push(B64[(n >> 12) as usize & 63] as char);
        o.push(if c.len() > 1 {
            B64[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        o.push(if c.len() > 2 {
            B64[n as usize & 63] as char
        } else {
            '='
        });
    }
    o
}

fn http_connect(s: &mut TcpStream, p: &ProxyUrl, host: &str, port: u16) -> io::Result<()> {
    let hp = if host.contains(':') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    };
    let mut req = format!("CONNECT {hp} HTTP/1.1\r\nHost: {hp}\r\nProxy-Connection: keep-alive\r\n");
    if let Some(u) = &p.user {
        let cred = format!("{u}:{}", p.pass.as_deref().unwrap_or(""));
        req.push_str(&format!(
            "Proxy-Authorization: Basic {}\r\n",
            base64(cred.as_bytes())
        ));
    }
    req.push_str("\r\n");
    s.write_all(req.as_bytes())?;
    // Read byte by byte up to the blank line: nothing of the tunnelled stream may be consumed.
    let mut head = Vec::new();
    let mut b = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        if head.len() > 16 * 1024 {
            return Err(err("proxy CONNECT: response head too large"));
        }
        if s.read(&mut b)? == 0 {
            return Err(err("proxy CONNECT: connection closed"));
        }
        head.push(b[0]);
    }
    let line = String::from_utf8_lossy(&head);
    let line = line.lines().next().unwrap_or("");
    let code = line.split_whitespace().nth(1).unwrap_or("");
    if code.starts_with('2') {
        Ok(())
    } else {
        Err(err(format!("proxy CONNECT refused: {line}")))
    }
}

fn read_exact(s: &mut TcpStream, n: usize) -> io::Result<Vec<u8>> {
    let mut v = vec![0u8; n];
    s.read_exact(&mut v)?;
    Ok(v)
}

fn socks5(s: &mut TcpStream, p: &ProxyUrl, host: &str, port: u16) -> io::Result<()> {
    let auth = p.user.is_some();
    s.write_all(if auth { &[5, 2, 0, 2][..] } else { &[5, 1, 0][..] })?;
    let r = read_exact(s, 2)?;
    if r[0] != 5 {
        return Err(err("SOCKS5: bad greeting reply"));
    }
    match r[1] {
        0 => {}
        2 if auth => {
            let (u, pw) = (p.user.as_deref().unwrap_or(""), p.pass.as_deref().unwrap_or(""));
            if u.len() > 255 || pw.len() > 255 {
                return Err(err("SOCKS5: credentials too long"));
            }
            let mut m = vec![1, u.len() as u8];
            m.extend_from_slice(u.as_bytes());
            m.push(pw.len() as u8);
            m.extend_from_slice(pw.as_bytes());
            s.write_all(&m)?;
            if read_exact(s, 2)?[1] != 0 {
                return Err(err("SOCKS5: proxy login rejected"));
            }
        }
        0xff => return Err(err("SOCKS5: no acceptable authentication method")),
        m => return Err(err(format!("SOCKS5: unsupported method {m}"))),
    }
    let mut m = vec![5, 1, 0];
    match host.parse::<IpAddr>() {
        Ok(IpAddr::V4(a)) => {
            m.push(1);
            m.extend_from_slice(&a.octets());
        }
        Ok(IpAddr::V6(a)) => {
            m.push(4);
            m.extend_from_slice(&a.octets());
        }
        Err(_) => {
            if host.len() > 255 {
                return Err(err("SOCKS5: host name too long"));
            }
            m.push(3);
            m.push(host.len() as u8);
            m.extend_from_slice(host.as_bytes());
        }
    }
    m.extend_from_slice(&port.to_be_bytes());
    s.write_all(&m)?;
    let h = read_exact(s, 4)?;
    if h[0] != 5 {
        return Err(err("SOCKS5: bad reply"));
    }
    if h[1] != 0 {
        let why = match h[1] {
            2 => "not allowed by ruleset",
            3 => "network unreachable",
            4 => "host unreachable",
            5 => "connection refused",
            6 => "TTL expired",
            _ => "request failed",
        };
        return Err(err(format!("SOCKS5 connect refused: {why}")));
    }
    let skip = match h[3] {
        1 => 4,
        4 => 16,
        3 => read_exact(s, 1)?[0] as usize,
        _ => return Err(err("SOCKS5: bad address type")),
    };
    read_exact(s, skip + 2)?;
    Ok(())
}
