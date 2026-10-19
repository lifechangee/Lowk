"""Convert owner recordings audio/NN_{me,them}_{q,n}.m4a -> 16 kHz mono WAV (afconvert) and write
reference .txt from docs/test-audio/SCRIPTS.md (scenario a..e = 01..05; Я -> me, Собеседник -> them)."""
import re, subprocess, pathlib
ROOT = pathlib.Path(__file__).resolve().parent.parent
S = (ROOT.parent.parent / "docs/test-audio/SCRIPTS.md").read_text()
secs = re.split(r"^### \((\w)\)", S, flags=re.M)[1:]
refs = {}
for k in range(0, len(secs), 2):
    n = "abcde".index(secs[k]) + 1; body = secs[k+1].split("\n---")[0]
    d = {"me": [], "them": []}
    for m in re.finditer(r"^\*\*(Я|Собеседник):\*\*\s*(.+)$", body, flags=re.M):
        t = re.sub(r"\([^)]*\)", "", m.group(2)).replace("`", "").replace("…", " ")
        d["me" if m.group(1) == "Я" else "them"].append(t.strip())
    refs[n] = {r: " ".join(v) for r, v in d.items()}
for f in sorted((ROOT / "audio").glob("0*_*.m4a")):
    wav = f.with_suffix(".wav")
    if not wav.exists():
        subprocess.run(["afconvert", "-f", "WAVE", "-d", "LEI16@16000", "-c", "1", str(f), str(wav)], check=True)
    nn, role, _ = f.stem.split("_")
    f.with_suffix(".txt").write_text(refs[int(nn)][role] + "\n"); print(wav.name)
