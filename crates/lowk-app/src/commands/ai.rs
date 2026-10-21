//! Cloud LLM helpers (M2, stubs until WP-E wires `lowk-llm`).
//! Events: `summary://event` (`SummaryEvent`).
use lowk_types::*;

/// Starts the post-call summary (streams `summary://event`; Done also writes `## Summary`).
#[tauri::command(async)]
#[specta::specta]
pub fn llm_summarize(_call: CallId) -> Result<()> {
    Err(AppError::not_implemented())
}

/// Cancels a running summary.
#[tauri::command(async)]
#[specta::specta]
pub fn llm_summarize_cancel(_call: CallId) -> Result<()> {
    Err(AppError::not_implemented())
}

/// AI tag suggestions for a call; the user confirms, then `call_set_tags`.
#[tauri::command(async)]
#[specta::specta]
pub fn llm_suggest_tags(_call: CallId) -> Result<Vec<TagSuggestion>> {
    Err(AppError::not_implemented())
}

/// Provider, pinned model/provider and whether the BYOK key is present.
#[tauri::command(async)]
#[specta::specta]
pub fn llm_provider_status() -> Result<LlmProviderStatus> {
    Err(AppError::not_implemented())
}

/// Cheap authenticated request to verify the OpenRouter key (`ApiKeyMissing`, `Network`, `LlmProvider`).
#[tauri::command(async)]
#[specta::specta]
pub fn llm_key_test() -> Result<()> {
    Err(AppError::not_implemented())
}
