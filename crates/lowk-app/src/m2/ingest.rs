//! Document import queue: a small worker pool converts files (`lowk-ingest`), stores the Markdown
//! (`lowk-store`), and hands the result to the index feed. Progress, cancel, retry, and crash
//! recovery (documents left `Queued` / `Converting` are queued again at start).
use super::{Job, Services};
use lowk_ingest::{Ingestor, Limits};
use lowk_types::*;
use parking_lot::Mutex;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

const WORKERS: usize = 2;

pub(crate) struct Queue {
    tx: Sender<DocId>,
    in_flight: Mutex<HashSet<DocId>>,
    cancels: Mutex<HashMap<DocId, Arc<AtomicBool>>>,
}

impl Queue {
    pub(crate) fn new() -> (Self, Receiver<DocId>) {
        let (tx, rx) = channel();
        (
            Self {
                tx,
                in_flight: Mutex::new(HashSet::new()),
                cancels: Mutex::new(HashMap::new()),
            },
            rx,
        )
    }

    /// Queues a document once (a second request while it is queued or running is ignored).
    pub(crate) fn enqueue(&self, id: DocId) {
        if self.in_flight.lock().insert(id) {
            let _ = self.tx.send(id);
        }
    }

    fn cancel(&self, id: &DocId) -> bool {
        match self.cancels.lock().get(id) {
            Some(f) => {
                f.store(true, Ordering::SeqCst);
                true
            }
            None => false,
        }
    }
}

pub(crate) fn spawn_workers(svc: Weak<Services>, rx: Receiver<DocId>) {
    let rx = Arc::new(Mutex::new(rx));
    for n in 0..WORKERS {
        let (svc, rx) = (svc.clone(), rx.clone());
        std::thread::Builder::new()
            .name(format!("lowk-ingest-{n}"))
            .spawn(move || loop {
                // the lock is held while waiting: exactly one idle worker takes the next job
                let id = {
                    let g = rx.lock();
                    match g.recv() {
                        Ok(id) => id,
                        Err(_) => return,
                    }
                };
                let Some(svc) = svc.upgrade() else { return };
                svc.convert_doc(id);
                svc.ingest.in_flight.lock().remove(&id);
            })
            .expect("spawn ingest worker");
    }
}

impl Services {
    fn progress(&self, d: &Document, status: IngestStatus, done: u32, total: u32, eta: Option<u32>, error: Option<AppError>) {
        self.env.emit(AppEvent::Ingest(IngestProgress {
            doc: d.id,
            folder: d.folder.clone(),
            status,
            done,
            total,
            eta_s: eta,
            error,
        }));
    }

    /// After a crash: documents that never finished converting start again.
    pub(crate) fn recover_ingest(&self) {
        let Ok(folders) = self.vault.folders() else { return };
        for f in folders.iter().filter(|f| f.id != FolderRef::Trash) {
            for d in self.vault.list_docs(&f.id).unwrap_or_default() {
                if matches!(d.status, IngestStatus::Queued | IngestStatus::Converting) {
                    tracing::info!(doc = %d.id, status = ?d.status, "re-queueing an unfinished document");
                    let _ = self.vault.set_doc_status(&d.id, IngestStatus::Queued, None);
                    self.ingest.enqueue(d.id);
                }
            }
        }
    }

    pub fn import_docs(&self, files: &[NewDocument]) -> Result<Vec<Document>> {
        let mut out = vec![];
        let mut first_err = None;
        for n in files {
            match self.vault.add_doc(n) {
                Ok(d) => {
                    self.env.emit(AppEvent::Store(StoreChange {
                        kind: StoreChangeKind::Created,
                        id: d.id,
                        is_folder: false,
                    }));
                    self.progress(&d, IngestStatus::Queued, 0, 0, None, None);
                    self.ingest.enqueue(d.id);
                    out.push(d);
                }
                Err(e) => {
                    tracing::warn!(?e, file = %n.source_path, "document not imported");
                    first_err.get_or_insert(e);
                }
            }
        }
        match (out.is_empty(), first_err) {
            (true, Some(e)) => Err(e),
            _ => Ok(out),
        }
    }

    pub fn retry_doc(&self, id: &DocId) -> Result<Document> {
        let d = self
            .vault
            .set_doc_status(id, IngestStatus::Queued, None)?;
        self.progress(&d, IngestStatus::Queued, 0, 0, None, None);
        self.ingest.enqueue(*id);
        Ok(d)
    }

    pub fn cancel_doc(&self, id: &DocId) -> Result<()> {
        if self.ingest.cancel(id) {
            return Ok(()); // the worker marks it cancelled
        }
        let d = self.vault.get_doc(id)?;
        if d.status == IngestStatus::Queued {
            let d = self.vault.set_doc_status(id, IngestStatus::Cancelled, None)?;
            self.progress(&d, IngestStatus::Cancelled, 0, 0, None, Some(AppError::new(ErrorCode::IngestCancelled)));
        }
        Ok(())
    }

    fn convert_doc(&self, id: DocId) {
        let Ok(doc) = self.vault.get_doc(&id) else { return };
        if doc.deleted || !matches!(doc.status, IngestStatus::Queued | IngestStatus::Converting) {
            return; // cancelled or removed while it waited
        }
        let cancel = Arc::new(AtomicBool::new(false));
        self.ingest.cancels.lock().insert(id, cancel.clone());
        let r = self.convert_inner(&doc, &cancel);
        self.ingest.cancels.lock().remove(&id);
        match r {
            Ok(saved) => {
                self.progress(&saved, IngestStatus::Indexing, saved.pages.unwrap_or(0), saved.pages.unwrap_or(0), None, None);
                let _ = self.feed.send(Job::Item(id));
            }
            Err(e) if e.code == ErrorCode::IngestCancelled => {
                if let Ok(d) = self.vault.set_doc_status(&id, IngestStatus::Cancelled, None) {
                    self.progress(&d, IngestStatus::Cancelled, 0, 0, None, Some(e));
                }
            }
            Err(e) => {
                tracing::warn!(?e, doc = %id, "conversion failed");
                if let Ok(d) = self.vault.set_doc_status(&id, IngestStatus::Failed, Some(e.clone())) {
                    self.progress(&d, IngestStatus::Failed, 0, 0, None, Some(e));
                }
            }
        }
    }

    fn convert_inner(&self, doc: &Document, cancel: &Arc<AtomicBool>) -> Result<Document> {
        let s = self.settings();
        self.vault.set_doc_status(&doc.id, IngestStatus::Converting, None)?;
        self.progress(doc, IngestStatus::Converting, 0, 0, None, None);
        let (_, original) = self.vault.doc_paths(&doc.id)?;
        let ing = Ingestor::new().with_limits(Limits::from_mb(s.docs.warn_slow_mb, s.docs.max_file_mb));
        let req = IngestRequest {
            doc: doc.id,
            source_path: original.to_string_lossy().into_owned(),
            kind: doc.kind,
            languages: s.stt.languages.clone(),
            ocr: s.docs.ocr,
        };
        let t0 = Instant::now();
        let mut last = Instant::now() - Duration::from_secs(1);
        let mut on_progress = |done: u32, total: u32| {
            if last.elapsed() < Duration::from_millis(100) && done != total {
                return;
            }
            last = Instant::now();
            let eta = (done > 0 && total > done).then(|| {
                let per = t0.elapsed().as_secs_f32() / done as f32;
                (per * (total - done) as f32).ceil() as u32
            });
            self.progress(doc, IngestStatus::Converting, done, total, eta, None);
        };
        let out = lowk_types::DocIngestor::convert(&ing, &req, &mut on_progress, &|| !cancel.load(Ordering::SeqCst))?;
        self.vault.save_converted_doc(&doc.id, &out)
    }
}
