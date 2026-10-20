//! Recent audio of one stream, addressed by monotonic time, so a finalized utterance can
//! be cut out and re-transcribed in the background.
use lowk_types::{Frame16k, MonoNs};
use std::collections::VecDeque;

const RATE: u64 = 16_000;

pub struct PcmRing {
    max_samples: usize,
    /// Capture time of `buf[0]`.
    t0: MonoNs,
    buf: VecDeque<f32>,
    /// End time of the newest sample.
    t_end: MonoNs,
}

fn ns_to_samples(ns: u64) -> usize {
    (ns * RATE / 1_000_000_000) as usize
}

impl PcmRing {
    pub fn new(seconds: u32) -> Self {
        Self {
            max_samples: seconds as usize * RATE as usize,
            t0: 0,
            buf: VecDeque::new(),
            t_end: 0,
        }
    }

    pub fn push(&mut self, f: &Frame16k) {
        if self.buf.is_empty() {
            self.t0 = f.t_mono_ns;
        } else if f.t_mono_ns > self.t_end + 40_000_000 {
            // gap (device switch, pause): pad with silence so times stay addressable
            let gap = ns_to_samples(f.t_mono_ns - self.t_end).min(self.max_samples);
            self.buf.extend(std::iter::repeat_n(0.0, gap));
        }
        self.buf.extend(f.pcm.iter().copied());
        self.t_end = f.t_mono_ns + (f.pcm.len() as u64) * 1_000_000_000 / RATE;
        while self.buf.len() > self.max_samples {
            let drop = self.buf.len() - self.max_samples;
            self.buf.drain(..drop);
            self.t0 += (drop as u64) * 1_000_000_000 / RATE;
        }
    }

    /// Samples in `[from, to)`, clipped to what is still buffered (`None` when empty).
    pub fn cut(&self, from: MonoNs, to: MonoNs) -> Option<Vec<f32>> {
        if self.buf.is_empty() || to <= from {
            return None;
        }
        let a = ns_to_samples(from.saturating_sub(self.t0));
        let b = ns_to_samples(to.saturating_sub(self.t0)).min(self.buf.len());
        if a >= b || from >= self.t_end {
            return None;
        }
        Some(self.buf.iter().skip(a).take(b - a).copied().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lowk_types::Stream;

    fn fr(t_ms: u64, v: f32) -> Frame16k {
        Frame16k {
            stream: Stream::Me,
            t_mono_ns: t_ms * 1_000_000,
            pcm: vec![v; 320],
        }
    }

    #[test]
    fn cuts_by_time_and_evicts_old_audio() {
        let mut r = PcmRing::new(1); // 1 s
        for i in 0..100u64 {
            r.push(&fr(i * 20, i as f32));
        }
        // 2 s of audio pushed, 1 s kept: [1000 ms, 2000 ms)
        assert!(r.cut(0, 500_000_000).is_none() || r.cut(0, 500_000_000).unwrap().is_empty());
        let c = r.cut(1_000_000_000, 1_040_000_000).unwrap();
        assert_eq!(c.len(), 640);
        assert_eq!(c[0], 50.0);
        assert_eq!(c[320], 51.0);
    }

    #[test]
    fn gaps_are_padded() {
        let mut r = PcmRing::new(10);
        r.push(&fr(0, 1.0));
        r.push(&fr(100, 2.0)); // 80 ms gap after the first 20 ms frame
        let c = r.cut(0, 120_000_000).unwrap();
        assert_eq!(c.len(), 1920);
        assert_eq!(c[0], 1.0);
        assert_eq!(c[400], 0.0);
        assert_eq!(c[1700], 2.0);
    }
}
