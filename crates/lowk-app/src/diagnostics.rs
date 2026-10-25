//! Diagnostics export («Save diagnostics») and the panic hook.
//!
//! The zip holds what a bug report needs and nothing personal: logs, versions, OS and hardware
//! summary, engine status and the settings. Everything goes through the redaction below first:
//! API keys (by shape and by exact value from the secret store), `Authorization` headers, the home
//! directory, every absolute path in the settings and the user's prompt templates. The vault is never read.
use lowk_types::*;
use regex::{Captures, Regex};
use serde_json::{json, Value};
use std::io::Write;
use std::path::Path;
use std::sync::OnceLock;

pub const REDACTED: &str = "[redacted]";

/// Everything the export is built from (collected by the command; plain data so it can be tested).
pub struct Report {
    pub version: String,
    pub commit: String,
    pub build_date: String,
    pub hw: HwProbe,
    pub engines: Value,
    pub settings: Value,
    pub log: Option<String>,
    pub prev_log: Option<String>,
    /// The user's home directory (replaced by `~` everywhere).
    pub home: String,
    /// Values currently held in the secret store (removed verbatim from every text).
    pub secrets: Vec<String>,
    /// Seconds since the Unix epoch.
    pub created: u64,
}

struct Patterns {
    bearer: Regex,
    key: Regex,
    google: Regex,
    github: Regex,
    assign: Regex,
}

fn patterns() -> &'static Patterns {
    static P: OnceLock<Patterns> = OnceLock::new();
    P.get_or_init(|| Patterns {
        bearer: Regex::new(r"(?i)\bBearer\s+[A-Za-z0-9._~+/=\-]{6,}").expect("regex"),
        key: Regex::new(r"\b(?:sk|pk|rk)-[A-Za-z0-9_\-]{6,}").expect("regex"),
        google: Regex::new(r"AIza[A-Za-z0-9_\-]{20,}").expect("regex"),
        github: Regex::new(r"\bgh[pousr]_[A-Za-z0-9]{20,}").expect("regex"),
        assign: Regex::new(
            r#"(?i)("?(?:api[_-]?key|authorization|access[_-]?token|token|secret|password|passwd)"?\s*[:=]\s*)("?)[^\s",;}\]]+"#,
        )
        .expect("regex"),
    })
}

/// Removes secrets from free text (logs, JSON strings).
pub fn redact_text(s: &str, home: &str, secrets: &[String]) -> String {
    let mut out = s.to_string();
    for v in secrets.iter().filter(|v| v.trim().len() >= 4) {
        out = out.replace(v.trim(), REDACTED);
    }
    let p = patterns();
    out = p.bearer.replace_all(&out, "Bearer [redacted]").into_owned();
    out = p.key.replace_all(&out, "[redacted-key]").into_owned();
    out = p.google.replace_all(&out, "[redacted-key]").into_owned();
    out = p.github.replace_all(&out, "[redacted-key]").into_owned();
    out = p
        .assign
        .replace_all(&out, |c: &Captures| format!("{}{}{REDACTED}", &c[1], &c[2]))
        .into_owned();
    scrub_home(&out, home)
}

/// `/Users/ivan/Documents` -> `~/Documents` (also with backslashes and forward slashes on Windows).
fn scrub_home(s: &str, home: &str) -> String {
    let home = home.trim_end_matches(['/', '\\']);
    if home.len() < 3 {
        return s.to_string();
    }
    let mut out = s.replace(home, "~");
    let fwd = home.replace('\\', "/");
    if fwd != home {
        out = out.replace(&fwd, "~");
    }
    out
}

fn looks_like_path(s: &str) -> bool {
    let b = s.as_bytes();
    s.starts_with('/')
        || s.starts_with("~/")
        || s.starts_with("\\\\")
        || (b.len() > 2 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b[2] == b'\\' || b[2] == b'/'))
}

fn secret_key(k: &str) -> bool {
    let k = k.to_ascii_lowercase();
    [
        "key",
        "token",
        "secret",
        "password",
        "passwd",
        "authorization",
        "credential",
    ]
    .iter()
    .any(|w| k.contains(w))
}

/// Settings with all secrets and personal paths removed. Structure and values (booleans, numbers, ids,
/// model names) stay, because that is what makes a bug report reproducible.
pub fn redact_settings(v: &Value, home: &str, secrets: &[String]) -> Value {
    fn walk(k: &str, v: &Value, home: &str, secrets: &[String]) -> Value {
        match v {
            Value::Object(m) => Value::Object(
                m.iter()
                    .map(|(k2, v2)| {
                        // user-written prompt templates are personal text: keep only how many there are
                        if k2 == "templates" {
                            let n = v2.as_array().map_or(0, Vec::len);
                            return (k2.clone(), json!({ "count": n, "content": REDACTED }));
                        }
                        (k2.clone(), walk(k2, v2, home, secrets))
                    })
                    .collect(),
            ),
            Value::Array(a) => Value::Array(a.iter().map(|x| walk(k, x, home, secrets)).collect()),
            Value::String(s) if s.is_empty() => v.clone(),
            Value::String(s) => {
                if secret_key(k) {
                    return json!(REDACTED);
                }
                if k.ends_with("base_url") && !(s.contains("localhost") || s.contains("127.0.0.1")) {
                    return json!("[custom url]");
                }
                if s != DEFAULT_VAULT_PATH && looks_like_path(s) {
                    return json!("[path]");
                }
                json!(redact_text(s, home, secrets))
            }
            _ => v.clone(),
        }
    }
    walk("", v, home, secrets)
}

/// Writes the zip to `dest` (via a temp file, so a failure never leaves a half-written archive).
pub fn write_zip(dest: &Path, r: &Report) -> Result<u64> {
    let tmp = dest.with_extension("zip.part");
    let res = write_zip_inner(&tmp, r);
    match res {
        Ok(()) => {
            std::fs::rename(&tmp, dest).map_err(io_err)?;
            Ok(std::fs::metadata(dest).map(|m| m.len()).unwrap_or(0))
        }
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(e)
        }
    }
}

fn io_err(e: std::io::Error) -> AppError {
    AppError::new(ErrorCode::Io).with_detail(e.to_string())
}

fn zip_err(e: zip::result::ZipError) -> AppError {
    AppError::new(ErrorCode::Io).with_detail(e.to_string())
}

fn write_zip_inner(path: &Path, r: &Report) -> Result<()> {
    let clean = |s: &str| redact_text(s, &r.home, &r.secrets);
    let pretty = |v: &Value| clean(&serde_json::to_string_pretty(v).unwrap_or_default());
    let info = &r.hw.report.info;
    let manifest = json!({
        "app": "Lowk",
        "version": r.version,
        "commit": r.commit,
        "build_date": r.build_date,
        "os": format!("{:?}", info.os),
        "os_version": info.os_version,
        "arch": info.arch,
        "created_unix": r.created,
    });
    let hardware = serde_json::to_value(&r.hw).unwrap_or(Value::Null);
    let settings = redact_settings(&r.settings, &r.home, &r.secrets);

    let mut z = zip::ZipWriter::new(std::fs::File::create(path).map_err(io_err)?);
    let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let mut put = |name: &str, body: &str| -> Result<()> {
        z.start_file(name, opts).map_err(zip_err)?;
        z.write_all(body.as_bytes()).map_err(io_err)
    };
    put("manifest.json", &pretty(&manifest))?;
    put("hardware.json", &pretty(&hardware))?;
    put("engines.json", &pretty(&r.engines))?;
    put("settings.json", &pretty(&settings))?;
    if let Some(l) = &r.log {
        put("lowk.log", &clean(l))?;
    }
    if let Some(l) = &r.prev_log {
        put("lowk.prev.log", &clean(l))?;
    }
    z.finish().map_err(zip_err)?;
    Ok(())
}

/// Text written to the log when the app panics.
pub fn panic_report(thread: &str, message: &str, location: &str, backtrace: &str) -> String {
    format!("PANIC in thread '{thread}' at {location}: {message}\nbacktrace:\n{backtrace}\n")
}

/// Panic hook: the message and a backtrace go to `lowk.log` (and stderr through the default hook)
/// before the process exits; a bundle has no terminal, so without this a crash leaves no trace.
pub fn install_panic_hook() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let message = match info.payload().downcast_ref::<&str>() {
            Some(s) => (*s).to_string(),
            None => info
                .payload()
                .downcast_ref::<String>()
                .cloned()
                .unwrap_or_else(|| "non-string panic payload".into()),
        };
        let location = info
            .location()
            .map_or_else(|| "unknown".into(), |l| l.to_string());
        let bt = std::backtrace::Backtrace::force_capture().to_string();
        let thread = std::thread::current().name().unwrap_or("unnamed").to_string();
        let text = panic_report(&thread, &message, &location, &bt);
        let _ = crate::log_raw(&text);
        default(info);
    }));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    const FAKE_OR_KEY: &str = "sk-or-v1-0123456789abcdefFAKEFAKE";
    const FAKE_STORE: &str = "zq9-store-value-NOT-A-KEY-SHAPE";

    fn hw() -> HwProbe {
        let report = HwReport {
            info: HwInfo {
                os: Os::Macos,
                os_version: "26.0".into(),
                arch: "aarch64".into(),
                chip: "Apple M4 Pro".into(),
                ram_gb: 24.0,
                gpus: vec![],
                cpu_features: vec![],
                free_disk_gb: 100.0,
                on_battery: false,
            },
            physical_cores: 10,
            logical_cores: 10,
            apple_silicon_gen: Some(4),
        };
        let recommendation = lowk_platform::hw::recommend(&report);
        HwProbe {
            report,
            recommendation,
        }
    }

    fn zip_text(path: &Path) -> Vec<(String, String)> {
        let mut z = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
        (0..z.len())
            .map(|i| {
                let mut f = z.by_index(i).unwrap();
                let mut s = String::new();
                f.read_to_string(&mut s).unwrap();
                (f.name().unwrap().to_string(), s)
            })
            .collect()
    }

    /// The export never contains key-like strings, the secret store values, the home directory or vault content.
    #[test]
    fn export_has_no_keys_secrets_or_vault_content() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home").join("ivan");
        let vault = home.join("Documents").join("Lowk");
        std::fs::create_dir_all(&vault).unwrap();
        let note = "CONFIDENTIAL-NOTE: we will fire Petrov on Friday";
        std::fs::write(vault.join("call.md"), note).unwrap();

        let mut settings = serde_json::to_value(Settings::default()).unwrap();
        settings["vault"]["path"] = json!(vault.display().to_string());
        settings["llm"]["base_url"] = json!(format!("https://{FAKE_OR_KEY}@proxy.corp.example/v1"));
        settings["llm"]["model"] = json!("acme/some-model");
        settings["templates"][0]["body"] = json!("PRIVATE PROMPT about Petrov");
        settings["extra_api_key"] = json!(FAKE_OR_KEY);

        let log = format!(
            "INFO startup\nWARN request failed Authorization: Bearer {FAKE_OR_KEY}\n\
             ERROR url=https://x.example/?api_key={FAKE_STORE} home={} vault={}\n\
             INFO key sk-proj-ABCDEFGH12345678 and token: {FAKE_STORE}\n",
            home.display(),
            vault.display()
        );
        let r = Report {
            version: "0.0.1".into(),
            commit: "abc123".into(),
            build_date: "2026-10-25".into(),
            hw: hw(),
            engines: json!([{ "id": "nemotron-local", "installed": true }]),
            settings,
            log: Some(log.clone()),
            prev_log: Some(log),
            home: home.display().to_string(),
            secrets: vec![FAKE_OR_KEY.into(), FAKE_STORE.into()],
            created: 1,
        };
        let dest = dir.path().join("out.zip");
        write_zip(&dest, &r).unwrap();
        let files = zip_text(&dest);
        let names: Vec<&str> = files.iter().map(|(n, _)| n.as_str()).collect();
        for want in [
            "manifest.json",
            "hardware.json",
            "engines.json",
            "settings.json",
            "lowk.log",
            "lowk.prev.log",
        ] {
            assert!(names.contains(&want), "{want} missing in {names:?}");
        }
        let all: String = files
            .iter()
            .map(|(_, t)| t.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        for bad in [
            FAKE_OR_KEY,
            FAKE_STORE,
            "sk-proj-ABCDEFGH",
            "sk-or-",
            "CONFIDENTIAL-NOTE",
            "Petrov",
            "PRIVATE PROMPT",
            "/ivan",
            "proxy.corp.example",
        ] {
            assert!(!all.contains(bad), "export leaks {bad:?}:\n{all}");
        }
        assert!(!all.contains("Bearer abc") && !all.contains(&format!("Bearer {FAKE_OR_KEY}")));
        // still useful: versions, hardware, non-secret settings
        let manifest = &files.iter().find(|(n, _)| n == "manifest.json").unwrap().1;
        assert!(manifest.contains("0.0.1") && manifest.contains("abc123") && manifest.contains("26.0"));
        assert!(all.contains("acme/some-model"));
        assert!(all.contains("Apple M4 Pro"));
    }

    #[test]
    fn redaction_catches_key_shapes_and_headers() {
        for s in [
            "sk-or-v1-abcdef123456",
            "Authorization: Bearer abc.def-123_456",
            "bearer ABCDEF123456",
            "AIzaSyA1234567890abcdefghijklmnop",
            "ghp_abcdefghijklmnopqrstuvwxyz0123",
            r#"{"api_key":"hunter2hunter2"}"#,
            "password=hunter2",
        ] {
            let out = redact_text(s, "", &[]);
            assert!(
                !out.contains("hunter2") && !out.contains("abcdef12") && !out.contains("A1234567890"),
                "{s} -> {out}"
            );
        }
        assert_eq!(
            redact_text("task-queue disk-usage", "", &[]),
            "task-queue disk-usage"
        );
        assert_eq!(redact_text("/Users/ivan/x", "/Users/ivan", &[]), "~/x");
    }

    #[test]
    fn settings_keep_structure_but_drop_paths_and_templates() {
        let s = serde_json::to_value(Settings::default()).unwrap();
        let r = redact_settings(&s, "/Users/ivan", &[]);
        assert_eq!(r["vault"]["path"], DEFAULT_VAULT_PATH);
        assert_eq!(r["schema_version"], s["schema_version"]);
        assert_eq!(r["templates"]["content"], REDACTED);
    }

    #[test]
    fn panic_report_has_message_location_and_backtrace() {
        let t = panic_report("main", "boom", "src/x.rs:1:2", "0: frame");
        assert!(
            t.contains("PANIC") && t.contains("boom") && t.contains("src/x.rs:1:2") && t.contains("0: frame")
        );
    }
}
