use lowk_store::*;
use lowk_types::*;
use std::fs;

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
        reasoning: false,
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
        reasoning: false,
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
