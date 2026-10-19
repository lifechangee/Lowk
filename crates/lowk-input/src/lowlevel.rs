//! Low-level keyboard/mouse backend: owns the native hook thread (CGEventTap /
//! WH_*_LL) and the shared [`Engine`]. The hook is started lazily, only while
//! some bind (or a record session) needs it, so the Input Monitoring prompt
//! is never triggered by plain-combo setups.
use crate::engine::Engine;
use crate::keys::is_known_code;
use crate::platform::{self, HookError, HookHandle, HookInfo, PermSnapshot};
use lowk_types::{
    AppError, BindEvent, BindReport, BindSpec, BindStatus, BindStatusEntry, ErrorCode, InputBackend,
    MouseBtn, PermKind, Result, Trigger,
};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};

pub struct LowLevelBackend {
    engine: Arc<Engine>,
    hook: Option<(HookHandle, HookInfo)>,
    last_error: Option<HookError>,
    active_binds: usize,
    rx: Mutex<Option<Receiver<BindEvent>>>,
}

fn unsupported_reason(b: &BindSpec) -> Option<String> {
    match &b.trigger {
        Trigger::Combo { key, .. } if !is_known_code(&key.0) => Some(format!("unknown key {}", key.0)),
        Trigger::Mouse {
            button: MouseBtn::Other(n),
            ..
        } if cfg!(windows) || *n < 5 => Some(format!("mouse button {n} is not supported")),
        _ => None,
    }
}

impl LowLevelBackend {
    pub fn new() -> Self {
        let (tx, rx) = channel();
        let mut s = Self::with_sender(tx);
        s.rx = Mutex::new(Some(rx));
        s
    }

    pub fn with_sender(tx: Sender<BindEvent>) -> Self {
        Self {
            engine: Arc::new(Engine::new(tx)),
            hook: None,
            last_error: None,
            active_binds: 0,
            rx: Mutex::new(None),
        }
    }

    /// Capability of the running hook, if any.
    pub fn hook_info(&self) -> Option<HookInfo> {
        self.hook.as_ref().map(|h| h.1)
    }

    pub fn last_error(&self) -> Option<&HookError> {
        self.last_error.as_ref()
    }

    pub fn permissions(&self) -> PermSnapshot {
        platform::permissions()
    }

    fn ensure_hook(&mut self, want_swallow: bool) -> std::result::Result<HookInfo, HookError> {
        if let Some((_, info)) = &self.hook {
            let upgradable =
                want_swallow && !info.swallow_capable && platform::permissions().accessibility == Some(true);
            if !upgradable {
                return Ok(*info);
            }
            self.hook = None; // restart as an active tap
        }
        match platform::start(self.engine.clone(), want_swallow) {
            Ok((h, info)) => {
                self.hook = Some((h, info));
                self.last_error = None;
                Ok(info)
            }
            Err(e) => {
                self.last_error = Some(e.clone());
                Err(e)
            }
        }
    }

    fn release_if_idle(&mut self) {
        if self.active_binds == 0 && !self.engine.is_recording() {
            self.hook = None;
        }
    }

    /// Apply binds; one status per input bind, same order.
    pub fn apply(&mut self, binds: &[BindSpec]) -> Vec<BindStatus> {
        let valid: Vec<&BindSpec> = binds.iter().filter(|b| unsupported_reason(b).is_none()).collect();
        self.engine
            .set_binds(valid.iter().map(|b| (*b).clone()).collect());
        self.active_binds = valid.len();
        let want_swallow = valid.iter().any(|b| b.swallow);
        let hook = if valid.is_empty() {
            self.release_if_idle();
            Ok(None)
        } else {
            self.ensure_hook(want_swallow).map(Some)
        };
        binds
            .iter()
            .map(|b| {
                if let Some(r) = unsupported_reason(b) {
                    return BindStatus::Failed(r);
                }
                match &hook {
                    Ok(Some(info)) if b.swallow && !info.swallow_capable => {
                        BindStatus::Degraded(PermKind::Accessibility)
                    }
                    Ok(_) => BindStatus::Active,
                    Err(HookError::Permission(k)) => BindStatus::Degraded(*k),
                    Err(HookError::Failed(s)) => BindStatus::Failed(s.clone()),
                    Err(HookError::Unsupported) => BindStatus::Failed("unsupported platform".into()),
                }
            })
            .collect()
    }

    /// Capture the next combo / modifier-only / mouse button. Binds are suspended
    /// meanwhile and the captured input is swallowed. Needs the hook (Input
    /// Monitoring on macOS).
    pub fn start_record(&mut self) -> Result<Receiver<Trigger>> {
        match self.ensure_hook(false) {
            Ok(_) => Ok(self.engine.start_record()),
            Err(HookError::Permission(PermKind::InputMonitoring)) => {
                Err(AppError::new(ErrorCode::PermInputMonitoring))
            }
            Err(HookError::Permission(_)) => Err(AppError::new(ErrorCode::PermissionDenied)),
            Err(HookError::Unsupported) => Err(AppError::new(ErrorCode::Unsupported)),
            Err(HookError::Failed(s)) => Err(AppError::new(ErrorCode::Unknown).with_detail(s)),
        }
    }

    pub fn stop_record(&mut self) {
        self.engine.stop_record();
        self.release_if_idle();
    }
}

impl Default for LowLevelBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl InputBackend for LowLevelBackend {
    fn set_binds(&mut self, binds: &[BindSpec]) -> Result<BindReport> {
        let st = self.apply(binds);
        Ok(BindReport {
            entries: binds
                .iter()
                .zip(st)
                .map(|(b, status)| BindStatusEntry {
                    action: b.action,
                    status,
                })
                .collect(),
        })
    }

    fn events(&self) -> Receiver<BindEvent> {
        self.rx
            .lock()
            .ok()
            .and_then(|mut g| g.take())
            .unwrap_or_else(|| channel().1)
    }
}
