//! Latency bookkeeping for the debug page: bounded sample windows with percentiles.
use lowk_types::{now_mono_ns, MonoNs, Percentiles, Stream, StreamDiag, SttEvent, SttKind};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Mutex;

const WINDOW: usize = 2000;

/// Last `WINDOW` samples (milliseconds).
#[derive(Default)]
pub struct Window(Mutex<VecDeque<u32>>);

impl Window {
    pub fn record(&self, ms: u32) {
        let mut w = self.0.lock().unwrap();
        if w.len() == WINDOW {
            w.pop_front();
        }
        w.push_back(ms);
    }
    pub fn percentiles(&self) -> Percentiles {
        let mut v: Vec<u32> = self.0.lock().unwrap().iter().copied().collect();
        percentiles(&mut v)
    }
}

/// p50 / p95 / max of `v` (sorted in place; nearest-rank).
pub fn percentiles(v: &mut [u32]) -> Percentiles {
    if v.is_empty() {
        return Percentiles::default();
    }
    v.sort_unstable();
    let at = |q: f64| v[(((v.len() as f64) * q).ceil() as usize).clamp(1, v.len()) - 1];
    Percentiles {
        samples: v.len() as u32,
        p50_ms: at(0.50),
        p95_ms: at(0.95),
        max_ms: *v.last().unwrap(),
    }
}

/// Shared by an engine's threads and read by `SttEngine::stats`.
#[derive(Default)]
pub struct StreamStats {
    pub emit_lag: Window,
    pub final_lag: Window,
    /// End time (mono ns) of the newest frame pushed to the engine.
    pub pushed_ns: AtomicU64,
    /// End time of the newest audio the engine has confirmed consuming.
    pub acked_ns: AtomicU64,
    pub restarts: AtomicU32,
    pub dropped_frames: AtomicU64,
}

impl StreamStats {
    pub fn note_pushed(&self, end_ns: MonoNs) {
        self.pushed_ns.store(end_ns, Ordering::Relaxed);
    }
    pub fn note_acked(&self, end_ns: MonoNs) {
        self.acked_ns.fetch_max(end_ns, Ordering::Relaxed);
    }
    /// Records the lag of an event that the engine is emitting now.
    pub fn note_event(&self, ev: &SttEvent) {
        self.note_acked(ev.t_audio_end_ns);
        let lag = (ev.t_emit_ns.saturating_sub(ev.t_audio_end_ns) / 1_000_000) as u32;
        match ev.kind {
            SttKind::Partial => self.emit_lag.record(lag),
            SttKind::Final => self.final_lag.record(lag),
        }
    }
    pub fn backlog_ms(&self) -> u32 {
        let p = self.pushed_ns.load(Ordering::Relaxed);
        let a = self.acked_ns.load(Ordering::Relaxed);
        (p.saturating_sub(a) / 1_000_000).min(u32::MAX as u64) as u32
    }
    pub fn diag(&self, stream: Stream) -> StreamDiag {
        StreamDiag {
            stream,
            emit_lag: self.emit_lag.percentiles(),
            final_lag: self.final_lag.percentiles(),
            backlog_ms: self.backlog_ms(),
            restarts: self.restarts.load(Ordering::Relaxed),
        }
    }
}

/// Mono-ns "now" in ms for log lines.
pub fn now_ms() -> u64 {
    now_mono_ns() / 1_000_000
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentiles_nearest_rank() {
        let mut v: Vec<u32> = (1..=100).collect();
        let p = percentiles(&mut v);
        assert_eq!((p.p50_ms, p.p95_ms, p.max_ms, p.samples), (50, 95, 100, 100));
        assert_eq!(percentiles(&mut []).samples, 0);
    }

    #[test]
    fn backlog_is_pushed_minus_acked() {
        let s = StreamStats::default();
        s.note_pushed(3_000_000_000);
        s.note_acked(1_000_000_000);
        assert_eq!(s.backlog_ms(), 2000);
        s.note_acked(500_000_000); // never goes back
        assert_eq!(s.backlog_ms(), 2000);
    }
}
