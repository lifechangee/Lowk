//! `settings.toml` in the app config dir (never in the vault): defaults for
//! every key, unknown keys preserved, atomic writes, JSON-merge-patch updates
//! and subscriptions.
use crate::templates::TemplateSet;
use crate::util::*;
use lowk_types::*;
use parking_lot::Mutex;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};

pub const SETTINGS_FILE: &str = "settings.toml";

pub struct FileSettingsStore {
    path: PathBuf,
    cur: Mutex<Settings>,
    /// The file as last read/written, as JSON: source of unknown keys.
    raw: Mutex<Value>,
    subs: Mutex<Vec<Sender<Settings>>>,
    /// Set when the file existed but could not be parsed (it was backed up).
    warning: Mutex<Option<String>>,
}

fn strip_nulls(v: &mut Value) {
    match v {
        Value::Object(m) => {
            m.retain(|_, x| !x.is_null());
            m.values_mut().for_each(strip_nulls);
        }
        Value::Array(a) => {
            a.retain(|x| !x.is_null());
            a.iter_mut().for_each(strip_nulls);
        }
        _ => {}
    }
}

/// Copy keys present in `raw` but unknown to the typed output (recursively
/// through objects) so hand-added or newer-version keys survive a rewrite.
fn keep_unknown(typed: &mut Value, raw: &Value) {
    if let (Value::Object(t), Value::Object(r)) = (typed, raw) {
        for (k, rv) in r {
            // `id` belongs to Target::App. Preserve unknown settings fields,
            // but do not resurrect a stale app id from raw TOML for the unit
            // `whole_system` variant.
            if k == "id" && t.get("kind").and_then(Value::as_str) == Some("whole_system") {
                continue;
            }
            match t.get_mut(k) {
                Some(tv) => keep_unknown(tv, rv),
                None => {
                    t.insert(k.clone(), rv.clone());
                }
            }
        }
    }
}

fn migrate_hold_delay(settings: &mut Settings) -> bool {
    if settings.schema_version < 3 && settings.hold_delay_ms == 500 {
        settings.hold_delay_ms = 250;
        true
    } else {
        false
    }
}

impl FileSettingsStore {
    /// Open `<config_dir>/settings.toml`; a missing file yields defaults (and is
    /// written with the built-in templates), a corrupt one is backed up to
    /// `settings.toml.bad` and replaced by defaults.
    pub fn open(config_dir: &Path) -> Result<Self> {
        fs::create_dir_all(config_dir)?;
        let path = config_dir.join(SETTINGS_FILE);
        let mut warning = None;
        let (settings, raw, fresh, parse_warning, visibility_migrated) = match fs::read_to_string(&path) {
            Ok(t) => match Self::parse(&t) {
                Ok((s, raw, parse_warning, migrated)) => (s, raw, false, parse_warning, migrated),
                Err(e) => {
                    let _ = fs::copy(&path, config_dir.join("settings.toml.bad"));
                    warning = e.detail.clone();
                    (Settings::default(), Value::Null, true, None, false)
                }
            },
            Err(_) => (Settings::default(), Value::Null, true, None, false),
        };
        if let Some(w) = parse_warning {
            tracing::warn!(warning = %w, "ignored malformed settings field");
            warning = Some(w);
        }
        let recovered_field = warning.is_some();
        let mut settings = settings;
        let migrated = visibility_migrated || (!fresh && settings.migrate_binds(Os::current()));
        let migrated = (!fresh && settings.migrate_templates()) || migrated;
        let migrated = (!fresh && migrate_hold_delay(&mut settings)) || migrated;
        if fresh {
            settings.templates = TemplateSet::defaults().into_vec();
        }
        let store = Self {
            path,
            cur: Mutex::new(settings),
            raw: Mutex::new(raw),
            subs: Mutex::new(vec![]),
            warning: Mutex::new(warning),
        };
        let needs_write = fresh
            || migrated
            || recovered_field
            || store.cur.lock().schema_version != SETTINGS_SCHEMA_VERSION;
        if needs_write {
            store.cur.lock().schema_version = SETTINGS_SCHEMA_VERSION;
            store.write()?;
        }
        Ok(store)
    }

    fn parse(text: &str) -> Result<(Settings, Value, Option<String>, bool)> {
        let bad = |e: String| err(ErrorCode::SettingsInvalid, e);
        let t: toml::Value = toml::from_str(text).map_err(|e| bad(e.to_string()))?;
        let mut raw = serde_json::to_value(&t).map_err(|e| bad(e.to_string()))?;
        let visibility_migrated = raw.pointer_mut("/overlay/visibility").is_some_and(|v| {
            if v.as_str() == Some("always") {
                *v = Value::String("recording_only".into());
                true
            } else {
                false
            }
        });
        match serde_json::from_value::<Settings>(raw.clone()) {
            Ok(s) => Ok((s, raw, None, visibility_migrated)),
            Err(original) => {
                // Settings are a single typed struct, so recover the known
                // optional source field independently and preserve all others.
                let source = raw
                    .get_mut("audio")
                    .and_then(Value::as_object_mut)
                    .and_then(|audio| audio.remove("last_source"));
                let Some(source) = source else {
                    return Err(bad(original.to_string()));
                };
                let s =
                    serde_json::from_value::<Settings>(raw.clone()).map_err(|_| bad(original.to_string()))?;
                Ok((
                    s,
                    raw,
                    Some(format!("audio.last_source ignored: {source}")),
                    visibility_migrated,
                ))
            }
        }
    }

    fn render(&self, s: &Settings) -> Result<String> {
        let mut v = serde_json::to_value(s).map_err(|e| err(ErrorCode::SettingsInvalid, e.to_string()))?;
        strip_nulls(&mut v);
        keep_unknown(&mut v, &self.raw.lock());
        let t = toml::Value::try_from(&v).map_err(|e| err(ErrorCode::SettingsInvalid, e.to_string()))?;
        let body = toml::to_string_pretty(&t).map_err(|e| err(ErrorCode::SettingsInvalid, e.to_string()))?;
        Ok(format!(
            "# Lowk settings. Safe to edit while the app is closed.\n{body}"
        ))
    }

    fn write(&self) -> Result<()> {
        let s = self.cur.lock().clone();
        let text = self.render(&s)?;
        atomic_write(&self.path, text.as_bytes())?;
        if let Ok((_, raw, _, _)) = Self::parse(&text) {
            *self.raw.lock() = raw;
        }
        Ok(())
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Reason the previous file was discarded at open time, if any.
    pub fn load_warning(&self) -> Option<String> {
        self.warning.lock().clone()
    }

    /// Re-read the file after an external edit; notifies subscribers on change.
    pub fn reload(&self) -> Result<Settings> {
        let text = fs::read_to_string(&self.path)?;
        let (mut s, raw, parse_warning, visibility_migrated) = Self::parse(&text)?;
        if let Some(w) = &parse_warning {
            tracing::warn!(warning = %w, "ignored malformed settings field");
            *self.warning.lock() = Some(w.clone());
        }
        *self.raw.lock() = raw;
        // S2: an externally restored old file (swapped screenshot binds) is repaired like on open.
        let migrated = visibility_migrated || s.migrate_binds(Os::current());
        let migrated = migrate_hold_delay(&mut s) || migrated;
        let old_schema = s.schema_version < SETTINGS_SCHEMA_VERSION;
        if old_schema {
            s.schema_version = SETTINGS_SCHEMA_VERSION;
        }
        let changed = {
            let mut cur = self.cur.lock();
            let changed = *cur != s;
            *cur = s.clone();
            changed
        };
        if migrated || old_schema || parse_warning.is_some() {
            self.write()?;
        }
        if changed {
            self.notify(&s);
        }
        Ok(s)
    }

    fn notify(&self, s: &Settings) {
        self.subs.lock().retain(|tx| tx.send(s.clone()).is_ok());
    }

    /// Replace the prompt templates (e.g. after editing them in the UI).
    pub fn set_templates(&self, t: &TemplateSet) -> Result<Settings> {
        self.patch(t.to_patch())
    }
}

impl SettingsStore for FileSettingsStore {
    fn get(&self) -> Settings {
        self.cur.lock().clone()
    }

    fn patch(&self, p: SettingsPatch) -> Result<Settings> {
        let next = {
            let mut cur = self.cur.lock();
            let next = cur.apply_patch(&p)?;
            if next == *cur {
                return Ok(next);
            }
            if next.templates != cur.templates {
                next.validate_templates()?;
            }
            let old = std::mem::replace(&mut *cur, next.clone());
            drop(cur);
            if let Err(e) = self.write() {
                *self.cur.lock() = old;
                return Err(e);
            }
            next
        };
        self.notify(&next);
        Ok(next)
    }

    fn subscribe(&self) -> Receiver<Settings> {
        let (tx, rx) = channel();
        self.subs.lock().push(tx);
        rx
    }
}
