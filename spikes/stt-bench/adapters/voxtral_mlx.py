"""Mistral Voxtral Mini 4B Realtime via mlx-audio. cfg: model=<hf repo, 4/6/8-bit>, delay_ms=480"""
from adapters._mlx_session import MLXSessionEngine
class VoxtralMLX(MLXSessionEngine):
    DEFAULT_MODEL = "mlx-community/Voxtral-Mini-4B-Realtime-2602-4bit"
    def _session_kwargs(self):
        return {"transcription_delay_ms": int(self.cfg.get("delay_ms", 480))}
