import time
class Engine:
    """Adapter contract. The harness calls start(emit) once, feed() with real-time sized chunks,
    finish() after the last chunk (block until everything is decoded, return final text).
    emit(committed: str, tentative: str) may be called from any thread whenever the
    running transcript changes; full transcript = committed + ' ' + tentative.
    Engines add time spent in model compute to self.compute_s (for RTF)."""
    name = "base"
    def __init__(self, **cfg): self.cfg = cfg; self.compute_s = 0.0; self.notes = {}
    def available(self): return True, ""        # (ok, reason-if-not)
    def start(self, emit): self.emit = emit
    def feed(self, pcm, t_audio_end): raise NotImplementedError
    def finish(self): raise NotImplementedError
    def close(self): pass
    def child_pids(self): return []             # extra processes to include in RAM accounting
