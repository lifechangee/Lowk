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
  const f2 = issues.filter((i) => i.rule === 'flex-gap' && /button\.btn > span\.lbl/.test(i.sel));
  add({ name: 'F2 header button spacing (real WKWebView)', fid: 'F2', ok: f2.length === 0, detail: [...new Set(f2.map((i) => i.msg))].join('; ') || 'dot/label/kbd spaced', shots: mainShots });
  lintCheck('Main home: layout lint in the real webview', issues.filter((i) => !f2.includes(i)), 'home', mainShots);

  for (const page of ['general', 'hotkeys', 'appearance', 'providers', 'storage', 'permissions']) {
    await cmd(`nav #/settings/${page}`);
    await sleep(500);
    const s = await shot('main', `main-settings-${page}.png`);
    const is = (await lint({ root: 'body' })).filter((i) => !(i.rule === 'flex-gap' && /button\.btn > span\.lbl/.test(i.sel)));
    lintCheck(`Main settings/${page}: layout lint (real webview)`, is, `settings-${page}`, s.ok ? [s.name] : []);
  }

  await cmd('nav #/onboarding/0');
  await sleep(700);
  const o = await shot('main', 'main-onboarding-0.png');
  const rect = await ev('main', `(()=>{const r=document.querySelector('.win').getBoundingClientRect();return {x:r.x,y:r.y,w:r.width,h:r.height,vw:innerWidth,vh:innerHeight}})()`);
  add({ name: 'F1 onboarding fills the window (real webview)', fid: 'F1', ok: rect.x === 0 && rect.y === 0 && rect.w === rect.vw && rect.h === rect.vh, detail: `card ${rect.w}x${rect.h} at ${rect.x},${rect.y} in ${rect.vw}x${rect.vh}`, shots: o.ok ? [o.name] : [] });
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
  add({ name: 'F10 overlay streaming answer: no duplicated words (real DOM)', fid: 'F10', ok: snaps.length > 0 && dups.length === 0, detail: snaps.length === 0 ? 'no answer text appeared in 45 s' : dups.length ? `duplicated: ${dups.slice(0, 6).join(', ')} | «${sample.slice(0, 100)}»` : `${snaps.length} snapshots clean`, shots: [] });

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
  const mt0 = await ev('main', `parseFloat(getComputedStyle(document.querySelector('.content h1, .content h2, .content')).fontSize)`);
  await cmd('font 18');
  await sleep(700);
  const m1f = await ev('main', `parseFloat(getComputedStyle(document.body).fontSize)`);
  const mt1 = await ev('main', `parseFloat(getComputedStyle(document.querySelector('.content h1, .content h2, .content')).fontSize)`);
  await cmd('font 14');
  add({ name: 'F6 main window text size follows ui.font_px (live)', fid: 'F6', ok: m1f > m0f || mt1 > mt0, detail: `body ${m0f}px -> ${m1f}px, content ${mt0}px -> ${mt1}px after font_px 14 -> 18` });

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
    for (let i = 0; i < 4; i++) { await cmd('mode interactive'); await sleep(450); await cmd('mode passive'); await sleep(450); }
  })();
  const sj = join(out, 'f4-sample.json');
  const sp = spawn(cap, ['sample', pid, 'overlay', '5', sj], { stdio: ['ignore', 'pipe', 'ignore'] });
  let so = '';
  sp.stdout.on('data', (d) => (so += d));
  await Promise.all([flips, new Promise((r) => sp.on('close', r))]);
  let s = {};
  try { s = JSON.parse(so); } catch {}
  const f4ok = s.ok === true && s.presenceHz >= 30;
  add({ name: 'F4 overlay never disappears during Interactive/Passive switches (>=30 Hz sampler)', fid: 'F4', ok: f4ok, detail: s.presenceHz === undefined ? `sampler failed: ${s.error ?? so}` : `${Math.round(s.presenceHz)} Hz presence, ${s.absentSamples} absent samples${s.absentRuns?.length ? ' (' + s.absentRuns.map((r) => `${(r.from).toFixed(2)}-${r.to.toFixed(2)}s`).join(', ') + ')' : ''}; stream ${s.frames} frames, ${s.blankFrames} blank`, shots: existsSync(join(out, 'f4-sample-last.png')) ? ['f4-sample-last.png'] : [] });
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
