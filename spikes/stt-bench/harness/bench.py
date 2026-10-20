#!/usr/bin/env python
"""STT bake-off runner.
  python harness/bench.py --engines whisper_cpp,nemotron_mlx --clips audio/*.wav [--chunk-ms 160] [--speed 1]
Each clip X.wav needs X.txt (reference). Results -> results/<run>.json and a printed table.
Audio is pushed in real-time-sized chunks paced by the wall clock (speed=1) so compute contention is real."""
import argparse, glob, importlib, json, os, pathlib, sys, threading, time, difflib, datetime
import numpy as np, psutil
sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent.parent))
from harness.common import *

ENGINES = {  # name -> (module, class)
    "whisper_cpp": ("adapters.whisper_cpp", "WhisperCppStream"),
    "nemotron_mlx": ("adapters.nemotron_mlx", "NemotronMLX"),
    "voxtral_mlx": ("adapters.voxtral_mlx", "VoxtralMLX"),
    "soniox": ("adapters.cloud", "Soniox"),
    "deepgram": ("adapters.cloud", "Deepgram"),
}

class RamSampler(threading.Thread):
    def __init__(self, eng): super().__init__(daemon=True); self.eng = eng; self.peak = 0; self.stop = False
    def run(self):
        me = psutil.Process()
        while not self.stop:
            try:
                procs = [me] + [psutil.Process(p) for p in self.eng.child_pids()]
                self.peak = max(self.peak, sum(p.memory_info().rss for p in procs))
            except Exception: pass
            time.sleep(0.1)

def run_one(eng, x, ref, chunk_ms, speed):
    snaps = []                       # (wall_t, committed, tentative)
    lock = threading.Lock()
    def emit(c, t):
        with lock: snaps.append((time.perf_counter(), c.strip(), t.strip()))
    eng.start(emit)
    ram = RamSampler(eng); ram.start()
    n = int(SR * chunk_ms / 1000)
    t0 = time.perf_counter()
    for i in range(0, len(x), n):
        chunk = x[i:i+n]
        due = t0 + (i + len(chunk)) / SR / speed          # chunk is "heard" when fully spoken
        d = due - time.perf_counter()
        if d > 0: time.sleep(d)
        eng.feed(chunk, (i + len(chunk)) / SR)
    t_audio_end = time.perf_counter()
    final = eng.finish()
    t_done = time.perf_counter()
    ram.stop = True; time.sleep(0.15)
    dur = len(x) / SR
    snaps_txt = [(t, (c + " " + tt).strip()) for t, c, tt in snaps]
    hyp_final = final if final is not None else (snaps_txt[-1][1] if snaps_txt else "")
    # ---- per-word latency vs estimated spoken-end time
    H = words(hyp_final); R = words(ref)
    mask = speech_mask(x); wend = word_end_times(R, mask)
    sm = difflib.SequenceMatcher(None, R, H, autojunk=False)
    h2r = {}
    for a, b, size in sm.get_matching_blocks():
        for k in range(size): h2r[b + k] = a + k
    first_seen = {}
    for t, txt in snaps_txt:
        w = words(txt)
        for i, wi in enumerate(w):
            if i < len(H) and wi == H[i] and i not in first_seen: first_seen[i] = t
    lags = [(first_seen[i] - t0) - wend[h2r[i]] / 1.0 * (1/speed) for i in h2r if i in first_seen]
    lags = np.array(lags) if lags else np.array([np.nan])
    # first-partial latency per speech burst: onset of each burst -> first snapshot with any new word
    act = np.flatnonzero(mask); bursts = []
    if len(act):
        start = act[0]; prev = act[0]
        for a in act[1:]:
            if a - prev > 15: bursts.append((start, prev)); start = a      # >0.48 s gap
            prev = a
        bursts.append((start, prev))
    onset_lat = []
    for s, e in bursts[:200]:
        ts = s * 512 / SR; te = (e + 1) * 512 / SR
        # first snapshot after onset whose word count grew compared with last snapshot before onset
        base = 0
        for t, txt in snaps_txt:
            if t - t0 <= ts / speed: base = len(words(txt))
        for t, txt in snaps_txt:
            if t - t0 > ts / speed and len(words(txt)) > base:
                onset_lat.append((t - t0) * speed - ts); break
    res = dict(
        wer=None, cer=None, hyp=hyp_final, ref=ref, duration_s=round(dur, 2),
        rtf_compute=round(eng.compute_s / dur, 3),
        rtf_wall=round((t_done - t0) / dur / (1 / speed), 3),
        final_latency_s=round((t_done - t_audio_end) * speed, 3),
        word_lag_median_s=round(float(np.nanmedian(lags)), 3), word_lag_p90_s=round(float(np.nanpercentile(lags, 90)), 3), word_lag_p95_s=round(float(np.nanpercentile(lags, 95)), 3),
        word_lags_s=[round(float(v), 3) for v in lags if v == v],
        onset_to_first_partial_median_s=round(float(np.median(onset_lat)), 3) if onset_lat else None,
        onset_to_first_partial_p90_s=round(float(np.percentile(onset_lat, 90)), 3) if onset_lat else None,
        n_bursts=len(bursts), n_updates=len(snaps), peak_ram_mb=round(ram.peak / 2**20),
        english_terms=english_terms(ref, hyp_final), notes=getattr(eng, "notes", {}))
    res.update(edit_rates(ref, hyp_final))
    return res

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--engines", default="whisper_cpp")
    ap.add_argument("--clips", nargs="+", default=["audio/*.wav"])
    ap.add_argument("--chunk-ms", type=int, default=160)
    ap.add_argument("--speed", type=float, default=1.0, help=">1 runs faster than real time (latency numbers then scaled)")
    ap.add_argument("--cfg", action="append", default=[], help="engine cfg k=v (applies to all selected engines)")
    ap.add_argument("--tag", default="")
    a = ap.parse_args()
    cfg = dict(kv.split("=", 1) for kv in a.cfg)
    clips = sorted({p for g in a.clips for p in glob.glob(g)})
    out = []
    for name in a.engines.split(","):
        mod, cls = ENGINES[name]
        eng = getattr(importlib.import_module(mod), cls)(name=name, **cfg)
        ok, why = eng.available()
        if not ok:
            print(f"[skip] {name}: {why}"); out.append(dict(engine=name, skipped=why)); continue
        label = name + (":" + a.tag if a.tag else "")
        for c in clips:
            ref = pathlib.Path(c).with_suffix(".txt").read_text()
            x = load_audio(c)
            e = getattr(importlib.import_module(mod), cls)(name=name, **cfg)
            try:
                r = run_one(e, x, ref, a.chunk_ms, a.speed)
            finally: e.close()
            r.update(engine=label, clip=pathlib.Path(c).name, chunk_ms=a.chunk_ms, cfg=cfg)
            out.append(r)
            print(f"{label:28s} {r['clip']:28s} WER {r['wer']:.3f} CER {r['cer']:.3f} lag50 {r['word_lag_median_s']}s lag90 {r['word_lag_p90_s']}s "
                  f"onset50 {r['onset_to_first_partial_median_s']}s final {r['final_latency_s']}s RTFc {r['rtf_compute']} RAM {r['peak_ram_mb']}MB", flush=True)
    d = ROOT / "results"; d.mkdir(exist_ok=True)
    fn = d / f"{datetime.datetime.now():%Y%m%d-%H%M%S}-{a.engines.replace(',','+')}{('-'+a.tag) if a.tag else ''}.json"
    fn.write_text(json.dumps(out, ensure_ascii=False, indent=1)); print("saved", fn)

if __name__ == "__main__": main()
