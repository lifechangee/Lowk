"""Minimal frame-level DER (10 ms frames, overlap scored, optional collar around reference boundaries,
optimal one-to-one speaker mapping via Hungarian). Same definition as NIST md-eval modulo frame rounding."""
import numpy as np
from scipy.optimize import linear_sum_assignment

FR = 0.01


def rasterize(segs, names, T):
    m = np.zeros((len(names), T), dtype=bool)
    ix = {n: i for i, n in enumerate(names)}
    for s in segs:
        a, b = int(s["start"] / FR), min(int(s["end"] / FR), T)
        m[ix[s["spk"]], a:b] = True
    return m


def der(ref, hyp, duration, collar=0.25, return_detail=False):
    rn = sorted({s["spk"] for s in ref}); hn = sorted({s["spk"] for s in hyp}) or ["_none"]
    T = int(duration / FR) + 1
    R, H = rasterize(ref, rn, T), rasterize(hyp, hn, T)
    keep = np.ones(T, dtype=bool)
    if collar > 0:
        c = int(collar / FR)
        for s in ref:
            for edge in (s["start"], s["end"]):
                e = int(edge / FR)
                keep[max(e - c, 0): e + c] = False
    R, H = R[:, keep], H[:, keep]
    # optimal mapping on overlap counts
    M = R.astype(np.int32) @ H.T.astype(np.int32)
    ri, hi = linear_sum_assignment(-M)
    Hm = np.zeros_like(R)
    for r, h in zip(ri, hi):
        Hm[r] = H[h]
    nref, nhyp = R.sum(0), H.sum(0)
    ncorrect = np.zeros(R.shape[1], dtype=int)
    for r, h in zip(ri, hi):
        ncorrect += (R[r] & H[h])
    miss = np.maximum(nref - nhyp, 0).sum()
    fa = np.maximum(nhyp - nref, 0).sum()
    conf = (np.minimum(nref, nhyp) - ncorrect).sum()
    tot = nref.sum()
    d = (miss + fa + conf) / max(tot, 1)
    if return_detail:
        return d, dict(miss=miss / tot, fa=fa / tot, conf=conf / tot, ref_speech_s=tot * FR, n_ref=len(rn), n_hyp=len(hn))
    return d
