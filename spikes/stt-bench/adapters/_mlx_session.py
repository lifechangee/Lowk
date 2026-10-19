"""Shared driver for mlx-audio streaming sessions (Nemotron, Voxtral Realtime).
All MLX work (load, step) happens on ONE worker thread; feed() only pushes PCM into the session."""
import os, queue, threading, time
from harness.base import Engine

class MLXSessionEngine(Engine):
    DEFAULT_MODEL = ""
    def __init__(self, **cfg):
        super().__init__(**cfg)
        self.model_id = cfg.get("model", self.DEFAULT_MODEL)
        os.environ.setdefault("HF_HOME", str(__import__("pathlib").Path(__file__).resolve().parent.parent / "models/hf"))
    def available(self):
        try: import mlx_audio  # noqa
        except Exception as e: return False, f"mlx-audio missing: {e}"
        return True, ""
    def _session_kwargs(self): return {}
    def start(self, emit):
        super().start(emit); self.text = ""; self.err = None
        self.ready = threading.Event(); self.closed = False; self.done = threading.Event()
        self.th = threading.Thread(target=self._run, daemon=True); self.th.start()
        self.ready.wait(1800)
        if self.err: raise self.err
    def _run(self):
        try:
            from mlx_audio.stt import load
            t = time.perf_counter(); model = load(self.model_id); self.notes["load_s"] = round(time.perf_counter() - t, 1)
            self.sess = model.create_streaming_session(**self._session_kwargs())
            # warm-up so first-use kernel compilation is not billed to the clip
            import numpy as np
            w = model.create_streaming_session(**self._session_kwargs()); w.feed(np.zeros(16000, np.float32)); w.close()
            while not w.done: w.step()
        except Exception as e:
            self.err = e; self.ready.set(); return
        self.ready.set()
        while True:
            t = time.perf_counter(); d = self.sess.step(); self.compute_s += time.perf_counter() - t
            if d:
                self.text += "".join(d); self.emit(self.text, "")
            if self.sess.done: break
            if not d: time.sleep(0.005)
        self.done.set()
    def feed(self, pcm, t_end): self.sess.feed(pcm)
    def finish(self):
        self.sess.close(); self.done.wait(300); return self.text.strip()
