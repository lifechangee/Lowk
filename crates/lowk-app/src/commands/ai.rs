//! Cloud / local LLM helpers (M2): summary, tags, provider status. Glue over `m2::Services`.
//! Events: `summary://event` (`SummaryEvent`). Local-only folders are served by the local model or refused.
use crate::state::Shared;
use lowk_types::*;
use tauri::State;

/// Starts the post-call summary (streams `summary://event`; Done also writes `## Summary`).
#[tauri::command(async)]
#[specta::specta]
pub fn llm_summarize(st: State<'_, Shared>, call: CallId) -> Result<()> {
    st.svc()?.summarize(call)
}

/// Cancels a running summary.
#[tauri::command(async)]
#[specta::specta]
pub fn llm_summarize_cancel(st: State<'_, Shared>, call: CallId) -> Result<()> {
    st.svc()?.summarize_cancel(&call);
    Ok(())
}

/// AI tag suggestions for a call; the user confirms, then `call_set_tags`.
#[tauri::command(async)]
#[specta::specta]
pub fn llm_suggest_tags(st: State<'_, Shared>, call: CallId) -> Result<Vec<TagSuggestion>> {
    st.svc()?.suggest_tags(&call)
}

/// Provider, pinned model/provider and whether the BYOK key is present.
#[tauri::command(async)]
#[specta::specta]
pub fn llm_provider_status(st: State<'_, Shared>) -> Result<LlmProviderStatus> {
    Ok(st.svc()?.provider_status())
}

/// Cheap authenticated request to verify the OpenRouter key (`ApiKeyMissing`, `Network`, `LlmProvider`).
#[tauri::command(async)]
#[specta::specta]
pub fn llm_key_test(st: State<'_, Shared>) -> Result<()> {
    st.svc()?.key_test()
}
