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
                return "usage: bind <action> <down|up> [template-id]".into();
            };
            let Ok(action) = serde_json::from_str::<lowk_types::ActionId>(&format!("\"{a}\"")) else {
                return format!("unknown action {a}");
            };
            let phase = if p == "down" {
                lowk_types::Phase::Down
            } else {
                lowk_types::Phase::Up
            };
            crate::actions::dispatch(
                st,
                lowk_types::BindEvent {
                    action,
                    phase,
                    t_mono_ns: 0,
                    template: it.next().map(str::to_owned),
                },
            );
            "ok".into()
        }
        // G6: overlay mode + the native click-through flag.
        "hover-state" => match st.stealth() {
            Ok(s) => format!(
                "{{\"mode\":\"{}\",\"ignores_mouse\":{}}}",
                serde_json::to_string(&s.controller.mode())
                    .unwrap_or_default()
                    .trim_matches('"'),
                s.overlay
                    .ignores_mouse()
                    .map_or("null".to_string(), |b| b.to_string())
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
            Ok(_) => {
                serde_json::to_string(&crate::commands::capture::queue_png_sizes(st)).unwrap_or_default()
            }
            Err(e) => format!("error: {e}"),
        },
        "queue-png" => {
            crate::commands::capture::queue_png_b64(st, rest.parse().unwrap_or(0)).unwrap_or_default()
        }
        "queue-clear" => {
            crate::commands::capture::clear_queue(st);
            "ok".into()
        }
        // O6: replay utterances `<from> <to>` (one per ~4 s of a call) through the real event bus: partials, final,
        // every 7th long, every 5th the user's own («Я»); ids start over at 80 (engine restart).
        "stt-replay" => {
            let mut it = rest.split_whitespace().filter_map(|x| x.parse::<u32>().ok());
            let (from, to) = (it.next().unwrap_or(0), it.next().unwrap_or(0));
            let mut last = String::new();
            for i in from..to {
                let utt = u64::from(if i < 80 { i + 1 } else { i - 79 });
                let stream = if i % 5 == 4 {
                    lowk_types::Stream::Me
                } else {
                    lowk_types::Stream::System
                };
                let n = if i % 7 == 3 { 40 } else { 8 + i % 6 };
                let words = [
                    "мы",
                    "обсуждаем",
                    "квартальный",
                    "отчёт",
                    "и",
                    "план",
                    "на",
                    "месяц",
                    "по",
                    "бюджету",
                ];
                let text: String = std::iter::once(format!("Реплика {}", i + 1))
                    .chain((0..n).map(|k| words[((i + k) % 10) as usize].to_string()))
                    .collect::<Vec<_>>()
                    .join(" ");
                let chars: Vec<char> = text.chars().collect();
                for (rev, cut) in [
                    (0u32, chars.len() / 3),
                    (0, chars.len() * 2 / 3),
                    (1, chars.len()),
                ] {
                    st.emit(lowk_types::AppEvent::Stt(lowk_types::SttEvent {
                        stream,
                        utt,
                        rev,
                        kind: if cut == chars.len() {
                            lowk_types::SttKind::Final
                        } else {
                            lowk_types::SttKind::Partial
                        },
                        text: chars[..cut].iter().collect(),
                        lang: None,
                        t_start_ns: 0,
                        t_audio_end_ns: 0,
                        t_emit_ns: 0,
                    }));
                    std::thread::sleep(Duration::from_millis(8));
                }
                last = text;
            }
            last
        }
        // O7: stream a long answer (`<lines>` numbered paragraphs) as an LLM request would.
        "llm-replay" => {
            let n: u32 = rest.parse().unwrap_or(30);
            let req = lowk_types::Ulid::new();
            st.emit(lowk_types::AppEvent::Llm(lowk_types::LlmEvent::Start {
                req_id: req,
                qa_id: lowk_types::Ulid::new(),
            }));
            for i in 0..n {
                st.emit(lowk_types::AppEvent::Llm(lowk_types::LlmEvent::Delta {
                    req_id: req,
                    text: format!("{}. **Пункт {}**: довольно длинное объяснение, которое занимает несколько строк в окне ответа.\n", i + 1, i + 1),
                }));
                std::thread::sleep(Duration::from_millis(15));
            }
            st.emit(lowk_types::AppEvent::Llm(lowk_types::LlmEvent::Done {
                req_id: req,
                truncated: false,
                usage: None,
            }));
            "ok".into()
        }
        // Adaptive-tone e2e: `region-png X Y W H SCALE /path.png` writes the screen region as the person sees it
        // (exclusion must have been lifted with `exclusion off`). Done in-process so no helper needs its own TCC grant.
        "region-png" => {
            let v: Vec<&str> = rest.split_whitespace().collect();
            let n = |i: usize| v.get(i).and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.0);
            let Some(path) = v.get(5) else {
                return "usage: region-png X Y W H SCALE PATH".into();
            };
            let r = lowk_types::Rect {
                x: n(0),
                y: n(1),
                w: n(2),
                h: n(3),
            };
            #[cfg(target_os = "macos")]
            {
                match lowk_capture::grab_scaled(r, n(4).max(1.0))
                    .and_then(|(px, w, h)| lowk_capture::encode_png(&px, w, h))
                {
                    Ok(png) => {
                        std::fs::write(path, png).map_or_else(|e| format!("error: {e}"), |_| "ok".into())
                    }
                    Err(e) => format!("error: {e}"),
                }
            }
            #[cfg(not(target_os = "macos"))]
            {
                let _ = (r, path);
                "error: macOS only".into()
            }
        }
        // Tone e2e backdrop: a borderless window of THIS process (so no helper app and no extra permission) that paints
        // known colour regions behind the overlay. `backdrop-info` -> primary display in points; `backdrop-set <json>`
        // with [{x,y,w,h,c:[r,g,b]}] in global top-left points; `backdrop-close`.
        "backdrop-info" => backdrop_info(st),
        "backdrop-set" => backdrop_set(st, rest),
        "backdrop-close" => {
            if let Some(w) = st.app.get_webview_window(BACKDROP) {
                let _ = w.destroy();
            }
            #[cfg(target_os = "macos")]
            lowk_capture::keep_window_titled(None);
            "ok".into()
        }
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

const BACKDROP: &str = "qa-backdrop";

fn backdrop_info(st: &Shared) -> String {
    match st.app.primary_monitor() {
        Ok(Some(m)) => {
            let k = m.scale_factor();
            format!(
                "{{\"x\":{},\"y\":{},\"w\":{},\"h\":{},\"scale\":{k}}}",
                f64::from(m.position().x) / k,
                f64::from(m.position().y) / k,
                f64::from(m.size().width) / k,
                f64::from(m.size().height) / k
            )
        }
        _ => "error: no primary monitor".into(),
    }
}

fn backdrop_set(st: &Shared, json: &str) -> String {
    let Ok(rects) = serde_json::from_str::<Vec<serde_json::Value>>(json) else {
        return "error: bad json".into();
    };
    let info: serde_json::Value = serde_json::from_str(&backdrop_info(st)).unwrap_or_default();
    let g = |k: &str| info[k].as_f64().unwrap_or(0.0);
    let (ox, oy, w, h) = (g("x"), g("y"), g("w"), g("h"));
    if st.app.get_webview_window(BACKDROP).is_none() {
        // The sampler excludes every window of this process except this one: it must see the backdrop like a normal app's window.
        #[cfg(target_os = "macos")]
        lowk_capture::keep_window_titled(Some("lowk-qa-backdrop".into()));
        let (tx, rx) = std::sync::mpsc::channel();
        let app = st.app.clone();
        let app2 = app.clone();
        let _ = app.run_on_main_thread(move || {
            let r = tauri::WebviewWindowBuilder::new(
                &app2,
                BACKDROP,
                tauri::WebviewUrl::External("about:blank".parse().expect("url")),
            )
            .title("lowk-qa-backdrop")
            .position(ox, oy)
            .inner_size(w, h)
            .decorations(false)
            .shadow(false)
            .resizable(false)
            .skip_taskbar(true)
            .focused(false)
            .build()
            .map(|win| {
                let _ = win.set_ignore_cursor_events(true);
            })
            .map_err(|e| e.to_string());
            let _ = tx.send(r);
        });
        match rx.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(())) => {}
            Ok(Err(e)) => return format!("error: {e}"),
            Err(_) => return "error: backdrop window timeout".into(),
        }
    }
    let Some(win) = wait_window(st, BACKDROP) else {
        return "error: no backdrop window".into();
    };
    let mut html = String::new();
    for r in &rects {
        let n = |k: &str| r[k].as_f64().unwrap_or(0.0);
        let c: Vec<i64> = r["c"]
            .as_array()
            .map(|a| a.iter().map(|v| v.as_i64().unwrap_or(0)).collect())
            .unwrap_or_default();
        html.push_str(&format!(
            "<div style=\"position:absolute;left:{}px;top:{}px;width:{}px;height:{}px;background:rgb({},{},{})\"></div>",
            n("x") - ox,
            n("y") - oy,
            n("w"),
            n("h"),
            c.first().unwrap_or(&0),
            c.get(1).unwrap_or(&0),
            c.get(2).unwrap_or(&0)
        ));
    }
    let js = format!(
        "document.documentElement.style.cssText='margin:0;background:#000;overflow:hidden';document.body.style.cssText='margin:0;background:#000;overflow:hidden';document.body.innerHTML={}",
        js_str(&html)
    );
    format!("{:?}", win.eval(&js).map_err(|e| e.to_string()))
}
