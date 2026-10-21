//! Tiny `Fake*` / `Noop*` implementations of the contract traits so dependents
//! can build and test before the real owners land.
use crate::*;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};

/// Runs everything inline on the calling thread.
#[derive(Default, Clone, Copy)]
pub struct InlineMainThread;
impl MainThread for InlineMainThread {
    fn run(&self, f: Box<dyn FnOnce() + Send>) {
        f()
    }
    fn is_main(&self) -> bool {
        true
    }
}

#[derive(Default, Clone, Copy)]
pub struct FixedCursor(pub f64, pub f64);
impl CursorSource for FixedCursor {
    fn position(&self) -> (f64, f64) {
        (self.0, self.1)
    }
}

#[derive(Default, Clone, Copy)]
pub struct NoopRetriever;
impl Retriever for NoopRetriever {
    fn retrieve(&self, _q: &RetrievalQuery) -> Result<Vec<ContextChunk>> {
        Ok(vec![])
    }
}

/// In-memory overlay controller that records mode/visibility.
pub struct NoopOverlayController {
    state: Mutex<(bool, OverlayMode)>,
}
impl Default for NoopOverlayController {
    fn default() -> Self {
        Self {
            state: Mutex::new((false, OverlayMode::Passive)),
        }
    }
}
impl NoopOverlayController {
    pub fn visible(&self) -> bool {
        self.state.lock().unwrap().0
    }
}
impl OverlayController for NoopOverlayController {
    fn show(&self) -> Result<()> {
        self.state.lock().unwrap().0 = true;
        Ok(())
    }
    fn hide(&self) {
        self.state.lock().unwrap().0 = false;
    }
    fn panic_hide(&self) {
        self.hide()
    }
    fn set_mode(&self, m: OverlayMode) -> Result<()> {
        self.state.lock().unwrap().1 = m;
        Ok(())
    }
    fn mode(&self) -> OverlayMode {
        self.state.lock().unwrap().1
    }
    fn move_hold(&self, _phase: Phase, _cursor: Arc<dyn CursorSource>) {}
    fn status(&self) -> StealthStatus {
        StealthStatus::Unavailable("noop controller".into())
    }
    fn selftest(&self) -> Result<SelfTestReport> {
        Err(AppError::not_implemented())
    }
}

/// In-memory settings store.
pub struct MemSettingsStore {
    cur: Mutex<Settings>,
    subs: Mutex<Vec<Sender<Settings>>>,
}
impl MemSettingsStore {
    pub fn new(s: Settings) -> Self {
        Self {
            cur: Mutex::new(s),
            subs: Mutex::new(vec![]),
        }
    }
}
impl Default for MemSettingsStore {
    fn default() -> Self {
        Self::new(Settings::default())
    }
}
impl SettingsStore for MemSettingsStore {
    fn get(&self) -> Settings {
        self.cur.lock().unwrap().clone()
    }
    fn patch(&self, p: SettingsPatch) -> Result<Settings> {
        let mut cur = self.cur.lock().unwrap();
        let next = cur.apply_patch(&p)?;
        *cur = next.clone();
        self.subs.lock().unwrap().retain(|s| s.send(next.clone()).is_ok());
        Ok(next)
    }
    fn subscribe(&self) -> Receiver<Settings> {
        let (tx, rx) = channel();
        self.subs.lock().unwrap().push(tx);
        rx
    }
}

/// Collects STT events and errors (tests).
#[derive(Default)]
pub struct CollectSttSink {
    pub events: Mutex<Vec<SttEvent>>,
    pub errors: Mutex<Vec<SttError>>,
}
impl SttSink for CollectSttSink {
    fn emit(&self, ev: SttEvent) {
        self.events.lock().unwrap().push(ev)
    }
    fn error(&self, e: SttError) {
        self.errors.lock().unwrap().push(e)
    }
}

/// Collects bus events (tests).
#[derive(Default)]
pub struct CollectEventSink(pub Mutex<Vec<AppEvent>>);
impl EventSink for CollectEventSink {
    fn emit(&self, ev: AppEvent) {
        self.0.lock().unwrap().push(ev)
    }
}

pub fn arc_sink() -> Arc<CollectSttSink> {
    Arc::new(CollectSttSink::default())
}

// --- M2 fakes (UI dev, WP integration tests) --------------------------------

/// In-memory `DocumentStore`: `add_doc` stores the file name only (the original is not read).
#[derive(Default)]
pub struct FakeDocumentStore {
    docs: Mutex<Vec<(Document, String)>>,
}
impl FakeDocumentStore {
    fn with<R>(&self, id: &DocId, f: impl FnOnce(&mut (Document, String)) -> R) -> Result<R> {
        let mut g = self.docs.lock().unwrap();
        g.iter_mut()
            .find(|(d, _)| d.id == *id)
            .map(f)
            .ok_or_else(|| AppError::new(ErrorCode::NotFound))
    }
}
impl DocumentStore for FakeDocumentStore {
    fn list_docs(&self, folder: &FolderRef) -> Result<Vec<Document>> {
        let g = self.docs.lock().unwrap();
        Ok(g.iter()
            .filter(|(d, _)| &d.folder == folder && !d.deleted)
            .map(|(d, _)| d.clone())
            .collect())
    }
    fn get_doc(&self, id: &DocId) -> Result<Document> {
        self.with(id, |(d, _)| d.clone())
    }
    fn add_doc(&self, n: &NewDocument) -> Result<Document> {
        let path = std::path::Path::new(&n.source_path);
        let file_name = path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let kind = path
            .extension()
            .and_then(|e| e.to_str())
            .and_then(DocKind::from_extension)
            .ok_or_else(|| AppError::new(ErrorCode::DocUnsupported))?;
        let doc = Document {
            id: Ulid::new(),
            folder: n.folder.clone(),
            title: n.title.clone().unwrap_or_else(|| file_name.clone()),
            kind,
            file_name: file_name.clone(),
            md_path: format!("Docs/{file_name}.md"),
            original_path: format!("Docs/{file_name}"),
            size_bytes: 0,
            pages: None,
            status: IngestStatus::Queued,
            error: None,
            warnings: vec![],
            pinned: n.pin,
            tokens: 0,
            tags: n.tags.clone(),
            added: "2026-01-01T00:00:00+00:00".into(),
            source_sha256: String::new(),
            converter: String::new(),
            deleted: false,
        };
        self.docs.lock().unwrap().push((doc.clone(), String::new()));
        Ok(doc)
    }
    fn set_status(&self, id: &DocId, status: IngestStatus, error: Option<AppError>) -> Result<Document> {
        self.with(id, |(d, _)| {
            d.status = status;
            d.error = error;
            d.clone()
        })
    }
    fn save_converted(&self, id: &DocId, doc: &IngestedDoc) -> Result<Document> {
        self.with(id, |(d, md)| {
            *md = doc.md.clone();
            d.pages = doc.pages;
            d.warnings = doc.warnings.clone();
            d.converter = doc.converter.clone();
            d.tokens = (doc.md.len() / 4) as u32;
            d.status = IngestStatus::Indexing;
            d.clone()
        })
    }
    fn read_md(&self, id: &DocId) -> Result<String> {
        self.with(id, |(_, md)| md.clone())
    }
    fn save_md(&self, id: &DocId, new_md: &str) -> Result<Document> {
        self.with(id, |(d, md)| {
            *md = new_md.to_string();
            d.clone()
        })
    }
    fn patch_doc(&self, id: &DocId, p: &DocPatch) -> Result<Document> {
        self.with(id, |(d, _)| {
            if let Some(t) = &p.title {
                d.title = t.clone();
            }
            if let Some(t) = &p.tags {
                d.tags = t.clone();
            }
            if let Some(x) = p.pinned {
                d.pinned = x;
            }
            if let Some(f) = &p.folder {
                d.folder = f.clone();
            }
            d.clone()
        })
    }
    fn delete_doc(&self, id: &DocId) -> Result<()> {
        self.with(id, |(d, _)| d.deleted = true)
    }
    fn restore_doc(&self, id: &DocId) -> Result<Document> {
        self.with(id, |(d, _)| {
            d.deleted = false;
            d.clone()
        })
    }
}

/// Converts any supported file into a one-page Markdown stub (never reads the file).
#[derive(Default, Clone, Copy)]
pub struct FakeIngestor;
impl DocIngestor for FakeIngestor {
    fn supports(&self, _kind: DocKind) -> bool {
        true
    }
    fn convert(
        &self,
        req: &IngestRequest,
        progress: &mut dyn FnMut(u32, u32),
        should_continue: &dyn Fn() -> bool,
    ) -> Result<IngestedDoc> {
        progress(0, 1);
        if !should_continue() {
            return Err(AppError::new(ErrorCode::IngestCancelled));
        }
        progress(1, 1);
        Ok(IngestedDoc {
            md: format!("<!-- p:1 -->\n# {}\n\nFake converted text.\n", req.source_path),
            pages: Some(1),
            warnings: vec![],
            converter: "fake".into(),
            assets: vec![],
        })
    }
}

/// Source label used by the fake retriever and search.
pub fn fake_source(kind: RefKind, title: &str) -> SourceRef {
    SourceRef {
        kind,
        id: Ulid::new(),
        folder: FolderRef::Unsorted,
        title: title.into(),
        at_s: matches!(kind, RefKind::Call).then_some(765),
        page: matches!(kind, RefKind::Doc).then_some(12),
        heading_path: vec![],
        chunk_id: None,
        pinned: false,
    }
}

/// Search over a fixed list of snippets (case-insensitive substring match).
pub struct FakeSearchIndex {
    pub items: Vec<(ChunkKind, SourceRef, String)>,
}
impl Default for FakeSearchIndex {
    fn default() -> Self {
        Self {
            items: vec![
                (
                    ChunkKind::Doc,
                    fake_source(RefKind::Doc, "Spec.pdf"),
                    "EBITDA is earnings before interest, taxes, depreciation".into(),
                ),
                (
                    ChunkKind::Speech,
                    fake_source(RefKind::Call, "Weekly sync"),
                    "А что значит EBITDA в вашем отчёте за квартал?".into(),
                ),
            ],
        }
    }
}
impl SearchIndex for FakeSearchIndex {
    fn search(&self, q: &SearchQuery) -> Result<SearchResults> {
        let needle = q.text.to_lowercase();
        let hits: Vec<SearchHit> = self
            .items
            .iter()
            .enumerate()
            .filter(|(_, (_, _, t))| t.to_lowercase().contains(&needle))
            .map(|(i, (k, s, t))| SearchHit {
                chunk_id: format!("fake-{i}"),
                kind: *k,
                source: s.clone(),
                folder_title: "Unsorted".into(),
                snippet: t.clone(),
                highlights: t
                    .to_lowercase()
                    .find(&needle)
                    .map(|b| {
                        let s0 = t[..b].encode_utf16().count() as u32;
                        vec![(s0, s0 + needle.encode_utf16().count() as u32)]
                    })
                    .unwrap_or_default(),
                tags: vec![],
                when: None,
                score: 1.0,
            })
            .take(q.limit.max(1) as usize)
            .collect();
        Ok(SearchResults {
            total: hits.len() as u32,
            hits,
            took_ms: 1,
            mode_used: SearchMode::Keyword,
            facets: vec![],
        })
    }
    fn status(&self) -> IndexStatus {
        IndexStatus {
            files: 2,
            chunks: self.items.len() as u32,
            pending: 0,
            semantic: SemanticState::Off,
            embedder_model: None,
            last_error: None,
        }
    }
    fn reindex(&self, _scope: ReindexScope) -> Result<()> {
        Ok(())
    }
    fn upsert(&self, _item: &IndexItem) -> Result<()> {
        Ok(())
    }
    fn upsert_folder(&self, _folder: &IndexFolder) -> Result<()> {
        Ok(())
    }
    fn known(&self) -> Result<Vec<(Ulid, String)>> {
        Ok(vec![])
    }
    fn remove(&self, _id: &Ulid, _purge: bool) -> Result<()> {
        Ok(())
    }
}

/// Canned summary and tags (no network).
#[derive(Default, Clone, Copy)]
pub struct FakeLlmService;
impl LlmService for FakeLlmService {
    fn summarize(
        &self,
        input: &SummarizeInput,
        on_delta: &mut dyn FnMut(&str),
        _should_continue: &dyn Fn() -> bool,
    ) -> Result<Summary> {
        let md = "- Fake point one\n- Fake point two\n".to_string();
        on_delta(&md);
        Ok(Summary {
            call: input.call,
            key_points: vec!["Fake point one".into(), "Fake point two".into()],
            decisions: vec![],
            tasks: vec![SummaryTask {
                who: Some("me".into()),
                what: "Send the offer".into(),
            }],
            questions: vec![],
            md,
            model: "fake".into(),
            generated_at: "2026-01-01T00:00:00+00:00".into(),
        })
    }
    fn suggest_tags(&self, input: &TagInput) -> Result<Vec<TagSuggestion>> {
        let mut v = vec![TagSuggestion {
            tag: "sales".into(),
            confidence: 0.8,
            existing: input.vocabulary.iter().any(|t| t == "sales"),
            reason: None,
        }];
        v.truncate(input.max.max(1) as usize);
        Ok(v)
    }
}

/// Retriever that returns one instructions chunk, one pinned doc and one relevant chunk, so the
/// Ask flow and the sources UI can be exercised.
#[derive(Default, Clone, Copy)]
pub struct FakeRetriever;
impl Retriever for FakeRetriever {
    fn retrieve(&self, _q: &RetrievalQuery) -> Result<Vec<ContextChunk>> {
        Ok(vec![chunk(
            ChunkKind::Doc,
            "Spec.pdf",
            "EBITDA is earnings before interest, taxes, depreciation.",
            false,
        )])
    }
    fn retrieve_pack(&self, q: &RetrievalQuery) -> Result<ContextPack> {
        Ok(ContextPack {
            instructions: q.include_folder_context.then(|| {
                chunk(
                    ChunkKind::Doc,
                    "Instructions",
                    "Client is a fintech; answer formally.",
                    false,
                )
            }),
            pinned: if q.include_folder_context {
                vec![chunk(
                    ChunkKind::Doc,
                    "Glossary.md",
                    "ARR: annual recurring revenue.",
                    true,
                )]
            } else {
                vec![]
            },
            relevant: self.retrieve(q)?,
            withheld: 0,
            pinned_dropped: 0,
        })
    }
}
fn chunk(kind: ChunkKind, title: &str, text: &str, pinned: bool) -> ContextChunk {
    let mut source_ref = fake_source(
        if kind == ChunkKind::Speech {
            RefKind::Call
        } else {
            RefKind::Doc
        },
        title,
    );
    source_ref.pinned = pinned;
    ContextChunk {
        id: format!("fake:{title}"),
        kind,
        text: text.into(),
        source: title.into(),
        heading_path: vec![],
        page: source_ref.page,
        score: 1.0,
        source_ref,
        pinned,
        tokens: (text.len() / 4) as u32,
    }
}
