"""sherpa-onnx offline diarization (pyannote-segmentation-3.0 + speaker embedding + clustering) on the mixtures.
Offline = upper bound for an online clusterer on the same embeddings; we also report RTF.

usage: python run_sherpa.py <models_dir> <mix_dir> <out_json> [embedding.onnx] [threshold]
"""
import json, os, sys, time, glob
import numpy as np, soundfile as sf, sherpa_onnx
sys.path.insert(0, os.path.dirname(__file__))
from der import der

models, mix_dir, out_json = sys.argv[1:4]
emb = sys.argv[4] if len(sys.argv) > 4 else "3dspeaker_speech_eres2net_base_sv_zh-cn_3dspeaker_16k.onnx"
thr = float(sys.argv[5]) if len(sys.argv) > 5 else 0.5


def make(num_clusters):
    cfg = sherpa_onnx.OfflineSpeakerDiarizationConfig(
        segmentation=sherpa_onnx.OfflineSpeakerSegmentationModelConfig(
            pyannote=sherpa_onnx.OfflineSpeakerSegmentationPyannoteModelConfig(
                model=os.path.join(models, "sherpa-onnx-pyannote-segmentation-3-0/model.onnx")),
            num_threads=2),
        embedding=sherpa_onnx.SpeakerEmbeddingExtractorConfig(model=os.path.join(models, emb), num_threads=2),
        clustering=sherpa_onnx.FastClusteringConfig(num_clusters=num_clusters, threshold=thr),
        min_duration_on=0.3, min_duration_off=0.5)
    return sherpa_onnx.OfflineSpeakerDiarization(cfg)


res = {}
for jf in sorted(glob.glob(mix_dir + "/*.json")):
    name = os.path.basename(jf)[:-5]
    meta = json.load(open(jf))
    audio, sr = sf.read(jf[:-5] + ".wav", dtype="float32")
    row = {}
    for label, nc in (("auto_thr", -1), ("known_n", len(meta["speakers"]))):
        sd = make(nc)
        t0 = time.perf_counter()
        r = sd.process(audio).sort_by_start_time()
        el = time.perf_counter() - t0
        segs = [{"spk": f"S{s.speaker}", "start": s.start, "end": s.end} for s in r]
        d, det = der(meta["ref"], segs, meta["duration"], collar=0.25, return_detail=True)
        row[label] = dict(der=round(float(d), 4), n_hyp=int(det["n_hyp"]), conf=round(float(det["conf"]), 4),
                          miss=round(float(det["miss"]), 4), fa=round(float(det["fa"]), 4), rtf=round(el / meta["duration"], 4))
    print(name, emb, row, flush=True)
    res[name] = row
json.dump(dict(embedding=emb, threshold=thr, results=res), open(out_json, "w"), indent=1)
