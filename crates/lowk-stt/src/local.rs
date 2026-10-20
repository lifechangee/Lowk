//! Local engines: Nemotron 3.5 ASR Streaming (live) and Whisper large-v3-turbo (background
//! correction), both run by the `lowk-stt-worker` sidecar on the ggml runtime
//! (Metal on macOS, Vulkan/CUDA on Windows). Models are downloaded on demand.
use crate::models::{DownloadSpec, ModelStore, ProgressFn};
use crate::sidecar::{SidecarEngine, SidecarSpec};
use crate::{SttEngine, SttEngineFactory};
use lowk_types::*;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

pub const NEMOTRON_ID: &str = "nemotron-local";
pub const WHISPER_ID: &str = "whisper-turbo-local";

const HF_NEMOTRON: &str = "https://huggingface.co/handy-computer/nemotron-3.5-asr-streaming-0.6b-gguf/resolve/main";
const HF_SILERO: &str = "https://huggingface.co/handy-computer/silero-vad-v6.2-gguf/resolve/main";
const HF_WHISPER: &str = "https://huggingface.co/handy-computer/whisper-large-v3-turbo-gguf/resolve/main";

pub const NEMOTRON_FILE: &str = "nemotron-3.5-asr-streaming-0.6b-Q8_0.gguf";
pub const SILERO_FILE: &str = "silero-vad-v6.2-F32.gguf";
pub const WHISPER_FILE: &str = "whisper-large-v3-turbo-Q5_K_M.gguf";

pub const NEMOTRON_LICENSE_URL: &str = "https://openmdw.ai/license/1-1/";

/// Pinned assets of the live engine. SHA-256 values are the Hugging Face LFS object ids.
pub fn nemotron_assets() -> Vec<DownloadSpec> {
    vec![
        DownloadSpec {
            file: NEMOTRON_FILE.into(),
            url: format!("{HF_NEMOTRON}/{NEMOTRON_FILE}"),
            sha256: "b94545b313b3223fda7b2857a52681da813935c2127643d1e9ff0c23d988089c".into(),
            size_bytes: 751_094_240,
            license: "OpenMDW-1.1".into(),
            license_url: NEMOTRON_LICENSE_URL.into(),
        },
        DownloadSpec {
            file: SILERO_FILE.into(),
            url: format!("{HF_SILERO}/{SILERO_FILE}"),
            sha256: "0d871a800c420fdd5d1b17a3aea4296d1c39608f0ff4a9295eb76457eb1c88e6".into(),
            size_bytes: 1_241_056,
            license: "MIT".into(),
            license_url: "https://github.com/snakers4/silero-vad/blob/master/LICENSE".into(),
        },
    ]
}

pub fn whisper_assets() -> Vec<DownloadSpec> {
    vec![DownloadSpec {
        file: WHISPER_FILE.into(),
        url: format!("{HF_WHISPER}/{WHISPER_FILE}"),
        sha256: "06fc9923c09fb3ae878ea12c34d1be3ed18fad039ab14b4b045c7b6c40af56b2".into(),
        size_bytes: 619_628_096,
        license: "MIT".into(),
        license_url: "https://github.com/openai/whisper/blob/main/LICENSE".into(),
    }]
}

/// Where the worker executable lives: `LOWK_STT_WORKER`, next to the app, in app data
/// (`engines/`), and (debug builds) the in-repo build output.
pub fn find_worker() -> Option<PathBuf> {
    let name = if cfg!(windows) { "lowk-stt-worker.exe" } else { "lowk-stt-worker" };
    if let Some(p) = std::env::var_os("LOWK_STT_WORKER").map(PathBuf::from) {
        return Some(p);
    }
    let mut c: Vec<PathBuf> = vec![];
    if let Ok(exe) = std::env::current_exe() {
        if let Some(d) = exe.parent() {
            c.push(d.join(name));
        }
    }
    if let Some(d) = lowk_platform::dirs::engines() {
        c.push(d.join(name));
    }
    if cfg!(debug_assertions) {
        c.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../workers/lowk-stt-worker/target/release").join(name));
    }
    c.into_iter().find(|p| p.exists())
}

#[derive(Clone, Debug)]
pub struct LocalTuning {
    /// Right context in encoder frames (80 ms each): 0, 3, 6 or 13.
    pub right: u32,
    pub chunk_ms: u32,
    /// Silence that closes an utterance.
    pub hang_ms: u32,
}
impl Default for LocalTuning {
    fn default() -> Self {
        Self {
            right: 6,
            chunk_ms: 160,
            hang_ms: 480,
        }
    }
}

impl LocalTuning {
    pub fn lookahead_ms(&self) -> u32 {
        self.right * 80 + self.chunk_ms
    }
}

pub struct NemotronFactory {
    pub store: ModelStore,
    pub worker: Option<PathBuf>,
    pub tuning: LocalTuning,
}

impl NemotronFactory {
    pub fn new(store: ModelStore) -> Self {
        Self {
            store,
            worker: find_worker(),
            tuning: LocalTuning::default(),
        }
    }
}

pub fn nemotron_descriptor() -> EngineDescriptor {
    EngineDescriptor {
        id: NEMOTRON_ID.into(),
        name: "NVIDIA Nemotron 3.5 ASR Streaming (local)".into(),
        kind: EngineKind::Local,
        languages: ["ru", "en", "de", "fr", "es", "it", "pt", "nl", "tr", "uk", "pl", "ja", "ko", "zh"]
            .iter()
            .map(|s| s.to_string())
            .collect(),
        streaming: StreamingMode::Native,
        size_mb: nemotron_assets().iter().map(DownloadSpec::size_mb).sum(),
        license: "OpenMDW-1.1".into(),
    }
}

impl SttEngineFactory for NemotronFactory {
    fn descriptor(&self) -> EngineDescriptor {
        nemotron_descriptor()
    }

    fn fitness(&self, hw: &HwInfo) -> Fitness {
        match hw.os {
            Os::Macos if hw.arch != "aarch64" => Fitness::Unsupported("intel_mac".into()),
            Os::Macos if hw.ram_gb >= 15.5 => Fitness::Recommended,
            Os::Macos => Fitness::Possible,
            _ => {
                let gpu = hw.gpus.iter().map(|g| g.vram_mb).max().unwrap_or(0);
                if gpu >= 4096 && hw.ram_gb >= 11.5 {
                    Fitness::Recommended
                } else if hw.ram_gb >= 7.5 {
                    Fitness::Possible
                } else {
                    Fitness::Unsupported("low_ram".into())
                }
            }
        }
    }

    fn create(&self, stream: Stream) -> Result<Box<dyn SttEngine>> {
        if !self.installed() {
            return Err(AppError::new(ErrorCode::NotFound).with_detail("engine assets not installed"));
        }
        let exe = self
            .worker
            .clone()
            .or_else(find_worker)
            .ok_or_else(|| AppError::new(ErrorCode::NotFound).with_detail("lowk-stt-worker"))?;
        let m = self.store.path(NEMOTRON_ID, NEMOTRON_FILE);
        let v = self.store.path(NEMOTRON_ID, SILERO_FILE);
        let t = &self.tuning;
        let args = [
            "stream".to_string(),
            "--model".into(),
            m.to_string_lossy().into_owned(),
            "--vad".into(),
            v.to_string_lossy().into_owned(),
            "--right".into(),
            t.right.to_string(),
            "--chunk-ms".into(),
            t.chunk_ms.to_string(),
            "--hang-ms".into(),
            t.hang_ms.to_string(),
        ]
        .to_vec();
        Ok(Box::new(SidecarEngine::new(
            nemotron_descriptor(),
            stream,
            SidecarSpec { exe, args, nice: 0 },
            t.lookahead_ms(),
        )))
    }

    fn assets(&self) -> Vec<DownloadSpec> {
        nemotron_assets()
    }

    fn installed(&self) -> bool {
        self.store.installed(NEMOTRON_ID, &nemotron_assets())
    }

    fn install(&self, cancel: &AtomicBool, progress: ProgressFn<'_>) -> Result<()> {
        self.store.install(NEMOTRON_ID, &nemotron_assets(), cancel, progress)
    }

    fn license_url(&self) -> String {
        NEMOTRON_LICENSE_URL.into()
    }
}

/// Install helper for the correction model (installed together with the live engine).
pub fn install_whisper(store: &ModelStore, cancel: &AtomicBool, progress: ProgressFn<'_>) -> Result<()> {
    store.install(WHISPER_ID, &whisper_assets(), cancel, progress)
}

pub fn whisper_installed(store: &ModelStore) -> bool {
    store.installed(WHISPER_ID, &whisper_assets())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asset_pins_are_well_formed() {
        for a in nemotron_assets().into_iter().chain(whisper_assets()) {
            assert_eq!(a.sha256.len(), 64, "{}", a.file);
            assert!(a.sha256.chars().all(|c| c.is_ascii_hexdigit()));
            assert!(a.url.starts_with("https://"));
        }
        assert!(nemotron_descriptor().size_mb > 700);
    }

    #[test]
    fn create_requires_installed_assets() {
        let f = NemotronFactory::new(ModelStore::new(std::env::temp_dir().join("lowk-none-xyz")));
        assert!(!f.installed());
        assert_eq!(f.create(Stream::Me).err().unwrap().code, ErrorCode::NotFound);
    }
}
