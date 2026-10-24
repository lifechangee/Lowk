//! Post-call summary and tag suggestions (`lowk-llm`), provider status, key test, and the one-time
//! latency probe that pins the fastest upstream provider. A folder with `allow_cloud = false`
//! is summarised by the local model or not at all (`LocalOnlyBlocked`), never by the cloud.
use super::route::{self, Purpose};
use super::{folder_allows_cloud, Services};
use lowk_llm::{CloudLlm, ProbeOptions};
use lowk_store::call_codec::{self, Block, Speaker};
use lowk_types::*;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

impl Services {
    fn local_only_call(&self, meta: &CallMeta) -> bool {
        !folder_allows_cloud(&self.vault, &meta.folder)
    }

    fn summary_route(&self, meta: &CallMeta) -> Result<route::Route> {
        route::resolve(
            &self.settings().llm,
            self.local_only_call(meta),
            self.env.keys.openrouter(),
            &Purpose::Summary,
        )
    }

    fn summarize_input(&self, id: &CallId) -> Result<(CallMeta, SummarizeInput)> {
        let call = self.vault.read_call(id)?;
        let doc = call_codec::parse(&call.md);
        let name = |s: &Speaker| match s {
            Speaker::Me => doc.fm.speaker_me.clone(),
            Speaker::System => doc.fm.speaker_system.clone(),
            Speaker::Other(n) => n.clone(),
        };
        let mut transcript = String::new();
        let mut qa = vec![];
        for b in doc.blocks() {
            match b {
                Block::Turn { at_s, speaker, text } => {
                    transcript.push_str(&format!(
                        "[{}] **{}:** {}\n",
                        fmt_offset(u64::from(*at_s)),
                        name(speaker),
                        text
                    ));
                }
                Block::Qa(q) => qa.push(QaDigest {
                    question: q.question.clone(),
                    hint: q.answer.clone(),
                    my_answer: q
                        .my_answer
                        .as_ref()
                        .map(|m| m.1.clone())
                        .filter(|t| !t.trim().is_empty()),
                }),
                _ => {}
            }
        }
        let instructions = match &call.meta.folder {
            FolderRef::User(u) => self
                .vault
                .folder_doc(u)
                .map(|d| d.instructions().to_string())
                .ok()
                .filter(|t| !t.trim().is_empty()),
            _ => None,
        };
        let input = SummarizeInput {
            call: *id,
            title: call.meta.title.clone(),
            transcript,
            qa,
            languages: call.meta.languages.clone(),
            output_language: None,
            folder_instructions: instructions,
        };
        Ok((call.meta, input))
    }

    /// Starts the summary in the background: streams `summary://event`; on success the Markdown is
    /// written into `## Summary` of the call file.
    pub fn summarize(self: &Arc<Self>, id: CallId) -> Result<()> {
        let (meta, input) = self.summarize_input(&id)?;
        // refuse before spawning: no key / local-only without a local model is an immediate error
        let route = self.summary_route(&meta)?;
        let cancel = Arc::new(AtomicBool::new(false));
        if self.summaries.lock().insert(id, cancel.clone()).is_some() {
            return Err(AppError::new(ErrorCode::SessionBusy).with_detail("summary already running"));
        }
        let svc = self.clone();
        std::thread::Builder::new()
            .name("lowk-summary".into())
            .spawn(move || {
                svc.env.emit(AppEvent::Summary(SummaryEvent::Start { call: id }));
                let llm = CloudLlm::new(route.cfg);
                let r = llm.summarize_with_progress(
                    &input,
                    &mut |t| {
                        svc.env.emit(AppEvent::Summary(SummaryEvent::Delta {
                            call: id,
                            text: t.to_string(),
                        }))
                    },
                    &mut |step, total| {
                        svc.env.emit(AppEvent::Summary(SummaryEvent::Progress {
                            call: id,
                            step,
                            total,
                        }))
                    },
                    &|| !cancel.load(Ordering::SeqCst),
                );
                svc.summaries.lock().remove(&id);
                match r {
                    Ok(summary) => match svc.vault.set_call_summary(&id, &summary.md) {
                        Ok(_) => {
                            svc.env.emit(AppEvent::Store(StoreChange {
                                kind: StoreChangeKind::Updated,
                                id,
                                is_folder: false,
                            }));
                            svc.env
                                .emit(AppEvent::Summary(SummaryEvent::Done { call: id, summary }));
                        }
                        Err(e) => svc
                            .env
                            .emit(AppEvent::Summary(SummaryEvent::Error { call: id, error: e })),
                    },
                    Err(e) => svc
                        .env
                        .emit(AppEvent::Summary(SummaryEvent::Error { call: id, error: e })),
                }
            })
            .map_err(|e| AppError::new(ErrorCode::Unknown).with_detail(e.to_string()))?;
        Ok(())
    }

    pub fn summarize_cancel(&self, id: &CallId) {
        if let Some(f) = self.summaries.lock().get(id) {
            f.store(true, Ordering::SeqCst);
        }
    }

    /// Tags from the vault's own vocabulary first; the user confirms (`call_set_tags`).
    pub fn suggest_tags(&self, id: &CallId) -> Result<Vec<TagSuggestion>> {
        let call = self.vault.read_call(id)?;
        let route = self.summary_route(&call.meta)?;
        let doc = call_codec::parse(&call.md);
        let mut excerpt = String::new();
        for b in doc.blocks() {
            if let Block::Turn { text, .. } = b {
                excerpt.push_str(text);
                excerpt.push('\n');
                if excerpt.chars().count() > 3000 {
                    break;
                }
            }
        }
        let mut vocab: Vec<String> = self
            .vault
            .calls(None)?
            .into_iter()
            .flat_map(|c| c.tags)
            .chain(self.vault.folders()?.into_iter().flat_map(|f| f.tags))
            .collect();
        vocab.sort();
        vocab.dedup();
        let summary = doc.summary().trim().to_string();
        let input = TagInput {
            call: *id,
            title: call.meta.title.clone(),
            summary: (!summary.is_empty()).then_some(summary),
            transcript_excerpt: excerpt.chars().take(3000).collect(),
            vocabulary: vocab,
            current_tags: call.meta.tags.clone(),
            max: 5,
        };
        CloudLlm::new(route.cfg).suggest_tags(&input)
    }

    pub fn provider_status(&self) -> LlmProviderStatus {
        let s = self.settings().llm;
        LlmProviderStatus {
            provider: s.provider,
            model: s.summary_model.clone(),
            pinned_provider: s.pinned_provider.clone(),
            key_present: self.env.keys.openrouter().is_some(),
        }
    }

    /// Settings «Проверить ключ»: one tiny authenticated request.
    pub fn key_test(&self) -> Result<()> {
        let s = self.settings().llm;
        let route = route::resolve(&s, false, self.env.keys.openrouter(), &Purpose::Summary)?;
        CloudLlm::new(route.cfg).test_key()
    }

    /// Once per key: measures the upstream providers of the live model and pins the fastest
    /// (`llm.pinned_provider`). Cheap (a few 8-token requests); runs in the background.
    pub fn spawn_provider_probe(self: &Arc<Self>) {
        let s = self.settings().llm;
        // the pin names an upstream of one model: only probe when ask and summary share it
        if s.provider != LlmProvider::Openrouter || s.pinned_provider.is_some() || s.model != s.summary_model
        {
            return;
        }
        let Some(key) = self.env.keys.openrouter() else {
            return;
        };
        let svc = self.clone();
        std::thread::Builder::new()
            .name("lowk-llm-probe".into())
            .spawn(move || {
                let cfg = lowk_llm::LlmConfig::for_summary(&s, Some(key));
                let t0 = std::time::Instant::now();
                match lowk_llm::probe_fastest_provider(&cfg, &s.model, &ProbeOptions::default(), &|| true) {
                    Ok(r) => {
                        tracing::info!(best = %r.best, ms = t0.elapsed().as_millis() as u64, model = %r.model, "llm provider pinned");
                        let _ = svc.env.settings.patch(SettingsPatch(
                            serde_json::json!({ "llm": { "pinned_provider": r.best } }),
                        ));
                    }
                    Err(e) => tracing::warn!(?e, "llm provider probe failed; routing stays unpinned"),
                }
            })
            .ok();
    }
}
