//! QA control channel (cargo feature `qa`, debug builds only). A localhost-only, token-protected
//! HTTP endpoint that drives the REAL app for `tools/qa` (navigate, run JS in a window and read the
//! result back, overlay mode/opacity/text size, demo session, capture-exclusion lift) on top of the
//! stealth-probe command set. It is never compiled into release builds: `lib.rs` has a
//! `compile_error!` for `qa` without `debug_assertions`, and `tools/qa/check-no-qa-release.sh` runs in CI.
//!
//! Protocol: `POST /cmd` with header `X-Lowk-Qa-Token: <token>` and the command line as the body; the
//! response body is the result text. Port/token: `LOWK_QA_PORT` (default random) and `LOWK_QA_TOKEN`
//! (default random) are written to `$LOWK_DATA_DIR/qa.json`.
//!
//!   nav <hash>                      main window: location.hash = hash (opens the window if needed)
//!   eval <main|overlay|pill> <js>   evaluate an expression (promises awaited); JSON result
//!   win <main|overlay|pill>         JSON: visible, position, size, scale
//!   pid                             the app's process id (for the capture tools)
//!   opacity <0..90> | font <px>     overlay settings (live)
//!   lang <ru|en>                    UI language
//!   mode <passive|interactive|moving>  overlay mode (moving = hold-to-move down)
//!   ... everything in probe.rs (overlay-show, demo-start, exclusion off, overlay-move, ...)
use crate::state::Shared;
use lowk_types::SettingsStore as _;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::{Duration, Instant};
use tauri::Manager;

pub fn spawn(st: Shared) {
    let port: u16 = std::env::var("LOWK_QA_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(0);
    let Ok(listener) = TcpListener::bind(("127.0.0.1", port)) else {
        tracing::error!("qa: cannot bind the control port");
        return;
    };
    let port = listener.local_addr().map(|a| a.port()).unwrap_or(0);
    let token = std::env::var("LOWK_QA_TOKEN").unwrap_or_else(|_| random_token());
    if let Ok(dir) = crate::state::data_dir() {
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(
            dir.join("qa.json"),
            format!(
                "{{\"port\":{port},\"token\":\"{token}\",\"pid\":{}}}",
                std::process::id()
            ),
        );
    }
    tracing::warn!(
        port,
        "QA CONTROL CHANNEL ENABLED (qa feature): never ship this build"
    );
    std::thread::Builder::new()
        .name("lowk-qa".into())
        .spawn(move || {
            for conn in listener.incoming().flatten() {
                let (st, token) = (st.clone(), token.clone());
                // One thread per request: `eval` waits on the webview, other requests must not queue behind it.
                std::thread::spawn(move || handle(conn, &st, &token));
            }
        })
        .expect("spawn qa");
}

fn random_token() -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut out = String::new();
    for _ in 0..2 {
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u128(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos()),
        );
        h.write_u32(std::process::id());
        out.push_str(&format!("{:016x}", h.finish()));
    }
    out
}

fn reply(mut c: TcpStream, status: &str, body: &str) {
    let _ = write!(
        c,
        "HTTP/1.1 {status}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
}

fn handle(mut c: TcpStream, st: &Shared, token: &str) {
    let _ = c.set_read_timeout(Some(Duration::from_secs(5)));
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    let head_end = loop {
        let Ok(n) = c.read(&mut chunk) else { return };
        if n == 0 {
            return;
        }
        buf.extend_from_slice(&chunk[..n]);
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i + 4;
        }
        if buf.len() > 64 * 1024 {
            return reply(c, "413 Payload Too Large", "");
        }
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
    let header = |name: &str| {
        head.lines().find_map(|l| {
            l.split_once(':')
                .filter(|(k, _)| k.eq_ignore_ascii_case(name))
                .map(|(_, v)| v.trim().to_string())
        })
    };
    // Anti DNS-rebinding + auth.
    let host_ok = header("host").is_some_and(|h| h.starts_with("127.0.0.1:") || h.starts_with("localhost:"));
    if !host_ok || header("x-lowk-qa-token").as_deref() != Some(token) {
        return reply(c, "403 Forbidden", "forbidden");
    }
    if !head.starts_with("POST /cmd") {
        return reply(c, "404 Not Found", "POST /cmd");
    }
    let len: usize = header("content-length")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0)
        .min(1 << 20);
    while buf.len() < head_end + len {
        let Ok(n) = c.read(&mut chunk) else { return };
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
    }
    let body = String::from_utf8_lossy(&buf[head_end..]).trim().to_string();
    let out = run(st, &body);
    reply(c, "200 OK", &out);
}

fn run(st: &Shared, line: &str) -> String {
    let (cmd, rest) = line.split_once(' ').unwrap_or((line, ""));
    let rest = rest.trim();
    match cmd {
        "pid" => std::process::id().to_string(),
        "nav" => {
            crate::windows::open_main(&st.app);
            let Some(w) = wait_window(st, "main") else {
                return "no main window".into();
            };
            let js = format!("location.hash = {}", js_str(rest));
            format!("{:?}", w.eval(&js).map_err(|e| e.to_string()))
        }
        "eval" => {
            let (label, js) = rest.split_once(' ').unwrap_or((rest, ""));
            eval(st, label, js)
        }
        "win" => match st.app.get_webview_window(rest) {
            None => "{\"exists\":false}".into(),
            Some(w) => {
                let p = w.outer_position().map(|p| (p.x, p.y)).unwrap_or((0, 0));
                let s = w.outer_size().map(|s| (s.width, s.height)).unwrap_or((0, 0));
                format!(
                    "{{\"exists\":true,\"visible\":{},\"x\":{},\"y\":{},\"w\":{},\"h\":{},\"scale\":{}}}",
                    w.is_visible().unwrap_or(false),
                    p.0,
                    p.1,
                    s.0,
                    s.1,
                    w.scale_factor().unwrap_or(1.0)
                )
            }
        },
        // G6: feed a bind event through the real dispatcher: `bind <action_snake_case> <down|up>`.
        "bind" => {
            let mut it = rest.split_whitespace();
            let (Some(a), Some(p)) = (it.next(), it.next()) else {
                return "usage: bind <action> <down|up>".into();
            };
            let Ok(action) = serde_json::from_str::<lowk_types::ActionId>(&format!("\"{a}\"")) else {
                return format!("unknown action {a}");
            };
            let phase = if p == "down" { lowk_types::Phase::Down } else { lowk_types::Phase::Up };
            crate::actions::dispatch(st, lowk_types::BindEvent { action, phase, t_mono_ns: 0 });
            "ok".into()
        }
        // G6: overlay mode + the native click-through flag.
        "hover-state" => match st.stealth() {
            Ok(s) => format!(
                "{{\"mode\":\"{}\",\"ignores_mouse\":{}}}",
                serde_json::to_string(&s.controller.mode()).unwrap_or_default().trim_matches('"'),
                s.overlay.ignores_mouse().map_or("null".to_string(), |b| b.to_string())
            ),
            Err(e) => e.to_string(),
        },
        // G2: the pill's frame and the remembered positions.
        "pill-state" => match st.stealth() {
            Ok(s) => {
                let f = s.pill.as_ref().map(|p| p.frame());
                format!(
                    "{{\"frame\":{},\"visible\":{},\"positions\":{}}}",
                    serde_json::to_string(&f).unwrap_or_default(),
                    s.pill.as_ref().is_some_and(|p| p.is_visible()),
                    serde_json::to_string(&st.settings.get().overlay.pill_positions).unwrap_or_default()
                )
            }
            Err(e) => e.to_string(),
        },
        // G7: same path as the screenshot bind; replies with `[[w,h,png_bytes],..]` of the queue.
        "screenshot" => match crate::commands::capture::capture_into_queue(st) {
            Ok(_) => format!("{:?}", crate::commands::capture::queue_png_sizes(st)),
            Err(e) => format!("error: {e}"),
        },
        "opacity" => crate::probe::run(
            st,
            &format!("settings-patch {{\"overlay\":{{\"opacity\":{rest}}}}}"),
        ),
        "font" => crate::probe::run(
            st,
            &format!("settings-patch {{\"overlay\":{{\"font_px\":{rest}}},\"ui\":{{\"font_px\":{rest}}}}}"),
        ),
        "lang" => crate::probe::run(
            st,
            &format!("settings-patch {{\"ui\":{{\"language\":\"{rest}\"}}}}"),
        ),
        "mode" if rest == "moving" => match st.stealth() {
            Ok(s) => {
                format!(
                    "{:?}",
                    s.controller
                        .set_mode(lowk_types::OverlayMode::Moving)
                        .map_err(|e| e.to_string())
                )
            }
            Err(e) => e.to_string(),
        },
        _ => crate::probe::run(st, line),
    }
}

fn js_str(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into())
}

fn wait_window(st: &Shared, label: &str) -> Option<tauri::WebviewWindow> {
    let t0 = Instant::now();
    while t0.elapsed() < Duration::from_secs(5) {
        if let Some(w) = st.app.get_webview_window(label) {
            return Some(w);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    None
}

/// Runs `expr` in the window and returns its JSON-serialised value. `Window::eval` has no return
/// channel (and the CSP forbids calling out), so the page parks the result in its own URL fragment
/// with `history.replaceState` (no hashchange, no navigation) and we read it back via `url()`.
fn eval(st: &Shared, label: &str, expr: &str) -> String {
    let Some(w) = st.app.get_webview_window(label) else {
        return format!("{{\"ok\":false,\"v\":\"no window {label}\"}}");
    };
    let id = format!(
        "{:x}",
        Instant::now().elapsed().as_nanos() ^ u128::from(std::process::id()) ^ rand_u64()
    );
    let tag = format!("#__qa={id}:");
    let before = w
        .url()
        .ok()
        .and_then(|u| u.fragment().map(|f| format!("#{f}")))
        .unwrap_or_default();
    let js = format!(
        "(async()=>{{let r;try{{r={{ok:true,v:await ({expr})}}}}catch(e){{r={{ok:false,v:String(e)}}}}\
         history.replaceState(null,'',location.pathname+location.search+{}+encodeURIComponent(JSON.stringify(r===undefined?null:r)))}})()",
        js_str(&tag)
    );
    if let Err(e) = w.eval(&js) {
        return format!("{{\"ok\":false,\"v\":\"{e}\"}}");
    }
    let t0 = Instant::now();
    while t0.elapsed() < Duration::from_secs(8) {
        std::thread::sleep(Duration::from_millis(25));
        let Ok(u) = w.url() else { continue };
        if let Some(f) = u.fragment() {
            if let Some(rest) = f.strip_prefix(&tag[1..]) {
                let decoded = percent_decode(rest);
                let _ = w.eval(format!(
                    "history.replaceState(null,'',location.pathname+location.search+{})",
                    js_str(&before)
                ));
                return decoded;
            }
        }
    }
    "{\"ok\":false,\"v\":\"timeout (window hidden or throttled?)\"}".into()
}

fn rand_u64() -> u128 {
    use std::hash::{BuildHasher, Hasher};
    u128::from(
        std::collections::hash_map::RandomState::new()
            .build_hasher()
            .finish(),
    )
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}
