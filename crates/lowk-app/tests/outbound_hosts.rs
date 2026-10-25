//! Privacy lint: every network host named in the source is on this list, so a new outbound
//! connection cannot sneak in unnoticed. Adding a host here is a product decision (README network table).
use std::collections::BTreeSet;
use std::path::Path;

/// (host, why it may be contacted)
const ALLOWED: &[(&str, &str)] = &[
    // local servers (user's machine only)
    ("localhost", "local LLM / embed servers"),
    ("127.0.0.1", "local LLM / embed servers, sidecars"),
    // model downloads (on demand, user-triggered)
    ("huggingface.co", "model files"),
    // providers the user configures with their own key
    ("openrouter.ai", "LLM / embeddings"),
    ("api.deepgram.com", "cloud speech"),
    ("stt-rt.soniox.com", "cloud speech"),
    // updates: the daily check (when enabled) and the release download
    ("github.com", "update manifest + release download"),
    (
        "objects.githubusercontent.com",
        "release assets (GitHub redirect target)",
    ),
    // links the UI shows or opens, never fetched in the background
    ("openmdw.ai", "licence link"),
    ("ai.google.dev", "licence link"),
    ("soniox.com", "terms link"),
    ("deepgram.com", "terms link"),
    ("www.w3.org", "XML namespace, not a request"),
    ("ollama.com", "docs link"),
    ("lmstudio.ai", "docs link"),
];

fn walk(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if p.is_dir() {
            if !matches!(name, "target" | "node_modules" | "dist" | ".git") {
                walk(&p, out);
            }
        } else if matches!(
            p.extension().and_then(|e| e.to_str()),
            Some("rs" | "ts" | "svelte")
        ) && name != "bindings.ts"
        {
            out.push(p);
        }
    }
}

fn hosts_in(s: &str) -> BTreeSet<String> {
    let mut hosts = BTreeSet::new();
    for scheme in ["https://", "http://", "wss://", "ws://"] {
        for part in s.split(scheme).skip(1) {
            let host: String = part
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '.' || *c == '-')
                .collect();
            let host = host.trim_end_matches('.').to_string();
            if !host.is_empty() {
                hosts.insert(host);
            }
        }
    }
    hosts
}

#[test]
fn only_allowed_hosts_appear_in_source() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut files = vec![];
    walk(&root.join("crates"), &mut files);
    walk(&root.join("ui/src"), &mut files);
    assert!(files.len() > 100, "scanned only {} files", files.len());
    let allowed: BTreeSet<&str> = ALLOWED.iter().map(|(h, _)| *h).collect();
    let mut bad = vec![];
    for f in &files {
        // production code only: integration-test dirs and `#[cfg(test)]` tails use fixture hosts
        if f.components().any(|c| c.as_os_str() == "tests") {
            continue;
        }
        let text = std::fs::read_to_string(f).unwrap_or_default();
        let text = text.split("#[cfg(test)]").next().unwrap_or_default();
        for h in hosts_in(text) {
            if !allowed.contains(h.as_str()) {
                bad.push(format!("{}: {h}", f.strip_prefix(&root).unwrap_or(f).display()));
            }
        }
    }
    assert!(bad.is_empty(), "hosts not on the outbound allow-list: {bad:#?}");
}

#[test]
fn the_update_endpoint_is_github_releases_only() {
    let u = lowk_app::updater::ENDPOINT.parse::<url::Url>().unwrap();
    assert_eq!(u.scheme(), "https");
    assert_eq!(u.host_str(), Some("github.com"));
    assert!(u
        .path()
        .starts_with("/lifechangee/Lowk/releases/latest/download/"));
}
