"""FLEURS ru_ru (CC-BY-4.0) has no speaker ids. Recover pseudo-speakers: embed every dev utterance with
TitaNet-small (neither tested system uses it), agglomerate at a strict cosine threshold, keep clusters with >= MIN utts.
Writes <out>/<spkN>/NN.wav (16k mono) for the biggest clusters of distinct genders.

usage: python fleurs_speakers.py <dev_extracted_dir> <dev.tsv> <titanet.onnx> <out_dir>
"""
import os, sys, csv, shutil
import numpy as np, soundfile as sf, sherpa_onnx
from scipy.cluster.hierarchy import linkage, fcluster

audio_dir, tsv, model, out = sys.argv[1:5]
MIN = 5
rows = [r for r in csv.reader(open(tsv, encoding="utf8"), delimiter="\t")]
ex = sherpa_onnx.SpeakerEmbeddingExtractor(sherpa_onnx.SpeakerEmbeddingExtractorConfig(model=model, num_threads=4))
E, F, G = [], [], []
for r in rows:
    p = os.path.join(audio_dir, r[1])
    if not os.path.exists(p):
        continue
    x, sr = sf.read(p, dtype="float32")
    if x.ndim > 1: x = x[:, 0]
    if len(x) < 3 * sr:
        continue
    s = ex.create_stream(); s.accept_waveform(sr, x); s.input_finished()
    e = np.array(ex.compute(s)); E.append(e / np.linalg.norm(e)); F.append(p); G.append(r[-1])
E = np.array(E)
Z = linkage(E, "average", metric="cosine")
lab = fcluster(Z, t=0.35, criterion="distance")
ids, cnt = np.unique(lab, return_counts=True)
good = [i for i, c in sorted(zip(ids, cnt), key=lambda t: -t[1]) if c >= MIN]
print("utts", len(E), "clusters", len(ids), "with >=%d utts:" % MIN, [(int(i), int(c)) for i, c in zip(ids, cnt) if c >= MIN])
os.makedirs(out, exist_ok=True)
for j, i in enumerate(good[:6]):
    d = os.path.join(out, f"spk{j}"); os.makedirs(d, exist_ok=True)
    gend = [G[k] for k in np.where(lab == i)[0]]
    print("spk%d" % j, "cluster", int(i), "n", len(gend), "gender", {g: gend.count(g) for g in set(gend)})
    for n, k in enumerate(np.where(lab == i)[0]):
        x, sr = sf.read(F[k], dtype="float32")
        sf.write(os.path.join(d, f"{n:02d}.wav"), x, sr, subtype="PCM_16")
