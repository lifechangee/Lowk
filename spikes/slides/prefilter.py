#!/usr/bin/env python3
"""Offline prototype of the slide-change pre-filter (stage 1). No network, no Jev.
Usage: python3 spikes/slides/prefilter.py [--seeds 30] [--dump DIR]
Needs numpy + Pillow (+ scipy-free: own connected components)."""
import argparse, os, random, sys
import numpy as np
from PIL import Image, ImageDraw, ImageFont, ImageFilter

W, H = 1280, 720
AW, AH = 384, 224            # analysis size, 24x14 grid of 16px cells
GX, GY = 24, 14
WORDS = "roadmap latency budget privacy vault index release metrics owner risk plan sync cache model audio".split()

def font(sz):
    for p in ("/System/Library/Fonts/Helvetica.ttc", "/Library/Fonts/Arial.ttf",
              "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf"):
        try: return ImageFont.truetype(p, sz)
        except Exception: pass
    return ImageFont.load_default()

def bullets(rng, n):
    return [" ".join(rng.choices(WORDS, k=rng.randint(3, 6))) for _ in range(n)]

def draw_slide(title, lines, bg=(255, 255, 255), extra_word=None):
    im = Image.new("RGB", (W, H), bg); d = ImageDraw.Draw(im)
    d.text((80, 60), title, fill=(20, 20, 60), font=font(56))
    y = 190
    for i, l in enumerate(lines):
        if extra_word and i == len(lines) - 1: l += " " + extra_word
        d.text((100, y), "• " + l, fill=(30, 30, 30), font=font(38)); y += 80
    return im

def synth(seed):
    """Returns frames (1 fps, list of PIL) and labels: list of (t, kind) 'worth saving' moments."""
    rng = random.Random(seed); frames, events = [], []
    def hold(im, n, fn=None):
        for k in range(n):
            f = im.copy()
            if fn: fn(f, k)
            frames.append(f)
    t = lambda: len(frames)
    # slide A, then A + one bullet (small addition), then + one word (tiny addition)
    ta = draw_slide("Quarterly plan " + str(seed), (b := bullets(rng, 3)))
    events.append((t(), "new")); hold(ta, 6)
    tb = draw_slide("Quarterly plan " + str(seed), b + bullets(rng, 1))
    events.append((t(), "add_bullet")); hold(tb, 6)
    tc = draw_slide("Quarterly plan " + str(seed), tb_lines := (b + [bullets(rng, 1)[0]]))
    # slide B (new, different layout) with cursor moves and toast
    sb = draw_slide("Architecture", bullets(rng, 4), bg=(245, 247, 252))
    events.append((t(), "new"))
    cx, cy = 300, 300
    def cursor(f, k):
        x = 300 + k * 90 + rng.randint(0, 5); y = 300 + (k % 3) * 70
        ImageDraw.Draw(f).polygon([(x, y), (x, y + 24), (x + 7, y + 18), (x + 16, y + 18)], fill=(0, 0, 0))
    hold(sb, 6, cursor)
    # toast notification at corner (2 s)
    def toast(f, k): ImageDraw.Draw(f).rectangle((W - 360, H - 110, W - 20, H - 20), fill=(40, 40, 40))
    hold(sb, 2, toast); hold(sb, 3)
    # video-like segment: noisy moving content in a tile + slow fade not counted
    def video(f, k):
        # moving coloured shapes on a shifting gradient: structured video-like motion
        d = ImageDraw.Draw(f); r = random.Random(seed * 100 + k)
        d.rectangle((700, 360, 1160, 620), fill=(r.randint(0, 255), r.randint(0, 255), r.randint(0, 255)))
        for _ in range(6):
            x, y = r.randint(700, 1060), r.randint(360, 520)
            d.ellipse((x, y, x + r.randint(40, 100), y + r.randint(40, 100)),
                      fill=(r.randint(0, 255), r.randint(0, 255), r.randint(0, 255)))
    hold(sb, 12, video)
    hold(sb, 4)   # video ended, same slide: nothing to save (video tile left behind is a change; see notes)
    # webcam tile with noise on slide C (new) - always noisy small tile
    sc = draw_slide("Results", bullets(rng, 3), bg=(255, 250, 240))
    events.append((t(), "new"))
    def webcam(f, k):
        a = np.array(f); vr = np.random.RandomState(seed * 7 + k)
        a[20:150, 1100:1260] = np.clip(vr.normal(120, 40, (130, 160, 3)), 0, 255).astype(np.uint8)
        f.paste(Image.fromarray(a))
    hold(sc, 8, webcam)
    # same slide gains a short word on the last bullet
    sc2 = draw_slide("Results", bullets(rng, 3), bg=(255, 250, 240))
    rng2 = random.Random(seed)  # keep identical lines: rebuild from sc via re-draw with same rng state is brittle -> use added word on copy
    sc3 = sc.copy(); ImageDraw.Draw(sc3).text((560, 190 + 160), " also 2027", fill=(30, 30, 30), font=font(38))
    events.append((t(), "add_word")); hold(sc3, 8, webcam)
    # compression noise level change for whole slides handled by jitter below
    return frames, events

def jitter(im, seed, level=2.0):
    a = np.asarray(im).astype(np.float32); r = np.random.RandomState(seed)
    return Image.fromarray(np.clip(a + r.normal(0, level, a.shape), 0, 255).astype(np.uint8))

def analysis(im):
    g = im.convert("L").resize((AW, AH), Image.BOX).filter(ImageFilter.GaussianBlur(1.0))
    return np.asarray(g, dtype=np.float32)

def cells(diff):
    return diff.reshape(GY, AH // GY, GX, AW // GX).mean(axis=(1, 3))

def components(mask):
    seen = np.zeros_like(mask, bool); out = []
    for y in range(mask.shape[0]):
        for x in range(mask.shape[1]):
            if mask[y, x] and not seen[y, x]:
                st = [(y, x)]; seen[y, x] = True; comp = []
                while st:
                    cy, cx = st.pop(); comp.append((cy, cx))
                    for dy, dx in ((1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (-1, -1), (1, -1), (-1, 1)):
                        ny, nx = cy + dy, cx + dx
                        if 0 <= ny < mask.shape[0] and 0 <= nx < mask.shape[1] and mask[ny, nx] and not seen[ny, nx]:
                            seen[ny, nx] = True; st.append((ny, nx))
                out.append(comp)
    return out

class Detector:
    T_CELL = 6.0; MIN_CELLS = 2; STABLE = 2; HARD_FRAC = 0.25; MASK_LEN = 4; MAX_MOVING = 3; HOLD = 8; COOL = 3
    def __init__(self, **kw):
        self.__dict__.update(kw); self.S = None; self.prev = None; self.stable = 0
        self.act = np.zeros((GY, GX)); self.dyn = np.zeros((GY, GX)); self.t = 0; self.last = -99
    def step(self, a):
        self.t += 1
        if self.S is None: self.S = a; self.prev = a; return "new"
        d_prev = cells(np.abs(a - self.prev)); self.prev_a = self.prev; self.prev = a
        # cell activity: how many consecutive frames the cell was changing frame-to-frame (video / webcam / cursor)
        self.act = np.where(d_prev > self.T_CELL, self.act + 1, 0)
        self.dyn = np.where(self.act >= self.MASK_LEN, self.HOLD, np.maximum(self.dyn - 1, 0))  # video mask with hysteresis
        masked = self.dyn > 0
        settled = (d_prev <= self.T_CELL)           # per cell
        d_s = cells(np.abs(a - self.S))
        halo = masked.copy()                       # video/webcam edges bleed into neighbours: dilate by 1 cell
        halo[1:] |= masked[:-1]; halo[:-1] |= masked[1:]; halo[:, 1:] |= masked[:, :-1]; halo[:, :-1] |= masked[:, 1:]
        changed = (d_s > self.T_CELL) & ~halo
        if changed.sum() == 0: self.stable = 0; return None
        # edge frames: ignore changes touching only the outer 1-cell border (toasts, banners)
        # stable = at most a few cells still moving frame-to-frame (cursor, noise); moving cells are excluded below
        moving = ~settled & ~masked
        if moving.sum() <= self.MAX_MOVING: self.stable += 1
        else: self.stable = 0
        changed = changed & ~moving
        inner = changed.copy(); inner[[0, -1], :] = False; inner[:, [0, -1]] = False
        if self.stable < self.STABLE: return None
        frac = changed.mean()
        comps = [c for c in components(inner) if len(c) >= self.MIN_CELLS]
        kind = "new" if frac >= self.HARD_FRAC else ("add" if comps else None)
        if kind is None: return None
        if self.t - self.last < self.COOL and kind != "new": return None
        self.S = a; self.last = self.t; self.stable = 0; self.act[:] = 0
        return kind

_cache = {}
def analysed(s, noise):
    k = (s, noise)
    if k not in _cache:
        frames, events = synth(s)
        _cache[k] = ([analysis(jitter(f, s * 1000 + i, noise)) for i, f in enumerate(frames)], events)
    return _cache[k]

def evaluate(seeds, noise=2.0, window=5, **kw):
    tp = fp = fn = 0; per = {}
    for s in range(seeds):
        frames, events = analysed(s, noise)
        det = Detector(**kw); emits = []
        for i, f in enumerate(frames):
            if det.step(f): emits.append(i)
        used = set()
        for (te, kind) in events:
            m = [e for e in emits if te <= e <= te + window and e not in used]
            per.setdefault(kind, [0, 0]); per[kind][1] += 1
            if m: used.add(m[0]); tp += 1; per[kind][0] += 1
            else: fn += 1
        fp += len([e for e in emits if e not in used])
    n_ev = tp + fn
    return dict(recall=tp / n_ev, precision=tp / max(tp + fp, 1), fp=fp, fn=fn, tp=tp, per=per)

if __name__ == "__main__":
    ap = argparse.ArgumentParser(); ap.add_argument("--seeds", type=int, default=30)
    ap.add_argument("--dump"); a = ap.parse_args()
    if a.dump:
        os.makedirs(a.dump, exist_ok=True)
        fr, ev = synth(0)
        for i, f in enumerate(fr): f.save(f"{a.dump}/f{i:03d}.png")
        print("events", ev); sys.exit()
    print("A  default       ", evaluate(a.seeds))
    print("B  noise 4       ", evaluate(a.seeds, noise=4.0))
    print("C  no video mask ", evaluate(a.seeds, MASK_LEN=999))
    print("D  min cells 1   ", evaluate(a.seeds, MIN_CELLS=1))
    print("E  T_CELL 3      ", evaluate(a.seeds, T_CELL=3.0))
    print("F  T_CELL 10     ", evaluate(a.seeds, T_CELL=10.0))
