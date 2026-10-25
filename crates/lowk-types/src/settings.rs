//! `Settings`: the schema of `settings.toml` with defaults for every key.
//!
//! Every struct is `#[serde(default)]` so an old/partial file loads and new keys
//! get defaults. Unknown-key preservation on rewrite is the store's job.
//! Secrets are never part of settings (they live in the OS keychain).
use crate::bind::*;
use crate::hw::Os;
use crate::{LangTag, MicMode, Target};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::collections::BTreeMap;

/// 2: `Template.reasoning` became tri-state (`None` = follow the global setting).
pub const SETTINGS_SCHEMA_VERSION: u32 = 2;
/// Symbolic default; `lowk-platform` resolves `~/Documents` (Windows: known folder).
pub const DEFAULT_VAULT_PATH: &str = "~/Documents/Lowk";
/// Live answers use the version-pinned snapshot too (BRIEF, bench 2026-10-21: 1.2 s TTFT p50). The plain
/// `deepseek/deepseek-v4.1-flash` slug routes to a different host on every call (2.3 s p50, p95 6 s+).
pub const DEFAULT_LLM_MODEL: &str = "deepseek/deepseek-v4.1-flash-1017";
/// The unpinned slug older builds wrote as the default; `lowk-app` migrates it to `DEFAULT_LLM_MODEL`.
pub const LEGACY_LLM_MODEL: &str = "deepseek/deepseek-v4.1-flash";
/// Vision-capable model that answers Asks with screenshots when the chosen model cannot read images
/// (OpenRouter slug; also the reliable fallback model of `lowk-llm`).
pub const DEFAULT_VISION_MODEL: &str = "anthropic/claude-haiku-5.5";
pub const DEFAULT_OPENROUTER_URL: &str = "https://openrouter.ai/api/v1";
/// Version- and provider-pinned default for cloud calls (summary, tags, Ask).
pub const DEFAULT_LLM_MODEL_PINNED: &str = "deepseek/deepseek-v4.1-flash-1017";
/// EmbeddingGemma GGUF used for local semantic search (about 310 MB).
pub const DEFAULT_EMBEDDER_ID: &str = "embeddinggemma-2-q8_0";
pub const TEMPLATE_BRIEF: &str = "brief";
pub const TEMPLATE_COMPLEMENT: &str = "complement";
pub const TEMPLATE_SUMMARY: &str = "summary";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(default)]
pub struct Settings {
    pub schema_version: u32,
    pub ui: UiSettings,
    pub onboarding: OnboardingSettings,
    pub vault: VaultSettings,
    pub audio: AudioSettings,
    pub stt: SttSettings,
    pub llm: LlmSettings,
    pub embedder: EmbedderSettings,
    pub docs: DocsSettings,
    pub overlay: OverlaySettings,
    pub capture: CaptureSettings,
    pub binds: Vec<BindSpec>,
    pub stealth: StealthSettings,
    pub updates: UpdateSettings,
    pub templates: Vec<Template>,
}

impl Default for Settings {
    fn default() -> Self {
        Self::defaults_for(Os::current())
    }
}

impl Settings {
    /// Deterministic defaults for a given OS (binds differ: Cmd/Ctrl, Option/Alt).
    pub fn defaults_for(os: Os) -> Self {
        Self {
            schema_version: SETTINGS_SCHEMA_VERSION,
            ui: UiSettings::default(),
            onboarding: OnboardingSettings::default(),
            vault: VaultSettings::default(),
            audio: AudioSettings::default(),
            stt: SttSettings::default(),
            llm: LlmSettings::default(),
            embedder: EmbedderSettings::default(),
            docs: DocsSettings::default(),
            overlay: OverlaySettings::default(),
            capture: CaptureSettings::default(),
            binds: default_binds(os),
            stealth: StealthSettings::default(),
            updates: UpdateSettings::default(),
            templates: default_templates(),
        }
    }

    /// Schema 1 -> 2: `Template.reasoning` was a plain bool whose `false` meant "off by default", not "force off".
    /// A pre-2 `false` becomes `None` (follow the global setting); `true` stays a forced on.
    /// Returns true when something changed.
    pub fn migrate_templates(&mut self) -> bool {
        if self.schema_version >= 2 {
            return false;
        }
        let mut changed = false;
        for t in &mut self.templates {
            if t.reasoning == Some(false) {
                t.reasoning = None;
                changed = true;
            }
        }
        changed
    }

    /// The prompt-template list must stay usable: every template has an id, non-empty names and instruction;
    /// ids and names are unique; the built-ins (brief, complement, summary) cannot be deleted.
    pub fn validate_templates(&self) -> crate::Result<()> {
        let bad = |d: String| crate::AppError::new(crate::ErrorCode::InvalidArgument).with_detail(d);
        let mut ids = std::collections::BTreeSet::new();
        let (mut ru, mut en) = (
            std::collections::BTreeSet::new(),
            std::collections::BTreeSet::new(),
        );
        for t in &self.templates {
            if t.id.trim().is_empty() || t.instruction.trim().is_empty() {
                return Err(bad(format!("template {:?} needs an id and an instruction", t.id)));
            }
            if t.names.ru.trim().is_empty() || t.names.en.trim().is_empty() {
                return Err(bad(format!("template {:?} needs a name", t.id)));
            }
            if !ids.insert(t.id.clone()) {
                return Err(bad(format!("duplicate template id {:?}", t.id)));
            }
            // summary stays out of the overlay list and the editor
            if t.id != TEMPLATE_SUMMARY
                && (!ru.insert(t.names.ru.trim().to_lowercase())
                    || !en.insert(t.names.en.trim().to_lowercase()))
            {
                return Err(bad(format!("duplicate template name of {:?}", t.id)));
            }
        }
        for id in [TEMPLATE_BRIEF, TEMPLATE_COMPLEMENT, TEMPLATE_SUMMARY] {
            if !ids.contains(id) {
                return Err(bad(format!("built-in template {id:?} cannot be deleted")));
            }
        }
        Ok(())
    }

    /// Repairs binds written by older builds: BRIEF fixes ⌘⇧S = screenshot of all monitors and
    /// ⌘⇧A = area. A swapped pair (or both on the same combo) is put back on the defaults.
    /// Returns true when something changed.
    pub fn migrate_binds(&mut self, os: Os) -> bool {
        let added = self.add_missing_default_binds(os);
        self.repair_screenshot_binds(os) || added
    }

    /// New default binds (e.g. hold-Ctrl = clickable) missing from saved settings are added; a customised or
    /// already present action is never touched, and a default whose trigger is taken by another bind is skipped.
    fn add_missing_default_binds(&mut self, os: Os) -> bool {
        let mut changed = false;
        for d in default_binds(os) {
            if d.template.is_some() || self.binds.iter().any(|b| b.action == d.action) {
                continue;
            }
            if self.binds.iter().any(|b| b.trigger == d.trigger) {
                continue;
            }
            self.binds.push(d);
            changed = true;
        }
        changed
    }

    fn repair_screenshot_binds(&mut self, os: Os) -> bool {
        let defaults = default_binds(os);
        let want = |a: ActionId| defaults.iter().find(|b| b.action == a).map(|b| b.trigger.clone());
        let (Some(shot), Some(area)) = (want(ActionId::Screenshot), want(ActionId::ScreenshotArea)) else {
            return false;
        };
        let cur = |a: ActionId, b: &[BindSpec]| b.iter().find(|x| x.action == a).map(|x| x.trigger.clone());
        let (cs, ca) = (
            cur(ActionId::Screenshot, &self.binds),
            cur(ActionId::ScreenshotArea, &self.binds),
        );
        let swapped = cs.as_ref() == Some(&area) && ca.as_ref() == Some(&shot);
        let same = cs.is_some() && cs == ca;
        let shot_on_area = cs.as_ref() == Some(&area) && ca.is_none();
        let area_on_shot = ca.as_ref() == Some(&shot) && cs.is_none();
        if !(swapped || same || shot_on_area || area_on_shot) {
            return false;
        }
        for b in &mut self.binds {
            match b.action {
                ActionId::Screenshot => b.trigger = shot.clone(),
                ActionId::ScreenshotArea => b.trigger = area.clone(),
                _ => {}
            }
        }
        true
    }

    /// RFC 7386 JSON merge patch applied to the serialized settings, then
    /// re-validated by deserializing. `null` resets nothing (use defaults API).
    pub fn apply_patch(&self, patch: &SettingsPatch) -> crate::Result<Settings> {
        let mut v = serde_json::to_value(self).map_err(|e| {
            crate::AppError::new(crate::ErrorCode::SettingsInvalid).with_detail(e.to_string())
        })?;
        merge(&mut v, &patch.0);
        serde_json::from_value(v)
            .map_err(|e| crate::AppError::new(crate::ErrorCode::SettingsInvalid).with_detail(e.to_string()))
    }
}

fn merge(dst: &mut serde_json::Value, patch: &serde_json::Value) {
    match (dst, patch) {
        // An internally/adjacently tagged value (`{"kind": ..}`, e.g. `Target`) is replaced as a whole: merging
        // would keep the previous variant's fields (`id` of an app) and break the unit variants.
        (d, p) if p.as_object().is_some_and(|o| o.contains_key("kind")) => *d = p.clone(),
        (serde_json::Value::Object(d), serde_json::Value::Object(p)) => {
            for (k, pv) in p {
                if pv.is_null() {
                    d.remove(k);
                } else {
                    merge(d.entry(k.clone()).or_insert(serde_json::Value::Null), pv);
                }
            }
        }
        (d, p) => *d = p.clone(),
    }
}

/// Partial settings as a JSON merge patch (objects merge, arrays replace).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(transparent)]
pub struct SettingsPatch(#[specta(type = specta_typescript::Unknown)] pub serde_json::Value);

// --- ui ------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum UiLanguage {
    Auto,
    Ru,
    En,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(default)]
pub struct UiSettings {
    pub language: UiLanguage,
    pub font_px: u8,
}
impl Default for UiSettings {
    fn default() -> Self {
        Self {
            language: UiLanguage::Auto,
            font_px: 14,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(default)]
pub struct OnboardingSettings {
    /// Last completed step (0 = not started); resumes after a quit mid-way.
    pub step: u8,
    pub done: bool,
    /// Mandatory acknowledgement of the recording-law notice (step 2).
    pub recording_notice_ack: bool,
    /// The first-run tour (6 steps) was finished or skipped; «Показать тур» resets it.
    pub tour_done: bool,
}

// --- vault ---------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(default)]
pub struct VaultSettings {
    pub path: String,
    /// On-disk names of the reserved system folders.
    pub unsorted_dir: String,
    pub trash_dir: String,
}
impl Default for VaultSettings {
    fn default() -> Self {
        Self {
            path: DEFAULT_VAULT_PATH.into(),
            unsorted_dir: crate::UNSORTED_DIR.into(),
            trash_dir: crate::TRASH_DIR.into(),
        }
    }
}

// --- audio / stt ---------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum AecMode {
    Auto,
    On,
    Off,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(default)]
pub struct AudioSettings {
    /// Default system source; `None` = ask / whole system.
    pub default_source: Option<Target>,
    /// The source the last call was started with; a new call preselects it (falls back to `default_source`).
    pub last_source: Option<Target>,
    pub mic_device: Option<String>,
    pub aec: AecMode,
    /// Keep the recorded audio next to the call (opt-in; default text only).
    pub keep_audio: bool,
    /// Global default of «record microphone» (folders may override: `FolderMeta.mic`). Default on.
    /// Old files stored a bool; `true`/`false` still load as on/off.
    pub record_mic: MicMode,
}
impl AudioSettings {
    /// The source a call starts with: the explicit choice, else the last one used, else the default setting.
    pub fn start_source(&self, explicit: Option<&Target>) -> Option<Target> {
        explicit
            .cloned()
            .or_else(|| self.last_source.clone())
            .or_else(|| self.default_source.clone())
    }
}

impl Default for AudioSettings {
    fn default() -> Self {
        Self {
            default_source: None,
            last_source: None,
            mic_device: None,
            aec: AecMode::Auto,
            keep_audio: false,
            record_mic: MicMode::On,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(default)]
pub struct SttSettings {
    /// Engine id from `EngineRegistry`; empty = chosen by the hardware check.
    pub engine: String,
    /// Default languages of a call (Settings «Языки созвона»).
    pub languages: Vec<LangTag>,
    /// Languages of the last call the user chose explicitly in the call selector; empty = use `languages`.
    /// Changing the default in Settings clears it.
    pub last_languages: Vec<LangTag>,
    pub vocab_hints: Vec<String>,
    /// Phrases dropped by the hallucination filter (editable).
    pub hallucination_phrases: Vec<String>,
    /// Cloud vendor when a cloud engine is selected (`soniox` | `deepgram`).
    pub cloud_vendor: String,
    /// Re-transcribe finished utterances with Whisper in the background (local engines only).
    pub background_correction: bool,
    /// Re-transcribe a call with Whisper large-v3-turbo as soon as the recording stops (needs
    /// `audio.keep_audio`). Folders may override: `FolderMeta.retx`. Default off.
    pub retranscribe_after: bool,
    /// Re-transcribe through Deepgram's pre-recorded API instead of the local Whisper. Sends the
    /// call's audio to Deepgram (BYOK key in the keychain); never used for local-only folders.
    pub retranscribe_cloud: bool,
    /// Cached hardware benchmark: (engine id, RTF).
    pub benchmark: Option<SttBenchmark>,
}
impl Default for SttSettings {
    fn default() -> Self {
        Self {
            engine: String::new(),
            languages: vec!["ru".into(), "en".into()],
            last_languages: vec![],
            vocab_hints: vec![],
            hallucination_phrases: vec![],
            cloud_vendor: "soniox".into(),
            background_correction: true,
            retranscribe_after: false,
            retranscribe_cloud: false,
            benchmark: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct SttBenchmark {
    pub engine: String,
    pub rtf: f32,
    pub p95_final_ms: u32,
    pub app_version: String,
}

// --- llm -----------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum LlmProvider {
    Openrouter,
    Ollama,
    Lmstudio,
    LlamaServer,
    Custom,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(default)]
pub struct LlmSettings {
    pub provider: LlmProvider,
    pub base_url: String,
    pub model: String,
    pub reasoning: bool,
    /// OpenRouter `provider.data_collection = "deny"`.
    pub data_collection_deny: bool,
    pub default_template: String,
    /// Model for summary and tag suggestions (cloud); pinned version.
    pub summary_model: String,
    /// OpenRouter upstream provider to pin (`provider.order` + no fallbacks); set after the
    /// latency probe, `None` = not probed yet.
    pub pinned_provider: Option<String>,
    /// Local OpenAI-compatible server (Ollama `http://localhost:11434/v1`, LM Studio
    /// `http://localhost:1234/v1`) used for folders marked local-only. Empty = not configured:
    /// asking in a local-only folder is then disabled (never sent to the cloud).
    pub local_base_url: String,
    pub local_model: String,
    /// Model for Asks that carry screenshots when `model` cannot read images (cloud slug). Empty = the
    /// built-in default (`DEFAULT_VISION_MODEL`).
    pub vision_model: String,
    /// Vision model of the local server (`local_base_url` / a local provider) for screenshots in local-only
    /// folders. Empty = the local model itself when its name is a known vision model, else images are refused.
    pub local_vision_model: String,
    /// Context window assumed for the per-call Ask conversation; the conversation is compacted at ~60% of
    /// it (summary of the oldest turns). 0 = automatic (128k cloud, 200k Claude, 16k local).
    pub ask_context_tokens: u32,
}
impl Default for LlmSettings {
    fn default() -> Self {
        Self {
            provider: LlmProvider::Openrouter,
            base_url: DEFAULT_OPENROUTER_URL.into(),
            model: DEFAULT_LLM_MODEL.into(),
            reasoning: false,
            data_collection_deny: true,
            default_template: TEMPLATE_BRIEF.into(),
            summary_model: DEFAULT_LLM_MODEL_PINNED.into(),
            pinned_provider: None,
            local_base_url: String::new(),
            local_model: String::new(),
            vision_model: DEFAULT_VISION_MODEL.into(),
            local_vision_model: String::new(),
            ask_context_tokens: 0,
        }
    }
}

// --- embedder and documents -------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum EmbedderMode {
    /// Local EmbeddingGemma (default); downloaded with the models.
    Local,
    /// OpenRouter embeddings (option; needs the OpenRouter key).
    Cloud,
    /// Keyword (FTS5) search only.
    Off,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(default)]
pub struct EmbedderSettings {
    pub mode: EmbedderMode,
    /// Local model id (`DEFAULT_EMBEDDER_ID`).
    pub model: String,
    /// Cloud embedding model slug (mode = cloud).
    pub cloud_model: String,
}
impl Default for EmbedderSettings {
    fn default() -> Self {
        Self {
            mode: EmbedderMode::Local,
            model: DEFAULT_EMBEDDER_ID.into(),
            cloud_model: "google/gemini-embedding-001".into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(default)]
pub struct DocsSettings {
    /// Pinned documents of a folder go to the model at most up to this many tokens.
    pub pinned_budget_tokens: u32,
    /// Warn before importing files above this size (slow conversion); files are never refused
    /// below `max_file_mb`.
    pub warn_slow_mb: u32,
    pub max_file_mb: u32,
    /// OCR for scanned PDFs and images.
    pub ocr: bool,
    /// Tokens the retriever may add for documents and previous calls per Ask.
    pub retrieved_budget_tokens: u32,
}
impl Default for DocsSettings {
    fn default() -> Self {
        Self {
            pinned_budget_tokens: 6000,
            warn_slow_mb: 25,
            max_file_mb: 500,
            ocr: true,
            retrieved_budget_tokens: 6000,
        }
    }
}

// --- overlay -------------------------------------------------------------

/// When the overlay is shown while not recording.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum OverlayVisibility {
    /// Shown whenever the app runs.
    Always,
    /// Hidden until recording starts (or the show/hide bind). Default.
    RecordingOnly,
    /// Only the `ToggleOverlay` bind controls it.
    Manual,
    /// A small unobtrusive pill on the screen edge while idle, stealth-protected
    /// like the overlay; clicking it starts recording.
    Pill,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum PillEdge {
    Left,
    Right,
    Top,
    Bottom,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ToneMode {
    Auto,
    Light,
    Dark,
}

/// Accent colour (the single accent of the design tokens).
pub const DEFAULT_ACCENT: &str = "#8ba6c4";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(default)]
pub struct OverlaySettings {
    /// 0..=90 (percent); scales the whole overlay.
    pub opacity: u8,
    pub font_px: u8,
    /// `primary` or a display id.
    pub display: String,
    /// Remembered origin per display id.
    pub positions: BTreeMap<String, OverlayPos>,
    /// G2: remembered pill origin per display (`x,y,WxH` of the display's visible frame).
    pub pill_positions: BTreeMap<String, OverlayPos>,
    pub visibility: OverlayVisibility,
    pub pill_edge: PillEdge,
    pub tone_mode: ToneMode,
    pub accent: String,
    /// «Adaptive scrim colour (for smart glasses)»: the screen under the overlay is sampled and the scrim follows
    /// it. Default OFF (also for settings files written before the key existed): the sampler does not run at all and
    /// the overlay uses a fixed dark scrim with the transparency slider.
    pub adaptive_scrim: bool,
}
impl Default for OverlaySettings {
    fn default() -> Self {
        Self {
            opacity: 60,
            font_px: 14,
            display: "primary".into(),
            positions: BTreeMap::new(),
            pill_positions: BTreeMap::new(),
            visibility: OverlayVisibility::RecordingOnly,
            pill_edge: PillEdge::Right,
            tone_mode: ToneMode::Auto,
            accent: DEFAULT_ACCENT.into(),
            adaptive_scrim: false,
        }
    }
}

impl OverlaySettings {
    /// The screen sampler (ScreenCaptureKit / Windows capture) runs only for the adaptive scrim.
    pub fn sampler_wanted(&self) -> bool {
        self.adaptive_scrim
    }

    /// The palette the overlay uses: the adaptive one only when the setting is on; otherwise the plain scrim
    /// (a fixed dark preset, or the light one when forced) with the transparency slider.
    pub fn effective_tone_mode(&self) -> ToneMode {
        match (self.adaptive_scrim, self.tone_mode) {
            (true, m) => m,
            (false, ToneMode::Light) => ToneMode::Light,
            (false, _) => ToneMode::Dark,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct OverlayPos {
    pub x: f64,
    pub y: f64,
}

// --- capture / stealth / updates -----------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum CaptureMonitors {
    /// All monitors, PrtScr-style.
    All,
    Primary,
    /// The monitor with the focused window (else the one under the mouse, else the primary): the default.
    Active,
}

/// What a new recording captures by default (the onboarding choice, F12).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum CaptureSource {
    /// One call app (recommended: YouTube / music next to the call stay out).
    OneApp,
    /// Everything the system plays (Windows 10, or the user's choice).
    WholeSystem,
}

impl CaptureSource {
    /// The mode that matches a concrete `audio.default_source` target.
    pub fn of_target(t: Option<&Target>) -> Self {
        match t {
            Some(Target::WholeSystem) => Self::WholeSystem,
            _ => Self::OneApp,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(default)]
pub struct CaptureSettings {
    pub monitors: CaptureMonitors,
    /// Default source mode of the home screen / settings. Written by onboarding and
    /// kept in sync with `audio.default_source` by the app. Default: one app.
    pub default_source: CaptureSource,
}
impl Default for CaptureSettings {
    fn default() -> Self {
        Self {
            monitors: CaptureMonitors::Active,
            default_source: CaptureSource::OneApp,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(default)]
pub struct StealthSettings {
    /// Also exclude the main window from capture (off by default).
    pub hide_main: bool,
    pub verify_interval_ms: u32,
}
impl Default for StealthSettings {
    fn default() -> Self {
        Self {
            hide_main: false,
            verify_interval_ms: 1000,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(default)]
pub struct UpdateSettings {
    /// The only automatic network call; on by default.
    pub check_daily: bool,
    /// RFC 3339 time of the last check.
    pub last_checked: Option<String>,
    /// A version the user chose to skip: automatic checks stop offering it.
    pub skipped_version: Option<String>,
    /// Download and install a found update without asking (still never during a recording). Off by default.
    pub auto_install: bool,
}
impl Default for UpdateSettings {
    fn default() -> Self {
        Self {
            check_daily: true,
            last_checked: None,
            skipped_version: None,
            auto_install: false,
        }
    }
}

// --- templates -----------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct LocalizedName {
    pub ru: String,
    pub en: String,
}

/// Prompt template. Lives in the *user* turn, never in the system prompt.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct Template {
    pub id: String,
    pub names: LocalizedName,
    pub instruction: String,
    #[serde(default)]
    pub needs_my_answer: bool,
    #[serde(default)]
    pub vision: bool,
    /// Per-template override of `llm.reasoning` for this Ask: `Some(true/false)` forces it on/off,
    /// `None` follows the global setting.
    #[serde(default)]
    pub reasoning: Option<bool>,
}

pub fn default_templates() -> Vec<Template> {
    vec![
        Template {
            id: TEMPLATE_BRIEF.into(),
            names: LocalizedName {
                ru: "Кратко".into(),
                en: "Brief".into(),
            },
            instruction: "Кратко ответь на вопрос, заданный в системе/приложении.".into(),
            needs_my_answer: false,
            vision: false,
            reasoning: None,
        },
        Template {
            id: TEMPLATE_COMPLEMENT.into(),
            names: LocalizedName {
                ru: "Дополни мой ответ".into(),
                en: "Complement my answer".into(),
            },
            instruction: "Я уже ответил. Добавь недостающие детали, не повторяя сказанное.".into(),
            needs_my_answer: true,
            vision: false,
            reasoning: None,
        },
        summary_template(),
    ]
}

/// «Резюме»: used by `llm_summarize`; not offered in the overlay chip by default.
pub fn summary_template() -> Template {
    Template {
        id: TEMPLATE_SUMMARY.into(),
        names: LocalizedName {
            ru: "Резюме".into(),
            en: "Summary".into(),
        },
        instruction: "Сделай краткое резюме созвона: ключевые темы, решения, договорённости и следующие шаги. Отвечай на языке разговора.".into(),
        needs_my_answer: false,
        vision: false,
        reasoning: None,
    }
}

// --- binds ---------------------------------------------------------------

/// Default binds. macOS: Cmd/Option; Windows: Ctrl/Alt.
pub fn default_binds(os: Os) -> Vec<BindSpec> {
    let mac = os == Os::Macos;
    let primary = if mac { Mods::META } else { Mods::CTRL };
    let primary_shift = if mac { Mods::META_SHIFT } else { Mods::CTRL_SHIFT };
    let combo = |action, mods, key: &str| BindSpec {
        action,
        trigger: Trigger::Combo {
            mods,
            key: KeyCode::new(key),
        },
        swallow: false,
        template: None,
    };
    vec![
        // G6: direct binds per template; plain `Ask` (chip template) is assignable but unbound.
        combo(ActionId::AskBrief, primary, "Enter"),
        combo(ActionId::AskComplement, primary_shift, "Enter"),
        combo(ActionId::Screenshot, primary_shift, "KeyS"),
        combo(ActionId::ScreenshotArea, primary_shift, "KeyA"),
        combo(ActionId::ToggleInteractive, primary_shift, "KeyI"),
        combo(ActionId::ToggleOverlay, primary, "Backslash"),
        // F7: PanicHide is deliberately NOT bound by default (action stays, assignable in settings).
        combo(ActionId::ToggleRecord, primary_shift, "KeyR"),
        BindSpec {
            action: ActionId::MoveHold,
            trigger: Trigger::ModifierOnly { mods: Mods::ALT },
            swallow: false,
            template: None,
        },
        // G6: hold Ctrl = clickable without focus. Move (Alt) wins when both are held.
        BindSpec {
            action: ActionId::ClickHold,
            trigger: Trigger::ModifierOnly { mods: Mods::CTRL },
            swallow: false,
            template: None,
        },
    ]
}
