//! Session DTOs.
use crate::{CallId, FolderRef, MicMode, SessionClock, Target, TranscriptLine};
use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum SessionPhase {
    Idle,
    Starting,
    Recording,
    Paused,
    Stopping,
}

/// What the platform indicator should show while Lowk is running.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum Indicator {
    Hidden,
    Background,
    Attention { reason: AttentionReason },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum AttentionReason {
    EngineMissing,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum Scenario {
    Listen,
    Assist,
    Run,
}

impl Scenario {
    pub fn name_ru(self) -> &'static str {
        match self {
            Self::Listen => "Я слушаю",
            Self::Assist => "Мне нужна помощь",
            Self::Run => "Я веду встречу",
        }
    }
    pub fn template(self) -> &'static str {
        match self {
            Self::Listen => "explain",
            Self::Assist => "brief",
            Self::Run => "say",
        }
    }
    pub fn next(self) -> Self {
        match self {
            Self::Listen => Self::Assist,
            Self::Assist => Self::Run,
            Self::Run => Self::Listen,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct SessionState {
    pub phase: SessionPhase,
    #[serde(default)]
    pub scenario: Option<Scenario>,
    pub call: Option<CallId>,
    /// True while the demo/replay feed drives the session.
    pub demo: bool,
    /// The microphone is being recorded (false = muted for this call; system audio is unaffected).
    #[serde(default = "mic_default")]
    pub mic_on: bool,
    /// What the user chose for this call: on / off / auto (auto = follow the call app's use of the microphone).
    #[serde(default)]
    pub mic_mode: MicMode,
    /// Auto was chosen but cannot be detected here (whole-system source or no OS support): it records like "on".
    #[serde(default)]
    pub mic_auto_unsupported: bool,
}

fn mic_default() -> bool {
    true
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct StartOpts {
    #[serde(default)]
    pub scenario: Option<Scenario>,
    pub title: Option<String>,
    pub folder: Option<FolderRef>,
    pub system: Option<Target>,
    pub mic: Option<String>,
    pub demo: bool,
    /// Languages of this call (primary tags); `None` = the remembered/default set from settings.
    /// Limited to what the engine supports; stored in the call frontmatter.
    #[serde(default)]
    pub languages: Option<Vec<String>>,
}

/// Full resync payload (`session_snapshot`): lagging subscribers rebuild from it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct SessionSnapshot {
    pub state: SessionState,
    pub clock: Option<SessionClock>,
    pub transcript: Vec<TranscriptLine>,
    pub queue: QueueState,
}

/// `queue://changed`: screenshots waiting for the next Ask.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct QueueState {
    pub shots: u32,
    /// Small `data:` URI thumbnails, one per queued image.
    pub thumbs: Vec<String>,
}

/// `audio://level` payload (10 Hz, bypasses the bus).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct AudioLevel {
    pub me: f32,
    pub sys: f32,
}
