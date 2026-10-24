#!/usr/bin/env node
// Adaptive-tone end-to-end on the REAL qa bundle (`just build-qa`: same "Lowk Dev" signature and bundle id as the
// normal build, so the existing Screen Recording grant applies and no new prompt appears). Manual: needs a local macOS GUI session.
// Everything happens inside Lowk: it draws a KNOWN backdrop in its own borderless window (qa command `backdrop-set`),
// moves the overlay to 5 positions, and captures the screen region through the window server (the overlay composited
// over the backdrop, as a person sees it; capture exclusion is lifted only for these captures). No helper process
// needs a permission and the keychain is never read (LOWK_NO_KEYCHAIN). Asserts:
//   1. the sampler grabbed exactly the overlay's frame (global top-left points)
//   2. the backend grid: every cell lying wholly inside one backdrop region has that region's polarity
//   3. the PAINTED scrim (text hidden) under every such cell has that polarity and keeps the region's hue
//   4. text pixels have WCAG contrast against what is behind them
//   5. (strict) BORDER BANDS: the painted scrim in bands 8-20 pt on both sides of every backdrop border inside the overlay follows
//      the side it is on: dark side dark with NO light haze (98th-percentile luminance below a threshold), light side light, hue kept;
//      the off-grid backdrop (borders in the middle of cells) is checked half a cell further out
//   6. (strict) LATENCY: time from the layout ack to the painted scrim matching the new backdrop (<= 250 ms); p50/max in results.json
// The backdrop is a web page (tools/qa/tone-backdrop.html) in the DEFAULT BROWSER, opened with `open` and driven over a local
// 127.0.0.1 server: another process behind the overlay, as in real use. `--backdrop=app` uses Lowk's own window instead
// (fallback; also used automatically when the browser page does not come up). results.json is written after every capture.
// Output: docs/qa/tone-e2e/{gallery.png, gallery-<backdrop>.png, results.json}.   node tools/qa/tone-e2e.mjs [--quick] [--backdrop=app] [--grid]
// --grid draws the 12x8 grid lines on the gallery images (the analysis uses the clean captures).
import { spawnSync } from 'node:child_process';
import { createServer } from 'node:http';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { deflateSync, inflateSync } from 'node:zlib';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..', '..');
const out = join(root, 'docs', 'qa', 'tone-e2e');
const targetDir = process.env.CARGO_TARGET_DIR ?? join(root, 'target');
const appPath = process.env.LOWK_QA_APP ?? join(targetDir, 'debug', 'bundle', 'macos', 'Lowk.app');
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const has = (f) => process.argv.includes(f);
const env = { ...process.env, PATH: `${process.env.HOME}/.cargo/bin:${process.env.HOME}/.local/node/current/bin:${process.env.HOME}/.local/bin:${process.env.PATH}` };
const work = mkdtempSync(join(tmpdir(), 'lowk-tone-e2e-'));
const checks = [];
// Global run budget: a stuck run must end with a clear message instead of taking 18+ minutes.
const BUDGET_MS = 5 * 60 * 1000, T_RUN = Date.now();
const READY_MS = 5000; // per position: how long to wait for the sampler to show the backdrop
const overBudget = () => Date.now() - T_RUN > BUDGET_MS;
const add = (ok, name, detail = '') => { checks.push({ ok, name, detail }); if (!ok || has('--verbose')) console.log(`${ok ? 'PASS' : 'FAIL'}  ${name}  ${detail}`); };

if (process.platform !== 'darwin') { console.log('tone e2e is macOS only'); process.exit(0); }
if (!existsSync(join(appPath, 'Contents', 'Info.plist'))) { console.error(`no qa bundle at ${appPath}\nbuild it once with: just build-qa`); process.exit(2); }

// ---------- PNG ----------
function readPng(buf) {
  let o = 8, w = 0, h = 0, ct = 6; const idat = [];
  while (o < buf.length) {
    const len = buf.readUInt32BE(o), type = buf.toString('ascii', o + 4, o + 8), d = buf.subarray(o + 8, o + 8 + len);
    if (type === 'IHDR') { w = d.readUInt32BE(0); h = d.readUInt32BE(4); ct = d[9]; if (d[8] !== 8 || d[12] !== 0) throw new Error('png: 8-bit non-interlaced only'); }
    if (type === 'IDAT') idat.push(d);
    o += 12 + len;
  }
  const bpp = ct === 6 ? 4 : 3, raw = inflateSync(Buffer.concat(idat)), stride = w * bpp, px = Buffer.alloc(h * stride);
  for (let y = 0; y < h; y++) {
    const f = raw[y * (stride + 1)], src = raw.subarray(y * (stride + 1) + 1, (y + 1) * (stride + 1)), row = px.subarray(y * stride, (y + 1) * stride), prev = y ? px.subarray((y - 1) * stride, y * stride) : null;
    for (let x = 0; x < stride; x++) {
      const a = x >= bpp ? row[x - bpp] : 0, b = prev ? prev[x] : 0, c = prev && x >= bpp ? prev[x - bpp] : 0;
      const p = a + b - c, pa = Math.abs(p - a), pb = Math.abs(p - b), pc = Math.abs(p - c);
      row[x] = (src[x] + (f === 0 ? 0 : f === 1 ? a : f === 2 ? b : f === 3 ? (a + b) >> 1 : pa <= pb && pa <= pc ? a : pb <= pc ? b : c)) & 255;
    }
  }
  return { w, h, bpp, px };
}
const crcT = new Int32Array(256).map((_, n) => { let c = n; for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1; return c; });
const crc = (b) => { let c = -1; for (const x of b) c = crcT[(c ^ x) & 255] ^ (c >>> 8); return (c ^ -1) >>> 0; };
function writePng(path, w, h, rgb) {
  const chunk = (t, d) => { const b = Buffer.alloc(12 + d.length); b.writeUInt32BE(d.length, 0); b.write(t, 4, 'ascii'); d.copy(b, 8); b.writeUInt32BE(crc(b.subarray(4, 8 + d.length)), 8 + d.length); return b; };
  const ih = Buffer.alloc(13); ih.writeUInt32BE(w, 0); ih.writeUInt32BE(h, 4); ih[8] = 8; ih[9] = 2;
  const raw = Buffer.alloc(h * (w * 3 + 1));
  for (let y = 0; y < h; y++) rgb.copy(raw, y * (w * 3 + 1) + 1, y * w * 3, (y + 1) * w * 3);
  writeFileSync(path, Buffer.concat([Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]), chunk('IHDR', ih), chunk('IDAT', deflateSync(raw, { level: 6 })), chunk('IEND', Buffer.alloc(0))]));
}
const px = (img, x, y) => { const i = (Math.floor(y) * img.w + Math.floor(x)) * img.bpp; return [img.px[i], img.px[i + 1], img.px[i + 2]]; };
const avg = (img, cx, cy, r) => { let s = [0, 0, 0], n = 0; for (let y = Math.floor(cy - r); y <= Math.floor(cy + r); y++) for (let x = Math.floor(cx - r); x <= Math.floor(cx + r); x++) { if (x < 0 || y < 0 || x >= img.w || y >= img.h) continue; const p = px(img, x, y); s[0] += p[0]; s[1] += p[1]; s[2] += p[2]; n++; } return s.map((v) => v / Math.max(n, 1)); };
const lin = (v) => { const c = v / 255; return c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4; };
const lum = (p) => 0.2126 * lin(p[0]) + 0.7152 * lin(p[1]) + 0.0722 * lin(p[2]);
const ratio = (a, b) => { const x = lum(a), y = lum(b); return (Math.max(x, y) + 0.05) / (Math.min(x, y) + 0.05); };

// ---------- app + stage ----------
const data = join(work, 'data');
mkdirSync(data, { recursive: true });
writeFileSync(join(data, 'settings.toml'), `[onboarding]\ndone = true\nrecording_notice_ack = true\n[vault]\npath = "${work}/vault"\n[overlay]\nopacity = 66\nvisibility = "always"\nadaptive_scrim = true\n`);
const port = 19000 + Math.floor(Math.random() * 1000);
const token = [...crypto.getRandomValues(new Uint8Array(16))].map((b) => b.toString(16).padStart(2, '0')).join('');
// Launched through LaunchServices (`open`), NOT as a child of this shell: macOS then attributes Screen Recording to Lowk itself
// (its own grant) instead of the terminal that started the script.
const envArgs = Object.entries({ LOWK_QA_MULTI: '1', LOWK_NO_KEYCHAIN: '1', LOWK_DATA_DIR: data, LOWK_QA_PORT: String(port), LOWK_QA_TOKEN: token }).flatMap(([k, v]) => ['--env', `${k}=${v}`]);
const opened = spawnSync('open', ['-n', ...envArgs, appPath], { stdio: 'inherit' });
if (opened.status !== 0) { console.error('open failed'); process.exit(2); }
const cmd = async (line) => (await fetch(`http://127.0.0.1:${port}/cmd`, { method: 'POST', headers: { 'X-Lowk-Qa-Token': token }, body: line })).text();
const ev = async (expr) => { const j = JSON.parse(await cmd(`eval overlay ${expr}`)); if (!j.ok) throw new Error(String(j.v)); return j.v; };
// ---- browser backdrop: local server + page in the default browser ----
const bk = { seq: 0, rects: [], bye: false, ack: -1, vp: null, hello: null };
const here = dirname(fileURLToPath(import.meta.url));
let bserver = null;
const startBackdropServer = () => new Promise((res) => {
  bserver = createServer((rq, rs) => {
    let body = '';
    rq.on('data', (c) => { body += c; });
    rq.on('end', () => {
      const j = () => { try { return JSON.parse(body); } catch { return {}; } };
      if (rq.url === '/state') { rs.setHeader('content-type', 'application/json'); rs.end(JSON.stringify({ seq: bk.seq, rects: bk.rects, bye: bk.bye })); }
      else if (rq.url === '/ack') { const o = j(); if (o.seq === bk.seq || o.seq === -2) { bk.ack = bk.seq; } bk.vp = o.vp ?? bk.vp; rs.end('ok'); }
      else if (rq.url === '/hello') { bk.hello = j().vp ?? null; bk.vp = bk.hello ?? bk.vp; rs.end('ok'); }
      else { rs.setHeader('content-type', 'text/html'); rs.end(readFileSync(join(here, 'tone-backdrop.html'))); }
    });
  }).listen(0, '127.0.0.1', () => res(bserver.address().port));
});
let backdropKind = has('--backdrop=app') ? 'app' : 'browser';
let cover = null; // the rectangle (global top-left points) the backdrop really covers
const browserLayout = async (rects) => {
  // A lost ack (seen once per run) is retried a few times with a fresh seq before giving up.
  for (let attempt = 1; attempt <= 4; attempt++) {
    bk.seq++; bk.rects = rects;
    for (let i = 0; i < 100 && bk.ack !== bk.seq; i++) await sleep(50);
    if (bk.ack === bk.seq) return 'ok';
    console.log(`browser backdrop did not acknowledge layout (attempt ${attempt}/4)`);
  }
  throw new Error('browser backdrop did not acknowledge a layout');
};
const stage = async (o) => {
  if (backdropKind === 'browser') {
    if (o.cmd === 'info') return { w: bk.vp.sw, h: bk.vp.sh, scale: (await appInfo()).scale };
    if (o.cmd === 'layout') return browserLayout(o.rects);
    bk.bye = true; bk.seq++; await sleep(400); return 'closed';
  }
  if (o.cmd === 'info') return JSON.parse(await cmd('backdrop-info'));
  if (o.cmd === 'layout') return cmd('backdrop-set ' + JSON.stringify(o.rects));
  return cmd('backdrop-close');
};
const appInfo = async () => JSON.parse(await cmd('backdrop-info'));
let cleaned = false;
const cleanup = async () => {
  if (cleaned) return; cleaned = true;
  try { await cmd('exclusion on'); } catch {}
  try { await cmd('quit'); } catch {}
  try { await stage({ cmd: 'close' }); } catch {}
  await sleep(400);
  try { bserver?.close(); } catch {}
  rmSync(work, { recursive: true, force: true });
};
process.on('SIGINT', async () => { await cleanup(); process.exit(130); });

// ---------- scenarios ----------
const RGB = { black: [0, 0, 0], white: [255, 255, 255], navy: [10, 20, 90], red: [200, 30, 30], yellow: [240, 220, 40], teal: [0, 110, 120], grey: [225, 225, 225], green: [30, 140, 60] };
const GC = 12, GR = 8; // backend tone grid (cols x rows), must equal lowk-stealth tone::GRID_COLS/GRID_ROWS
const POL = { black: 'dark', navy: 'dark', red: 'dark', teal: 'dark', green: 'dark', white: 'light', yellow: 'light', grey: 'light' };
// Each backdrop: regions as fractions of the OVERLAY frame, plus a fixed-to-screen variant for some. Region boundaries sit on cell edges
// (cols 1/4 1/2 3/4, rows 1/3 2/3) so most cells are "pure", but the screen positions below make the boundaries land on different pixels.
const BACKDROPS = [
  { name: 'left-black-right-white', regions: [{ x0: -1, x1: 0.5, y0: -1, y1: 2, c: 'black' }, { x0: 0.5, x1: 2, y0: -1, y1: 2, c: 'white' }] },
  { name: 'quadrants-navy-red-yellow-teal', regions: [{ x0: -1, x1: 0.5, y0: -1, y1: 0.5, c: 'navy' }, { x0: 0.5, x1: 2, y0: -1, y1: 0.5, c: 'red' }, { x0: -1, x1: 0.5, y0: 0.5, y1: 2, c: 'yellow' }, { x0: 0.5, x1: 2, y0: 0.5, y1: 2, c: 'teal' }] },
  { name: 'vertical-split-top-yellow-bottom-navy', regions: [{ x0: -1, x1: 2, y0: -1, y1: 0.5, c: 'yellow' }, { x0: -1, x1: 2, y0: 0.5, y1: 2, c: 'navy' }] },
  { name: 'left-quarter-green-rest-grey', regions: [{ x0: -1, x1: 0.25, y0: -1, y1: 2, c: 'green' }, { x0: 0.25, x1: 2, y0: -1, y1: 2, c: 'grey' }] },
  { name: 'right-quarter-black-rest-white', regions: [{ x0: -1, x1: 0.75, y0: -1, y1: 2, c: 'white' }, { x0: 0.75, x1: 2, y0: -1, y1: 2, c: 'black' }] },
  // borders in the MIDDLE of cells (5.16 and 3.28 cells): the grid cannot be exact here, the paint must still follow the majority
  { name: 'off-grid-quadrants-navy-red-yellow-teal', offGrid: true, regions: [{ x0: -1, x1: 0.43, y0: -1, y1: 0.41, c: 'navy' }, { x0: 0.43, x1: 2, y0: -1, y1: 0.41, c: 'red' }, { x0: -1, x1: 0.43, y0: 0.41, y1: 2, c: 'yellow' }, { x0: 0.43, x1: 2, y0: 0.41, y1: 2, c: 'teal' }] },
];
const appUp = async () => { for (let i = 0; i < 150 && !existsSync(join(data, 'qa.json')); i++) await sleep(100); for (let i = 0; i < 100; i++) { try { await cmd('pid'); return; } catch { await sleep(100); } } };
await appUp();
const dinfo = await (async () => { for (let i = 0; i < 100; i++) { try { return await appInfo(); } catch { await sleep(100); } } })();
const D = { w: dinfo.w, h: dinfo.h, scale: dinfo.scale };
console.log(`display ${D.w}x${D.h} pt, scale ${D.scale}`);
if (backdropKind === 'browser') {
  const bport = await startBackdropServer();
  spawnSync('open', [`http://127.0.0.1:${bport}/`], { stdio: 'inherit' });
  console.log('backdrop page opened in your default browser; make it fill the screen (Ctrl+Cmd+F) - starts within 10 s of the page loading');
  // wait for the page (up to 30 s), then at most 10 s more for full screen
  for (let i = 0; i < 300 && !bk.hello; i++) await sleep(100);
  for (let i = 0; i < 100 && bk.hello && !bk.hello.full; i++) await sleep(100);
  if (!bk.hello) { console.log('browser page did not report in: falling back to the in-app backdrop'); backdropKind = 'app'; }
  else if (!bk.hello.full) console.log('browser window is not full-screen: only the part it covers is checked (cells outside it are not asserted)');
}
const browserCover = () => (backdropKind === 'browser' && bk.vp ? { x: bk.vp.x, y: bk.vp.y, w: bk.vp.w, h: bk.vp.h } : { x: 0, y: 0, w: D.w, h: D.h });
console.log(`backdrop: ${backdropKind}`);
const OW = 600, OH = 384;
const POSITIONS = [
  { name: 'centre', x: Math.round((D.w - OW) / 2), y: Math.round((D.h - OH) / 2) },
  { name: 'top-left-edge', x: 0, y: 40 },
  { name: 'right-edge', x: Math.round(D.w - OW), y: 120 },
  { name: 'bottom-edge', x: Math.round(D.w * 0.3), y: Math.round(D.h - OH) },
  { name: 'odd-offset', x: 173, y: 97 },
  // hangs over the left and bottom display edges (if the app lets it): clipped cells must follow the on-screen backdrop
  { name: 'off-screen-corner', x: -180, y: Math.round(D.h - OH + 110) },
];
const BDS = has('--quick') ? BACKDROPS.slice(0, 2) : BACKDROPS;
const latencies = []; // ms from layout ack to the painted scrim matching, one per backdrop change

try {
  await sleep(800);
  await cmd('overlay-show');
  await cmd('demo-start');
  await sleep(2500);
  await cmd('demo-stop');
  await sleep(600);
  await cmd('overlay-show');
  await sleep(500);
  await cmd('exclusion off');
  mkdirSync(out, { recursive: true });
  const tiles = []; // { bd, pos, w, h, rgb }
  const results = [];
  const NOTEXT = `.ov [data-blk], .ov [data-gt]{visibility:hidden!important}`;
  const TRANSP = `.ov [data-blk], .ov [data-blk] *, .ov [data-gt], .ov [data-gt] *{color:transparent!important;-webkit-text-fill-color:transparent!important}.ov svg{visibility:hidden!important}.ov .caret{visibility:hidden!important}.ov [data-grad]{background-image:none!important}`;
  const styleOn = (css) => ev(`(()=>{let s=document.getElementById('qa-s');if(!s){s=document.createElement('style');s.id='qa-s';document.head.appendChild(s)}s.textContent=${JSON.stringify(css)};return 1})()`);
  const styleOff = () => ev(`(()=>{document.getElementById('qa-s')?.remove();return 1})()`);

  for (const pos of POSITIONS) {
    { await cmd(`overlay-move ${pos.x} ${pos.y}`); await sleep(300); }
    for (const bd of BDS) {
      if (overBudget()) throw new Error(`ABORT: run budget of ${BUDGET_MS / 1000} s exceeded before ${bd.name} @ ${pos.name}; ${checks.filter((c) => !c.ok).length} checks failed so far (results.json holds the partial run)`);
      const fr = JSON.parse(await cmd('overlay-frame')).frame;
      const rects = bd.regions.map((r) => ({ x: fr.x + r.x0 * fr.w, y: fr.y + r.y0 * fr.h, w: (r.x1 - r.x0) * fr.w, h: (r.y1 - r.y0) * fr.h, c: RGB[r.c] }));
      // painted scrim (text hidden) of the whole overlay, one lum per cell centre, as fast as a capture goes (latency probe)
      const probe = async () => {
        const pth = join(work, 'probe.png');
        const t = performance.now();
        const r = await cmd(`region-png ${fr.x} ${fr.y} ${fr.w} ${fr.h} 1 ${pth}`);
        if (r !== 'ok') throw new Error('probe capture failed: ' + r);
        const im = readPng(readFileSync(pth)), kk = im.w / fr.w;
        return { ms: performance.now() - t, lum: Array.from({ length: GC * GR }, (_, i) => lum(avg(im, ((i % GC) + 0.5) * (fr.w / GC) * kk, (Math.floor(i / GC) + 0.5) * (fr.h / GR) * kk, 3))) };
      };
      await styleOn(NOTEXT); await sleep(400);
      const before = await probe();
      await stage({ cmd: 'layout', rects });
      const tAck = performance.now();
      let latency = null;
      {
        // a cell is a latency witness when its pure expectation is known and the painted polarity before the layout was the other one
        const cvz = browserCover();
        const wit = [];
        for (let i = 0; i < GC * GR; i++) {
          const x0 = fr.x + (i % GC) * (fr.w / GC) + 8, x1 = fr.x + ((i % GC) + 1) * (fr.w / GC) - 8, y0 = fr.y + Math.floor(i / GC) * (fr.h / GR) + 8, y1 = fr.y + (Math.floor(i / GC) + 1) * (fr.h / GR) - 8;
          if (!(x0 >= cvz.x && x1 <= cvz.x + cvz.w && y0 >= cvz.y && y1 <= cvz.y + cvz.h)) continue;
          const rr = bd.regions.filter((r) => x0 >= fr.x + r.x0 * fr.w && x1 <= fr.x + r.x1 * fr.w && y0 >= fr.y + r.y0 * fr.h && y1 <= fr.y + r.y1 * fr.h);
          if (rr.length !== 1) continue;
          const dark = POL[rr[0].c] === 'dark', wasDark = before.lum[i] < 0.3;
          if (dark !== wasDark) wit.push({ i, dark });
        }
        if (wit.length >= 3) {
          let last = null;
          for (let n = 0; n < 60; n++) {
            const pr = await probe(); last = pr;
            if (wit.every((w) => (w.dark ? pr.lum[w.i] < 0.2 : pr.lum[w.i] > 0.5))) { { const total = Math.round(performance.now() - tAck), g = Math.round(pr.ms); latency = { ms: Math.max(0, total - g), total_ms: total, grab_ms: g, witnesses: wit.length }; } break; }
          }
          if (!latency) latency = { ms: null, witnesses: wit.length, never: wit.filter((w) => (w.dark ? last.lum[w.i] >= 0.2 : last.lum[w.i] <= 0.5)).map((w) => w.i) };
        }
      }
      if (latency?.ms != null) latencies.push(latency.ms);
      const tag = `${bd.name} @ ${pos.name}`;
      add(!latency || (latency.ms != null && latency.ms <= 250), `${tag}: backdrop change reaches the painted scrim within 250 ms of the layout ack (Lowk side: harness grab time subtracted)`, latency ? JSON.stringify(latency) : 'no cell changed polarity: not measured');
      const ofr = JSON.parse(await cmd('overlay-frame'));
      const s = ofr.sampled, f2 = ofr.frame;
      // Expected colour per cell from the REAL overlay frame vs the backdrop geometry: a cell is asserted only when it
      // lies fully inside ONE colour region (inset INSET pt for the sampler's box filter) and inside what the backdrop
      // really covers (outside it the stage is black / the desktop). Everything else is not asserted.
      const INSET = 8;
      const cw = f2.w / GC, ch = f2.h / GR;
      const cv = browserCover();
      const regs = bd.regions.map((r) => ({ c: r.c, x0: fr.x + r.x0 * fr.w, x1: fr.x + r.x1 * fr.w, y0: fr.y + r.y0 * fr.h, y1: fr.y + r.y1 * fr.h }));
      // Cells hanging over the display edge are clipped to the display (and the cover): only their on-screen part is asserted,
      // the sampler builds them from on-screen pixels only; cells fully off-screen are not asserted.
      const clipTo = { x0: Math.max(0, cv.x), y0: Math.max(0, cv.y), x1: Math.min(D.w, cv.x + cv.w), y1: Math.min(D.h, cv.y + cv.h) };
      const spot = []; // asserted rectangle per cell (global points) or null
      const expect = Array.from({ length: GC * GR }, (_, i) => {
        const gx0 = f2.x + (i % GC) * cw, gx1 = gx0 + cw, gy0 = f2.y + Math.floor(i / GC) * ch, gy1 = gy0 + ch;
        const clipped = gx0 < clipTo.x0 || gx1 > clipTo.x1 || gy0 < clipTo.y0 || gy1 > clipTo.y1;
        const ins = (edgeOut) => (edgeOut ? 2 : INSET);
        const cx0 = Math.max(gx0, clipTo.x0) + ins(gx0 < clipTo.x0), cx1 = Math.min(gx1, clipTo.x1) - ins(gx1 > clipTo.x1);
        const cy0 = Math.max(gy0, clipTo.y0) + ins(gy0 < clipTo.y0), cy1 = Math.min(gy1, clipTo.y1) - ins(gy1 > clipTo.y1);
        spot[i] = null;
        if (cx1 - cx0 < 3 || cy1 - cy0 < 3 || (clipped && (cx1 - cx0 < 6 || cy1 - cy0 < 6))) return null;
        const rr = regs.filter((r) => cx0 >= r.x0 && cx1 <= r.x1 && cy0 >= r.y0 && cy1 <= r.y1);
        if (rr.length !== 1) return null;
        spot[i] = { cx: (cx0 + cx1) / 2, cy: (cy0 + cy1) / 2, clipped };
        return rr[0].c;
      });
      // Readiness: the first capture per backdrop used to be stale (the browser had not repainted yet). Poll the sampler's
      // own cell_means until every pure cell shows its expected backdrop colour (or time out and fail loudly), then let the
      // the 2-sample confirmation (2 x 100 ms polls) settle.
      const near = (m, c) => m && Math.abs(m[0] - c[0]) <= 14 && Math.abs(m[1] - c[1]) <= 14 && Math.abs(m[2] - c[2]) <= 14;
      const pureIdx = expect.map((c, i) => (c ? i : -1)).filter((i) => i >= 0);
      // Only frames grabbed AFTER the layout ack and the overlay move count: remember the sampler's frame counter now and
      // wait for it to advance by 2 before comparing (tone-diag.sampler.frames; a frame in flight at the ack may be stale).
      const frameNow = async () => { try { return JSON.parse(await cmd('tone-diag')).sampler?.frames ?? 0; } catch { return 0; } };
      const f0 = await frameNow();
      let sdiag = null, ready = pureIdx.length === 0, lastMeans = null, frames = f0, mism = [];
      for (let t = 0; t < READY_MS / 250 && !ready; t++) { // 5 s
        try { const d = JSON.parse(await cmd('tone-diag')); lastMeans = d.sampler?.cell_means ?? []; frames = d.sampler?.frames ?? frames; sdiag = d.sampler; } catch { lastMeans = []; }
        mism = pureIdx.filter((i) => !near(lastMeans[i], RGB[expect[i]]));
        ready = (frames >= f0 + 2 || (sdiag?.mode === 'stream' && frames >= f0 + 1)) && mism.length === 0;
        if (!ready) await sleep(250);
      }
      add(ready, `${tag}: backdrop on screen (sampler cell_means match the pattern)`, ready ? '' : JSON.stringify({ frames_at_ack: f0, frames_now: frames, mismatched_cells: mism.map((i) => ({ cell: i, row: Math.floor(i / GC), expected: RGB[expect[i]], got: lastMeans?.[i] })) }));
      await sleep(1500);
      // 1. the sampler looks where the overlay is
      add(!!s && Math.abs(s.x - f2.x) < 1.5 && Math.abs(s.y - f2.y) < 1.5 && Math.abs(s.w - f2.w) < 1.5 && Math.abs(s.h - f2.h) < 1.5, `${tag}: sampler region == overlay frame`, JSON.stringify({ sampled: s, frame: f2 }));
      const nPure = pureIdx.length;
      // edge positions (incl. clipped cells) legitimately leave few pure cells (the overlay straddles region borders or the cover): need at least one
      add(nPure >= 1, `${tag}: at least one pure cell to assert`, `${nPure}/${GC * GR} pure (others not asserted)`);
      // 2. backend grid
      const diagRaw = JSON.parse(await cmd('tone-diag'));
      const tone = diagRaw.tone;
      const diag = { sampler: diagRaw.sampler, grab: diagRaw.grab };
      const grid = tone.grid?.cells ?? [];
      add(grid.length === GC * GR, `${tag}: backend sent a ${GC}x${GR} grid`, `${grid.length} cells`);
      if (diag.sampler?.mode === 'stream') {
        // stream mode has no polled-grab object: assert on the stream's own health
        const sm = diag.sampler;
        add(sm.grab_errors === 0 && !sm.last_error && sm.frames_per_s > 0 && sm.last_frame_age_ms != null && sm.last_frame_age_ms < 5000, `${tag}: stream healthy (frames arriving, no errors)`, JSON.stringify({ mode: sm.mode, frames_per_s: sm.frames_per_s, last_frame_age_ms: sm.last_frame_age_ms, grab_errors: sm.grab_errors, last_error: sm.last_error }));
      } else {
        add(!!diag.grab && !diag.grab.error && (diag.sampler?.grab_errors ?? 1) === 0, `${tag}: sampler grabs succeed`, JSON.stringify({ mode: diag.sampler?.mode, err: diag.grab?.error, grab_errors: diag.sampler?.grab_errors, last_error: diag.sampler?.last_error }));
      }
      add((diag.sampler?.cell_means ?? []).some((m) => m[0] + m[1] + m[2] > 0), `${tag}: sampler image is not all black`, JSON.stringify(diag.sampler?.cell_means));
      const be = [];
      // the polarity that matters is the scrim luminance (dark chromatic backdrops are tone kind "color")
      const scrimLum = (rgbStr) => lum(rgbStr.split(',').map(Number));
      expect.forEach((c, i) => { if (!c || !grid[i]) return; const L = scrimLum(grid[i].rgb); const ok = POL[c] === 'dark' ? L < 0.12 : L > 0.6; if (!ok) be.push(`cell ${i} ${c}: backend scrim ${grid[i].rgb} (lum ${L.toFixed(2)})`); });
      add(be.length === 0, `${tag}: backend cell polarity follows the backdrop`, be.join(' | '));
      // 3. painted scrim, text hidden
      const reg = { x: Math.max(0, f2.x - 40), y: Math.max(0, f2.y - 40) };
      reg.w = Math.min(D.w, f2.x + f2.w + 40) - reg.x; reg.h = Math.min(D.h, f2.y + f2.h + 40) - reg.y;
      const grab = async (name) => { const p = join(work, name + '.png'); const r = await cmd(`region-png ${reg.x} ${reg.y} ${reg.w} ${reg.h} ${D.scale} ${p}`); if (r !== 'ok') throw new Error('capture failed: ' + r); return readPng(readFileSync(p)); };
      await styleOn(NOTEXT); await sleep(350);
      const bare = await grab('bare');
      await styleOn(TRANSP); await sleep(350);
      const base = await grab('base');
      await styleOff(); await sleep(350);
      const full = await grab('full');
      const k = bare.w / reg.w;
      const ox = (f2.x - reg.x) * k, oy = (f2.y - reg.y) * k;
      const pa = [];
      const cellsOut = [];
      expect.forEach((c, i) => {
        const sp = spot[i];
        const cx = sp ? (sp.cx - reg.x) * k : ox + ((i % GC) + 0.5) * cw * k, cy = sp ? (sp.cy - reg.y) * k : oy + (Math.floor(i / GC) + 0.5) * ch * k;
        const S = avg(bare, cx, cy, 5), L = lum(S);
        cellsOut.push({ i, backdrop: c, painted: S.map(Math.round) });
        if (!c) return;
        const dark = POL[c] === 'dark';
        if (dark ? L > 0.2 : L < 0.5) pa.push(`cell ${i} ${c}: painted rgb(${S.map(Math.round)}) lum ${L.toFixed(2)} is not ${POL[c]}`);
        const B = RGB[c];
        const sat = Math.max(...B) - Math.min(...B);
        if (sat > 90 && dark) { // a saturated dark backdrop keeps its hue in the scrim: its strongest channel stays the strongest
          const bi = B.indexOf(Math.max(...B)), others = [0, 1, 2].filter((j) => j !== bi);
          const top = Math.max(...S);
          if (c === 'teal') { if (!(S[1] > S[0] + 8 && S[2] > S[0] + 8)) pa.push(`cell ${i} teal: painted rgb(${S.map(Math.round)}) lost the hue`); }
          else if (S[bi] < top - 0.5 || S[bi] - Math.max(...others.map((j) => S[j])) < 8) pa.push(`cell ${i} ${c}: painted rgb(${S.map(Math.round)}) lost the hue`);
        }
        if (c === 'yellow' && !(S[2] < Math.min(S[0], S[1]) - 3)) pa.push(`cell ${i} yellow: painted rgb(${S.map(Math.round)}) lost the warm tint`);
      });
      add(pa.length === 0, `${tag}: painted scrim follows the backdrop under each cell`, pa.join(' | '));
      // 3b. strict: bands 8-20 pt (off-grid backdrop: half a cell further out) on BOTH sides of every border inside the overlay
      const bandFail = []; const bandsOut = [];
      {
        const lo = bd.offGrid ? 0.5 * cw + 6 : 8, hi = lo + 12, trim = hi + 4;
        const cvz = browserCover();
        const clip = (a, b, lo2, hi2) => [Math.max(a, lo2), Math.min(b, hi2)];
        const test = (side, c, x0, x1, y0, y1, lab) => { // rectangle in global points -> painted pixels of `bare`
          [x0, x1] = clip(x0, x1, Math.max(f2.x + 1, cvz.x), Math.min(f2.x + f2.w - 1, cvz.x + cvz.w));
          [y0, y1] = clip(y0, y1, Math.max(f2.y + 1, cvz.y), Math.min(f2.y + f2.h - 1, cvz.y + cvz.h));
          if (x1 - x0 < 4 || y1 - y0 < 4) return;
          const ls = [], mean = [0, 0, 0]; let n = 0;
          for (let y = Math.floor((y0 - reg.y) * k); y < Math.ceil((y1 - reg.y) * k); y++) for (let x = Math.floor((x0 - reg.x) * k); x < Math.ceil((x1 - reg.x) * k); x++) { const p = px(bare, x, y); ls.push(lum(p)); mean[0] += p[0]; mean[1] += p[1]; mean[2] += p[2]; n++; }
          if (!n) return;
          ls.sort((a, b) => a - b);
          const M = mean.map((v) => v / n), dark = POL[c] === 'dark';
          const hi98 = ls[Math.floor((ls.length - 1) * 0.98)], lo02 = ls[Math.floor((ls.length - 1) * 0.02)];
          let why = '';
          if (dark && hi98 > 0.25) why = `light haze over a dark region (98th-percentile luminance ${hi98.toFixed(2)} > 0.25)`;
          else if (!dark && lo02 < 0.45) why = `dark smear over a light region (2nd-percentile luminance ${lo02.toFixed(2)} < 0.45)`;
          const B = RGB[c], sat = Math.max(...B) - Math.min(...B);
          if (!why && dark && sat > 90) { const bi = B.indexOf(Math.max(...B)); if (c === 'teal' ? !(M[1] > M[0] + 6 && M[2] > M[0] + 6) : (M[bi] < Math.max(...M) - 0.5 || M[bi] - Math.max(...[0, 1, 2].filter((j) => j !== bi).map((j) => M[j])) < 6)) why = `hue lost (mean rgb ${M.map(Math.round)})`; }
          if (!why && c === 'yellow' && !(M[2] < Math.min(M[0], M[1]) - 3)) why = `warm tint lost (mean rgb ${M.map(Math.round)})`;
          bandsOut.push({ border: lab, side, backdrop: c, mean: M.map(Math.round), p02: +lo02.toFixed(2), p98: +hi98.toFixed(2), ok: !why });
          if (why) bandFail.push(`${lab} ${side} (${c}): ${why}`);
        };
        for (const A of regs) for (const B2 of regs) {
          if (A === B2 || A.c === B2.c) continue;
          if (Math.abs(A.x1 - B2.x0) < 0.5 && A.x1 > f2.x && A.x1 < f2.x + f2.w) { // vertical border at X
            const X = A.x1, ya = Math.max(A.y0, B2.y0) + trim, yb = Math.min(A.y1, B2.y1) - trim, lab = `v@${Math.round(X - f2.x)}`;
            if (yb - ya >= 12) { test('left', A.c, X - hi, X - lo, ya, yb, lab); test('right', B2.c, X + lo, X + hi, ya, yb, lab); }
          }
          if (Math.abs(A.y1 - B2.y0) < 0.5 && A.y1 > f2.y && A.y1 < f2.y + f2.h) { // horizontal border at Y
            const Y = A.y1, xa = Math.max(A.x0, B2.x0) + trim, xb = Math.min(A.x1, B2.x1) - trim, lab = `h@${Math.round(Y - f2.y)}`;
            if (xb - xa >= 12) { test('above', A.c, xa, xb, Y - hi, Y - lo, lab); test('below', B2.c, xa, xb, Y + lo, Y + hi, lab); }
          }
        }
      }
      add(bandFail.length === 0, `${tag}: border bands follow the backdrop (no haze across borders)`, bandFail.join(' | ') || `${bandsOut.length} bands`);
      // 4. text contrast against what is directly behind the glyph
      const ratios = []; const bad = [];
      for (let y = Math.round(oy); y < Math.round(oy + f2.h * k); y++) for (let x = Math.round(ox); x < Math.round(ox + f2.w * k); x++) {
        if (x < 0 || y < 0 || x >= full.w || y >= full.h) continue;
        const a = px(full, x, y), b = px(base, x, y);
        if (Math.max(Math.abs(a[0] - b[0]), Math.abs(a[1] - b[1]), Math.abs(a[2] - b[2])) < 110) continue; // glyph core only
        ratios.push(ratio(a, b));
        if (ratio(a, b) < 3) bad.push([x, y]);
      }
      ratios.sort((p, q) => p - q);
      const pct = (q) => ratios.length ? ratios[Math.floor(ratios.length * q)] : 0;
      add(ratios.length > 400 && pct(0.1) >= 3 && pct(0.5) >= 4.5, `${tag}: text contrast`, `${ratios.length} glyph px, p10 ${pct(0.1).toFixed(1)}:1, median ${pct(0.5).toFixed(1)}:1`);
      // tile for the gallery (downscaled 2x by box filter to keep the sheet small)
      const dw = Math.floor(full.w / 2), dh = Math.floor(full.h / 2), t = Buffer.alloc(dw * dh * 3);
      for (let y = 0; y < dh; y++) for (let x = 0; x < dw; x++) for (let c3 = 0; c3 < 3; c3++) { let sacc = 0; for (const [dx, dy] of [[0, 0], [1, 0], [0, 1], [1, 1]]) sacc += full.px[((y * 2 + dy) * full.w + x * 2 + dx) * full.bpp + c3]; t[(y * dw + x) * 3 + c3] = sacc / 4; }
      if (has('--grid')) { // gallery only: the 12x8 grid lines of the overlay frame (tile px = capture px / 2)
        const dot = (x, y) => { if (x < 0 || y < 0 || x >= dw || y >= dh) return; for (const [c3, v] of [[0, 0], [1, 255], [2, 255]]) t[(y * dw + x) * 3 + c3] = (t[(y * dw + x) * 3 + c3] + v) >> 1; };
        for (let c = 0; c <= GC; c++) for (let y = Math.round(oy / 2); y <= Math.round((oy + f2.h * k) / 2); y++) dot(Math.round((ox + c * cw * k) / 2), y);
        for (let r = 0; r <= GR; r++) for (let x = Math.round(ox / 2); x <= Math.round((ox + f2.w * k) / 2); x++) dot(x, Math.round((oy + r * ch * k) / 2));
      }
      tiles.push({ bd: bd.name, pos: pos.name, w: dw, h: dh, rgb: t });
      const writeResults = () => writeFileSync(join(out, 'results.json'), JSON.stringify({ display: D, backdropKind, partial: true, checks, results }, null, 1));
      results.push({ backdrop: bd.name, backdropKind, cover: browserCover(), diag,  position: pos.name, latency, bands: bandsOut, frame: f2, sampled: s, region: reg, cells: cellsOut, glyph: { n: ratios.length, p10: pct(0.1), median: pct(0.5) }, backend: grid.map((g) => ({ tone: g.tone, rgb: g.rgb })) });
      writeResults();
    }
  }
  // gallery sheets
  const sheet = (list, file, cols) => {
    const tw = Math.max(...list.map((t) => t.w)) + 8, th = Math.max(...list.map((t) => t.h)) + 8, rows = Math.ceil(list.length / cols);
    const W = tw * cols, H = th * rows, buf = Buffer.alloc(W * H * 3, 32);
    list.forEach((t, n) => { const bx = (n % cols) * tw + 4, by = Math.floor(n / cols) * th + 4; for (let y = 0; y < t.h; y++) t.rgb.copy(buf, ((by + y) * W + bx) * 3, y * t.w * 3, (y + 1) * t.w * 3); });
    writePng(file, W, H, buf);
  };
  for (const bd of BDS) sheet(tiles.filter((t) => t.bd === bd.name), join(out, `gallery-${bd.name}.png`), 3);
  sheet(tiles, join(out, 'gallery.png'), POSITIONS.length);
  const sorted = [...latencies].sort((a, b) => a - b);
  const latencySummary = { n: sorted.length, p50: sorted.length ? sorted[Math.floor((sorted.length - 1) / 2)] : null, max: sorted.length ? sorted[sorted.length - 1] : null, samples_ms: latencies, limit_ms: 250, p50_limit_ms: 150 };
  console.log(`latency (layout ack -> painted scrim): n=${latencySummary.n} p50=${latencySummary.p50} ms max=${latencySummary.max} ms (limit p50 150 / max 250)`);
  add(latencySummary.n > 0 && latencySummary.max <= 250 && latencySummary.p50 <= 150, 'latency: p50 <= 150 ms, every measured backdrop change painted within 250 ms', JSON.stringify(latencySummary));
  const failed = checks.filter((c) => !c.ok);
  writeFileSync(join(out, 'results.json'), JSON.stringify({ display: D, latency: latencySummary, order: 'gallery.png: one row per backdrop (' + BDS.map((b) => b.name).join(', ') + '), columns = positions (' + POSITIONS.map((p) => p.name).join(', ') + ')', checks, results }, null, 1));
  console.log(`\n${checks.length - failed.length}/${checks.length} checks passed; ${tiles.length} captures (${BDS.length} backdrops x ${POSITIONS.length} positions) -> ${out}`);
  await cleanup();
  process.exit(failed.length ? 1 : 0);
} catch (e) {
  console.error(e);
  await cleanup();
  process.exit(2);
}
