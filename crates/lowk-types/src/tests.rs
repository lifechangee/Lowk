use super::*;
use crate::fakes::*;
use std::sync::Arc;

fn roundtrip<T: serde::Serialize + serde::de::DeserializeOwned + PartialEq + std::fmt::Debug>(v: &T) {
    let j = serde_json::to_string(v).unwrap();
    let back: T = serde_json::from_str(&j).unwrap();
    assert_eq!(&back, v, "json: {j}");
}

#[test]
fn settings_defaults_reflect_brief() {
    let s = Settings::defaults_for(Os::Macos);
    assert_eq!(s.vault.path, "~/Documents/Lowk");
    assert_eq!(s.vault.unsorted_dir, "Unsorted");
    assert_eq!(s.overlay.visibility, OverlayVisibility::RecordingOnly);
    assert_eq!(s.overlay.display, "primary");
    assert!(s.overlay.opacity <= 90);
    assert_eq!(s.overlay.accent, "#8ba6c4");
    assert!(s.updates.check_daily);
    assert_eq!(s.capture.monitors, CaptureMonitors::All);
    assert!(!s.audio.keep_audio);
    assert!(!s.stealth.hide_main);
    assert_eq!(s.llm.model, "deepseek/deepseek-v4.1-flash");
    assert!(!s.llm.reasoning);
    assert_eq!(s.stt.languages, vec!["ru", "en"]);
    assert_eq!(s.templates[0].id, "brief");
    assert!(s.templates[1].needs_my_answer);
    assert_eq!(s.llm.default_template, s.templates[0].id);
    assert!(!s.onboarding.recording_notice_ack);
}

#[test]
fn default_binds_per_os() {
    let find = |b: &[BindSpec], a| b.iter().find(|x| x.action == a).unwrap().trigger.clone();
    let mac = default_binds(Os::Macos);
    let win = default_binds(Os::Windows);
    assert_eq!(mac.len(), 8);
    assert_eq!(
        find(&mac, ActionId::Ask),
        Trigger::Combo {
            mods: Mods::META,
            key: KeyCode::new("Enter")
        }
    );
    assert_eq!(
        find(&win, ActionId::Ask),
        Trigger::Combo {
            mods: Mods::CTRL,
            key: KeyCode::new("Enter")
        }
    );
    assert_eq!(
        find(&mac, ActionId::PanicHide),
        Trigger::Combo {
            mods: Mods::META_SHIFT,
            key: KeyCode::new("KeyH")
        }
    );
    assert_eq!(
        find(&mac, ActionId::MoveHold),
        Trigger::ModifierOnly { mods: Mods::ALT }
    );
    assert_eq!(
        find(&win, ActionId::MoveHold),
        Trigger::ModifierOnly { mods: Mods::ALT }
    );
    assert_eq!(
        find(&win, ActionId::ToggleOverlay),
        Trigger::Combo {
            mods: Mods::CTRL,
            key: KeyCode::new("Backslash")
        }
    );
}

#[test]
fn overlay_visibility_has_pill_and_serializes_snake_case() {
    assert_eq!(
        serde_json::to_string(&OverlayVisibility::RecordingOnly).unwrap(),
        "\"recording_only\""
    );
    assert_eq!(
        serde_json::to_string(&OverlayVisibility::Pill).unwrap(),
        "\"pill\""
    );
    for v in [
        OverlayVisibility::Always,
        OverlayVisibility::RecordingOnly,
        OverlayVisibility::Manual,
        OverlayVisibility::Pill,
    ] {
        roundtrip(&v);
    }
}

#[test]
fn settings_roundtrip_json_and_partial_input() {
    let s = Settings::defaults_for(Os::Windows);
    roundtrip(&s);
    // Partial / old file: missing keys take defaults, unknown keys are ignored.
    let p: Settings = serde_json::from_str(r#"{"overlay":{"opacity":30},"future_key":1}"#).unwrap();
    assert_eq!(p.overlay.opacity, 30);
    assert_eq!(p.overlay.visibility, OverlayVisibility::RecordingOnly);
    assert_eq!(p.vault.path, DEFAULT_VAULT_PATH);
}

#[test]
fn settings_patch_merges_and_validates() {
    let store = MemSettingsStore::default();
    let rx = store.subscribe();
    let out = store
        .patch(SettingsPatch(
            serde_json::json!({"overlay": {"visibility": "pill", "opacity": 42}}),
        ))
        .unwrap();
    assert_eq!(out.overlay.visibility, OverlayVisibility::Pill);
    assert_eq!(out.overlay.opacity, 42);
    assert_eq!(out.overlay.font_px, OverlaySettings::default().font_px);
    assert_eq!(rx.try_recv().unwrap(), out);
    let bad = store.patch(SettingsPatch(
        serde_json::json!({"overlay": {"visibility": "nope"}}),
    ));
    assert_eq!(bad.unwrap_err().code, ErrorCode::SettingsInvalid);
    assert_eq!(store.get().overlay.opacity, 42);
}

#[test]
fn ulid_roundtrip_is_string() {
    let u = Ulid::new();
    let j = serde_json::to_string(&u).unwrap();
    assert_eq!(j, format!("\"{u}\""));
    assert_eq!(serde_json::from_str::<Ulid>(&j).unwrap(), u);
    assert_eq!(u.to_string().parse::<Ulid>().unwrap(), u);
}

#[test]
fn events_roundtrip_and_names() {
    let stt = SttEvent {
        stream: Stream::System,
        utt: 7,
        rev: 2,
        kind: SttKind::Partial,
        text: "А что значит EBITDA?".into(),
        lang: Some("ru".into()),
        t_start_ns: 1,
        t_audio_end_ns: 2,
        t_emit_ns: 3,
    };
    let evs = vec![
        AppEvent::Stt(stt.clone()),
        AppEvent::Session(SessionState {
            phase: SessionPhase::Recording,
            call: Some(Ulid::new()),
            demo: false,
        }),
        AppEvent::Llm(LlmEvent::Delta {
            req_id: Ulid::new(),
            text: "x".into(),
        }),
        AppEvent::Llm(LlmEvent::Error {
            req_id: Ulid::new(),
            error: ErrorCode::Network.into(),
        }),
        AppEvent::Overlay(OverlayEvent::Mode(OverlayMode::Interactive)),
        AppEvent::Stealth(StealthStatus::Degraded("x".into())),
        AppEvent::Bind(BindEvent {
            action: ActionId::Ask,
            phase: Phase::Down,
            t_mono_ns: 5,
        }),
        AppEvent::Perm {
            kind: PermKind::Microphone,
            state: PermState::Granted,
        },
        AppEvent::Queue(QueueState {
            shots: 2,
            thumbs: vec![],
        }),
        AppEvent::Level(AudioLevel { me: 0.1, sys: 0.2 }),
    ];
    for e in &evs {
        roundtrip(e);
        assert!(e.name().contains("://"));
    }
    assert_eq!(evs[0].name(), events::STT_SEGMENT);
    let j = serde_json::to_value(&evs[0]).unwrap();
    assert_eq!(j["type"], "stt");
    assert_eq!(j["data"]["stream"], "system");
}

#[test]
fn dto_roundtrips() {
    roundtrip(
        &AppError::new(ErrorCode::PermMicrophone)
            .with_param("app", "Zoom")
            .with_detail("d"),
    );
    roundtrip(&BindSpec {
        action: ActionId::Screenshot,
        trigger: Trigger::Mouse {
            button: MouseBtn::Back,
            mods: Mods::NONE,
        },
        swallow: true,
    });
    roundtrip(&FolderRef::Unsorted);
    roundtrip(&FolderRef::User(Ulid::new()));
    roundtrip(&Target::App("us.zoom.xos".into()));
    roundtrip(&CallEntry::MyAnswer {
        qa_id: Ulid::new(),
        at_s: 31,
        text: "ok".into(),
    });
    roundtrip(&Fitness::Unsupported("no gpu".into()));
    roundtrip(&default_templates());
}

#[test]
fn error_display_is_english_and_code_is_snake_case() {
    let e = AppError::new(ErrorCode::VaultNotOpen).with_detail("x");
    assert_eq!(e.to_string(), "vault is not open: x");
    assert_eq!(
        serde_json::to_string(&ErrorCode::VaultNotOpen).unwrap(),
        "\"vault_not_open\""
    );
}

#[test]
fn offsets_format() {
    assert_eq!(fmt_offset(3), "00:00:03");
    assert_eq!(fmt_offset(4363), "01:12:43");
    assert_eq!(fmt_offset(100 * 3600), "100:00:00");
    let c = SessionClock {
        t0_mono: 10_000_000_000,
        t0_wall_ms: 0,
    };
    assert_eq!(c.offset_label(12_500_000_000), "00:00:02");
    assert!(now_mono_ns() <= now_mono_ns());
}

#[test]
fn main_thread_run_sync_and_fakes() {
    let mt: Arc<dyn MainThread> = Arc::new(InlineMainThread);
    assert_eq!(mt.run_sync(|| 21 * 2), 42);
    let oc = NoopOverlayController::default();
    oc.show().unwrap();
    oc.set_mode(OverlayMode::Interactive).unwrap();
    assert!(oc.visible());
    assert_eq!(oc.mode(), OverlayMode::Interactive);
    oc.panic_hide();
    assert!(!oc.visible());
    assert!(NoopRetriever
        .retrieve(&RetrievalQuery {
            text: "q".into(),
            scope: RetrievalScope {
                folder: None,
                call: None,
                tags: vec![],
                all_folders: true
            },
            budget_tokens: 100,
            exclude: vec![],
            cloud_provider: false
        })
        .unwrap()
        .is_empty());
    let sink = arc_sink();
    let dynsink: Arc<dyn SttSink> = sink.clone();
    dynsink.error(SttError {
        stream: Stream::Me,
        message: "m".into(),
        recoverable: true,
    });
    assert_eq!(sink.errors.lock().unwrap().len(), 1);
}
