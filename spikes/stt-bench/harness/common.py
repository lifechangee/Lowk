"""Shared helpers: audio IO, Silero VAD, text normalisation, metrics."""
import re, difflib, pathlib
import numpy as np, soundfile as sf, soxr

ROOT = pathlib.Path(__file__).resolve().parent.parent
SR = 16000

def load_audio(path):
    x, sr = sf.read(str(path), dtype="float32")
    if x.ndim > 1: x = x.mean(1)
    if sr != SR: x = soxr.resample(x, sr, SR)
    return x

class SileroVAD:
    """Streaming Silero VAD v5 (onnx). feed 512-sample frames -> speech prob."""
    def __init__(self, path=None):
        import onnxruntime as ort
        o = ort.SessionOptions(); o.inter_op_num_threads = 1; o.intra_op_num_threads = 1
        self.s = ort.InferenceSession(str(path or ROOT / "models" / "silero_vad.onnx"), o, providers=["CPUExecutionProvider"])
        self.reset()
    def reset(self):
        self.state = np.zeros((2, 1, 128), np.float32); self.ctx = np.zeros((1, 64), np.float32)
    def __call__(self, frame512):
        x = np.concatenate([self.ctx, frame512.reshape(1, -1)], 1).astype(np.float32)
        out, self.state = self.s.run(None, {"input": x, "state": self.state, "sr": np.array(16000, np.int64)})
        self.ctx = x[:, -64:]
        return float(out[0, 0])

def speech_mask(x, thr=0.5):
    """Per-frame (32 ms) boolean speech mask for a whole clip."""
    v = SileroVAD(); n = len(x) // 512
    return np.array([v(x[i*512:(i+1)*512]) > thr for i in range(n)])

# ---------- text ----------
_PUNCT = re.compile(r"[^\w\s]|_", re.UNICODE)
def normalize(t):
    t = t.lower().replace("ё", "е").replace("—", " ").replace("–", " ")
    t = _PUNCT.sub(" ", t)
    return re.sub(r"\s+", " ", t).strip()
def words(t): return normalize(t).split()
def is_latin(w): return bool(re.search(r"[a-z]", w)) and not re.search(r"[а-я]", w)

def edit_rates(ref, hyp):
    import jiwer
    r, h = normalize(ref), normalize(hyp)
    if not r: return dict(wer=None, cer=None)
    return dict(wer=jiwer.wer(r, h or "x"), cer=jiwer.cer(r, h or "x"))

def english_terms(ref, hyp):
    """For each Latin-script word in the reference, how did the engine render it?
    -> list of (ref_term, status, hyp_rendering) ; status in exact | cyrillic | other | missing"""
    R, H = words(ref), words(hyp)
    sm = difflib.SequenceMatcher(None, R, H, autojunk=False)
    out = []
    for tag, i1, i2, j1, j2 in sm.get_opcodes():
        for i in range(i1, i2):
            if not is_latin(R[i]): continue
            if tag == "equal": out.append((R[i], "exact", R[i]))
            elif tag == "replace":
                seg = " ".join(H[j1:j2])
                out.append((R[i], "cyrillic" if re.search(r"[а-я]", seg) else "other", seg))
            else: out.append((R[i], "missing", ""))
    return out

def word_end_times(ref_words, mask, frame_s=512/SR):
    """Estimate end time (s) of each reference word: spread words over speech-active
    frames proportionally to character count (no forced aligner needed)."""
    act = np.flatnonzero(mask)
    if len(act) == 0 or not ref_words: return [0.0] * len(ref_words)
    ch = np.cumsum([len(w) + 1 for w in ref_words]); ch = ch / ch[-1]
    idx = np.minimum((ch * len(act)).astype(int), len(act) - 1)
    return [(act[i] + 1) * frame_s for i in idx]
