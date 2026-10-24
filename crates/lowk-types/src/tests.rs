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
    assert_eq!(s.capture.monitors, CaptureMonitors::Active);
    assert_eq!(
        s.capture.default_source,
        CaptureSource::OneApp,
        "F12: onboarding recommends one app"
    );
    assert_eq!(
        CaptureSource::of_target(Some(&Target::WholeSystem)),
        CaptureSource::WholeSystem
    );
    assert_eq!(
        CaptureSource::of_target(Some(&Target::App("us.zoom.xos".into()))),
        CaptureSource::OneApp
    );
    assert!(!s.audio.keep_audio);
    assert!(!s.stealth.hide_main);
    assert_eq!(s.llm.model, DEFAULT_LLM_MODEL_PINNED);
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
    assert_eq!(mac.len(), 9);
    assert_eq!(
        find(&mac, ActionId::AskBrief),
        Trigger::Combo {
            mods: Mods::META,
            key: KeyCode::new("Enter")
        }
    );
    assert_eq!(
        find(&mac, ActionId::AskComplement),
        Trigger::Combo {
            mods: Mods::META_SHIFT,
            key: KeyCode::new("Enter")
        }
    );
    assert_eq!(
        find(&mac, ActionId::ClickHold),
        Trigger::ModifierOnly { mods: Mods::CTRL }
    );
    assert_eq!(ActionId::AskComplement.template_id(), Some("complement"));
    assert_eq!(ActionId::Ask.template_id(), None);
    assert_eq!(
        find(&win, ActionId::AskBrief),
        Trigger::Combo {
            mods: Mods::CTRL,
            key: KeyCode::new("Enter")
        }
    );
    // F7: panic is unbound by default; show/hide overlay is Cmd/Ctrl+\.
    assert!(default_binds(Os::Macos)
        .iter()
        .all(|b| b.action != ActionId::PanicHide));
    assert!(default_binds(Os::Windows)
        .iter()
        .all(|b| b.action != ActionId::PanicHide));
    assert_eq!(
        find(&mac, ActionId::ToggleOverlay),
        Trigger::Combo {
            mods: Mods::META,
            key: KeyCode::new("Backslash")
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
            mic_on: true,
            mic_mode: MicMode::On,
            mic_auto_unsupported: false,
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
            template: None,
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
        template: None,
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
            cloud_provider: false,
            pinned_budget_tokens: 0,
            include_folder_context: false,
            include_previous_calls: false
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

#[test]
fn source_info_roundtrip() {
    roundtrip(&SourceInfo {
        id: "com.google.Chrome".into(),
        name: "Google Chrome".into(),
        bundle_id: "com.google.Chrome".into(),
        pid: 42,
        icon_png_b64: Some("iVBOR".into()),
        producing_audio: true,
        kind: SourceKind::App,
    });
    let j = serde_json::to_value(SourceKind::System).unwrap();
    assert_eq!(j, "system");
}

fn bind_key(s: &Settings, a: ActionId) -> Option<String> {
    s.binds
        .iter()
        .find(|b| b.action == a)
        .and_then(|b| match &b.trigger {
            Trigger::Combo { key, .. } => Some(key.0.clone()),
            _ => None,
        })
}

#[test]
fn s2_default_screenshot_is_all_monitors_on_s_and_area_on_a() {
    for os in [Os::Macos, Os::Windows] {
        let s = Settings::defaults_for(os);
        assert_eq!(bind_key(&s, ActionId::Screenshot).as_deref(), Some("KeyS"));
        assert_eq!(bind_key(&s, ActionId::ScreenshotArea).as_deref(), Some("KeyA"));
    }
}

#[test]
fn s2_migration_repairs_swapped_or_duplicated_screenshot_binds() {
    let mut s = Settings::defaults_for(Os::Macos);
    // swapped
    for b in &mut s.binds {
        if let Trigger::Combo { key, .. } = &mut b.trigger {
            match b.action {
                ActionId::Screenshot => *key = KeyCode::new("KeyA"),
                ActionId::ScreenshotArea => *key = KeyCode::new("KeyS"),
                _ => {}
            }
        }
    }
    assert!(s.migrate_binds(Os::Macos));
    assert_eq!(bind_key(&s, ActionId::Screenshot).as_deref(), Some("KeyS"));
    assert_eq!(bind_key(&s, ActionId::ScreenshotArea).as_deref(), Some("KeyA"));
    // already right: untouched, and idempotent
    assert!(!s.migrate_binds(Os::Macos));
    // a deliberate custom bind is left alone
    for b in &mut s.binds {
        if b.action == ActionId::Screenshot {
            b.trigger = Trigger::Combo {
                mods: Mods::META_SHIFT,
                key: KeyCode::new("KeyJ"),
            };
        }
    }
    assert!(!s.migrate_binds(Os::Macos));
    assert_eq!(bind_key(&s, ActionId::Screenshot).as_deref(), Some("KeyJ"));
    // both on the same combo
    for b in &mut s.binds {
        if b.action == ActionId::Screenshot || b.action == ActionId::ScreenshotArea {
            b.trigger = Trigger::Combo {
                mods: Mods::META_SHIFT,
                key: KeyCode::new("KeyA"),
            };
        }
    }
    assert!(s.migrate_binds(Os::Macos));
    assert_eq!(bind_key(&s, ActionId::Screenshot).as_deref(), Some("KeyS"));
}

#[test]
fn template_bind_roundtrips_and_old_binds_still_load() {
    let b = BindSpec::for_template(
        "my-tpl",
        Trigger::Combo {
            mods: Mods::META,
            key: KeyCode::new("KeyJ"),
        },
    );
    let j = serde_json::to_string(&b).unwrap();
    assert!(j.contains("\"template\":\"my-tpl\""), "{j}");
    assert_eq!(serde_json::from_str::<BindSpec>(&j).unwrap(), b);
    assert_eq!(b.ask_template(), Some("my-tpl"));
    // a bind written by an older build has no template field
    let old: BindSpec =
        serde_json::from_str(r#"{"action":"ask_brief","trigger":{"kind":"modifier_only","mods":{}}}"#)
            .unwrap();
    assert_eq!((old.template.clone(), old.ask_template()), (None, Some("brief")));
    assert!(!old.same_slot(&b) && b.same_slot(&b.clone()));
}

#[test]
fn m2_contracts_roundtrip() {
    // events
    let src = fakes::fake_source(RefKind::Doc, "Spec.pdf");
    let ev = AppEvent::Llm(LlmEvent::Sources {
        req_id: Ulid::new(),
        sources: vec![src.clone()],
        withheld: 0,
    });
    assert_eq!(ev.name(), events::LLM_SOURCES);
    let j = serde_json::to_string(&ev).unwrap();
    assert_eq!(serde_json::from_str::<AppEvent>(&j).unwrap(), ev);
    assert_eq!(
        AppEvent::Index(fakes::FakeSearchIndex::default().status()).name(),
        events::INDEX_STATUS
    );
    // old settings.toml/json without the new groups still loads with defaults
    let old = serde_json::json!({ "ui": { "language": "en" } });
    let s: Settings = serde_json::from_value(old).unwrap();
    assert_eq!(s.embedder.mode, EmbedderMode::Local);
    assert_eq!(s.docs.pinned_budget_tokens, 6000);
    assert_eq!(s.llm.summary_model, DEFAULT_LLM_MODEL_PINNED);
    assert!(!s.onboarding.tour_done);
    // documents
    assert_eq!(DocKind::from_extension("KEY"), Some(DocKind::Keynote));
    assert_eq!(DocKind::from_extension("rs"), None);
    let store = fakes::FakeDocumentStore::default();
    let d = store
        .add_doc(&NewDocument {
            folder: FolderRef::Unsorted,
            source_path: "/tmp/a.pdf".into(),
            title: None,
            pin: true,
            tags: vec![],
            on_duplicate: Default::default(),
        })
        .unwrap();
    assert_eq!(d.status, IngestStatus::Queued);
    let ing = fakes::FakeIngestor
        .convert(
            &IngestRequest {
                doc: d.id,
                source_path: d.file_name.clone(),
                kind: d.kind,
                languages: vec![],
                ocr: true,
            },
            &mut |_, _| {},
            &|| true,
        )
        .unwrap();
    assert_eq!(
        store.save_converted(&d.id, &ing).unwrap().status,
        IngestStatus::Indexing
    );
    assert_eq!(store.list_docs(&FolderRef::Unsorted).unwrap().len(), 1);
    store.delete_doc(&d.id).unwrap();
    assert!(store.list_docs(&FolderRef::Unsorted).unwrap().is_empty());
    assert_eq!(
        store
            .add_doc(&NewDocument {
                folder: FolderRef::Unsorted,
                source_path: "x.rs".into(),
                title: None,
                pin: false,
                tags: vec![],
                on_duplicate: Default::default(),
            })
            .unwrap_err()
            .code,
        ErrorCode::DocUnsupported
    );
    // search + retrieval + llm fakes
    let q = SearchQuery {
        text: "ebitda".into(),
        filters: SearchFilters::default(),
        mode: SearchMode::Hybrid,
        limit: 10,
        offset: 0,
    };
    let r = fakes::FakeSearchIndex::default().search(&q).unwrap();
    assert_eq!(r.hits.len(), 2);
    assert!(!r.hits[0].highlights.is_empty());
    let pack = fakes::FakeRetriever
        .retrieve_pack(&RetrievalQuery {
            text: "q".into(),
            scope: RetrievalScope {
                folder: None,
                call: None,
                tags: vec![],
                all_folders: false,
            },
            budget_tokens: 100,
            exclude: vec![],
            cloud_provider: true,
            pinned_budget_tokens: 100,
            include_folder_context: true,
            include_previous_calls: true,
        })
        .unwrap();
    assert!(pack.instructions.is_some() && pack.pinned.len() == 1 && pack.relevant.len() == 1);
    let sum = fakes::FakeLlmService
        .summarize(
            &SummarizeInput {
                call: Ulid::new(),
                title: "t".into(),
                transcript: String::new(),
                qa: vec![],
                languages: vec![],
                output_language: None,
                folder_instructions: None,
            },
            &mut |_| {},
            &|| true,
        )
        .unwrap();
    assert!(!sum.key_points.is_empty());
}

#[test]
fn mic_preset_resolution_incl_inherit_and_auto() {
    for global in [MicMode::On, MicMode::Off, MicMode::Auto] {
        assert_eq!(
            MicPreset::Inherit.resolve(global),
            global,
            "inherit follows the global setting"
        );
        assert_eq!(MicPreset::On.resolve(global), MicMode::On);
        assert_eq!(MicPreset::Off.resolve(global), MicMode::Off);
        assert_eq!(MicPreset::Auto.resolve(global), MicMode::Auto);
    }
    assert_eq!(MicPreset::default(), MicPreset::Inherit);
}

#[test]
fn mic_mode_migrates_from_the_old_bool_setting() {
    let load = |v: serde_json::Value| -> Settings {
        serde_json::from_value(serde_json::json!({ "audio": { "record_mic": v } })).unwrap()
    };
    assert_eq!(load(serde_json::json!(true)).audio.record_mic, MicMode::On);
    assert_eq!(load(serde_json::json!(false)).audio.record_mic, MicMode::Off);
    assert_eq!(load(serde_json::json!("auto")).audio.record_mic, MicMode::Auto);
    assert_eq!(load(serde_json::json!("off")).audio.record_mic, MicMode::Off);
    // written back as a string
    let s = load(serde_json::json!(false));
    assert_eq!(serde_json::to_value(&s).unwrap()["audio"]["record_mic"], "off");
    let bad: Result<Settings, _> =
        serde_json::from_value(serde_json::json!({ "audio": { "record_mic": "sometimes" } }));
    assert!(bad.is_err());
}

#[test]
fn retranscribe_defaults_are_off_and_the_folder_preset_resolves_like_the_mic_one() {
    let s = Settings::default();
    assert!(!s.stt.retranscribe_after, "automatic re-transcription is opt-in");
    assert!(!s.stt.retranscribe_cloud, "the cloud engine is opt-in");
    for global in [true, false] {
        assert_eq!(RetxPreset::Inherit.resolve(global), global);
        assert!(RetxPreset::On.resolve(global));
        assert!(!RetxPreset::Off.resolve(global));
    }
    // an old settings file without the keys loads with the defaults
    let old: Settings = serde_json::from_value(serde_json::json!({ "stt": { "engine": "x" } })).unwrap();
    assert!(!old.stt.retranscribe_after && !old.stt.retranscribe_cloud);
}

#[test]
fn mic_and_adaptive_scrim_defaults() {
    let s = Settings::defaults_for(Os::Macos);
    assert_eq!(s.audio.record_mic, MicMode::On, "global default: microphone on");
    assert!(!s.overlay.adaptive_scrim, "adaptive scrim is opt-in");
    assert!(
        default_binds(Os::Macos)
            .iter()
            .all(|b| b.action != ActionId::ToggleMic),
        "the mic bind is unbound by default"
    );
}

#[test]
fn old_settings_file_migrates_to_gradient_off_and_mic_on() {
    // A file written before both keys existed (and with a customised overlay section).
    let old = serde_json::json!({
        "schema_version": 1,
        "audio": { "aec": "off", "keep_audio": true },
        "overlay": { "opacity": 40, "tone_mode": "light" }
    });
    let s: Settings = serde_json::from_value(old).unwrap();
    assert!(!s.overlay.adaptive_scrim, "existing users get the gradient off");
    assert_eq!(s.overlay.opacity, 40, "other values survive");
    assert_eq!(
        s.audio.record_mic,
        MicMode::On,
        "existing users keep recording the microphone"
    );
    assert!(s.audio.keep_audio);
    // an explicit opt-in survives a patch round trip
    let on = s
        .apply_patch(&SettingsPatch(
            serde_json::json!({ "overlay": { "adaptive_scrim": true } }),
        ))
        .unwrap();
    assert!(on.overlay.adaptive_scrim);
}
