//! Prints bind events. Usage:
//!   cargo run -p lowk-input --example lowk-input-demo -- [--record] [--secs N]
//! Binds the OS default binds plus mouse Back/Forward through the low-level hook
//! (no combo adapter here, so combos go through the hook too). On macOS this needs
//! Input Monitoring; without it the demo prints which binds are degraded.
use lowk_types::{default_binds, ActionId, BindSpec, InputBackend, Mods, MouseBtn, Os, Trigger};
use std::time::{Duration, Instant};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let secs: u64 = args
        .iter()
        .position(|a| a == "--secs")
        .and_then(|i| args.get(i + 1))
        .and_then(|s| s.parse().ok())
        .unwrap_or(10);
    let os = Os::current();
    let mut m = lowk_input::InputManager::new(None);
    let cur = m.cursor();
    let rx = m.events();

    if args.iter().any(|a| a == "--record") {
        match m.record_start() {
            Ok(r) => {
                println!("recording: press a combo, a lone modifier, or a mouse button ({secs}s)");
                match r.recv_timeout(Duration::from_secs(secs)) {
                    Ok(t) => println!("captured: {t:?} = {}", lowk_input::keys::describe(os, &t)),
                    Err(_) => println!("nothing captured"),
                }
            }
            Err(e) => println!("record unavailable: {e}"),
        }
        m.record_stop();
        return;
    }

    let mut binds = default_binds(os);
    for (action, button) in [
        (ActionId::CancelAnswer, MouseBtn::Back),
        (ActionId::ClearShots, MouseBtn::Forward),
    ] {
        binds.push(BindSpec {
            action,
            trigger: Trigger::Mouse {
                button,
                mods: Mods::NONE,
            },
            swallow: false,
        });
    }
    let rep = m.set_binds(&binds).expect("set_binds");
    for (b, e) in binds.iter().zip(&rep.entries) {
        println!(
            "{:<18} {:<22} {:?}",
            format!("{:?}", b.action),
            lowk_input::keys::describe(os, &b.trigger),
            e.status
        );
    }
    println!("status: {:?}", m.status());
    println!("listening {secs}s (hold Option/Alt, press side buttons, combos)...");
    let end = Instant::now() + Duration::from_secs(secs);
    let mut last_cursor = Instant::now();
    while Instant::now() < end {
        if let Ok(ev) = rx.recv_timeout(Duration::from_millis(100)) {
            let (x, y) = cur.position();
            println!(
                "{:>14} ns  {:?} {:?}  cursor=({x:.0},{y:.0})",
                ev.t_mono_ns, ev.action, ev.phase
            );
        }
        if last_cursor.elapsed() > Duration::from_secs(3) {
            let (x, y) = cur.position();
            println!("cursor=({x:.0},{y:.0})");
            last_cursor = Instant::now();
        }
    }
}
