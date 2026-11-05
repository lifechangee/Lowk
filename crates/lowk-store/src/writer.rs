//! Live-call durability (architecture section 4.4): every entry is appended to a
//! JSONL journal under `.lowk/state/live/`, the `.md` is materialized atomically
//! on demand / every ~20 s / on finish, and an unfinished journal is recovered
//! into a `.md` after a crash.
use crate::call_codec::{self, Block, CallDoc, CallFm, ImageBlock, Part, QaBlock, Speaker};
use crate::fm;
use crate::util::*;
use crate::vault::{Vault, VaultInner};
use lowk_types::*;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap};
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Consecutive same-speaker finals closer than this merge into one line, but only while the
/// previous one ends mid-sentence (no `.?!…`): a finished sentence/utterance gets its own line.
const MERGE_GAP_S: u32 = 2;
/// A line spanning more than this starts a new timestamp; a longer final is split at sentence ends.
const MAX_TURN_S: u32 = 15;
type FinalPieces = Vec<(u32, Speaker, String)>;

/// Splits one final that spans more than `MAX_TURN_S` into sentence groups of about equal length,
/// each stamped by its character share of the audio span. Always returns at least one segment.
pub(crate) fn segment(text: &str, at: u32, end: u32) -> Vec<(String, u32)> {
    let span = end.saturating_sub(at);
    if span <= MAX_TURN_S {
        return vec![(text.to_string(), at)];
    }
    let mut sentences: Vec<String> = vec![];
    let mut cur = String::new();
    let mut it = text.chars().peekable();
    while let Some(c) = it.next() {
        cur.push(c);
        if matches!(c, '.' | '?' | '!' | '…') && it.peek().is_none_or(|n| n.is_whitespace()) {
            sentences.push(std::mem::take(&mut cur).trim().to_string());
        }
    }
    if !cur.trim().is_empty() {
        sentences.push(cur.trim().to_string());
    }
    let groups = span.div_ceil(MAX_TURN_S).max(1) as usize;
    let total: usize = sentences
        .iter()
        .map(|x| x.chars().count() + 1)
        .sum::<usize>()
        .max(1);
    let per = total / groups + 1;
    let (mut out, mut acc, mut used, mut start_chars) = (vec![], String::new(), 0usize, 0usize);
    for sn in sentences {
        if acc.is_empty() {
            start_chars = used;
        }
        used += sn.chars().count() + 1;
        if !acc.is_empty() {
            acc.push(' ');
        }
        acc.push_str(&sn);
        if used - start_chars >= per {
            let t = at + (span as u64 * start_chars as u64 / total as u64) as u32;
            out.push((std::mem::take(&mut acc), t));
        }
    }
    if !acc.is_empty() {
        let t = at + (span as u64 * start_chars as u64 / total as u64) as u32;
        out.push((acc, t));
    }
    if out.is_empty() {
        out.push((text.to_string(), at));
    }
    out
}
const SYNC_EVERY: Duration = Duration::from_secs(2);
const MATERIALIZE_EVERY: Duration = Duration::from_secs(20);

#[derive(Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
enum Line {
    Begin {
        id: Ulid,
        new: NewCall,
        clock: SessionClock,
        rel: String,
        folder_title: String,
    },
    Entry {
        e: CallEntry,
    },
    Heartbeat {
        offset_s: u32,
    },
    End {
        ended: String,
    },
}

/// Things the session may want to show the user.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WriterNotice {
    /// The file was edited outside Lowk during the call; its non-transcript
    /// content (summary, notes, tags) was kept and merged.
    ExternalEditMerged,
}

/// In-memory model of a call being recorded. Shared by the writer and recovery.
pub(crate) struct Model {
    pub doc: CallDoc,
    clock: SessionClock,
    last_end: u32,
    last_at: u32,
    langs: BTreeSet<String>,
    finals: HashMap<(Stream, UttId), FinalPieces>,
}

impl Model {
    fn new(id: Ulid, m: &NewCall, clock: SessionClock, folder_title: &str) -> Self {
        let mut f = CallFm::new(&id.to_string(), &m.title, &m.started);
        f.folder = folder_title.to_string();
        f.languages = m.languages.clone();
        f.source_me = m.sources.me.clone();
        f.source_system = m.sources.system.clone();
        f.stt_engine = m.stt_engine.clone();
        f.speaker_me = m.speaker_me.clone();
        f.speaker_system = m.speaker_system.clone();
        Self {
            doc: CallDoc::new(f),
            clock,
            last_end: 0,
            last_at: 0,
            langs: m.languages.iter().cloned().collect(),
            finals: HashMap::new(),
        }
    }

    pub(crate) fn apply(&mut self, e: &CallEntry) -> Result<()> {
        match e {
            CallEntry::Final { event } => {
                if event.kind != SttKind::Final || event.text.trim().is_empty() {
                    return Ok(());
                }
                let at = (self.clock.offset_ns(event.t_start_ns) / 1_000_000_000) as u32;
                let end =
                    (self.clock.offset_ns(event.t_audio_end_ns.max(event.t_start_ns)) / 1_000_000_000) as u32;
                let sp = match event.stream {
                    Stream::Me => Speaker::Me,
                    Stream::System => Speaker::System,
                };
                if let Some(l) = &event.lang {
                    if self.langs.insert(l.clone()) {
                        self.doc.fm.languages.push(l.clone());
                    }
                }
                let cleaned = lowk_types::text::clean_speech(&event.text);
                if cleaned.is_empty() {
                    return Ok(());
                }
                let pieces = segment(&cleaned, at, end);
                let key = (event.stream, event.utt);
                if let Some(previous) = self.finals.remove(&key) {
                    let mut rendered = Vec::with_capacity(pieces.len());
                    for (i, (text, at)) in pieces.iter().enumerate() {
                        rendered.push((*at, sp.clone(), text.clone()));
                        if let Some((old_at, old_sp, old_text)) = previous.get(i) {
                            if let Some(Block::Turn { text: current, .. }) = self.doc.blocks_mut().iter_mut().find(|b| {
                                matches!(b, Block::Turn { at_s, speaker, text } if at_s == old_at && speaker == old_sp && text.contains(old_text))
                            }) {
                                *current = current.replacen(old_text, text, 1);
                            } else {
                                self.add_turn(sp.clone(), *at, end, text);
                            }
                        } else {
                            self.add_turn(sp.clone(), *at, end, text);
                        }
                    }
                    self.finals.insert(key, rendered);
                    self.last_end = self.last_end.max(end);
                } else {
                    let mut rendered = Vec::with_capacity(pieces.len());
                    for (text, at) in pieces {
                        self.add_turn(sp.clone(), at, end, &text);
                        rendered.push((at, sp.clone(), text));
                    }
                    self.finals.insert(key, rendered);
                }
            }
            CallEntry::Qa {
                id,
                at_s,
                template,
                model,
                range,
                question,
                answer,
            } => {
                self.doc.blocks_mut().push(Block::Qa(QaBlock {
                    id: id.to_string(),
                    at_s: *at_s,
                    template: template.clone(),
                    model: model.clone(),
                    range: range.clone(),
                    extra_attrs: vec![],
                    question: question.clone(),
                    answer: answer.clone(),
                    my_answer: None,
                }));
            }
            CallEntry::Image {
                at_s,
                rel_path,
                source,
                caption,
            } => {
                if source.starts_with("ask:") {
                    if let Some(Block::Image(existing)) = self
                        .doc
                        .blocks_mut()
                        .iter_mut()
                        .find(|b| matches!(b, Block::Image(i) if i.rel_path == *rel_path))
                    {
                        existing.source = source.clone();
                        existing.caption = caption.clone();
                        return Ok(());
                    }
                }
                self.doc.blocks_mut().push(Block::Image(ImageBlock {
                    rel_path: rel_path.clone(),
                    at_s: *at_s,
                    source: source.clone(),
                    caption: caption.clone(),
                    callout: true,
                }));
            }
            CallEntry::Marker { at_s, text } => {
                for s in [
                    lowk_types::Scenario::Listen,
                    lowk_types::Scenario::Assist,
                    lowk_types::Scenario::Run,
                ] {
                    if *text == format!("Сценарий: {}", s.name_ru()) {
                        self.doc.fm.scenario = Some(s);
                    }
                }
                self.doc.blocks_mut().push(Block::Marker {
                    at_s: *at_s,
                    text: text.clone(),
                });
            }
            CallEntry::MyAnswer { qa_id, at_s, text } => {
                let id = qa_id.to_string();
                let text = &lowk_types::text::clean_speech(text);
                let q = self.doc.blocks_mut().iter_mut().rev().find_map(|b| match b {
                    Block::Qa(q) if q.id == id => Some(q),
                    _ => None,
                });
                let q = q.ok_or_else(|| err(ErrorCode::NotFound, format!("no Q&A {id}")))?;
                match &mut q.my_answer {
                    Some((_, t)) => {
                        t.push(' ');
                        t.push_str(text);
                    }
                    None => q.my_answer = Some((*at_s, text.to_string())),
                }
            }
        }
        Ok(())
    }

    /// Adds one finished line, ordered by audio time (S6) and merged into the previous one only
    /// while that is the same speaker, mid-sentence, close in time and short (segmentation).
    fn add_turn(&mut self, sp: Speaker, at: u32, end: u32, text: &str) {
        let (last_end, _last_at) = (self.last_end, self.last_at);
        let blocks = self.doc.blocks_mut();
        // S6: lines are ordered by audio time, not by when their final arrived. A Me final
        // that lands after a later System line (or after a Q&A block asked meanwhile)
        // goes back where it was spoken.
        let block_at = |b: &Block| match b {
            Block::Turn { at_s, .. } => Some(*at_s),
            Block::Qa(q) => Some(q.at_s),
            Block::Image(i) => Some(i.at_s),
            Block::Marker { at_s, .. } => Some(*at_s),
            Block::Raw(_) => None,
        };
        let mut pos = blocks.len();
        while pos > 0 && block_at(&blocks[pos - 1]).is_some_and(|t| t > at) {
            pos -= 1;
        }
        if pos < blocks.len() {
            blocks.insert(
                pos,
                Block::Turn {
                    at_s: at,
                    speaker: sp,
                    text: text.to_string(),
                },
            );
            self.last_end = self.last_end.max(end);
            return;
        }
        if let Some(Block::Turn {
            at_s,
            speaker,
            text: t,
        }) = blocks.last_mut()
        {
            if *speaker == sp
                && at.saturating_sub(last_end) <= MERGE_GAP_S
                && end.saturating_sub(*at_s) <= MAX_TURN_S
                && !t.ends_with(['.', '?', '!', '…'])
            {
                t.push(' ');
                t.push_str(text);
                self.last_end = end.max(last_end);
                return;
            }
        }
        blocks.push(Block::Turn {
            at_s: at,
            speaker: sp,
            text: text.to_string(),
        });
        self.last_end = end;
        self.last_at = at;
    }

    fn last_offset(&self) -> u32 {
        let mut m = self.last_end.max(self.last_at);
        for b in self.doc.blocks() {
            match b {
                Block::Qa(q) => m = m.max(q.at_s).max(q.my_answer.as_ref().map_or(0, |a| a.0)),
                Block::Image(i) => m = m.max(i.at_s),
                _ => {}
            }
        }
        m
    }
}

pub struct CallWriter {
    v: Arc<VaultInner>,
    id: Ulid,
    path: PathBuf,
    journal_path: PathBuf,
    journal: File,
    m: Model,
    last_sync: Instant,
    last_mat: Instant,
    last_heartbeat: u32,
    unsynced: bool,
    written_hash: Option<String>,
    notice: Option<WriterNotice>,
    done: bool,
}

impl CallWriter {
    pub(crate) fn create(
        v: &Vault,
        id: Ulid,
        new: NewCall,
        clock: SessionClock,
        path: PathBuf,
        folder_title: String,
    ) -> Result<Self> {
        let inner: Arc<VaultInner> = v.arc();
        let rel = rel_string(&inner.root, &path);
        let journal_path = inner.live_dir().join(format!("{id}.jsonl"));
        fs::create_dir_all(inner.live_dir())?;
        let journal = OpenOptions::new().create(true).append(true).open(&journal_path)?;
        let m = Model::new(id, &new, clock, &folder_title);
        let mut w = Self {
            v: inner.clone(),
            id,
            path,
            journal_path,
            journal,
            m,
            last_sync: Instant::now(),
            last_mat: Instant::now(),
            last_heartbeat: 0,
            unsynced: false,
            written_hash: None,
            notice: None,
            done: false,
        };
        w.append(&Line::Begin {
            id,
            new,
            clock,
            rel,
            folder_title,
        })?;
        w.sync()?;
        inner.live.lock().insert(id);
        w.write_md()?;
        Ok(w)
    }

    pub fn id(&self) -> Ulid {
        self.id
    }

    /// Vault-relative path of the call file.
    pub fn rel_path(&self) -> String {
        rel_string(&self.v.root, &self.path)
    }

    pub fn take_notice(&mut self) -> Option<WriterNotice> {
        self.notice.take()
    }

    fn append(&mut self, l: &Line) -> Result<()> {
        let mut s = serde_json::to_string(l).map_err(|e| err(ErrorCode::Io, e.to_string()))?;
        s.push('\n');
        self.journal.write_all(s.as_bytes())?;
        self.unsynced = true;
        Ok(())
    }

    fn sync(&mut self) -> Result<()> {
        self.journal.flush()?;
        self.journal.sync_data()?;
        self.unsynced = false;
        self.last_sync = Instant::now();
        Ok(())
    }

    /// Writes `bytes` as `<call stem>.assets/<name>` next to the call file and returns the link used in
    /// the call (`<stem>.assets/<name>`, relative to the call's folder, as in `![[...]]`). Moving or
    /// renaming the call moves the folder and rewrites the embeds (`Vault::move_with_assets`).
    pub fn save_asset(&mut self, name: &str, bytes: &[u8]) -> Result<String> {
        let clean: String = name
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        let stem = self
            .path
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or_else(|| err(ErrorCode::InvalidArgument, "call path"))?
            .to_string();
        let dir = self.path.with_file_name(format!("{stem}.assets"));
        fs::create_dir_all(&dir)?;
        fs::write(dir.join(&clean), bytes)?;
        Ok(format!("{stem}.assets/{clean}"))
    }

    /// Append an entry. Partial STT events are ignored. Flush immediately; fsync is
    /// rate-limited to at most once every ~2 s.
    /// the `.md` is re-materialized every ~20 s.
    pub fn push(&mut self, e: CallEntry) -> Result<()> {
        if let CallEntry::Final { event } = &e {
            if event.kind != SttKind::Final {
                return Ok(());
            }
        }
        self.m.apply(&e)?;
        self.append(&Line::Entry { e })?;
        self.journal.flush()?;
        if self.last_sync.elapsed() >= SYNC_EVERY {
            self.sync()?;
        }
        if self.last_mat.elapsed() >= MATERIALIZE_EVERY {
            self.materialize()?;
        }
        Ok(())
    }

    /// Journal call progress even when no speech or other content is emitted.
    /// Heartbeats are coalesced to one per 2 s and share the journal's rate-limited fsync.
    pub fn heartbeat(&mut self, offset_s: u32) -> Result<()> {
        if offset_s < self.last_heartbeat.saturating_add(2) {
            return Ok(());
        }
        self.append(&Line::Heartbeat { offset_s })?;
        self.journal.flush()?;
        if self.last_sync.elapsed() >= SYNC_EVERY {
            self.sync()?;
        }
        self.last_heartbeat = offset_s;
        Ok(())
    }

    fn write_md(&mut self) -> Result<()> {
        // a title typed during the recording: the file already carries it (written by the vault)
        let pending = self.v.pending_titles.lock().get(&self.id).cloned();
        // external edit detection: the file changed since our last write
        if let (Some(h), Ok(cur)) = (&self.written_hash, fs::read(&self.path)) {
            let ours = pending.as_ref().is_some_and(|(_, ph)| *ph == hash_bytes(&cur));
            if !ours && hash_bytes(&cur) != *h {
                let ext = call_codec::parse(&String::from_utf8_lossy(&cur));
                if ext.fm.id == self.id.to_string() {
                    let mine = std::mem::take(self.m.doc.blocks_mut());
                    let mut merged = ext;
                    *merged.blocks_mut() = mine;
                    // keep what the machine owns
                    merged.fm.ended = self.m.doc.fm.ended.clone();
                    merged.fm.duration_s = self.m.doc.fm.duration_s;
                    merged.fm.languages = self.m.doc.fm.languages.clone();
                    self.m.doc = merged;
                    self.notice = Some(WriterNotice::ExternalEditMerged);
                }
            }
        }
        if let Some((t, _)) = pending {
            self.m.doc.fm.title = t.clone();
            self.m.doc.heading = t;
        }
        let text = call_codec::render(&self.m.doc);
        atomic_write(&self.path, text.as_bytes())?;
        self.written_hash = Some(hash_bytes(text.as_bytes()));
        self.last_mat = Instant::now();
        Ok(())
    }

    /// Atomically write the current state to the `.md`.
    pub fn materialize(&mut self) -> Result<()> {
        if self.unsynced {
            self.sync()?;
        }
        self.write_md()
    }

    fn meta_now(&self) -> Result<CallMeta> {
        self.v
            .rescan_meta(&self.id)
            .ok_or_else(|| err(ErrorCode::NotFound, "finished call not found"))
    }

    pub fn finish(self) -> Result<CallMeta> {
        let ended = now_in_offset_of(&self.m.doc.fm.started);
        self.finish_at(ended)
    }

    /// `finish` with an explicit end time (RFC 3339), for deterministic callers.
    pub fn finish_at(mut self, ended: String) -> Result<CallMeta> {
        self.m.doc.fm.duration_s = diff_secs(&self.m.doc.fm.started, &ended).or(Some(0));
        self.m.doc.fm.ended = Some(ended.clone());
        self.append(&Line::End { ended })?;
        self.materialize()?;
        self.done = true;
        let _ = fs::remove_file(&self.journal_path);
        self.v.live.lock().remove(&self.id);
        self.v.resync()?;
        if self.v.pending_titles.lock().contains_key(&self.id) {
            self.v.rename_finished(&self.id)?;
            self.v.resync()?;
        }
        self.meta_now()
    }
}

impl Drop for CallWriter {
    fn drop(&mut self) {
        // An unfinished writer leaves its journal behind: that is the recovery path.
        if !self.done {
            let _ = self.journal.flush();
            let _ = self.journal.sync_data();
        }
        self.v.live.lock().remove(&self.id);
    }
}

/// A journal without an `End` line (crash / force quit).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Recoverable {
    pub id: Ulid,
    pub title: String,
    pub started: String,
    pub entries: usize,
}

type JournalRead = (Line, Vec<CallEntry>, Option<String>, Option<u32>);

fn read_journal(path: &std::path::Path) -> Option<JournalRead> {
    let f = File::open(path).ok()?;
    let mut begin = None;
    let mut entries = vec![];
    let mut end = None;
    let mut heartbeat: Option<u32> = None;
    for (i, l) in BufReader::new(f).lines().enumerate() {
        let Ok(l) = l else { break };
        // a torn trailing line (crash mid-write) is skipped
        let Ok(line) = serde_json::from_str::<Line>(&l) else {
            continue;
        };
        match (i, line) {
            (0, b @ Line::Begin { .. }) => begin = Some(b),
            (_, Line::Entry { e }) => entries.push(e),
            (_, Line::Heartbeat { offset_s }) => {
                heartbeat = Some(heartbeat.unwrap_or_default().max(offset_s));
            }
            (_, Line::End { ended }) => end = Some(ended),
            _ => {}
        }
    }
    Some((begin?, entries, end, heartbeat))
}

impl Vault {
    /// Journals of calls that never finished and are not being recorded now.
    pub fn recoverable_calls(&self) -> Result<Vec<Recoverable>> {
        let mut out = vec![];
        let Ok(rd) = fs::read_dir(self.live_dir()) else {
            return Ok(out);
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().is_none_or(|x| x != "jsonl") || p.to_string_lossy().ends_with(".chat.jsonl") {
                continue;
            }
            if let Some((Line::Begin { id, new, .. }, entries, None, _)) = read_journal(&p) {
                if !self.live.lock().contains(&id) {
                    out.push(Recoverable {
                        id,
                        title: new.title,
                        started: new.started,
                        entries: entries.len(),
                    });
                }
            }
        }
        out.sort_by(|a, b| a.started.cmp(&b.started));
        Ok(out)
    }

    /// Rebuild the `.md` from the journal and mark the call finished at the
    /// later of its last content offset and last heartbeat.
    pub fn recover_call(&self, id: &Ulid) -> Result<CallMeta> {
        let _g = self.op_lock();
        if self.live.lock().contains(id) {
            return Err(err(ErrorCode::SessionBusy, "call is being recorded"));
        }
        let jp = self.live_dir().join(format!("{id}.jsonl"));
        let (begin, entries, _, heartbeat) =
            read_journal(&jp).ok_or_else(|| err(ErrorCode::NotFound, "no journal for call"))?;
        let Line::Begin {
            id: jid,
            new,
            clock,
            rel,
            folder_title,
        } = begin
        else {
            return Err(err(ErrorCode::FormatInvalid, "journal has no header"));
        };
        let mut m = Model::new(jid, &new, clock, &folder_title);
        for e in &entries {
            m.apply(e)?;
        }
        let duration = m.last_offset().max(heartbeat.unwrap_or_default());
        let ended = add_secs(&new.started, duration as u64);
        m.doc.fm.duration_s = Some(duration as u64);
        m.doc.fm.ended = Some(ended);
        let mut path = self.root.join(&rel);
        // keep user edits made to an already materialized file
        if let Ok(cur) = fs::read_to_string(&path) {
            let ext = call_codec::parse(&cur);
            if ext.fm.id == jid.to_string() {
                let mine = std::mem::take(m.doc.blocks_mut());
                let (ended, dur, langs) = (
                    m.doc.fm.ended.clone(),
                    m.doc.fm.duration_s,
                    m.doc.fm.languages.clone(),
                );
                m.doc = ext;
                *m.doc.blocks_mut() = mine;
                m.doc.fm.ended = ended;
                m.doc.fm.duration_s = dur;
                m.doc.fm.languages = langs;
            }
        } else if let Some(found) = self.scan_find_path(&jid) {
            path = found;
        }
        atomic_write(&path, call_codec::render(&m.doc).as_bytes())?;
        fs::remove_file(&jp)?;
        self.resync()?;
        self.rescan_meta(&jid)
            .ok_or_else(|| err(ErrorCode::NotFound, "recovered call not found"))
    }

    /// Startup reconcile: a call left in the recording state (`ended` unset) that no live writer owns
    /// (the app crashed or was killed) is finished. A leftover journal is replayed (`recover_call`);
    /// without one the end is the last timestamp found in the file. Returns the ids that were finished.
    pub fn reconcile_stale_recordings(&self) -> Result<Vec<Ulid>> {
        Ok(self.reconcile_recordings()?.into_iter().map(|r| r.id).collect())
    }

    /// Same as `reconcile_stale_recordings`, with what the user needs to be told about each call:
    /// its title and how much was saved.
    pub fn reconcile_recordings(&self) -> Result<Vec<RecoveredCall>> {
        let mut done = vec![];
        let note = |m: &CallMeta| RecoveredCall {
            id: m.id,
            title: m.title.clone(),
            duration_s: m.duration_s.unwrap_or(0),
        };
        for r in self.recoverable_calls()? {
            if let Ok(m) = self.recover_call(&r.id) {
                done.push(note(&m));
            }
        }
        for c in self.calls(None)? {
            if c.ended.is_some() || self.live.lock().contains(&c.id) {
                continue;
            }
            let upd = self.update_call(&c.id, |d| {
                let last = d
                    .blocks()
                    .iter()
                    .map(|b| match b {
                        Block::Turn { at_s, .. } | Block::Marker { at_s, .. } => *at_s,
                        Block::Qa(q) => q.at_s.max(q.my_answer.as_ref().map_or(0, |a| a.0)),
                        Block::Image(i) => i.at_s,
                        _ => 0,
                    })
                    .max()
                    .unwrap_or(0);
                d.fm.ended = Some(add_secs(&d.fm.started, u64::from(last)));
                d.fm.duration_s = Some(u64::from(last));
            });
            if let Ok(m) = upd {
                done.push(note(&m));
            }
        }
        Ok(done)
    }

    /// Drop a journal without recovering it (user chose "discard").
    pub fn discard_recovery(&self, id: &Ulid) -> Result<()> {
        let jp = self.live_dir().join(format!("{id}.jsonl"));
        if self.live.lock().contains(id) {
            return Err(err(ErrorCode::SessionBusy, "call is being recorded"));
        }
        if jp.exists() {
            fs::remove_file(jp)?;
        }
        Ok(())
    }
}

// silence unused warnings for items only used by the codec re-exports
const _: fn() = || {
    let _ = fm::quote;
    let _ = Part::Other(String::new(), String::new());
};

#[cfg(test)]
mod segment_tests {
    use super::*;

    fn test_call() -> NewCall {
        NewCall {
            title: "Crash recovery".into(),
            folder: FolderRef::Unsorted,
            started: "2026-11-04T13:00:00Z".into(),
            sources: CallSources {
                me: None,
                system: None,
            },
            stt_engine: Some("test".into()),
            languages: vec![],
            speaker_me: "Me".into(),
            speaker_system: "System".into(),
            clock: None,
        }
    }

    #[test]
    fn dropped_writer_recovers_finals_and_duration_from_recent_journal() {
        let dir = tempfile::tempdir().unwrap();
        let vault = Vault::create_in(dir.path()).unwrap();
        let clock = SessionClock {
            t0_mono: 0,
            t0_wall_ms: 1_794_000_000_000,
        };
        let mut writer = vault.begin_call_with_clock(test_call(), clock).unwrap();
        let id = writer.id();
        for (sec, text) in [(1u64, "first final"), (2, "second final"), (5, "third final")] {
            writer
                .push(CallEntry::Final {
                    event: SttEvent {
                        stream: Stream::System,
                        utt: sec,
                        rev: 1,
                        kind: SttKind::Final,
                        text: text.into(),
                        lang: None,
                        lang_src: None,
                        t_start_ns: sec * 1_000_000_000,
                        t_audio_end_ns: sec * 1_000_000_000,
                        t_emit_ns: 0,
                    },
                })
                .unwrap();
            std::thread::sleep(Duration::from_millis(1_050));
        }
        std::thread::sleep(Duration::from_secs(2)); // Every final is now over 2 s old.
        std::mem::forget(writer); // Simulates SIGKILL: neither finish nor Drop runs.
        drop(vault);
        let vault = Vault::open(dir.path()).unwrap();

        let recovered = vault.recover_call(&id).unwrap();
        let doc = vault.read_call_doc(&id).unwrap();
        let text = doc
            .blocks()
            .iter()
            .filter_map(|b| match b {
                Block::Turn { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(" ");
        assert!(
            text.contains("first final") && text.contains("second final") && text.contains("third final"),
            "{text}"
        );
        assert!(recovered.duration_s.unwrap_or_default().abs_diff(5) <= 2);
    }

    fn recover_silent_tail(engine: &str) -> u64 {
        let dir = tempfile::tempdir().unwrap();
        let vault = Vault::create_in(dir.path()).unwrap();
        let mut new = test_call();
        new.stt_engine = Some(engine.into());
        let clock = SessionClock {
            t0_mono: 0,
            t0_wall_ms: 1_794_000_000_000,
        };
        let mut writer = vault.begin_call_with_clock(new, clock).unwrap();
        let id = writer.id();
        writer
            .push(CallEntry::Final {
                event: SttEvent {
                    stream: Stream::System,
                    utt: 1,
                    rev: 0,
                    kind: SttKind::Final,
                    text: "last words".into(),
                    lang: None,
                    lang_src: None,
                    t_start_ns: 1_000_000_000,
                    t_audio_end_ns: 1_000_000_000,
                    t_emit_ns: 1_000_000_000,
                },
            })
            .unwrap();
        // The call then runs silently for several seconds. Heartbeats are the only durable evidence
        // after the last final, as happens in both demo replay and an idle local engine.
        for offset in [2, 4, 6] {
            writer.heartbeat(offset).unwrap();
        }
        std::mem::forget(writer);
        drop(vault);
        Vault::open(dir.path())
            .unwrap()
            .recover_call(&id)
            .unwrap()
            .duration_s
            .unwrap()
    }

    #[test]
    fn demo_crash_with_silent_tail_recovers_duration_from_heartbeat() {
        assert!(recover_silent_tail("demo").abs_diff(7) <= 2);
    }

    #[test]
    fn fake_local_engine_crash_with_silent_tail_recovers_duration_from_heartbeat() {
        assert!(recover_silent_tail("fake-local").abs_diff(7) <= 2);
    }

    #[test]
    fn short_final_is_one_segment() {
        assert_eq!(
            segment("Привет. Как дела?", 3, 9),
            vec![("Привет. Как дела?".to_string(), 3)]
        );
    }

    #[test]
    fn long_monologue_is_split_at_sentences_with_increasing_stamps() {
        let t = "Спасибо, что нашёл время. Давай сразу к цифрам. Какой у вас сейчас ARR? А burn rate какой? \
                 Тогда какой runway? Четыре месяца — это мало. Вы уже в раунде? Давай про unit economics. \
                 LTV к CAC какой? А EBITDA когда будет положительной?";
        let segs = segment(t, 100, 145);
        assert!(segs.len() >= 3, "{segs:?}");
        assert!(segs.windows(2).all(|w| w[0].1 < w[1].1));
        assert_eq!(segs[0].1, 100);
        assert!(segs.iter().all(|s| s.0.ends_with(['.', '?', '!'])));
        assert_eq!(segs.iter().map(|s| s.0.as_str()).collect::<Vec<_>>().join(" "), t);
    }
}
