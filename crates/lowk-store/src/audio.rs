//! Saved call audio (setting `audio.keep_audio`): one 16 kHz mono 16-bit WAV per stream in
//! `<vault>/.lowk/audio/<call id>/{me,system}.wav`.
//!
//! The sample index of a file IS the call time: `sample / 16000` seconds after the call's clock start.
//! Stretches with no audio (microphone off, pause) are holes (a seek, so a sparse file where the
//! filesystem allows it) and read back as silence, so timestamps of a re-transcription line up with
//! the live transcript. A file left by a crash has a header with size 0: readers use the file length.
use lowk_types::{Frame16k, Stream};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

pub const SAMPLE_RATE: u64 = 16_000;
const HEADER: u64 = 44;
/// Frames closer than this to the write position are contiguous (capture clock jitter).
const JITTER_SAMPLES: u64 = 160;

pub fn stream_file(s: Stream) -> &'static str {
    match s {
        Stream::Me => "me.wav",
        Stream::System => "system.wav",
    }
}

fn header(data_bytes: u32) -> [u8; 44] {
    let mut h = [0u8; 44];
    h[0..4].copy_from_slice(b"RIFF");
    h[4..8].copy_from_slice(&data_bytes.saturating_add(36).to_le_bytes());
    h[8..16].copy_from_slice(b"WAVEfmt ");
    h[16..20].copy_from_slice(&16u32.to_le_bytes());
    h[20..22].copy_from_slice(&1u16.to_le_bytes()); // PCM
    h[22..24].copy_from_slice(&1u16.to_le_bytes()); // mono
    h[24..28].copy_from_slice(&(SAMPLE_RATE as u32).to_le_bytes());
    h[28..32].copy_from_slice(&(SAMPLE_RATE as u32 * 2).to_le_bytes());
    h[32..34].copy_from_slice(&2u16.to_le_bytes());
    h[34..36].copy_from_slice(&16u16.to_le_bytes());
    h[36..40].copy_from_slice(b"data");
    h[40..44].copy_from_slice(&data_bytes.to_le_bytes());
    h
}

/// Appends samples at call-time positions.
pub struct WavWriter {
    f: BufWriter<File>,
    /// Next sample index to be written.
    pos: u64,
}

impl WavWriter {
    pub fn create(path: &Path) -> std::io::Result<Self> {
        if let Some(d) = path.parent() {
            fs::create_dir_all(d)?;
        }
        let mut f = File::create(path)?;
        f.write_all(&header(0))?;
        Ok(Self {
            f: BufWriter::with_capacity(64 * 1024, f),
            pos: 0,
        })
    }

    /// Writes `pcm` so that its first sample has index `at` (gaps become holes, overlaps are trimmed).
    pub fn write_at(&mut self, at: u64, pcm: &[f32]) -> std::io::Result<()> {
        let at = if at <= self.pos + JITTER_SAMPLES {
            self.pos
        } else {
            at
        };
        if at > self.pos {
            self.f.seek(SeekFrom::Current(((at - self.pos) * 2) as i64))?;
            self.pos = at;
        }
        let mut buf = Vec::with_capacity(pcm.len() * 2);
        for s in pcm {
            buf.extend_from_slice(&((s.clamp(-1.0, 1.0) * 32767.0).round() as i16).to_le_bytes());
        }
        self.f.write_all(&buf)?;
        self.pos += pcm.len() as u64;
        Ok(())
    }

    pub fn flush(&mut self) -> std::io::Result<()> {
        self.f.flush()
    }

    pub fn samples(&self) -> u64 {
        self.pos
    }

    /// Patches the header sizes; the file ends exactly at the last written sample.
    pub fn finish(mut self) -> std::io::Result<u64> {
        self.f.flush()?;
        let bytes = (self.pos * 2).min(u32::MAX as u64) as u32;
        let f = self.f.get_mut();
        f.set_len(HEADER + self.pos * 2)?;
        f.seek(SeekFrom::Start(0))?;
        f.write_all(&header(bytes))?;
        f.sync_data()?;
        Ok(self.pos)
    }
}

/// Random-access reader of a stream file.
pub struct WavReader {
    f: File,
    len: u64,
}

impl WavReader {
    pub fn open(path: &Path) -> std::io::Result<Self> {
        let mut f = File::open(path)?;
        let mut h = [0u8; 44];
        f.read_exact(&mut h)?;
        let ok = &h[0..4] == b"RIFF"
            && &h[8..12] == b"WAVE"
            && u16::from_le_bytes([h[20], h[21]]) == 1
            && u16::from_le_bytes([h[22], h[23]]) == 1
            && u32::from_le_bytes([h[24], h[25], h[26], h[27]]) as u64 == SAMPLE_RATE
            && u16::from_le_bytes([h[34], h[35]]) == 16
            && &h[36..40] == b"data";
        if !ok {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "not a 16 kHz mono 16-bit WAV written by Lowk",
            ));
        }
        let file_len = f.metadata()?.len();
        Ok(Self {
            f,
            len: file_len.saturating_sub(HEADER) / 2,
        })
    }

    pub fn len_samples(&self) -> u64 {
        self.len
    }

    /// `n` samples from `start`, clipped at the end.
    pub fn read(&mut self, start: u64, n: usize) -> std::io::Result<Vec<f32>> {
        let start = start.min(self.len);
        let n = n.min((self.len - start) as usize);
        self.f.seek(SeekFrom::Start(HEADER + start * 2))?;
        let mut b = vec![0u8; n * 2];
        self.f.read_exact(&mut b)?;
        Ok(b.chunks_exact(2)
            .map(|c| i16::from_le_bytes([c[0], c[1]]) as f32 / 32768.0)
            .collect())
    }
}

/// Where a saved stream lives.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StreamAudio {
    pub stream: Stream,
    pub path: PathBuf,
    pub samples: u64,
}

impl StreamAudio {
    pub fn seconds(&self) -> f64 {
        self.samples as f64 / SAMPLE_RATE as f64
    }
}

/// The streams saved in `dir` (empty files do not count).
pub fn list_audio(dir: &Path) -> Vec<StreamAudio> {
    [Stream::Me, Stream::System]
        .into_iter()
        .filter_map(|s| {
            let path = dir.join(stream_file(s));
            let samples = fs::metadata(&path).ok()?.len().saturating_sub(HEADER) / 2;
            (samples > 0).then_some(StreamAudio {
                stream: s,
                path,
                samples,
            })
        })
        .collect()
}

/// Writes the frames of a running call. Cheap enough for the capture callback (buffered, no fsync).
pub struct AudioKeeper {
    dir: PathBuf,
    t0_mono: u64,
    w: Mutex<HashMap<Stream, WavWriter>>,
    failed: Mutex<bool>,
}

impl AudioKeeper {
    pub fn create(dir: PathBuf, t0_mono: u64) -> std::io::Result<Self> {
        fs::create_dir_all(&dir)?;
        Ok(Self {
            dir,
            t0_mono,
            w: Mutex::new(HashMap::new()),
            failed: Mutex::new(false),
        })
    }

    pub fn write(&self, f: &Frame16k) {
        let at = f.t_mono_ns.saturating_sub(self.t0_mono) * SAMPLE_RATE / 1_000_000_000;
        let mut g = self.w.lock();
        if let std::collections::hash_map::Entry::Vacant(v) = g.entry(f.stream) {
            match WavWriter::create(&self.dir.join(stream_file(f.stream))) {
                Ok(w) => {
                    v.insert(w);
                }
                Err(e) => {
                    let mut failed = self.failed.lock();
                    if !*failed {
                        *failed = true;
                        tracing::warn!(?e, "cannot save call audio");
                    }
                    return;
                }
            }
        }
        if let Some(w) = g.get_mut(&f.stream) {
            // a disk-full style error stops saving that stream, never the call
            if let Err(e) = w.write_at(at, &f.pcm) {
                tracing::warn!(
                    ?e,
                    "saving call audio failed; dropping the stream's audio from here"
                );
                g.remove(&f.stream);
            }
        }
    }

    /// Flushes and closes every stream file; true when any audio was saved.
    pub fn finish(&self) -> bool {
        let writers: Vec<WavWriter> = self.w.lock().drain().map(|(_, w)| w).collect();
        let mut any = false;
        for w in writers {
            match w.finish() {
                Ok(n) => any |= n > 0,
                Err(e) => tracing::warn!(?e, "closing call audio failed"),
            }
        }
        any
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(s: Stream, t_ms: u64, n: usize, v: f32) -> Frame16k {
        Frame16k {
            stream: s,
            t_mono_ns: 1_000 + t_ms * 1_000_000,
            pcm: vec![v; n],
        }
    }

    #[test]
    fn samples_sit_at_call_time_and_gaps_are_silence() {
        let d = tempfile::tempdir().unwrap();
        let k = AudioKeeper::create(d.path().join("a"), 1_000).unwrap();
        k.write(&frame(Stream::Me, 0, 320, 0.5));
        k.write(&frame(Stream::Me, 20, 320, 0.5)); // contiguous
                                                   // mic off for 2 s, then audio again at t = 2.04 s
        k.write(&frame(Stream::Me, 2040, 320, -0.5));
        k.write(&frame(Stream::System, 0, 160, 0.25));
        assert!(k.finish());
        let files = list_audio(&d.path().join("a"));
        assert_eq!(files.len(), 2);
        let me = files.iter().find(|f| f.stream == Stream::Me).unwrap();
        assert_eq!(me.samples, 2040 * 16 + 320);
        let mut r = WavReader::open(&me.path).unwrap();
        assert_eq!(r.len_samples(), me.samples);
        assert!(r.read(100, 10).unwrap().iter().all(|x| (x - 0.5).abs() < 1e-3));
        assert!(
            r.read(16_000, 100).unwrap().iter().all(|x| *x == 0.0),
            "the gap reads as silence"
        );
        assert!(r
            .read(2040 * 16, 10)
            .unwrap()
            .iter()
            .all(|x| (x + 0.5).abs() < 1e-3));
        // clipped at the end
        assert_eq!(r.read(me.samples - 5, 50).unwrap().len(), 5);
    }

    #[test]
    fn an_unfinished_file_is_readable_by_its_length() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("me.wav");
        let mut w = WavWriter::create(&p).unwrap();
        w.write_at(0, &[0.1; 1000]).unwrap();
        w.flush().unwrap();
        drop(w); // crash: header says 0 bytes
        let r = WavReader::open(&p).unwrap();
        assert_eq!(r.len_samples(), 1000);
    }
}
