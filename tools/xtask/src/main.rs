//! `cargo xtask <task>`: repo automation.
use anyhow::{bail, Context, Result};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::process::Command;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root")
}

fn main() -> Result<()> {
    let task = std::env::args().nth(1).unwrap_or_default();
    match task.as_str() {
        "bindings" => bindings(),
        "check-deps" => check_deps(),
        "license-report" | "fixtures" | "audio-isolation" => {
            bail!("`{task}` is not implemented yet (M0 skeleton)")
        }
        _ => {
            eprintln!("usage: cargo xtask <bindings|check-deps|license-report|fixtures|audio-isolation>");
            std::process::exit(2);
        }
    }
}

/// Regenerates `ui/src/lib/ipc/bindings.ts` from the Rust types and commands.
fn bindings() -> Result<()> {
    let out = root().join("ui/src/lib/ipc/bindings.ts");
    std::fs::create_dir_all(out.parent().unwrap())?;
    let status = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
        .current_dir(root())
        .args([
            "run",
            "--quiet",
            "-p",
            "lowk-app",
            "--example",
            "export_bindings",
            "--",
        ])
        .arg(&out)
        .status()
        .context("running export_bindings")?;
    if !status.success() {
        bail!("bindings export failed");
    }
    println!("wrote {}", out.display());
    Ok(())
}

/// Enforces section 1.2: feature crates depend only on `lowk-types` /
/// `lowk-platform`; `lowk-session` may use traits only (no feature crates yet);
/// only `lowk-app` (and xtask) may depend on `tauri*`.
fn check_deps() -> Result<()> {
    let out = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
        .current_dir(root())
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .output()?;
    let meta: serde_json::Value = serde_json::from_slice(&out.stdout)?;
    let mut deps: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for p in meta["packages"].as_array().context("packages")? {
        let name = p["name"].as_str().unwrap().to_string();
        let set = p["dependencies"]
            .as_array()
            .unwrap()
            .iter()
            .map(|d| d["name"].as_str().unwrap().to_string())
            .collect();
        deps.insert(name, set);
    }
    let mut bad = vec![];
    for (name, ds) in &deps {
        if !name.starts_with("lowk-") {
            continue;
        }
        for d in ds {
            if d.starts_with("tauri") && name != "lowk-app" {
                bad.push(format!("{name} must not depend on {d} (only lowk-app may)"));
            }
            if d.starts_with("lowk-") && name != "lowk-app" && name != "lowk-session" {
                let allowed = d == "lowk-types" || d == "lowk-platform";
                if !allowed {
                    bad.push(format!("{name} must not depend on {d}"));
                }
            }
            if name == "lowk-types" && d.starts_with("lowk-") {
                bad.push("lowk-types must not depend on any lowk crate".into());
            }
        }
    }
    if bad.is_empty() {
        println!("dependency rules: ok");
        Ok(())
    } else {
        bail!("{}", bad.join("\n"))
    }
}
