"""Cloud streaming adapters (WebSocket). Skipped when the API key env var is absent.
  SONIOX_API_KEY   -> Soniox stt-rt-v5   (wss://stt-rt.soniox.com/transcribe-websocket)
  DEEPGRAM_API_KEY -> Deepgram Nova-3 multilingual (wss://api.deepgram.com/v1/listen)
NOTE: written from the vendor docs, NOT exercised in the first pass (no keys available). Expect to
fix small protocol details on first real run."""
import asyncio, json, os, threading, time
import numpy as np
from harness.base import Engine
from harness.common import SR

class _WS(Engine):
    KEY = ""
    def available(self):
        return (True, "") if os.environ.get(self.KEY) else (False, f"{self.KEY} not set")
    def start(self, emit):
        super().start(emit); self.final_toks = []; self.q = None; self.done = threading.Event(); self.err = None
        self.loop = asyncio.new_event_loop(); self.ready = threading.Event()
        self.th = threading.Thread(target=self._run, daemon=True); self.th.start(); self.ready.wait(15)
    def _run(self):
        asyncio.set_event_loop(self.loop)
        try: self.loop.run_until_complete(self._main())
        except Exception as e: self.err = e
        finally: self.done.set(); self.ready.set()
    def feed(self, pcm, t):
        b = (np.clip(pcm, -1, 1) * 32767).astype("<i2").tobytes()
        self.loop.call_soon_threadsafe(self.q.put_nowait, b)
    def finish(self):
        self.loop.call_soon_threadsafe(self.q.put_nowait, None); self.done.wait(30)
        return " ".join(self.final_toks).strip()

class Soniox(_WS):
    KEY = "SONIOX_API_KEY"
    async def _main(self):
        import websockets
        self.q = asyncio.Queue()
        async with websockets.connect("wss://stt-rt.soniox.com/transcribe-websocket") as ws:
            await ws.send(json.dumps(dict(api_key=os.environ[self.KEY], model=self.cfg.get("model", "stt-rt-v5"),
                audio_format="pcm_s16le", sample_rate=SR, num_channels=1, language_hints=["ru", "en"], enable_language_identification=True)))
            self.ready.set()
            async def tx():
                while (b := await self.q.get()) is not None: await ws.send(b)
                await ws.send("")                       # end of audio
            async def rx():
                async for m in ws:
                    r = json.loads(m); nf = []
                    for t in r.get("tokens", []):
                        (self.final_toks.append(t["text"]) if t.get("is_final") else nf.append(t["text"]))
                    self.emit("".join(self.final_toks), "".join(nf))
                    if r.get("finished"): return
            await asyncio.gather(tx(), rx())
        self.final_toks = ["".join(self.final_toks)]

class Deepgram(_WS):
    KEY = "DEEPGRAM_API_KEY"
    async def _main(self):
        import websockets
        self.q = asyncio.Queue()
        url = f"wss://api.deepgram.com/v1/listen?model={self.cfg.get('model','nova-3')}&language=multi&encoding=linear16&sample_rate={SR}&interim_results=true&smart_format=true&endpointing=300"
        async with websockets.connect(url, additional_headers={"Authorization": "Token " + os.environ[self.KEY]}) as ws:
            self.ready.set()
            async def tx():
                while (b := await self.q.get()) is not None: await ws.send(b)
                await ws.send(json.dumps({"type": "CloseStream"}))
            async def rx():
                async for m in ws:
                    r = json.loads(m)
                    if r.get("type") != "Results": continue
                    t = r["channel"]["alternatives"][0]["transcript"]
                    if r.get("is_final"):
                        if t: self.final_toks.append(t)
                        self.emit(" ".join(self.final_toks), "")
                    else: self.emit(" ".join(self.final_toks), t)
            await asyncio.gather(tx(), rx())
