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
        "licenses" => licenses(std::env::args().nth(2).as_deref() == Some("--check")),
        "fixtures" | "audio-isolation" => {
            bail!("`{task}` is not implemented yet")
        }
        _ => {
            eprintln!("usage: cargo xtask <bindings|check-deps|licenses [--check]|fixtures|audio-isolation>");
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

/// Cargo packages that ship: the normal-dependency closure of `root_pkg` in the resolve graph
/// (all platforms and features, so the result is the same on every host). `(name, version, license, repository)`.
fn crate_licenses(
    manifest: Option<&str>,
    root_pkg: &str,
) -> Result<BTreeSet<(String, String, String, String)>> {
    let mut cmd = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
    cmd.current_dir(root())
        .args(["metadata", "--format-version", "1", "--locked", "--all-features"]);
    if let Some(m) = manifest {
        cmd.args(["--manifest-path", m]);
    }
    let out = cmd.output().context("cargo metadata")?;
    if !out.status.success() {
        bail!("cargo metadata failed: {}", String::from_utf8_lossy(&out.stderr));
    }
    let meta: serde_json::Value = serde_json::from_slice(&out.stdout)?;
    let pkgs: BTreeMap<&str, &serde_json::Value> = meta["packages"]
        .as_array()
        .context("packages")?
        .iter()
        .map(|p| (p["id"].as_str().unwrap_or_default(), p))
        .collect();
    let nodes: BTreeMap<&str, &serde_json::Value> = meta["resolve"]["nodes"]
        .as_array()
        .context("resolve")?
        .iter()
        .map(|n| (n["id"].as_str().unwrap_or_default(), n))
        .collect();
    let root_id = pkgs
        .iter()
        .find(|(_, p)| p["name"] == root_pkg)
        .map(|(id, _)| *id)
        .with_context(|| format!("package {root_pkg} not found"))?;
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    let mut stack = vec![root_id];
    while let Some(id) = stack.pop() {
        if !seen.insert(id) {
            continue;
        }
        for d in nodes[id]["deps"].as_array().into_iter().flatten() {
            let normal = d["dep_kinds"]
                .as_array()
                .is_some_and(|k| k.iter().any(|k| k["kind"].is_null()));
            if normal {
                stack.push(d["pkg"].as_str().unwrap_or_default());
            }
        }
    }
    Ok(seen
        .into_iter()
        .filter(|id| *id != root_id && !pkgs[id]["name"].as_str().unwrap_or_default().starts_with("lowk-"))
        .map(|id| {
            let p = pkgs[id];
            let get = |k: &str| p[k].as_str().unwrap_or_default().to_string();
            let lic = match get("license") {
                l if !l.is_empty() => l,
                _ => match p["license_file"].as_str() {
                    Some(_) => "see license file in the crate".to_string(),
                    None => "unknown".to_string(),
                },
            };
            (get("name"), get("version"), lic, get("repository"))
        })
        .collect())
}

fn esc(s: &str) -> String {
    s.replace('|', "\\|").replace('\n', " ")
}

/// Production frontend dependencies (`pnpm licenses`): `(name, version, license, homepage)`.
fn frontend_licenses() -> Result<BTreeSet<(String, String, String, String)>> {
    let out = Command::new("pnpm")
        .current_dir(root())
        .args(["--dir", "ui", "licenses", "list", "--prod", "--json"])
        .output()
        .context("running pnpm (is `pnpm --dir ui install` done?)")?;
    if !out.status.success() {
        bail!("pnpm licenses failed: {}", String::from_utf8_lossy(&out.stderr));
    }
    let v: serde_json::Value = serde_json::from_slice(&out.stdout)?;
    let mut set = BTreeSet::new();
    for (lic, pkgs) in v.as_object().context("pnpm licenses output")? {
        for p in pkgs.as_array().into_iter().flatten() {
            let name = p["name"].as_str().unwrap_or_default().to_string();
            let home = p["homepage"].as_str().unwrap_or_default().to_string();
            for ver in p["versions"].as_array().into_iter().flatten() {
                set.insert((
                    name.clone(),
                    ver.as_str().unwrap_or_default().to_string(),
                    lic.clone(),
                    home.clone(),
                ));
            }
        }
    }
    Ok(set)
}

fn link(url: &str) -> String {
    if url.is_empty() {
        "-".into()
    } else {
        format!("<{url}>")
    }
}

/// Builds the text of THIRD_PARTY.md.
fn third_party() -> Result<String> {
    use std::fmt::Write as _;
    let comp: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(
        root().join("tools/licenses/components.json"),
    )?)?;
    let app = crate_licenses(None, "lowk-app")?;
    let worker_all = crate_licenses(Some("workers/lowk-stt-worker/Cargo.toml"), "lowk-stt-worker")?;
    let app_names: BTreeSet<(&str, &str)> = app.iter().map(|c| (c.0.as_str(), c.1.as_str())).collect();
    let worker: Vec<_> = worker_all
        .iter()
        .filter(|c| !app_names.contains(&(c.0.as_str(), c.1.as_str())))
        .collect();
    let front = frontend_licenses()?;

    let mut o = String::new();
    o.push_str("# Third-party components\n\n");
    o.push_str("<!-- Generated by `just licenses` (cargo xtask licenses). Do not edit by hand: CI fails when this file is stale. -->\n\n");
    o.push_str("Lowk is free software under the GNU Affero General Public License, version 3 or (at your option) any later version (see `LICENSE`). ");
    o.push_str("It includes and downloads the components below, each under its own licence.\n\n");

    o.push_str(
        "## Bundled native components\n\n| Component | Version | Licence | Source |\n|---|---|---|---|\n",
    );
    for c in comp["native"].as_array().context("native")? {
        let g = |k: &str| c[k].as_str().unwrap_or_default();
        writeln!(
            o,
            "| {} | {} | [{}]({}) | {} |",
            esc(g("name")),
            esc(g("version")),
            g("license"),
            g("license_url"),
            link(g("url"))
        )?;
    }

    o.push_str("\n## Downloadable models\n\nModels are not part of the app: they are downloaded on demand from the source below and checked against a pinned SHA-256.\n\n");
    o.push_str("| Model | Used for | Licence | Source |\n|---|---|---|---|\n");
    for m in comp["models"].as_array().context("models")? {
        let g = |k: &str| m[k].as_str().unwrap_or_default();
        writeln!(
            o,
            "| {} | {} | [{}]({}) | {} |",
            esc(g("name")),
            esc(g("purpose")),
            g("license"),
            g("license_url"),
            link(g("source_url"))
        )?;
    }

    writeln!(
        o,
        "\n## Frontend packages ({})\n\n| Package | Version | Licence | Homepage |\n|---|---|---|---|",
        front.len()
    )?;
    for (n, v, l, h) in &front {
        writeln!(o, "| {} | {} | {} | {} |", esc(n), esc(v), esc(l), link(h))?;
    }

    writeln!(
        o,
        "\n## Rust crates in the app ({})\n\n| Crate | Version | Licence | Repository |\n|---|---|---|---|",
        app.len()
    )?;
    for (n, v, l, r) in &app {
        writeln!(o, "| {} | {} | {} | {} |", esc(n), esc(v), esc(l), link(r))?;
    }

    writeln!(o, "\n## Rust crates in the speech worker only ({})\n\n| Crate | Version | Licence | Repository |\n|---|---|---|---|", worker.len())?;
    for (n, v, l, r) in worker {
        writeln!(o, "| {} | {} | {} | {} |", esc(n), esc(v), esc(l), link(r))?;
    }
    Ok(o)
}

/// `cargo xtask licenses` writes THIRD_PARTY.md; `--check` fails when the committed file differs.
fn licenses(check: bool) -> Result<()> {
    let text = third_party()?;
    let path = root().join("THIRD_PARTY.md");
    if check {
        let cur = std::fs::read_to_string(&path).unwrap_or_default();
        if cur != text {
            bail!("THIRD_PARTY.md is out of date: run `just licenses` and commit the result");
        }
        println!("THIRD_PARTY.md: up to date");
    } else {
        std::fs::write(&path, text)?;
        println!("wrote {}", path.display());
    }
    Ok(())
}
