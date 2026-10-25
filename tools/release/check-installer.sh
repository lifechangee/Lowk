#!/usr/bin/env bash
# CI-friendly check (no NSIS needed): the installer hook, the branding files and the dmg background exist, are
# referenced from tauri.conf.json, have the right BMP format/size, and the hook still carries the macros Tauri calls.
set -euo pipefail
cd "$(dirname "$0")/../.."
app=crates/lowk-app
conf=$app/tauri.conf.json
fail() { echo "check-installer: $*" >&2; exit 1; }

python3 - "$conf" <<'PY'
import json, struct, sys, pathlib
conf = pathlib.Path(sys.argv[1])
base = conf.parent
c = json.loads(conf.read_text())["bundle"]
nsis = c["windows"]["nsis"]
refs = {
    "installerHooks": nsis.get("installerHooks"),
    "installerIcon": nsis.get("installerIcon"),
    "uninstallerIcon": nsis.get("uninstallerIcon"),
    "sidebarImage": nsis.get("sidebarImage"),
    "headerImage": nsis.get("headerImage"),
    "uninstallerHeaderImage": nsis.get("uninstallerHeaderImage"),
    "licenseFile": c.get("licenseFile"),
    "dmg.background": c.get("macOS", {}).get("dmg", {}).get("background"),
}
for lang, f in nsis.get("customLanguageFiles", {}).items():
    refs[f"language {lang}"] = f
errs = []
for name, rel in refs.items():
    if not rel:
        errs.append(f"{name} is not set in tauri.conf.json")
    elif not (base / rel).is_file():
        errs.append(f"{name}: {rel} does not exist")
def bmp(rel, w, h):
    d = (base / rel).read_bytes()
    if d[:2] != b"BM":
        return f"{rel}: not a BMP"
    ww, hh, _, bpp = struct.unpack("<iiHH", d[18:30])
    if (ww, abs(hh), bpp) != (w, h, 24):
        return f"{rel}: {ww}x{abs(hh)} {bpp}-bit, expected {w}x{h} 24-bit"
for rel, w, h in ((nsis.get("sidebarImage"), 164, 314), (nsis.get("headerImage"), 150, 57)):
    if rel and (base / rel).is_file():
        e = bmp(rel, w, h)
        if e: errs.append(e)
if c.get("licenseFile") and "AFFERO" not in (base / c["licenseFile"]).read_text(errors="ignore")[:200].upper():
    errs.append("licenseFile is not the AGPL")
if nsis.get("installMode") != "currentUser":
    errs.append("nsis.installMode must be currentUser (per-user, no admin)")
for lang in ("Russian", "English"):
    if lang not in nsis.get("languages", []):
        errs.append(f"nsis.languages lacks {lang}")
if errs:
    print("check-installer: " + "\n  ".join(errs), file=sys.stderr)
    sys.exit(1)
PY

hook=$app/windows/nsis/hooks.nsh
for m in NSIS_HOOK_PREINSTALL NSIS_HOOK_POSTINSTALL NSIS_HOOK_PREUNINSTALL NSIS_HOOK_POSTUNINSTALL; do
  grep -q "^!macro $m\$" "$hook" || fail "$hook lacks !macro $m"
done
grep -q -- '--purge-user-data' "$hook" || fail "hook does not call the app's --purge-user-data"
grep -q -- '"--purge-user-data"' $app/src/main.rs || fail "main.rs does not handle --purge-user-data"
grep -q 'lowk-stt-worker.exe' "$hook" || fail "hook does not stop the STT worker"
# every !macro has its !macroend
[ "$(grep -c '^!macro ' "$hook")" = "$(grep -c '^!macroend' "$hook")" ] || fail "unbalanced !macro / !macroend in hooks.nsh"
echo "check-installer: ok"
