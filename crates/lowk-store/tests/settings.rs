use lowk_store::*;
use lowk_types::*;
use std::fs;

/// A real saved settings file (copy; predates hold-Ctrl): the migration adds the missing default binds
/// without touching anything customised, writes them to disk, and is idempotent.
#[test]
fn legacy_settings_gain_new_default_binds_without_overriding() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("settings.toml");
    fs::copy(
        concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/legacy-settings.toml"),
        &p,
    )
    .unwrap();
    let before = FileSettingsStore::open(d.path()).unwrap().get();
    let trig = |s: &Settings, a: ActionId| s.binds.iter().find(|b| b.action == a).map(|b| b.trigger.clone());
    assert_eq!(
        trig(&before, ActionId::ClickHold),
        Some(Trigger::ModifierOnly { mods: Mods::CTRL })
    );
    // existing ones untouched: the Alt move-hold and Cmd-Enter ask stay exactly as saved
    assert_eq!(
        trig(&before, ActionId::MoveHold),
        Some(Trigger::ModifierOnly { mods: Mods::ALT })
    );
    assert_eq!(
        trig(&before, ActionId::Ask),
        Some(Trigger::Combo {
            mods: Mods::META,
            key: KeyCode::new("Enter")
        })
    );
    // no two binds share a trigger
    for (i, a) in before.binds.iter().enumerate() {
        for b in &before.binds[i + 1..] {
            assert_ne!(a.trigger, b.trigger, "{:?} vs {:?}", a.action, b.action);
        }
    }
    // persisted, and a second open changes nothing
    assert!(fs::read_to_string(&p).unwrap().contains("click_hold"));
    assert_eq!(FileSettingsStore::open(d.path()).unwrap().get(), before);
    // a user who customised click_hold keeps it
    let mut custom = before.clone();
    for b in &mut custom.binds {
        if b.action == ActionId::ClickHold {
            b.trigger = Trigger::ModifierOnly { mods: Mods::META };
        }
    }
    assert!(!custom.migrate_binds(Os::Macos));
    assert_eq!(
        trig(&custom, ActionId::ClickHold),
        Some(Trigger::ModifierOnly { mods: Mods::META })
    );
}

#[test]
fn defaults_written_patch_persists_and_notifies() {
    let d = tempfile::tempdir().unwrap();
    let s = FileSettingsStore::open(d.path()).unwrap();
    assert!(d.path().join("settings.toml").exists());
    assert_eq!(s.get().templates.len(), 3);
    let rx = s.subscribe();
    let out = s
        .patch(SettingsPatch(
            serde_json::json!({"ui": {"font_px": 18}, "overlay": {"opacity": 40}}),
        ))
        .unwrap();
    assert_eq!(out.ui.font_px, 18);
    assert_eq!(rx.try_recv().unwrap().ui.font_px, 18);
    // survives restart
    let s2 = FileSettingsStore::open(d.path()).unwrap();
    assert_eq!(s2.get(), out);
    // invalid patch is rejected and changes nothing
    assert!(s2
        .patch(SettingsPatch(serde_json::json!({"ui": {"font_px": "big"}})))
        .is_err());
    assert_eq!(s2.get(), out);
}

#[test]
fn unknown_keys_survive_and_corrupt_file_is_backed_up() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("settings.toml");
    fs::write(&p, "future_key = 7\n[ui]\nfont_px = 20\nnew_ui_key = \"x\"\n").unwrap();
    let s = FileSettingsStore::open(d.path()).unwrap();
    assert_eq!(s.get().ui.font_px, 20);
    s.patch(SettingsPatch(serde_json::json!({"ui": {"font_px": 21}})))
        .unwrap();
    let t = fs::read_to_string(&p).unwrap();
    assert!(t.contains("future_key = 7") && t.contains("new_ui_key"), "{t}");

    fs::write(&p, "this is = not [toml").unwrap();
    let s = FileSettingsStore::open(d.path()).unwrap();
    assert!(s.load_warning().is_some());
    assert!(d.path().join("settings.toml.bad").exists());
    assert_eq!(s.get().ui.font_px, Settings::default().ui.font_px);
}

#[test]
fn external_edit_reload_notifies() {
    let d = tempfile::tempdir().unwrap();
    let s = FileSettingsStore::open(d.path()).unwrap();
    let rx = s.subscribe();
    let t = fs::read_to_string(s.path())
        .unwrap()
        .replace("font_px = 14", "font_px = 16");
    fs::write(s.path(), t).unwrap();
    assert_eq!(s.reload().unwrap().ui.font_px, 16);
    assert_eq!(rx.try_recv().unwrap().ui.font_px, 16);
}

#[test]
fn templates() {
    let mut t = TemplateSet::defaults();
    assert_eq!(t.all().len(), 3);
    assert!(t.get("brief").unwrap().instruction.contains("Кратко ответь"));
    assert!(t.get("complement").unwrap().needs_my_answer);
    assert_eq!(t.get(TEMPLATE_SUMMARY).unwrap().names.ru, "Резюме");
    assert_eq!(t.resolve(Some("nope"), "brief").unwrap().id, "brief");
    assert_eq!(t.resolve(Some("complement"), "brief").unwrap().id, "complement");
    assert!(t.remove("brief").is_err());
    let custom = Template {
        id: "mine".into(),
        names: LocalizedName {
            ru: "Моё".into(),
            en: "Mine".into(),
        },
        instruction: "Do it".into(),
        needs_my_answer: false,
        vision: false,
        reasoning: None,
    };
    t.upsert(custom).unwrap();
    t.remove("mine").unwrap();

    let d = tempfile::tempdir().unwrap();
    let s = FileSettingsStore::open(d.path()).unwrap();
    let mut t = TemplateSet::from_settings(&s.get());
    t.upsert(Template {
        id: "x".into(),
        names: LocalizedName {
            ru: "x".into(),
            en: "x".into(),
        },
        instruction: "y".into(),
        needs_my_answer: true,
        vision: false,
        reasoning: None,
    })
    .unwrap();
    let out = s.set_templates(&t).unwrap();
    assert_eq!(out.templates.len(), 4);
    assert_eq!(
        FileSettingsStore::open(d.path()).unwrap().get().templates.len(),
        4
    );
}

/// F12: the source mode chosen in onboarding persists; the default is "one app".
#[test]
fn capture_default_source_defaults_to_one_app_and_persists() {
    let d = tempfile::tempdir().unwrap();
    let s = FileSettingsStore::open(d.path()).unwrap();
    assert_eq!(s.get().capture.default_source, CaptureSource::OneApp);
    s.patch(SettingsPatch(serde_json::json!({
        "audio": {"default_source": {"kind": "whole_system"}},
        "capture": {"default_source": "whole_system"}
    })))
    .unwrap();
    let s2 = FileSettingsStore::open(d.path()).unwrap();
    assert_eq!(s2.get().capture.default_source, CaptureSource::WholeSystem);
    assert_eq!(s2.get().audio.default_source, Some(Target::WholeSystem));
}

#[test]
fn s2_open_migrates_swapped_screenshot_binds_on_disk() {
    let d = tempfile::tempdir().unwrap();
    let mut s = Settings::default();
    for b in &mut s.binds {
        if let Trigger::Combo { key, .. } = &mut b.trigger {
            match b.action {
                ActionId::Screenshot => *key = KeyCode::new("KeyA"),
                ActionId::ScreenshotArea => *key = KeyCode::new("KeyS"),
                _ => {}
            }
        }
    }
    {
        let st = FileSettingsStore::open(d.path()).unwrap();
        st.patch(SettingsPatch(serde_json::json!({ "binds": s.binds })))
            .unwrap();
    }
    let st = FileSettingsStore::open(d.path()).unwrap();
    let key = |a: ActionId| {
        st.get()
            .binds
            .iter()
            .find(|b| b.action == a)
            .map(|b| match &b.trigger {
                Trigger::Combo { key, .. } => key.0.clone(),
                _ => String::new(),
            })
    };
    assert_eq!(key(ActionId::Screenshot).as_deref(), Some("KeyS"));
    assert_eq!(key(ActionId::ScreenshotArea).as_deref(), Some("KeyA"));
    // persisted, not just in memory
    let text = std::fs::read_to_string(d.path().join("settings.toml")).unwrap();
    assert!(text.contains("screenshot_area"));
    let st2 = FileSettingsStore::open(d.path()).unwrap();
    assert_eq!(st2.get().binds, st.get().binds);
}

/// An old settings file has neither new key: after loading, the adaptive scrim is OFF and the microphone is ON,
/// the rest is untouched, and an explicit opt-in is persisted and survives a reopen.
#[test]
fn old_settings_file_loads_with_gradient_off_and_mic_on() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("settings.toml");
    fs::copy(
        concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/legacy-settings.toml"),
        &p,
    )
    .unwrap();
    let raw = fs::read_to_string(&p).unwrap();
    assert!(
        !raw.contains("adaptive_scrim") && !raw.contains("record_mic"),
        "fixture predates both keys"
    );
    let s = FileSettingsStore::open(d.path()).unwrap();
    let cur = s.get();
    assert!(!cur.overlay.adaptive_scrim);
    assert!(!cur.overlay.sampler_wanted());
    assert_eq!(cur.audio.record_mic, lowk_types::MicMode::On);
    assert!(
        cur.binds.iter().all(|b| b.action != ActionId::ToggleMic),
        "the mic bind is not added unasked"
    );
    s.patch(SettingsPatch(serde_json::json!({
        "overlay": { "adaptive_scrim": true },
        "audio": { "record_mic": false }
    })))
    .unwrap();
    let again = FileSettingsStore::open(d.path()).unwrap().get();
    assert!(again.overlay.adaptive_scrim);
    assert_eq!(again.audio.record_mic, lowk_types::MicMode::Off);
}

fn mine(id: &str, ru: &str, en: &str) -> Template {
    Template {
        id: id.into(),
        names: LocalizedName {
            ru: ru.into(),
            en: en.into(),
        },
        instruction: "Do it".into(),
        needs_my_answer: false,
        vision: false,
        reasoning: None,
    }
}

/// Schema 1 -> 2: the old plain-bool `reasoning = false` of a template means "follow the global setting".
#[test]
fn templates_reasoning_migrates_from_bool_to_tristate() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("settings.toml");
    fs::copy(
        concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/legacy-settings.toml"),
        &p,
    )
    .unwrap();
    let raw = fs::read_to_string(&p).unwrap();
    assert!(raw.contains("schema_version = 1") && raw.contains("reasoning = false"));
    let s = FileSettingsStore::open(d.path()).unwrap().get();
    assert_eq!(s.schema_version, 2);
    assert!(s.templates.iter().all(|t| t.reasoning.is_none()));
    // a forced-on template survives the migration, and so does a forced-off one chosen after it
    let st = FileSettingsStore::open(d.path()).unwrap();
    let mut t = TemplateSet::from_settings(&st.get());
    let mut on = mine("on", "Вкл", "On");
    on.reasoning = Some(true);
    let mut off = mine("off", "Выкл", "Off");
    off.reasoning = Some(false);
    t.upsert(on).unwrap();
    t.upsert(off).unwrap();
    st.set_templates(&t).unwrap();
    let again = FileSettingsStore::open(d.path()).unwrap().get();
    assert_eq!(
        again.templates.iter().find(|t| t.id == "on").unwrap().reasoning,
        Some(true)
    );
    assert_eq!(
        again.templates.iter().find(|t| t.id == "off").unwrap().reasoning,
        Some(false)
    );
    assert!(again
        .templates
        .iter()
        .find(|t| t.id == "brief")
        .unwrap()
        .reasoning
        .is_none());
}

/// Create / edit / duplicate / reorder / delete / restore through the store; everything persists.
#[test]
fn template_crud_persists_and_keeps_order() {
    let d = tempfile::tempdir().unwrap();
    let s = FileSettingsStore::open(d.path()).unwrap();
    let ids = |s: &FileSettingsStore| s.get().templates.iter().map(|t| t.id.clone()).collect::<Vec<_>>();
    let mut t = TemplateSet::from_settings(&s.get());
    t.upsert(mine("a", "Первый", "First")).unwrap();
    t.duplicate("a", "a2").unwrap();
    s.set_templates(&t).unwrap();
    assert_eq!(ids(&s), ["brief", "complement", "summary", "a", "a2"]);
    let copy = s.get().templates.into_iter().find(|t| t.id == "a2").unwrap();
    assert_eq!(copy.names.en, "First (copy)");
    assert_eq!(copy.instruction, "Do it");

    t.move_to("a2", 0).unwrap();
    s.set_templates(&t).unwrap();
    assert_eq!(
        ids(&FileSettingsStore::open(d.path()).unwrap()),
        ["a2", "brief", "complement", "summary", "a"]
    );

    // edit a built-in, then restore: the shipped text returns, user templates stay
    let mut b = t.get("brief").unwrap().clone();
    b.instruction = "Другое".into();
    t.upsert(b).unwrap();
    t.remove("a").unwrap();
    t.restore_builtins();
    assert_eq!(
        t.get("brief").unwrap().instruction,
        TemplateSet::defaults().get("brief").unwrap().instruction
    );
    s.set_templates(&t).unwrap();
    assert_eq!(ids(&s), ["a2", "brief", "complement", "summary"]);
    assert!(t.remove("complement").is_err());
}

/// The store refuses a template list the UI could never have produced.
#[test]
fn template_patch_validation() {
    let d = tempfile::tempdir().unwrap();
    let s = FileSettingsStore::open(d.path()).unwrap();
    let with = |extra: Template| {
        let mut t = TemplateSet::from_settings(&s.get());
        t.upsert(extra).unwrap();
        s.set_templates(&t)
    };
    assert!(
        with(mine("n", "Кратко", "Other")).is_err(),
        "ru name taken (case-insensitive)"
    );
    assert!(with(mine("n", "Новый", "brief")).is_err(), "en name taken");
    assert!(with(mine("n", " ", "New")).is_err(), "empty name");
    let mut e = mine("n", "Новый", "New");
    e.instruction = "  ".into();
    assert!(
        s.patch(SettingsPatch(serde_json::json!({ "templates": [e] })))
            .is_err(),
        "empty instruction"
    );
    let only_custom = serde_json::json!({ "templates": [mine("n", "Новый", "New")] });
    assert!(
        s.patch(SettingsPatch(only_custom)).is_err(),
        "built-ins cannot be deleted"
    );
    assert_eq!(s.get().templates.len(), 3, "nothing was written");
    with(mine("n", "Новый", "New")).unwrap();
}
