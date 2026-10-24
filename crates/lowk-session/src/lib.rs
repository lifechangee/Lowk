//! Session orchestrator (WP7): state machine, event bus, transcript buffer,
//! Q&A links («Мой ответ»), replay/demo feed. Wires feature crates only
//! through traits, so the whole call flow runs without Tauri.
mod bus;
mod deps;
mod echo;
mod qa;
mod replay;
mod session;
mod transcript;

pub use bus::EventBus;
pub use deps::{
    AudioTap, CallRecorder, CallStore, HintGenerator, HintOutcome, HintRequest, MemCallStore, NullCallStore,
    SessionDeps, StubHints,
};
pub use lowk_types::{CloseReason, QaRecord, QaUpdate};
pub use qa::QaTracker;
pub use replay::{rebase, DemoPace, Pacer, ReplayItem, ReplayKind, ReplayScript, ReplaySource};
pub use session::{phase_allowed, Session};
pub use transcript::{Applied, TranscriptBuf};

#[cfg(test)]
mod tests;
