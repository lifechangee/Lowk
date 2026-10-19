"""Whisper large-v3-turbo (whisper.cpp, Metal) pseudo-streaming:
Silero VAD cuts utterances; while open, re-decode the utterance every `stride_ms`; LocalAgreement-2
over successive hypotheses gives the committed prefix; on end-of-speech one final decode."""
import io, json, os, queue, subprocess, threading, time, socket, pathlib, re
import numpy as np, requests, soundfile as sf
from harness.base import Engine
from harness.common import SileroVAD, ROOT, SR, words

BIN = ROOT / "engines/whisper.cpp/build/bin/whisper-server"
HALLU = re.compile(r"(субтитры|продолжение следует|редактор субтитров|thanks for watching|спасибо за просмотр|dimatorzok)", re.I)

class WhisperCppStream(Engine):
    def __init__(self, **cfg):
        super().__init__(**cfg)
        self.model = cfg.get("model", str(ROOT / "models/ggml-large-v3-turbo-q5_0.bin"))
        self.lang = cfg.get("language", "auto")
        self.prompt = cfg.get("prompt", "")        # e.g. "EBITDA, KPI, sprint, roadmap"
        self.stride = float(cfg.get("stride_ms", 700)) / 1000
        self.hang = float(cfg.get("hangover_ms", 400)) / 1000
        self.max_utt = float(cfg.get("max_utt_s", 15))
        self.proc = None
    def available(self):
        if not BIN.exists(): return False, "whisper-server not built"
        if not pathlib.Path(self.model).exists(): return False, f"model missing: {self.model}"
        return True, ""
    def child_pids(self): return [self.proc.pid] if self.proc else []
    def _launch(self):
        s = socket.socket(); s.bind(("127.0.0.1", 0)); self.port = s.getsockname()[1]; s.close()
        env = dict(os.environ, DYLD_LIBRARY_PATH=str(BIN.parent))
        self.proc = subprocess.Popen([str(BIN), "-m", self.model, "-l", self.lang, "-nf", "-t", "4", *(["-ac", str(self.cfg["audio_ctx"])] if "audio_ctx" in self.cfg else []), "--port", str(self.port), "-sns"],
                                     env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        for _ in range(200):
            try: requests.get(f"http://127.0.0.1:{self.port}/", timeout=1); break
            except Exception: time.sleep(0.2)
        self._decode(np.zeros(SR, np.float32))     # warm-up (Metal kernel compile)
        self.compute_s = 0.0
    def _decode(self, pcm, prompt=""):
        b = io.BytesIO(); sf.write(b, pcm, SR, format="WAV", subtype="PCM_16"); b.seek(0)
        t = time.perf_counter()
        r = requests.post(f"http://127.0.0.1:{self.port}/inference", files={"file": ("a.wav", b, "audio/wav")},
                          data=dict(temperature="0.0", response_format="json", language=self.lang,
                                    prompt=(self.prompt + " " + prompt).strip()), timeout=60)
        self.compute_s += time.perf_counter() - t
        txt = r.json().get("text", "").strip()
        txt = re.sub(r"\s+", " ", txt)
        if HALLU.search(txt): return ""
        # greedy decoding sometimes repeats the whole sentence on trailing silence: collapse consecutive duplicates
        sents = re.split(r"(?<=[.!?])\s+", txt); out = []
        for s_ in sents:
            if not out or words(s_) != words(out[-1]): out.append(s_)
        return " ".join(out)
    def start(self, emit):
        super().start(emit); self._launch()
        self.vad = SileroVAD(); self.q = queue.Queue(); self.lock = threading.Lock()
        self.carry = np.zeros(0, np.float32)
        self.finals = []; self.utt = None; self.silence = 0.0; self.t_audio = 0.0
        self.committed = []; self.prev_hyp = []; self.pending_partial = None; self.stopped = False
        self.last_partial_len = 0
        self.th = threading.Thread(target=self._worker, daemon=True); self.th.start()
    def _publish(self, tentative=""):
        self.emit(" ".join(self.finals + ([" ".join(self.committed)] if self.committed else [])), tentative)
    def _worker(self):
        while True:
            job = self.q.get()
            if job is None: return
            kind, pcm = job
            if kind == "final_end": continue
            if kind == "partial":
                with self.lock:
                    if self.pending_partial is not pcm: continue        # superseded
                    self.pending_partial = None
                hyp = self._decode(pcm[-int(self.max_utt * SR):], " ".join(self.finals)[-200:]).split()
                with self.lock:
                    if self.utt is None: continue                        # utterance already closed
                    # LocalAgreement-2: longest common prefix of the last two hypotheses is committed
                    n = 0
                    while n < min(len(hyp), len(self.prev_hyp)) and words(hyp[n]) == words(self.prev_hyp[n]): n += 1
                    if n > len(self.committed): self.committed = hyp[:n]
                    self.prev_hyp = hyp
                    self._publish(" ".join(hyp[len(self.committed):]))
            else:
                hyp = self._decode(pcm, " ".join(self.finals)[-200:])
                with self.lock:
                    if hyp: self.finals.append(hyp)
                    if self.utt is None or kind == "final_cut": pass
                    self.committed = []; self.prev_hyp = []
                    self._publish("")
                if kind == "final_end": self.done_evt.set() if hasattr(self, "done_evt") else None
    def _close_utt(self, kind="final"):
        pcm = np.concatenate(self.utt)
        if kind == "final": pcm = pcm[:max(len(pcm) - int(max(self.silence - 0.2, 0) * SR), SR // 2)]    # trim trailing silence to 0.2 s
        self.utt = None; self.silence = 0.0; self.committed = []; self.prev_hyp = []; self.pending_partial = None
        self.q.put((kind, pcm))
    def feed(self, pcm, t_end):
        buf = np.concatenate([self.carry, pcm]); i = 0
        pre = int(0.25 * SR)
        while i + 512 <= len(buf):
            fr = buf[i:i+512]; p = self.vad(fr); i += 512; self.t_audio += 512 / SR
            with self.lock:
                if self.utt is None:
                    self.ring = np.concatenate([getattr(self, "ring", np.zeros(0, np.float32)), fr])[-pre:]
                    if p > 0.5:
                        self.utt = [self.ring.copy()]; self.silence = 0.0; self.utt_len = len(self.ring) / SR; self.last_partial_len = 0
                else:
                    self.utt.append(fr); self.utt_len += 512 / SR
                    self.silence = self.silence + 512 / SR if p < 0.35 else 0.0
                    if self.silence >= self.hang or self.utt_len >= self.max_utt:
                        self._close_utt("final" if self.silence >= self.hang else "final_cut"); continue
                    if self.utt_len - self.last_partial_len >= self.stride and self.q.qsize() < 3:
                        self.last_partial_len = self.utt_len
                        pcm_u = np.concatenate(self.utt); self.pending_partial = pcm_u; self.q.put(("partial", pcm_u))
        self.carry = buf[i:]
    def finish(self):
        with self.lock:
            if self.utt is not None: self._close_utt("final")
        self.done_evt = threading.Event(); self.q.put(("final_end", np.zeros(SR // 2, np.float32)))
        self.q.put(None); self.th.join(timeout=120)
        return " ".join(self.finals)
    def close(self):
        if self.proc: self.proc.terminate(); self.proc.wait(timeout=5)
