#!/usr/bin/env python3
"""Score applestt JSONL runs with the same method as spikes/stt-bench/harness/bench.py.
usage: score.py RUN_DIR PREFIX [--vad silero_vad.onnx] [--bench-json stt-bench result.json]
Prints a markdown table per set (noisy _n / quiet _q) and English-term stats."""
import sys, json, glob, pathlib, difflib, argparse
import numpy as np
HERE = pathlib.Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent / "stt-bench"))
from harness import common as C

ap = argparse.ArgumentParser()
ap.add_argument("rundir"); ap.add_argument("prefix")
ap.add_argument("--vad", required=True); ap.add_argument("--bench-json")
ap.add_argument("--clips", default="*"); ap.add_argument("--wavdir", required=True)
a = ap.parse_args()
WAVDIR = pathlib.Path(a.wavdir)
AUD = HERE.parent / "stt-bench" / "audio"
_vad = C.SileroVAD(a.vad)
def mask(x):
    _vad.reset(); n = len(x) // 512
    return np.array([_vad(x[i*512:(i+1)*512]) > 0.5 for i in range(n)])

def score_clip(name, events, ref, x):
    snaps = [(e["t"], e["text"]) for e in events if "text" in e]
    finals = [(t, s) for (t, s), e in zip(snaps, [e for e in events if "text" in e]) if e["final"]]
    hyp = finals[-1][1] if finals else (snaps[-1][1] if snaps else "")
    H, R = C.words(hyp), C.words(ref)
    m = mask(x); wend = C.word_end_times(R, m)
    sm = difflib.SequenceMatcher(None, R, H, autojunk=False)
    h2r = {}
    for aa, b, size in sm.get_matching_blocks():
        for k in range(size): h2r[b + k] = aa + k
    def lags(S):
        seen = {}
        for t, txt in S:
            w = C.words(txt)
            for i, wi in enumerate(w):
                if i < len(H) and wi == H[i] and i not in seen: seen[i] = t
        return [seen[i] - wend[h2r[i]] for i in h2r if i in seen]
    lv, lf = lags(snaps), lags(finals)
    rates = C.edit_rates(ref, hyp)
    summ = [e["summary"] for e in events if "summary" in e][0]
    # onset -> first partial
    act = np.flatnonzero(m); bursts = []
    if len(act):
        st = pv = act[0]
        for q in act[1:]:
            if q - pv > 15: bursts.append((st, pv)); st = q
            pv = q
        bursts.append((st, pv))
    onset = []
    for s, e in bursts:
        ts = s * 512 / 16000; base = 0
        for t, txt in snaps:
            if t <= ts: base = len(C.words(txt))
        for t, txt in snaps:
            if t > ts and len(C.words(txt)) > base: onset.append(t - ts); break
    return dict(clip=name, hyp=hyp, ref=ref, wer=rates["wer"], cer=rates["cer"], lag_volatile=lv, lag_final=lf, onset=onset,
                final_after_eos=summ["final_after_eos_s"], cpu_rtf=summ["cpu_s"] / summ["duration_s"], ram=summ["peak_rss_mb"],
                terms=C.english_terms(ref, hyp))

res = []
for f in sorted(glob.glob(f"{a.rundir}/{a.prefix}_{a.clips}.jsonl")):
    name = pathlib.Path(f).stem[len(a.prefix) + 1:]
    ev = [json.loads(l) for l in open(f) if l.strip()]
    ref = (AUD / f"{name}.txt").read_text()
    res.append(score_clip(name, ev, ref, C.load_audio(WAVDIR / f"{name}.wav")))
json.dump(res, open(f"{a.rundir}/{a.prefix}_scored.json", "w"), ensure_ascii=False, indent=1)

def pct(v, p): return float(np.percentile(v, p)) if len(v) else float("nan")
def row(label, rs):
    lv = [l for r in rs for l in r["lag_volatile"]]; lf = [l for r in rs for l in r["lag_final"]]
    on = [l for r in rs for l in r["onset"]]
    t = [x for r in rs for x in r["terms"]]
    ex = sum(1 for x in t if x[1] == "exact")
    print(f"| {label} | {len(rs)} | {np.mean([r['wer'] for r in rs]):.3f} | {np.mean([r['cer'] for r in rs]):.3f} | "
          f"{pct(lv,50):.2f}/{pct(lv,95):.2f} | {pct(lf,50):.2f}/{pct(lf,95):.2f} | {pct(on,50):.2f} | {np.mean([r['final_after_eos'] for r in rs]):.2f} | "
          f"{np.mean([r['cpu_rtf'] for r in rs]):.3f} | {max(r['ram'] for r in rs):.0f} | {ex}/{len(t)} ({100*ex/max(1,len(t)):.0f}%) |")
print("| set | clips | WER | CER | word lag volatile p50/p95 (s) | word lag final p50/p95 (s) | onset->1st partial p50 (s) | final after EOS (s) | CPU RTF | peak RAM MB | EN terms exact |")
print("|---|---|---|---|---|---|---|---|---|---|---|")
for lab, suf in [("noisy _n", "_n"), ("quiet _q", "_q")]:
    rs = [r for r in res if r["clip"].endswith(suf)]
    if rs: row(f"{a.prefix} {lab}", rs)
for lab, pre in [("RU+EN jargon 01-04", ("01", "02", "03", "04")), ("EN-heavy 05", ("05",))]:
    rs = [r for r in res if r["clip"][:2] in pre]
    if rs: row(f"{a.prefix} {lab}", rs)
if a.bench_json:
    # same aggregation for a stt-bench result file
    B = json.load(open(a.bench_json))
    print()
    for lab, suf in [("noisy _n", "_n"), ("quiet _q", "_q")]:
        rs = [r for r in B if r["clip"].endswith(suf + ".wav")]
        lv = [l for r in rs for l in r["word_lags_s"]]
        print(f"| bench {lab} | {len(rs)} | {np.mean([r['wer'] for r in rs]):.3f} | {np.mean([r['cer'] for r in rs]):.3f} | {pct(lv,50):.2f}/{pct(lv,95):.2f} | final {np.mean([r['final_latency_s'] for r in rs]):.2f} | ram {max(r['peak_ram_mb'] for r in rs)} |")
