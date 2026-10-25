//! The Windows uninstaller must offer «also delete settings, models and logs» (unchecked by default) through
//! the Tauri NSIS hooks, remove the credential entries, and never touch the vault. NSIS itself is only built
//! in the release workflow, so this statically checks the wiring.
use std::path::Path;

fn read(rel: &str) -> String {
    std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(rel))
        .unwrap_or_else(|e| panic!("{rel}: {e}"))
}

#[test]
fn nsis_hook_file_is_referenced_in_tauri_conf() {
    let conf: serde_json::Value = serde_json::from_str(&read("tauri.conf.json")).unwrap();
    let nsis = &conf["bundle"]["windows"]["nsis"];
    assert_eq!(nsis["installMode"], "currentUser", "per-user, no admin");
    let hooks = nsis["installerHooks"].as_str().expect("installerHooks set");
    let body = read(hooks);
    assert!(body.contains("NSIS_HOOK_POSTUNINSTALL"));
    assert!(
        body.contains("$DeleteAppDataCheckboxState = 1"),
        "gated by the checkbox"
    );
    assert!(body.contains("RmDir /r \"$APPDATA\\Lowk\""));
    assert!(body.contains("cmdkey /delete:openrouter.") && body.contains("cmdkey /delete:stt-cloud."));
    assert!(
        !body.to_uppercase().contains("$DOCUMENTS"),
        "the vault (Documents\\Lowk) is never deleted"
    );
    for lang in ["English", "Russian"] {
        let f = nsis["customLanguageFiles"][lang]
            .as_str()
            .expect("custom language file");
        let t = read(f);
        assert!(t.contains("LangString deleteAppData"), "{lang}");
        assert!(
            t.contains("Documents"),
            "{lang}: the text says the notes are kept"
        );
    }
}

#[test]
fn credential_targets_match_the_secret_providers() {
    // hooks delete «<provider>.<service>»; the app stores providers under SERVICE = the bundle id.
    let conf: serde_json::Value = serde_json::from_str(&read("tauri.conf.json")).unwrap();
    assert_eq!(conf["identifier"], lowk_platform::secrets::SERVICE);
    assert!(read("src/commands/settings.rs").contains(r#"["openrouter", "stt-cloud"]"#));
}
