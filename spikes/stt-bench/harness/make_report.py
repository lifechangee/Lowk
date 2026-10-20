"""Aggregate results/*.json (latest run per engine tag) into markdown tables. python harness/make_report.py [results_dir]"""
import json, glob, sys, pathlib, collections, statistics as st
d = pathlib.Path(sys.argv[1] if len(sys.argv) > 1 else pathlib.Path(__file__).resolve().parent.parent / "results")
rows = []
for f in sorted(glob.glob(str(d / "2*.json"))):
    for r in json.load(open(f)):
        if "skipped" in r: continue
        rows.append(r)
def group(c):
    if c.startswith(("fleurs", "synth")): return "secondary (FLEURS/synthetic)"
    return "real " + ("noisy (_n)" if c.endswith("_n.wav") else "quiet (_q)")
def role(c): return "them" if "_them_" in c else ("me" if "_me_" in c else "-")
by = collections.defaultdict(list)
for r in rows: by[(r["engine"], group(r["clip"]))].append(r)
f = lambda v, n=2: "-" if v is None else f"{v:.{n}f}"
med = lambda xs: st.median(xs) if xs else None
out = ["| engine | set | clips | WER | CER | word lag p50/p90 (s) | onset->1st partial p50 (s) | final after EOS (s) | RTF (compute) | peak RAM MB |", "|---|---|---|---|---|---|---|---|---|---|"]
for (e, g), rs in sorted(by.items(), key=lambda x: (x[0][1], x[0][0])):
    w = lambda k: [r[k] for r in rs if r.get(k) is not None]
    out.append(f"| {e} | {g} | {len(rs)} | {f(st.mean(w('wer')),3)} | {f(st.mean(w('cer')),3)} | {f(med(w('word_lag_median_s')))}/{f(med(w('word_lag_p90_s')))} | "
               f"{f(med(w('onset_to_first_partial_median_s')))} | {f(med(w('final_latency_s')))} | {f(st.mean(w('rtf_compute')))} | {max(w('peak_ram_mb'))} |")
print("\n".join(out))
# English terms: RU speech with EN jargon (scenarios 01-04 + synthetic) vs English-heavy scenario 05
STOP = set("the i is a it and not ok can we do ll s right wants talk thanks".split())
engines = sorted({r["engine"] for r in rows})
def tally_for(pred):
    t = {e: collections.Counter() for e in engines}
    for r in rows:
        if r["clip"].startswith("fleurs") or not pred(r["clip"]): continue
        for term, st_, h in r["english_terms"]: t[r["engine"]][st_] += 1
    return t
for title, pred in (("RU with EN jargon (01-04, real; quiet+noisy)", lambda c: c[:2] in ("01","02","03","04")), ("English-heavy with RU inserts (05, real)", lambda c: c.startswith("05"))):
    print(f"\n### English terms: {title}\n\n| engine | exact Latin | Cyrillic transliteration | other mismatch | missing | exact % |\n|---|---|---|---|---|---|")
    for e, c in tally_for(pred).items():
        n = sum(c.values()); print(f"| {e} | {c['exact']} | {c['cyrillic']} | {c['other']} | {c['missing']} | {100*c['exact']/max(n,1):.0f}% |")
terms = collections.defaultdict(lambda: collections.defaultdict(list))
for r in rows:
    if not r["clip"][:2] in ("01","02","03","04"): continue
    for t, s_, h in r["english_terms"]:
        if len(t) >= 3 and t not in STOP: terms[t][r["engine"]].append(h if s_ != "exact" else "OK")
print("\n### Per-term rendering (01-04; ok = kept in Latin, otherwise what the engine wrote)\n\n| term | " + " | ".join(engines) + " |\n|---|" + "---|" * len(engines))
for t in sorted(terms, key=lambda t: -sum(len(v) for v in terms[t].values()))[:28]:
    cells = []
    for e in engines:
        v = terms[t].get(e, []); ok = v.count("OK"); bad = collections.Counter(x[:24] for x in v if x != "OK")
        cells.append(f"{ok}/{len(v)}" + (" -> " + "; ".join(k for k, _ in bad.most_common(2)) if bad else ""))
    print(f"| {t} | " + " | ".join(cells) + " |")
