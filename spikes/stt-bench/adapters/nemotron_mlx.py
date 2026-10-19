"""NVIDIA Nemotron 3.5 ASR Streaming Multilingual 0.6B via mlx-audio (cache-aware FastConformer-RNNT).
cfg: model=<hf repo> (default bf16; try ...-8bit), language=<e.g. ru-RU; default auto-detect>"""
from adapters._mlx_session import MLXSessionEngine
class NemotronMLX(MLXSessionEngine):
    DEFAULT_MODEL = "mlx-community/nemotron-3.5-asr-streaming-0.6b"
    def _session_kwargs(self):
        kw = {}
        if self.cfg.get("language"): kw["language"] = self.cfg["language"]
        if self.cfg.get("delay_ms"): kw["transcription_delay_ms"] = int(self.cfg["delay_ms"])
        return kw
