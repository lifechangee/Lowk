"""Build multi-speaker test mixtures from single-speaker clips (all file-only, no sound output).

Speakers come from the owner's recordings (stt-bench: *_them_* = partner, *_me_* = owner, both used as
"remote" voices here), macOS `say` Milena (synthetic), and pitch-shifted copies (hard look-alike case).
Reference = per-clip energy VAD (30 ms frames) scheduled into turns; written as RTTM-like JSON.

usage: python make_mix.py <wav_dir> <out_dir>
"""
import json, os, random, sys
import numpy as np, soundfile as sf, librosa

SR = 16000
wav_dir, out_dir = sys.argv[1], sys.argv[2]
os.makedirs(out_dir, exist_ok=True)


def load(name):
    x, sr = sf.read(os.path.join(wav_dir, name + ".wav"), dtype="float32")
    assert sr == SR
    return x


def vad(x, hop=480, min_pause=0.45, min_seg=0.4):
    n = len(x) // hop
    e = np.sqrt((x[: n * hop].reshape(n, hop) ** 2).mean(1) + 1e-12)
    thr = max(np.percentile(e, 20) * 3.0, np.percentile(e, 95) * 0.06)
    act = e > thr
    segs, s = [], None
    for i, a in enumerate(act):
        if a and s is None:
            s = i
        if not a and s is not None:
            segs.append([s, i]); s = None
    if s is not None:
        segs.append([s, n])
    merged = []
    for a, b in segs:
        if merged and (a - merged[-1][1]) * hop / SR < min_pause:
            merged[-1][1] = b
        else:
            merged.append([a, b])
    return [(a * hop / SR, b * hop / SR) for a, b in merged if (b - a) * hop / SR >= min_seg]


def turns_of(x, lo=4.0, hi=13.0):
    """Cut a clip into turns at VAD pauses: list of (audio, [(s,e) rel])."""
    segs = vad(x)
    out, cur = [], []
    for s, e in segs:
        if cur and e - cur[0][0] > hi:
            out.append(cur); cur = []
        cur.append((s, e))
    if cur:
        out.append(cur)
    res = []
    for c in out:
        a, b = max(c[0][0] - 0.05, 0), min(c[-1][1] + 0.05, len(x) / SR)
        if b - a < lo * 0.5:
            continue
        res.append((x[int(a * SR): int(b * SR)], [(s - a, e - a) for s, e in c]))
    return res


def pitch(x, st):
    return librosa.effects.pitch_shift(x, sr=SR, n_steps=st).astype(np.float32)


def speaker_pool(parts):
    pool = []
    for p in parts:
        pool += turns_of(p)
    return pool


def scenario(name, spk_audio, seed, n_turns, overlap_p=0.15):
    rnd = random.Random(seed)
    pools = {k: speaker_pool(v) for k, v in spk_audio.items()}
    idx = {k: 0 for k in pools}
    names = list(pools)
    total = np.zeros(SR * 600, dtype=np.float32)
    ref, t, last = [], 0.5, None
    for _ in range(n_turns):
        cands = [k for k in names if k != last and idx[k] < len(pools[k])]
        if not cands:
            break
        k = rnd.choice(cands)
        a, segs = pools[k][idx[k]]; idx[k] += 1
        dur = len(a) / SR
        start = t
        if last is not None and rnd.random() < overlap_p and dur > 3:
            start = max(t - rnd.uniform(0.8, 1.6), 0)   # barge-in overlap
        else:
            start = t + rnd.uniform(0.2, 0.9)
        i0 = int(start * SR)
        total[i0: i0 + len(a)] += a
        for s, e in segs:
            ref.append({"spk": k, "start": round(start + s, 3), "end": round(start + e, 3)})
        t = start + dur
        last = k
    n = int((t + 0.5) * SR)
    mix = total[:n]
    mix = mix / max(np.abs(mix).max() * 1.1, 1e-6)
    sf.write(os.path.join(out_dir, name + ".wav"), mix, SR, subtype="PCM_16")
    json.dump({"duration": n / SR, "speakers": names, "ref": ref}, open(os.path.join(out_dir, name + ".json"), "w"), indent=1)
    print(name, round(n / SR, 1), "s", len(ref), "segments", {k: sum(1 for r in ref if r['spk'] == k) for k in names})


them = [load(f"0{i}_them_q") for i in range(1, 6)]
me = [load(f"0{i}_me_q") for i in range(1, 6)]
mil = [load("milena1"), load("milena2")]
them_hi = [pitch(x, 4.0) for x in them]
mil_lo = [pitch(x, -3.5) for x in mil]

scenario("s1_2spk_real", {"A_partner": them, "B_owner": me[:2]}, 1, 14)
scenario("s2_3spk_real_synth", {"A_partner": them, "B_owner": me[:3], "C_milena": mil * 2}, 2, 22)
scenario("s3_3spk_lookalike", {"A_partner": them, "B_partner_p4": them_hi, "C_owner": me[:3]}, 3, 22)
scenario("s4_4spk", {"A_partner": them, "B_owner": me[:3], "C_milena": mil * 2, "D_milena_m35": mil_lo * 2}, 4, 30, overlap_p=0.2)

# --- FLEURS pseudo-speakers (optional 3rd arg: dir with spk0..spk5 from fleurs_speakers.py) ---
# NB: the owner's "me" and "them" clips turned out to be ONE voice (cosine 0.7-0.9 for both,
# same as within-speaker), so the s1..s4 scenarios above are weak tests of real speaker change.
if len(sys.argv) > 3:
    import glob as _g
    fl = {k: [sf.read(f, dtype="float32")[0] for f in sorted(_g.glob(f"{sys.argv[3]}/spk{k}/*.wav"))[:14]] for k in range(6)}
    FEMALE = (0, 1, 3)  # see fleurs_speakers.py output

    def fturns(k):
        out = []
        for x in fl[k]:
            segs = vad(x)
            if segs:
                a, b = max(segs[0][0] - 0.05, 0), min(segs[-1][1] + 0.05, len(x) / SR)
                out.append((x[int(a * SR): int(b * SR)], [(s - a, e - a) for s, e in segs]))
        return out

    def fscen(name, ks, seed, n, ov=0.12):
        global speaker_pool
        pools = {f"{'F' if k in FEMALE else 'M'}{k}": fturns(k) for k in ks}
        orig, speaker_pool = speaker_pool, (lambda parts: parts)  # pools are already turn lists
        scenario(name, pools, seed, n, overlap_p=ov)
        speaker_pool = orig

    fscen("f2_2spk_FM", [0, 2], 11, 14)
    fscen("f3_3spk_FFM", [0, 1, 2], 12, 22)
    fscen("f4_4spk_2F2M", [0, 1, 2, 4], 13, 30)
    fscen("f6_6spk", [0, 1, 2, 3, 4, 5], 14, 40)
