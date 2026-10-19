//! `Settings`: the schema of `settings.toml` with defaults for every key.
//!
//! Every struct is `#[serde(default)]` so an old/partial file loads and new keys
//! get defaults. Unknown-key preservation on rewrite is the store's job.
//! Secrets are never part of settings (they live in the OS keychain).
use crate::bind::*;
use crate::hw::Os;
use crate::{LangTag, Target};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::collections::BTreeMap;

pub const SETTINGS_SCHEMA_VERSION: u32 = 1;
/// Symbolic default; `lowk-platform` resolves `~/Documents` (Windows: known folder).
pub const DEFAULT_VAULT_PATH: &str = "~/Documents/Lowk";
pub const DEFAULT_LLM_MODEL: &str = "deepseek/deepseek-v4.1-flash";
pub const DEFAULT_OPENROUTER_URL: &str = "https://openrouter.ai/api/v1";
pub const TEMPLATE_BRIEF: &str = "brief";
pub const TEMPLATE_COMPLEMENT: &str = "complement";

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
            overlay: OverlaySettings::default(),
            capture: CaptureSettings::default(),
            binds: default_binds(os),
            stealth: StealthSettings::default(),
            updates: UpdateSettings::default(),
            templates: default_templates(),
        }
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
    pub mic_device: Option<String>,
    pub aec: AecMode,
    /// Keep the recorded audio next to the call (opt-in; default text only).
    pub keep_audio: bool,
}
impl Default for AudioSettings {
    fn default() -> Self {
        Self {
            default_source: None,
            mic_device: None,
            aec: AecMode::Auto,
            keep_audio: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
#[serde(default)]
pub struct SttSettings {
    /// Engine id from `EngineRegistry`; empty = chosen by the hardware check.
    pub engine: String,
    pub languages: Vec<LangTag>,
    pub vocab_hints: Vec<String>,
    /// Phrases dropped by the hallucination filter (editable).
    pub hallucination_phrases: Vec<String>,
    /// Cloud vendor when a cloud engine is selected (`soniox` | `deepgram`).
    pub cloud_vendor: String,
    /// Cached hardware benchmark: (engine id, RTF).
    pub benchmark: Option<SttBenchmark>,
}
impl Default for SttSettings {
    fn default() -> Self {
        Self {
            engine: String::new(),
            languages: vec!["ru".into(), "en".into()],
            vocab_hints: vec![],
            hallucination_phrases: vec![],
            cloud_vendor: "soniox".into(),
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
    pub visibility: OverlayVisibility,
    pub pill_edge: PillEdge,
    pub tone_mode: ToneMode,
    pub accent: String,
}
impl Default for OverlaySettings {
    fn default() -> Self {
        Self {
            opacity: 60,
            font_px: 14,
            display: "primary".into(),
            positions: BTreeMap::new(),
            visibility: OverlayVisibility::RecordingOnly,
            pill_edge: PillEdge::Right,
            tone_mode: ToneMode::Auto,
            accent: DEFAULT_ACCENT.into(),
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
    /// All monitors, PrtScr-style (default).
    All,
    Primary,
    Active,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(default)]
pub struct CaptureSettings {
    pub monitors: CaptureMonitors,
}
impl Default for CaptureSettings {
    fn default() -> Self {
        Self {
            monitors: CaptureMonitors::All,
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
}
impl Default for UpdateSettings {
    fn default() -> Self {
        Self {
            check_daily: true,
            last_checked: None,
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
    #[serde(default)]
    pub reasoning: bool,
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
            reasoning: false,
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
            reasoning: false,
        },
    ]
}

// --- binds ---------------------------------------------------------------

/// Product-owner defaults (BRIEF round 3). macOS: Cmd/Option; Windows: Ctrl/Alt.
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
    };
    vec![
        combo(ActionId::Ask, primary, "Enter"),
        combo(ActionId::Screenshot, primary_shift, "KeyS"),
        combo(ActionId::ScreenshotArea, primary_shift, "KeyA"),
        combo(ActionId::ToggleInteractive, primary_shift, "KeyI"),
        combo(ActionId::ToggleOverlay, primary, "Backslash"),
        combo(ActionId::PanicHide, primary_shift, "KeyH"),
        combo(ActionId::ToggleRecord, primary_shift, "KeyR"),
        BindSpec {
            action: ActionId::MoveHold,
            trigger: Trigger::ModifierOnly { mods: Mods::ALT },
            swallow: false,
        },
    ]
}
