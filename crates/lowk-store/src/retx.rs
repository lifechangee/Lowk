//! Transcript revisions and the «re-transcribe recording» write path.
//!
//! The call `.md` always holds the CURRENT transcript. Before a re-transcription replaces it, the
//! whole previous file is set aside as a revision under `<vault>/.lowk/revisions/<call id>/`:
//!
//! * `r<N>.md`   the complete previous call file (transcript, Q&A, notes, summary as they were);
//! * `r<N>.json` `{ "kind": "live" | "retx", "at": "<RFC 3339>" }`.
//!
//! The current file carries `transcript_rev: <N>` (absent = 1, the live transcript), plus
//! `retranscribed: <RFC 3339>` and `retranscribed_by: <engine>` after a re-transcription.
//! Revision files are never deleted by Lowk (`restore` sets the old version back as a NEW revision
//! number, so nothing is ever overwritten). The service directory is not indexed or synced as content.
use crate::audio::{self, AudioKeeper, StreamAudio};
use crate::call_codec::{self, Block, CallDoc, Speaker};
use crate::util::err;
use crate::vault::Vault;
use crate::writer::segment;
use lowk_types::{ErrorCode, Result, RetxRevision, Stream, Ulid};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

const ORDER: [&str; 5] = [
    "audio",
    "transcript_rev",
    "retranscribed",
    "retranscribed_by",
    "stt",
];

/// One re-transcribed stretch of one stream.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RetxChunk {
    pub stream: Stream,
    pub start_ms: u64,
    pub end_ms: u64,
    /// `None` = the pass could not read this stretch: the live text stays.
    pub text: Option<String>,
}

/// The outcome of a pass, ready to be merged into a call.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RetxResult {
    pub chunks: Vec<RetxChunk>,
    pub engine: String,
}

#[derive(Serialize, Deserialize)]
struct RevMeta {
    kind: String,
    at: String,
}

/// Current transcript revision of a parsed call (1 = live).
pub fn transcript_rev(doc: &CallDoc) -> u32 {
    doc.fm
        .raw
        .get("transcript_rev")
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(1)
}

fn set_rev(doc: &mut CallDoc, rev: u32, retx: Option<(&str, &str)>) {
    doc.fm.raw.set_ordered("transcript_rev", rev.to_string(), &ORDER);
    match retx {
        Some((at, engine)) => {
            doc.fm
                .raw
                .set_ordered("retranscribed", format!("\"{at}\""), &ORDER);
            doc.fm
                .raw
                .set_ordered("retranscribed_by", format!("\"{engine}\""), &ORDER);
        }
        None => {
            doc.fm.raw.remove("retranscribed");
            doc.fm.raw.remove("retranscribed_by");
        }
    }
}

/// True when `at_s` (a turn's start second) lies inside a successfully re-transcribed stretch of `stream`.
fn replaced(chunks: &[RetxChunk], stream: Stream, at_s: u32) -> bool {
    let mid = u64::from(at_s) * 1000 + 500;
    chunks.iter().any(|c| {
        c.text.is_some() && c.stream == stream && c.start_ms <= mid && mid < c.end_ms.max(c.start_ms + 1000)
    })
}

/// Merges the new text into the old blocks: Me/System turns inside re-transcribed stretches are
/// replaced; everything else (Q&A, images, mic-off markers, unreadable stretches, uncovered live text,
/// raw lines) stays. Blocks are ordered by time (markers first, then turns, then Q&A / images).
pub fn merge_transcript(old: &[Block], chunks: &[RetxChunk]) -> Vec<Block> {
    // (time, rank, seq, block)
    let mut v: Vec<(u32, u8, usize, Block)> = vec![];
    let mut last_t = 0u32;
    for (i, b) in old.iter().enumerate() {
        let (t, rank) = match b {
            Block::Turn { at_s, speaker, .. } => {
                let st = match speaker {
                    Speaker::Me => Some(Stream::Me),
                    Speaker::System => Some(Stream::System),
                    Speaker::Other(_) => None,
                };
                if st.is_some_and(|s| replaced(chunks, s, *at_s)) {
                    last_t = *at_s;
                    continue;
                }
                (*at_s, 1)
            }
            Block::Marker { at_s, .. } => (*at_s, 0),
            Block::Qa(q) => (q.at_s, 2),
            Block::Image(m) => (m.at_s, 2),
            Block::Raw(_) => (last_t, 3),
        };
        last_t = t;
        v.push((t, rank, i, b.clone()));
    }
    let mut seq = old.len();
    for c in chunks {
        let Some(text) = &c.text else { continue };
        let sp = match c.stream {
            Stream::Me => Speaker::Me,
            Stream::System => Speaker::System,
        };
        let (at, end) = ((c.start_ms / 1000) as u32, (c.end_ms / 1000) as u32);
        for (t, at_s) in segment(text, at, end) {
            v.push((
                at_s,
                1,
                seq,
                Block::Turn {
                    at_s,
                    speaker: sp.clone(),
                    text: t,
                },
            ));
            seq += 1;
        }
    }
    v.sort_by_key(|(t, r, s, _)| (*t, *r, *s));
    v.into_iter().map(|(_, _, _, b)| b).collect()
}

impl Vault {
    pub fn audio_dir(&self, id: &Ulid) -> PathBuf {
        self.root()
            .join(crate::vault::SERVICE_DIR)
            .join("audio")
            .join(id.to_string())
    }

    fn revisions_dir(&self, id: &Ulid) -> PathBuf {
        self.root()
            .join(crate::vault::SERVICE_DIR)
            .join("revisions")
            .join(id.to_string())
    }

    /// The saved streams of a call (empty = no audio kept).
    pub fn call_audio(&self, id: &Ulid) -> Vec<StreamAudio> {
        audio::list_audio(&self.audio_dir(id))
    }

    /// A writer for the audio of a call that is being recorded (`t0_mono` = the session clock start).
    pub fn audio_keeper(&self, id: &Ulid, t0_mono: u64) -> Result<AudioKeeper> {
        Ok(AudioKeeper::create(self.audio_dir(id), t0_mono)?)
    }

    /// Removes the saved audio and revisions of a call (permanent delete of the call).
    pub fn purge_call_files(&self, id: &Ulid) {
        let _ = fs::remove_dir_all(self.audio_dir(id));
        let _ = fs::remove_dir_all(self.revisions_dir(id));
    }

    /// Revisions set aside for a call, oldest first.
    pub fn revisions(&self, id: &Ulid) -> Vec<RetxRevision> {
        let mut out = vec![];
        let Ok(rd) = fs::read_dir(self.revisions_dir(id)) else {
            return out;
        };
        for e in rd.flatten() {
            let n = e.file_name().to_string_lossy().into_owned();
            let Some(rev) = n
                .strip_prefix('r')
                .and_then(|x| x.strip_suffix(".json"))
                .and_then(|x| x.parse::<u32>().ok())
            else {
                continue;
            };
            if let Some(m) = fs::read_to_string(e.path())
                .ok()
                .and_then(|s| serde_json::from_str::<RevMeta>(&s).ok())
            {
                out.push(RetxRevision {
                    rev,
                    kind: m.kind,
                    at: m.at,
                });
            }
        }
        out.sort_by_key(|r| r.rev);
        out
    }

    fn next_rev(&self, id: &Ulid, cur: u32) -> u32 {
        self.revisions(id)
            .iter()
            .map(|r| r.rev)
            .max()
            .unwrap_or(0)
            .max(cur)
            + 1
    }

    /// Sets the previous file aside as revision `cur` (never overwrites an existing one).
    fn set_aside(&self, id: &Ulid, doc: &CallDoc, cur: u32, at: &str) -> Result<()> {
        let dir = self.revisions_dir(id);
        fs::create_dir_all(&dir)?;
        let (md, meta) = (dir.join(format!("r{cur}.md")), dir.join(format!("r{cur}.json")));
        if md.exists() {
            return Err(err(ErrorCode::Io, format!("revision r{cur} already exists")));
        }
        let kind = if doc.fm.raw.get("retranscribed").is_some() {
            "retx"
        } else {
            "live"
        };
        crate::atomic_write(&md, call_codec::render(doc).as_bytes())?;
        crate::atomic_write(
            &meta,
            serde_json::to_string(&RevMeta {
                kind: kind.into(),
                at: at.into(),
            })
            .map_err(|e| err(ErrorCode::Io, e.to_string()))?
            .as_bytes(),
        )?;
        Ok(())
    }

    /// Writes a re-transcription as the call's new transcript revision; returns its number. The
    /// previous file is kept as a revision first. Rejected while the call is being recorded.
    pub fn apply_retranscript(&self, id: &Ulid, res: &RetxResult, at: &str) -> Result<u32> {
        let mut out: Result<u32> = Err(err(ErrorCode::Unknown, "not applied"));
        self.update_call(id, |doc| {
            out = (|| {
                let cur = transcript_rev(doc);
                self.set_aside(id, doc, cur, at)?;
                let new = self.next_rev(id, cur);
                let merged = merge_transcript(doc.blocks(), &res.chunks);
                *doc.blocks_mut() = merged;
                set_rev(doc, new, Some((at, &res.engine)));
                Ok(new)
            })();
        })?;
        out
    }

    /// Brings revision `rev` back as the current transcript (under a new revision number; the file
    /// being replaced is kept as a revision too). Returns the new number.
    pub fn restore_revision(&self, id: &Ulid, rev: u32, at: &str) -> Result<u32> {
        let md = fs::read_to_string(self.revisions_dir(id).join(format!("r{rev}.md")))
            .map_err(|_| err(ErrorCode::NotFound, format!("revision {rev}")))?;
        let mut out: Result<u32> = Err(err(ErrorCode::Unknown, "not restored"));
        self.update_call(id, |doc| {
            out = (|| {
                let cur = transcript_rev(doc);
                self.set_aside(id, doc, cur, at)?;
                let new = self.next_rev(id, cur);
                let mut old = call_codec::parse(&md);
                if old.fm.id != doc.fm.id {
                    return Err(err(ErrorCode::FormatInvalid, "revision belongs to another call"));
                }
                let retx = old
                    .fm
                    .raw
                    .get("retranscribed")
                    .map(|s| s.trim().trim_matches('"').to_string());
                let by = old
                    .fm
                    .raw
                    .get("retranscribed_by")
                    .map(|s| s.trim().trim_matches('"').to_string())
                    .unwrap_or_default();
                set_rev(&mut old, new, retx.as_deref().map(|a| (a, by.as_str())));
                *doc = old;
                Ok(new)
            })();
        })?;
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lowk_types::*;

    fn new_call(title: &str) -> NewCall {
        NewCall {
            title: title.into(),
            folder: FolderRef::Unsorted,
            started: "2026-10-24T10:00:00+03:00".into(),
            sources: CallSources {
                me: None,
                system: None,
            },
            stt_engine: Some("nemotron-local".into()),
            languages: vec!["ru".into()],
            speaker_me: "Я".into(),
            speaker_system: "Система".into(),
            clock: None,
        }
    }

    fn call(v: &Vault) -> Ulid {
        let clock = SessionClock {
            t0_mono: 0,
            t0_wall_ms: 0,
        };
        let mut w = v.begin_call_with_clock(new_call("Lecture"), clock).unwrap();
        let id = w.id();
        for (at, text) in [(2u64, "привет всем"), (14, "сегодня тема лекции"), (40, "конец")]
        {
            w.push(CallEntry::Final {
                event: SttEvent {
                    stream: Stream::System,
                    utt: at,
                    rev: 1,
                    kind: SttKind::Final,
                    text: text.into(),
                    lang: None,
                    t_start_ns: at * 1_000_000_000,
                    t_audio_end_ns: at * 1_000_000_000 + 1_000_000_000,
                    t_emit_ns: 0,
                },
            })
            .unwrap();
        }
        w.push(CallEntry::Marker {
            at_s: 20,
            text: "Микрофон выключен".into(),
        })
        .unwrap();
        w.finish_at("2026-10-24T10:01:00+03:00".into()).unwrap();
        id
    }

    fn blocks_text(v: &Vault, id: &Ulid) -> Vec<String> {
        v.read_call_doc(id)
            .unwrap()
            .blocks()
            .iter()
            .map(|b| match b {
                Block::Turn { at_s, text, .. } => format!("{at_s}:{text}"),
                Block::Marker { at_s, text } => format!("{at_s}:*{text}*"),
                _ => "?".into(),
            })
            .collect()
    }

    #[test]
    fn merge_replaces_only_covered_turns_and_keeps_markers_and_unreadable_stretches() {
        let old = vec![
            Block::Turn {
                at_s: 2,
                speaker: Speaker::System,
                text: "a".into(),
            },
            Block::Turn {
                at_s: 12,
                speaker: Speaker::System,
                text: "b".into(),
            },
            Block::Marker {
                at_s: 20,
                text: "Микрофон выключен".into(),
            },
            Block::Turn {
                at_s: 30,
                speaker: Speaker::System,
                text: "c".into(),
            },
            Block::Turn {
                at_s: 31,
                speaker: Speaker::Me,
                text: "me".into(),
            },
        ];
        let chunks = vec![
            RetxChunk {
                stream: Stream::System,
                start_ms: 1_000,
                end_ms: 15_000,
                text: Some("А. Б.".into()),
            },
            RetxChunk {
                stream: Stream::System,
                start_ms: 29_000,
                end_ms: 33_000,
                text: None,
            }, // unreadable
        ];
        let m = merge_transcript(&old, &chunks);
        let s: Vec<String> = m
            .iter()
            .map(|b| match b {
                Block::Turn { at_s, text, .. } => format!("{at_s}:{text}"),
                Block::Marker { at_s, .. } => format!("{at_s}:marker"),
                _ => "?".into(),
            })
            .collect();
        assert_eq!(s, ["1:А. Б.", "20:marker", "30:c", "31:me"]);
    }

    #[test]
    fn a_new_revision_keeps_the_live_file_and_can_be_restored() {
        let d = tempfile::tempdir().unwrap();
        let v = Vault::open(d.path()).unwrap();
        let id = call(&v);
        let live = blocks_text(&v, &id);
        assert_eq!(live.len(), 4, "{live:?}");
        assert_eq!(transcript_rev(&v.read_call_doc(&id).unwrap()), 1);

        let res = RetxResult {
            engine: "whisper-large-v3-turbo".into(),
            chunks: vec![RetxChunk {
                stream: Stream::System,
                start_ms: 0,
                end_ms: 18_000,
                text: Some("Привет всем! Сегодня тема лекции.".into()),
            }],
        };
        let rev = v
            .apply_retranscript(&id, &res, "2026-10-24T12:00:00+03:00")
            .unwrap();
        assert_eq!(rev, 2);
        let doc = v.read_call_doc(&id).unwrap();
        assert_eq!(transcript_rev(&doc), 2);
        assert_eq!(
            doc.fm.raw.get("retranscribed_by").unwrap(),
            "\"whisper-large-v3-turbo\""
        );
        let now = blocks_text(&v, &id);
        assert!(
            now.contains(&"0:Привет всем! Сегодня тема лекции.".to_string()),
            "{now:?}"
        );
        assert!(
            now.contains(&"20:*Микрофон выключен*".to_string()),
            "mic-off marker kept"
        );
        assert!(now.contains(&"40:конец".to_string()), "uncovered live text kept");
        assert!(!now.iter().any(|l| l.contains("привет всем")));

        // the live version is on disk, complete
        let revs = v.revisions(&id);
        assert_eq!(revs.len(), 1);
        assert_eq!((revs[0].rev, revs[0].kind.as_str()), (1, "live"));
        let snap = fs::read_to_string(v.revisions_dir(&id).join("r1.md")).unwrap();
        assert!(snap.contains("привет всем") && snap.contains("сегодня тема лекции"));

        // restore: live text is current again, as revision 3; the retx file is kept as r2
        let r3 = v.restore_revision(&id, 1, "2026-10-24T12:05:00+03:00").unwrap();
        assert_eq!(r3, 3);
        assert_eq!(blocks_text(&v, &id), live);
        let kinds: Vec<(u32, String)> = v.revisions(&id).iter().map(|r| (r.rev, r.kind.clone())).collect();
        assert_eq!(kinds, [(1, "live".to_string()), (2, "retx".to_string())]);
        // and a second re-transcription still never overwrites anything
        assert_eq!(
            v.apply_retranscript(&id, &res, "2026-10-24T12:10:00+03:00")
                .unwrap(),
            4
        );
        assert_eq!(v.revisions(&id).len(), 3);
    }

    #[test]
    fn refused_while_the_call_is_recording() {
        let d = tempfile::tempdir().unwrap();
        let v = Vault::open(d.path()).unwrap();
        let w = v.begin_call(new_call("x")).unwrap();
        let res = RetxResult {
            engine: "e".into(),
            chunks: vec![],
        };
        let e = v
            .apply_retranscript(&w.id(), &res, "2026-10-24T12:00:00+03:00")
            .unwrap_err();
        assert_eq!(e.code, ErrorCode::SessionBusy);
    }
}
