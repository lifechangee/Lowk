"""Voice-enrolment feasibility: enrol each FLEURS pseudo-speaker from ONE 5-10 s utterance that is NOT in the
mixture (utterances 20..22), then identify every reference turn of the 6-speaker mixture (oracle boundaries,
>= 2 s) by nearest cosine. Reports accuracy and the margin between the best and the second-best enrolment.

usage: python enroll_test.py <embedding.onnx> <fleurs_spk_dir> <mix_dir>
"""
import glob, json, os, sys
import numpy as np, soundfile as sf, sherpa_onnx

model, spk_dir, mix_dir = sys.argv[1:4]
ex = sherpa_onnx.SpeakerEmbeddingExtractor(sherpa_onnx.SpeakerEmbeddingExtractorConfig(model=model, num_threads=2))


def emb(x, sr=16000):
    s = ex.create_stream(); s.accept_waveform(sr, x); s.input_finished()
    e = np.array(ex.compute(s)); return e / np.linalg.norm(e)


names = {0: "F0", 1: "F1", 2: "M2", 3: "F3", 4: "M4", 5: "M5"}
enrol = {}
for k, n in names.items():
    fs = sorted(glob.glob(f"{spk_dir}/spk{k}/*.wav"))
    x = np.concatenate([sf.read(f, dtype="float32")[0] for f in fs[20:22]])  # ~10-20 s, unseen
    enrol[n] = emb(x)
meta = json.load(open(f"{mix_dir}/f6_6spk.json"))
audio, sr = sf.read(f"{mix_dir}/f6_6spk.wav", dtype="float32")
# merge adjacent ref segments of one speaker into turns
turns = []
for r in sorted(meta["ref"], key=lambda r: r["start"]):
    if turns and turns[-1]["spk"] == r["spk"] and r["start"] - turns[-1]["end"] < 1.0:
        turns[-1]["end"] = r["end"]
    else:
        turns.append(dict(r))
ok = tot = 0; margins = []
M = np.array([enrol[n] for n in names.values()])
for t in turns:
    if t["end"] - t["start"] < 2.0:
        continue
    e = emb(audio[int(t["start"] * sr): int(t["end"] * sr)])
    sims = M @ e
    order = np.argsort(-sims)
    pred = list(names.values())[order[0]]
    ok += pred == t["spk"]; tot += 1; margins.append(sims[order[0]] - sims[order[1]])
print(os.path.basename(model), f"turns>=2s: {tot}, correct: {ok} ({ok / tot:.1%}), margin p10/p50: {np.percentile(margins, 10):.2f}/{np.percentile(margins, 50):.2f}")
