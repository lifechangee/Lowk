//! The cloud embedder and the embedder model download go through the system / manual proxy
//! (local mock HTTP-CONNECT proxy, no real network); loopback is never proxied.
use lowk_index::embedder::models::{hex, DownloadSpec, ModelStore};
use lowk_index::embedder::CloudEmbedder;
use lowk_index::Embedder;
use lowk_platform::proxy::{
    self,
    mock::{Kind, MockProxy},
};
use sha2::{Digest, Sha256};
use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpListener};

/// Answers every request: `/embeddings` with one 4-d vector, anything else with a 4 KiB file.
fn backend() -> SocketAddr {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    std::thread::spawn(move || {
        for c in l.incoming() {
            let Ok(mut s) = c else { continue };
            std::thread::spawn(move || {
                let mut r = BufReader::new(s.try_clone().unwrap());
                let mut first = String::new();
                let _ = r.read_line(&mut first);
                let mut len = 0usize;
                loop {
                    let mut line = String::new();
                    if r.read_line(&mut line).unwrap_or(0) == 0 || line.trim().is_empty() {
                        break;
                    }
                    if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                        len = v.trim().parse().unwrap_or(0);
                    }
                }
                let mut body = vec![0u8; len];
                let _ = std::io::Read::read_exact(&mut r, &mut body);
                let (ct, payload) = if first.contains("/embeddings") {
                    (
                        "application/json",
                        br#"{"data":[{"index":0,"embedding":[0.5,0.5,0.5,0.5]}]}"#.to_vec(),
                    )
                } else {
                    ("application/octet-stream", vec![7u8; 4096])
                };
                let _ = s.write_all(
                    format!("HTTP/1.1 200 OK\r\nContent-Type: {ct}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", payload.len())
                        .as_bytes(),
                );
                let _ = s.write_all(&payload);
            });
        }
    });
    addr
}

#[test]
fn embeddings_and_model_downloads_go_through_the_proxy() {
    let backend = backend();
    let p = MockProxy::start(Kind::HttpConnect, backend);
    assert!(proxy::set_manual(&p.url()));

    let e = CloudEmbedder::new("http://embed.mock.test:9000/api/v1", "sk-test", "m", 4);
    let v = e.embed_docs(&["hello".to_string()]).unwrap();
    assert_eq!(v.len(), 1);
    assert_eq!(p.seen(), vec!["embed.mock.test:9000".to_string()]);

    let body = vec![7u8; 4096];
    let dir = std::env::temp_dir().join(format!("lowk-index-proxy-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let spec = DownloadSpec {
        file: "m.bin".into(),
        url: "http://models.mock.test:8080/m.bin".into(),
        sha256: hex(&Sha256::digest(&body)),
        size_bytes: body.len() as u64,
        license: "MIT".into(),
        license_url: String::new(),
    };
    ModelStore::new(dir.clone())
        .install(
            "e",
            &[spec],
            &std::sync::atomic::AtomicBool::new(false),
            &mut |_, _, _, _| {},
        )
        .unwrap();
    assert_eq!(p.seen().len(), 2);
    assert_eq!(p.seen()[1], "models.mock.test:8080");

    // loopback: direct
    let e = CloudEmbedder::new(&format!("http://{backend}/api/v1"), "sk-test", "m", 4);
    e.embed_docs(&["hello".to_string()]).unwrap();
    assert_eq!(p.seen().len(), 2);
    assert!(proxy::set_manual(""));
    let _ = std::fs::remove_dir_all(&dir);
}
