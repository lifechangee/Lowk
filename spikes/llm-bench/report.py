"""Aggregate results.json -> markdown table. python report.py results.json"""
import json, sys, re, statistics as st, collections
from bench import pct
d = json.load(open(sys.argv[1]))
print("| model | variant | ok | TTFT p50 / p95 (s) | tok/s p50 | out tok | prompt tok | $/100 asks | providers | cyr% | words |\n|---|---|---|---|---|---|---|---|---|---|---|")
for k, rs in d.items():
    m, v = k.split("|"); ok = [r for r in rs if r["ttft"] is not None]
    if not ok: print(f"| {m} | {v} | 0/{len(rs)} | - | - | - | - | - | - | - | - |  {rs[0]['error'][:80]}"); continue
    tt = [r["ttft"] for r in ok]; tps = [r["tps"] for r in ok if r["tps"]]
    u = [r["usage"] or {} for r in ok]; cost = [x.get("cost") for x in u if x.get("cost") is not None]
    prov = collections.Counter(r["provider"] for r in ok)
    txt = ok[-1]["text"]; letters = re.findall(r"[A-Za-zА-Яа-яЁё]", txt); cyr = sum(1 for c in letters if re.match("[А-Яа-яЁё]", c)) / max(len(letters), 1)
    wc = st.mean(len(r["text"].split()) for r in ok)
    print(f"| {m} | {v} | {len(ok)}/{len(rs)} | {pct(tt,50):.2f} / {pct(tt,95):.2f} | {st.median(tps) if tps else 0:.0f} | {st.mean(x.get('completion_tokens',0) for x in u):.0f} | {st.mean(x.get('prompt_tokens',0) for x in u):.0f} | {100*st.mean(cost) if cost else 0:.3f} | {', '.join(f'{p}x{n}' for p,n in prov.most_common(3))} | {100*cyr:.0f} | {wc:.0f} |")
