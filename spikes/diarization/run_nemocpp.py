"""NVIDIA NeMo-Speech.cpp (Apache-2.0, ggml) `nemo-speech diarize` with the Q8_0 GGUF of Nemotron-3-Diarization:
wall time and DER per mixture. NB: the CLI processes a whole file (RTF only; per-chunk streaming latency
comes from run_nemotron3.py / the model card). `--preset v3-streaming` is the streaming-attention variant.

usage: python run_nemocpp.py <nemo-speech-bin> <gguf> <mix_dir> <out_json> [preset] [device]
"""
import json, os, subprocess, sys, time, glob
sys.path.insert(0, os.path.dirname(__file__))
from der import der

binp, gguf, mix_dir, out_json = sys.argv[1:5]
preset = sys.argv[5] if len(sys.argv) > 5 else "v3-streaming"
dev = sys.argv[6] if len(sys.argv) > 6 else "metal"
res = {}
for jf in sorted(glob.glob(mix_dir + "/*.json")):
    name = os.path.basename(jf)[:-5]
    meta = json.load(open(jf))
    t0 = time.perf_counter()
    p = subprocess.run([binp, "diarize", jf[:-5] + ".wav", "-m", gguf, "--device", dev, "--preset", preset, "--format", "rttm", "--recording-id", name],
                       capture_output=True, text=True)
    el = time.perf_counter() - t0
    segs = []
    for line in p.stdout.splitlines():
        f = line.split()
        if len(f) > 8 and f[0] == "SPEAKER":
            st, du = float(f[3]), float(f[4])
            segs.append({"spk": f[7], "start": st, "end": st + du})
    if not segs:
        print(name, "NO OUTPUT", p.stderr[-300:])
    d, det = der(meta["ref"], segs, meta["duration"], collar=0.25, return_detail=True)
    r = dict(der=round(float(d), 4), n_hyp=int(det["n_hyp"]), n_ref=int(det["n_ref"]), conf=round(float(det["conf"]), 4),
             miss=round(float(det["miss"]), 4), fa=round(float(det["fa"]), 4), wall_s=round(el, 2), rtf_incl_load=round(el / meta["duration"], 4))
    print(name, r, flush=True)
    res[name] = r
json.dump(dict(preset=preset, device=dev, gguf=os.path.basename(gguf), results=res), open(out_json, "w"), indent=1)
