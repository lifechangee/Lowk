#!/usr/bin/env node
// Real-app QA (macOS). Builds + launches the `qa` build of Lowk, drives it over the local control
// channel and checks the REAL WKWebView windows:
//   - layout lint inside the real webview (same rules as the browser suite)
//   - F1/F2/F6/F10/F4/F13 detectors (expected-fail until the fix lands)
//   - ScreenCaptureKit captures of main + overlay (exclusion lifted only for the capture)
//   - >=30 Hz overlay presence sampler across overlay mode switches
// Output: docs/qa/real/results.json + PNGs (consumed by tools/qa/report.mjs).
//   node tools/qa/app-qa.mjs [--no-build]
import { spawn, spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync, mkdtempSync } from 'node:fs';
import { inflateSync } from 'node:zlib';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { lintInPage, dupWords } from '../../ui/e2e/lib/lint.ts';
import { classify } from '../../ui/e2e/known.ts';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..', '..');
const out = join(root, 'docs', 'qa', 'real');
const targetDir = process.env.CARGO_TARGET_DIR ?? join(root, 'target', 'qa');
const bin = join(targetDir, 'debug', 'lowk');
const cap = join(tmpdir(), 'lowk-qa-cap');
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const checks = [];
const add = (c) => { checks.push(c); console.log(`${c.ok ? 'PASS' : c.fid ? 'CAUGHT' : 'FAIL'}  ${c.name}${c.fid ? ` [${c.fid}]` : ''}  ${c.detail ?? ''}`); };

rmSync(out, { recursive: true, force: true });
mkdirSync(out, { recursive: true });
const env = { ...process.env, PATH: `${process.env.HOME}/.cargo/bin:${process.env.HOME}/.local/node/current/bin:${process.env.HOME}/.local/bin:${process.env.PATH}` };

if (!process.argv.includes('--no-build')) {
  for (const [cmd, args, cwd] of [
    ['pnpm', ['--dir', 'ui', 'build'], root],
    ['cargo', ['build', '-q', '-p', 'lowk-app', '--features', 'qa,custom-protocol'], root],
  ]) {
    const r = spawnSync(cmd, args, { cwd, env: { ...env, CARGO_TARGET_DIR: targetDir }, stdio: 'inherit' });
    if (r.status !== 0) { console.error('build failed:', cmd, args.join(' ')); process.exit(2); }
  }
}
if (!existsSync(bin)) { console.error('missing binary', bin); process.exit(2); }
const sw = spawnSync('swiftc', ['-O', '-o', cap, join(root, 'tools/qa/qa-cap.swift')], { stdio: ['ignore', 'ignore', 'ignore'] });
if (sw.status !== 0) { console.error('swiftc failed'); process.exit(2); }

// ---- launch ----
const work = mkdtempSync(join(tmpdir(), 'lowk-qa-'));
const data = join(work, 'data');
mkdirSync(data, { recursive: true });
writeFileSync(join(data, 'settings.toml'), `[onboarding]\ndone = true\nrecording_notice_ack = true\n[vault]\npath = "${work}/vault"\n`);
const port = 17000 + Math.floor(Math.random() * 2000);
const token = [...crypto.getRandomValues(new Uint8Array(16))].map((b) => b.toString(16).padStart(2, '0')).join('');
const app = spawn(bin, [], { env: { ...env, LOWK_QA_MULTI: "1", LOWK_DATA_DIR: data, LOWK_QA_PORT: String(port), LOWK_QA_TOKEN: token }, stdio: ['ignore', 'ignore', 'ignore'] });
let quitting = false;
const cleanup = async () => {
  if (quitting) return;
  quitting = true;
  try { await cmd('exclusion on'); } catch {}
  try { await cmd('quit'); } catch {}
  await sleep(500);
  try { app.kill(); } catch {}
};
process.on('SIGINT', async () => { await cleanup(); process.exit(130); });

async function cmd(line) {
  const r = await fetch(`http://127.0.0.1:${port}/cmd`, { method: 'POST', headers: { 'X-Lowk-Qa-Token': token }, body: line });
  return r.text();
}
async function ev(win, expr) {
  const t = await cmd(`eval ${win} ${expr}`);
  try { const j = JSON.parse(t); if (!j.ok) throw new Error(j.v); return j.v; } catch (e) { throw new Error(`eval failed: ${e.message} :: ${t.slice(0, 200)}`); }
}
/** Minimal PNG reader (8-bit RGB/RGBA, non-interlaced): enough to count sentinel-coloured pixels. */
function pngPixels(buf) {
  let o = 8, w = 0, h = 0, ct = 6; const idat = [];
  while (o < buf.length) {
    const len = buf.readUInt32BE(o), type = buf.toString('ascii', o + 4, o + 8), d = buf.subarray(o + 8, o + 8 + len);
    if (type === 'IHDR') { w = d.readUInt32BE(0); h = d.readUInt32BE(4); ct = d[9]; }
    if (type === 'IDAT') idat.push(d);
    o += 12 + len;
  }
  const bpp = ct === 6 ? 4 : 3, raw = inflateSync(Buffer.concat(idat)), stride = w * bpp, out = Buffer.alloc(h * stride);
  for (let y = 0; y < h; y++) {
    const f = raw[y * (stride + 1)], src = raw.subarray(y * (stride + 1) + 1, (y + 1) * (stride + 1)), row = out.subarray(y * stride, (y + 1) * stride), prev = y ? out.subarray((y - 1) * stride, y * stride) : null;
    for (let x = 0; x < stride; x++) {
      const a = x >= bpp ? row[x - bpp] : 0, b = prev ? prev[x] : 0, c = prev && x >= bpp ? prev[x - bpp] : 0;
      const p = a + b - c, pa = Math.abs(p - a), pb = Math.abs(p - b), pc = Math.abs(p - c);
      row[x] = (src[x] + (f === 0 ? 0 : f === 1 ? a : f === 2 ? b : f === 3 ? (a + b) >> 1 : pa <= pb && pa <= pc ? a : pb <= pc ? b : c)) & 255;
    }
  }
  return { w, h, bpp, px: out };
}
const countPx = (img, pred) => { let n = 0; for (let i = 0; i < img.px.length; i += img.bpp) if (pred(img.px[i], img.px[i + 1], img.px[i + 2])) n++; return n; };
const isGreen = (r, g, b) => g > 200 && g - r > 60 && g - b > 60;
const isMagenta = (r, g, b) => r > 170 && b > 170 && g < 110 && r > g + 90;
const lintKey = (state) => (i) => classify(i.rule, i.sel, `main/${state}`);
const lintCheck = (name, issues, state, shots) => {
  const bad = issues.filter((i) => i.rule !== 'grid' && !lintKey(state)(i));
  const known = [...new Set(issues.filter((i) => i.rule !== 'grid' && lintKey(state)(i)).map(lintKey(state)))];
  add({ name, ok: bad.length === 0, detail: (bad.map((i) => `${i.rule}: ${i.sel} ${i.msg}`).join(' | ') || 'clean').slice(0, 500) + (known.length ? ` (known: ${known.join(', ')})` : ''), shots });
};
const lint = (opts) => ev('main', `(${lintInPage.toString()})(${JSON.stringify(opts)})`);
async function shot(win, name) {
  const r = spawnSync(cap, ['shot', await cmd('pid'), win, join(out, name)], { encoding: 'utf8' });
  let j = {}; try { j = JSON.parse(r.stdout); } catch {}
  return { ok: r.status === 0, name, err: j.error ?? r.stderr };
}

try {
  for (let i = 0; i < 100 && !(existsSync(join(data, 'qa.json'))); i++) await sleep(100);
  for (let i = 0; i < 100; i++) { try { await cmd('pid'); break; } catch { await sleep(100); } }
  const pid = await cmd('pid');
  await sleep(2500);

  // ---- main window, real WKWebView ----
  await cmd('nav #/');
  await sleep(1200);
  const mainShots = [];
  const m0 = await shot('main', 'main-home.png');
  if (m0.ok) mainShots.push('main-home.png');
  const issues = await lint({ root: 'body' });
  const hdrBtns = await ev('main', `document.querySelectorAll('.tb button.btn').length`);
  const heroBtns = await ev('main', `document.querySelectorAll('.nc button.primary').length`);
  add({ name: 'G5 library home has no header «New call» button; the card keeps the single Start button (real WKWebView)', ok: hdrBtns === 0 && heroBtns === 1, detail: `header buttons ${hdrBtns}, card start buttons ${heroBtns}`, shots: mainShots });
  lintCheck('Main home: layout lint in the real webview', issues, 'home', mainShots);

  for (const page of ['general', 'hotkeys', 'appearance', 'providers', 'storage', 'permissions']) {
    await cmd(`nav #/settings/${page}`);
    await sleep(500);
    const s = await shot('main', `main-settings-${page}.png`);
    const is = (await lint({ root: 'body' })).filter((i) => !(i.rule === 'flex-gap' && /button\.btn > span\.lbl/.test(i.sel)));
    lintCheck(`Main settings/${page}: layout lint (real webview)`, is, `settings-${page}`, s.ok ? [s.name] : []);
  }

  // F2 + G5: the header button and its popover on a non-home screen
  await cmd('nav #/settings/general');
  await sleep(600);
  const hl = (await lint({ root: '.tb' })).filter((i) => i.rule === 'flex-gap' && /button\.btn > span\.lbl/.test(i.sel));
  add({ name: 'F2 header button spacing (real WKWebView)', ok: hl.length === 0, detail: [...new Set(hl.map((i) => i.msg))].join('; ') || 'dot/label/kbd spaced' });
  const opened = await ev('main', `(()=>{const b=document.querySelector('.tb button.btn.primary');if(!b)return 'no header button';b.click();return 'clicked'})()`);
  await sleep(500);
  const np = await ev('main', `(()=>{const p=document.querySelector('.np');return p?{selects:p.querySelectorAll('select').length,picker:p.querySelectorAll('[data-source-value]').length,hash:location.hash}:null})()`);
  const npShot = await shot('main', 'main-newcall-popover.png');
  add({ name: 'G5 header «New call» opens a compact popover (folder + source + Start) instead of jumping home', ok: opened === 'clicked' && !!np && np.selects === 1 && np.picker === 1 && np.hash === '#/settings/general', detail: JSON.stringify(np), shots: npShot.ok ? [npShot.name] : [] });
  // G3 in the real webview: presets listed, no free-text identifier field
  await ev('main', `document.querySelector('.np .trig').click()`);
  await sleep(500);
  const pk = await ev('main', `(()=>{const l=document.querySelector('[role=listbox]');return l?{opts:[...l.querySelectorAll('[role=option]')].map(o=>o.innerText.trim()),inputs:l.querySelectorAll('input').length}:null})()`);
  const pkShot = await shot('main', 'main-source-picker.png');
  const want = ['Zoom', 'Google Meet', 'Teams', 'Discord', 'Telegram', 'Slack'];
  const have = (pk?.opts ?? []).join('|');
  add({ name: 'G3 source picker (real app): presets + «Other…» + whole system, no identifier input', ok: !!pk && pk.inputs === 0 && want.every((n) => have.includes(n)) && /Other|Другое/.test(have), detail: have.slice(0, 220), shots: pkShot.ok ? [pkShot.name] : [] });
  const pkLint = await lint({ root: 'body' });
  lintCheck('Main with the new-call popover and source picker open: layout lint (real webview)', pkLint.filter((i) => !(i.rule === 'flex-gap' && /button\.btn > span\.lbl/.test(i.sel))), 'newcall-popover', pkShot.ok ? [pkShot.name] : []);
  await ev('main', `document.dispatchEvent(new KeyboardEvent('keydown',{key:'Escape'}))||window.dispatchEvent(new KeyboardEvent('keydown',{key:'Escape'}))`);
  await cmd('nav #/');
  await sleep(300);

  await cmd('nav #/onboarding/0');
  await sleep(700);
  const o = await shot('main', 'main-onboarding-0.png');
  const rect = await ev('main', `(()=>{const r=document.querySelector('.win').getBoundingClientRect();return {x:r.x,y:r.y,w:r.width,h:r.height,vw:innerWidth,vh:innerHeight}})()`);
  add({ name: 'F1 onboarding fills the window (real webview)', ok: rect.x === 0 && rect.y === 0 && rect.w === rect.vw && rect.h === rect.vh, detail: `card ${rect.w}x${rect.h} at ${rect.x},${rect.y} in ${rect.vw}x${rect.vh}`, shots: o.ok ? [o.name] : [] });
  await cmd('nav #/');
  await sleep(500);

  // ---- overlay ----
  await cmd('exclusion off'); // QA only: SCK cannot see an NSWindowSharingNone window (see qa-cap.swift)
  await cmd('overlay-show');
  await sleep(800);
  await cmd('demo-start');
  const snaps = [];
  const t0 = Date.now();
  let lastFont = null;
  while (Date.now() - t0 < 45_000) {
    const t = await ev('overlay', `(document.querySelector('.ans .body')||{}).innerText||''`).catch(() => '');
    if (t) snaps.push(t);
    if (snaps.length > 8 && /\S{12}/.test(t) && Date.now() - t0 > 22_000) break;
    await sleep(150);
  }
  const dups = [...new Set(snaps.flatMap((s) => dupWords(s)))];
  const sample = snaps.at(-1) ?? '';
  add({ name: 'F10 overlay streaming answer: no duplicated words (real DOM)', ok: snaps.length > 0 && dups.length === 0, detail: snaps.length === 0 ? 'no answer text appeared in 45 s' : dups.length ? `duplicated: ${dups.slice(0, 6).join(', ')} | «${sample.slice(0, 100)}»` : `${snaps.length} snapshots clean`, shots: [] });

  const ovLint = await ev('overlay', `(${lintInPage.toString()})(${JSON.stringify({ root: 'body' })})`);
  const ovBad = ovLint.filter((i) => ['overflow', 'outside', 'text-overlap', 'box-overlap'].includes(i.rule));
  const ov = await shot('overlay', 'overlay-passive.png');
  add({ name: 'Overlay passive: no clipping / overlap in the real 600x384 window', ok: ovBad.length === 0, detail: ovBad.map((i) => `${i.rule}: ${i.sel} ${i.msg}`).join(' | ').slice(0, 500) || 'clean', shots: ov.ok ? [ov.name] : [] });
  add({ name: 'Overlay capture via per-window ScreenCaptureKit (exclusion lifted for QA)', ok: ov.ok, detail: ov.ok ? 'captured' : ov.err, shots: ov.ok ? [ov.name] : [] });

  // F6: text size
  const f0 = await ev('overlay', `parseFloat(getComputedStyle(document.querySelector('.ov')).fontSize)`);
  await cmd('font 18');
  await sleep(600);
  const f1 = await ev('overlay', `parseFloat(getComputedStyle(document.querySelector('.ov')).fontSize)`);
  await cmd('font 14');
  add({ name: 'Overlay text size follows overlay.font_px (live)', ok: f1 > f0, detail: `font ${f0}px -> ${f1}px after font_px 14 -> 18` });

  const m0f = await ev('main', `parseFloat(getComputedStyle(document.body).fontSize)`);
  const mt0 = await ev('main', `document.querySelector('h1').getBoundingClientRect().height`);
  await cmd('font 18');
  await sleep(700);
  const m1f = await ev('main', `parseFloat(getComputedStyle(document.body).fontSize)`);
  const mt1 = await ev('main', `document.querySelector('h1').getBoundingClientRect().height`);
  await cmd('font 14');
  add({ name: 'F6 main window text size follows ui.font_px (live)', ok: m1f > m0f || mt1 > mt0 * 1.15, detail: `body ${m0f}px -> ${m1f}px, content ${mt0}px -> ${mt1}px after font_px 14 -> 18` });

  // opacity states
  const opShots = [];
  for (const t of [0, 60, 90]) {
    await cmd(`opacity ${t}`);
    await sleep(500);
    const s = await shot('overlay', `overlay-opacity-${t}.png`);
    if (s.ok) opShots.push(s.name);
  }
  await cmd('opacity 60');
  add({ name: 'Overlay captures at opacity 0 / 60 / 90', ok: opShots.length === 3, detail: `${opShots.length}/3 captured`, shots: opShots });

  // F4: presence sampler while the mode flips
  const flips = (async () => {
    await sleep(700);
    for (let i = 0; i < 20; i++) { await cmd('mode interactive'); await sleep(200); await cmd('mode passive'); await sleep(200); }
  })();
  const sj = join(out, 'f4-sample.json');
  const sp = spawn(cap, ['sample', pid, 'overlay', '11', sj], { stdio: ['ignore', 'pipe', 'ignore'] });
  let so = '';
  sp.stdout.on('data', (d) => (so += d));
  await Promise.all([flips, new Promise((r) => sp.on('close', r))]);
  let s = {};
  try { s = JSON.parse(so); } catch {}
  const f4ok = s.ok === true && s.presenceHz >= 30 && s.absentSamples === 0;
  add({ name: 'F4 overlay never disappears during 20 Interactive/Passive switches (>=30 Hz sampler, 0 absent)', ok: f4ok, detail: s.presenceHz === undefined ? `sampler failed: ${s.error ?? so}` : `${Math.round(s.presenceHz)} Hz presence, ${s.absentSamples} absent samples${s.absentRuns?.length ? ' (' + s.absentRuns.map((r) => `${(r.from).toFixed(2)}-${r.to.toFixed(2)}s`).join(', ') + ')' : ''}; stream ${s.frames} frames, ${s.blankFrames} blank`, shots: existsSync(join(out, 'f4-sample-last.png')) ? ['f4-sample-last.png'] : [] });

  // ---- Round 2 (G1 G2 G6 G7), real app ----
  const hs = async () => JSON.parse(await cmd('hover-state'));
  const pillState = async () => JSON.parse(await cmd('pill-state'));
  const sess = () => ev('main', `window.__TAURI_INTERNALS__.invoke('session_state')`);

  // G1: the REAL settings control (click in the main window) -> settings_patch -> settings://changed -> overlay
  await cmd('overlay-show');
  await cmd('nav #/settings/appearance');
  await sleep(800);
  const pick = (i, id) => ev('main', `(()=>{const g=document.querySelectorAll('[role=radiogroup]')[${i}];const b=g&&g.querySelector('[data-size="${id}"]');if(!b)return false;b.click();return true})()`);
  const ovFs = () => ev('overlay', `parseFloat(getComputedStyle(document.querySelector('.ov')).fontSize)`);
  const segW = () => ev('main', `document.querySelectorAll('[role=radiogroup]')[0].getBoundingClientRect().width`);
  const w0 = await segW();
  const sizes = {};
  for (const id of ['s', 'xl', 'm']) { await pick(1, id); await sleep(800); sizes[id] = await ovFs(); }
  const mainZoom = [];
  for (const id of ['xl', 's', 'm']) { await pick(0, id); await sleep(600); mainZoom.push([await ev('main', `Number(document.getElementById('app').style.zoom)`), await segW()]); }
  const toml = readFileSync(join(data, 'settings.toml'), 'utf8');
  add({ name: 'G1 overlay text size S/M/XL clicked in the real settings page reaches the overlay (settings_patch -> event -> overlay)', ok: sizes.s === 12 && sizes.xl === 18 && sizes.m === 14, detail: `overlay font S ${sizes.s}px, XL ${sizes.xl}px, M ${sizes.m}px` });
  add({ name: 'G1 main window text size presets zoom the window and the control never moves/resizes under the pointer', ok: Math.abs(mainZoom[0][0] - 18 / 14) < 0.01 && Math.abs(mainZoom[1][0] - 13 / 14) < 0.01 && mainZoom.every(([, w]) => Math.abs(w - w0) < 1) && /font_px = 14/.test(toml), detail: `zoom XL ${mainZoom[0][0].toFixed(3)} S ${mainZoom[1][0].toFixed(3)}, control width ${w0}->${mainZoom.map(([, w]) => w).join(',')}` });
  const g1shot = await shot('main', 'main-settings-size-presets.png');

  // G6: hold Ctrl through the real dispatcher: overlay takes mouse events, never hides, never key
  await cmd('overlay-show');
  await sleep(400);
  const h0 = await hs();
  const stateLog = [];
  const holds = (async () => {
    await sleep(600);
    for (let i = 0; i < 20; i++) {
      await cmd('bind click_hold down'); await sleep(280);
      stateLog.push(await hs());
      await cmd('bind click_hold up'); await sleep(60);
      stateLog.push(await hs());
      await sleep(80);
    }
  })();
  const sj6 = join(out, 'g6-sample.json');
  const sp6 = spawn(cap, ['sample', pid, 'overlay', '11', sj6], { stdio: ['ignore', 'pipe', 'ignore'] });
  let so6 = '';
  sp6.stdout.on('data', (d) => (so6 += d));
  await Promise.all([holds, new Promise((r) => sp6.on('close', r))]);
  let s6 = {}; try { s6 = JSON.parse(so6); } catch {}
  const downs = stateLog.filter((_, i) => i % 2 === 0), ups = stateLog.filter((_, i) => i % 2 === 1);
  add({ name: 'G6 hold-Ctrl: 20x the overlay turns clickable (ignoresMouseEvents=false) and back, 0 absent frames', ok: h0.mode === 'passive' && h0.ignores_mouse === true && downs.every((x) => x.mode === 'hover' && x.ignores_mouse === false) && ups.every((x) => x.mode === 'passive' && x.ignores_mouse === true) && s6.ok === true && s6.absentSamples === 0 && s6.presenceHz >= 30, detail: `start ${JSON.stringify(h0)}; hover ${downs.filter((x) => x.mode === 'hover' && x.ignores_mouse === false).length}/20, release ${ups.filter((x) => x.mode === 'passive' && x.ignores_mouse === true).length}/20; sampler ${s6.presenceHz === undefined ? 'failed ' + (s6.error ?? so6) : Math.round(s6.presenceHz) + ' Hz, ' + s6.absentSamples + ' absent'}` });
  // a quick tap (< debounce) is not a hold; move wins over hover; both released = click-through
  await cmd('bind click_hold down'); await sleep(30); await cmd('bind click_hold up'); await sleep(300);
  const tap = await hs();
  await cmd('bind click_hold down'); await sleep(280);
  await cmd('bind move_hold down'); await sleep(200);
  const mv = await hs();
  await cmd('bind move_hold up'); await sleep(200);
  const back = await hs();
  await cmd('bind click_hold up'); await sleep(150);
  const fin = await hs();
  add({ name: 'G6 a quick Ctrl tap is not a hold; move (hold-Alt) wins over hold-Ctrl and Ctrl resumes after', ok: tap.mode === 'passive' && mv.mode === 'moving' && back.mode === 'hover' && fin.mode === 'passive' && fin.ignores_mouse === true, detail: `tap ${tap.mode}, both ${mv.mode}, after move ${back.mode}, released ${fin.mode}/${fin.ignores_mouse}` });
  // template binds: Ask-with-complement works with no mode switch while recording
  await cmd('demo-start');
  await sleep(1500);
  await cmd('bind ask_complement down');
  let ans = '';
  for (let i = 0; i < 40 && !ans; i++) { await sleep(250); ans = await ev('overlay', `(document.querySelector('.ans .body')||{}).innerText||''`).catch(() => ''); if (/Press|Нажмите/.test(ans)) ans = ''; }
  add({ name: 'G6 ask bind per template (⌘⇧↵ = «Дополни мой ответ») answers without entering the interactive mode', ok: ans.length > 5 && (await hs()).mode === 'passive', detail: `«${ans.slice(0, 60)}»` });
  await cmd('demo-stop');
  await sleep(1500);

  // G7: screenshot queue: all monitors, our own windows excluded
  await cmd('nav #/');
  await cmd('main-show');
  await sleep(800);
  await cmd('exclusion off'); // so a plain capturer COULD see the overlay: the queue still must not
  await cmd('overlay-show');
  await sleep(500);
  await ev('main', `(()=>{const s=document.createElement('div');s.style.cssText='position:fixed;inset:0;z-index:99999;background:#00ff00';document.body.appendChild(s);return true})()`);
  await ev('overlay', `(()=>{const s=document.createElement('div');s.style.cssText='position:fixed;inset:0;z-index:99999;background:#ff00ff';document.body.appendChild(s);return true})()`);
  await sleep(700);
  // control: per-window ScreenCaptureKit captures (the QA run lifted the overlay exclusion) must SEE the sentinels
  const cm = await shot('main', 'g7-control-main.png');
  const co = await shot('overlay', 'g7-control-overlay.png');
  const ctlGreen = cm.ok ? countPx(pngPixels(readFileSync(join(out, cm.name))), isGreen) : 0;
  const ctlMag = co.ok ? countPx(pngPixels(readFileSync(join(out, co.name))), isMagenta) : 0;
  const q = await cmd('screenshot');
  let qs = []; try { qs = JSON.parse(q); } catch {}
  let qGreen = 0, qMag = 0, qPx = 0;
  for (let i = 0; i < qs.length; i++) { const img = pngPixels(Buffer.from(await cmd(`queue-png ${i}`), 'base64')); qGreen += countPx(img, isGreen); qMag += countPx(img, isMagenta); qPx += img.w * img.h; }
  await sleep(500);
  const chip = await ev('overlay', `(()=>{const c=document.querySelector('.head .chip');return c?{n:c.querySelector('.lbl').innerText,thumbs:c.querySelectorAll('img.thumb').length}:null})()`);
  await ev('overlay', `document.querySelector('.head .chip .clr')?.click()`);
  await sleep(500);
  const cleared = await ev('overlay', `document.querySelectorAll('.head .chip').length`);
  const qAfter = await cmd('screenshot'); // a second grab proves the queue appends, then clear again
  let qs2 = []; try { qs2 = JSON.parse(qAfter); } catch {}
  await cmd('queue-clear');
  add({ name: 'G7 capture_screenshot queues one PNG per monitor (queue://changed -> overlay chip with count + thumbnails, clear empties it)', ok: qs.length >= 1 && qs.every(([w, h, n]) => w > 200 && h > 200 && n > 5000) && chip?.thumbs === Math.min(qs.length, 3) && chip?.n === `×${qs.length}` && cleared === 0 && qs2.length === qs.length, detail: `${qs.length} monitor(s) ${JSON.stringify(qs)}; overlay chip ${JSON.stringify(chip)}; after clear chips=${cleared}` });
  add({ name: 'G7 our own windows are EXCLUDED from screenshots (main=green and overlay=magenta sentinels absent; a per-window capture sees them)', ok: ctlGreen > 20000 && ctlMag > 5000 && qGreen < 5000 && qMag < 5000, detail: `control (per-window capture): ${ctlGreen} green / ${ctlMag} magenta px; queue (${qs.length} shot(s), ${qPx} px): ${qGreen} green / ${qMag} magenta` });
  await cmd('nav #/');
  await cmd('exclusion on');

  // G2: the pill is draggable (pointer drag, through the real overlay_* IPC), the spot is remembered per display
  await cmd('overlay-hide');
  await cmd(`settings-patch {"overlay":{"visibility":"pill"}}`);
  await sleep(1200);
  const p0 = await pillState();
  await ev('pill', `(async()=>{const b=document.querySelector('.pill');const mk=(t,x,y)=>b.dispatchEvent(new PointerEvent(t,{bubbles:true,pointerId:1,button:0,screenX:x,screenY:y,clientX:5,clientY:5}));const w=(ms)=>new Promise(r=>setTimeout(r,ms));mk('pointerdown',800,500);await w(60);mk('pointermove',790,505);await w(60);mk('pointermove',500,560);await w(120);mk('pointerup',500,560);await w(200);return true})()`);
  await sleep(500);
  const p1 = await pillState();
  const phase = (await sess()).phase;
  await cmd(`settings-patch {"overlay":{"visibility":"recording_only"}}`);
  await sleep(1000);
  const p2 = await pillState();
  await cmd(`settings-patch {"overlay":{"visibility":"pill"}}`);
  await sleep(1200);
  const p3 = await pillState();
  const dx = p1.frame.x - p0.frame.x, dy = p1.frame.y - p0.frame.y;
  add({ name: 'G2 pill drag moves the pill (no recording started by the drag) and the position is saved per display', ok: p0.visible && Math.abs(dx + 300) < 3 && Math.abs(dy - 60) < 3 && phase === 'idle' && Object.keys(p1.positions).length === 1, detail: `moved (${dx}, ${dy}) from ${p0.frame.x},${p0.frame.y}; phase ${phase}; saved ${JSON.stringify(p1.positions)}` });
  add({ name: 'G2 pill comes back at the remembered spot after hide/show', ok: !p2.visible && p3.visible && Math.abs(p3.frame.x - p1.frame.x) < 2 && Math.abs(p3.frame.y - p1.frame.y) < 2, detail: `hidden ${!p2.visible}, back at ${p3.frame.x},${p3.frame.y} (was ${p1.frame.x},${p1.frame.y})` });
  await cmd(`settings-patch {"overlay":{"visibility":"recording_only"}}`);
  await sleep(600);

  // F8: stop button in the overlay header -> recording ends -> main window on library home
  await cmd('demo-start');
  await sleep(1500);
  await cmd('nav #/settings/general');
  await sleep(500);
  await cmd('main-hide');
  await sleep(300);
  const clicked = await ev('overlay', `(()=>{const b=document.querySelector('button.stop');if(!b)return false;b.click();return true})()`);
  await sleep(2000);
  const w = JSON.parse(await cmd('win main'));
  const hash = await ev('main', 'location.hash').catch(() => '?');
  add({ name: 'F8 overlay stop button ends the recording and opens main on library home (real app)', ok: clicked === true && w.exists && w.visible && (hash === '#/' || hash === ''), detail: `clicked=${clicked} main visible=${w.visible} hash=${hash}` });
} catch (e) {
  add({ name: 'real-app QA run', ok: false, detail: String(e.message ?? e) });
} finally {
  await cleanup();
}

writeFileSync(join(out, 'results.json'), JSON.stringify({ at: new Date().toISOString(), checks }, null, 2));
const unexpected = checks.filter((c) => (c.fid ? c.ok : !c.ok));
console.log(`\nreal-app: ${checks.length} checks, ${checks.filter((c) => !c.ok).length} failing, ${checks.filter((c) => !c.ok && c.fid).length} known F-bugs caught`);
if (unexpected.length) console.log('UNEXPECTED:', unexpected.map((c) => c.name).join(' | '));
process.exit(unexpected.length ? 1 : 0);
