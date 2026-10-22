"""Streaming Nemotron-3-Diarization via HF Transformers (reference implementation; the product runtime
would be the ggml GGUF Q8_0 build). Audio is fed chunk by chunk exactly as live capture would deliver it;
we record, for every chunk, the wall time of the forward pass and the audio time at which its labels become final.

usage: python run_nemotron3.py <model_dir> <mix_dir> <out_json> [mode] [device]
"""
import json, os, sys, time, glob
import numpy as np, soundfile as sf, torch
from transformers import AutoModelForAudioFrameClassification, AutoProcessor
sys.path.insert(0, os.path.dirname(__file__))
from der import der

model_dir, mix_dir, out_json = sys.argv[1:4]
mode = sys.argv[4] if len(sys.argv) > 4 else "low_latency"
dev = sys.argv[5] if len(sys.argv) > 5 else "cpu"
torch.set_num_threads(4)
proc = AutoProcessor.from_pretrained(model_dir)
model = AutoModelForAudioFrameClassification.from_pretrained(model_dir).to(dev).eval()
proc.set_streaming_mode(mode)
print("mode", mode, "latency_ms", proc.streaming_latency_ms, "device", dev, flush=True)
sr = proc.feature_extractor.sampling_rate


def run(audio):
    chunks = []
    def gen():
        yield proc(audio[: proc.num_samples_first_audio_chunk], sampling_rate=sr, is_streaming=True, is_first_audio_chunk=True), proc.num_samples_first_audio_chunk
        mi = proc.num_mel_frames_per_step
        st = proc.audio_chunk_start(mi)
        while (en := st + proc.num_samples_per_audio_chunk) <= audio.shape[0]:
            yield proc(audio[st:en], sampling_rate=sr, is_streaming=True, is_first_audio_chunk=False), en
            mi += proc.num_mel_frames_per_step
            st = proc.audio_chunk_start(mi)
        yield proc(audio[st:], sampling_rate=sr, is_streaming=True, is_first_audio_chunk=False, is_last_audio_chunk=True), audio.shape[0]
    cache, logits, comp, avail = None, [], [], []
    with torch.inference_mode():
        for inp, end in gen():
            t0 = time.perf_counter()
            inp = inp.to(dev, dtype=model.dtype)
            out = model(**inp, speaker_cache=cache)
            if dev == "mps":
                torch.mps.synchronize()
            comp.append(time.perf_counter() - t0)
            logits.append(out.logits); cache = out.speaker_cache
            avail.append(end / sr)
    L = torch.cat(logits, 1)
    return L, comp, avail


def to_segs(L, thr=0.5):
    p = torch.sigmoid(L)[0].float().cpu().numpy() if L.min() < 0 or L.max() > 1 else L[0].float().cpu().numpy()
    act = p > thr
    segs = []
    for k in range(act.shape[1]):
        s = None
        for i, a in enumerate(act[:, k]):
            if a and s is None: s = i
            if not a and s is not None:
                if (i - s) * 0.01 >= 0.2: segs.append({"spk": f"S{k}", "start": s * 0.01, "end": i * 0.01})
                s = None
        if s is not None: segs.append({"spk": f"S{k}", "start": s * 0.01, "end": act.shape[0] * 0.01})
    return segs


res = {}
for jf in sorted(glob.glob(mix_dir + "/*.json")):
    name = os.path.basename(jf)[:-5]
    meta = json.load(open(jf))
    audio, _ = sf.read(jf[:-5] + ".wav", dtype="float32")
    L, comp, avail = run(audio)
    segs = to_segs(L)
    d, det = der(meta["ref"], segs, meta["duration"], collar=0.25, return_detail=True)
    d0 = der(meta["ref"], segs, meta["duration"], collar=0.0)
    r = dict(der_collar025=round(d, 4), der_nocollar=round(d0, 4), **{k: round(float(v), 4) for k, v in det.items()},
             audio_s=meta["duration"], compute_s=round(sum(comp), 2), rtf=round(sum(comp) / meta["duration"], 4),
             chunk_compute_ms_p50=round(float(np.percentile(comp, 50)) * 1000, 1), chunk_compute_ms_p95=round(float(np.percentile(comp, 95)) * 1000, 1),
             chunks=len(comp), latency_ms_model=proc.streaming_latency_ms)
    print(name, r, flush=True)
    res[name] = r
json.dump(dict(mode=mode, device=dev, results=res), open(out_json, "w"), indent=1)
