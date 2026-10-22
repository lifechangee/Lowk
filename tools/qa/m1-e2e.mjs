#!/usr/bin/env node
// M1 end-to-end QA in the REAL app (macOS, `qa` build). NO sound is ever played: the System and Me
// streams are fed from files (LOWK_QA_SYSTEM_FILE / LOWK_QA_ME_FILE, qa builds only).
//
//   node tools/qa/m1-e2e.mjs [--no-build] [--stage live|noengine|install|loading|bundle|warmup]
//   Audible, opt-in (real capture, real speakers, display must be awake): --stage audible-app,audible-system
//
// live      : record from files with the owner's local model; live subtitles in the overlay, finals in the
//             call .md, background correction, Ask on a partial, "Мой ответ" closes only on the next Ask,
//             stop -> main window home.
// noengine  : HOME points at an empty dir (no model, no worker): the record bind must say why it plays the demo.
// install   : same empty HOME, drive the real download card: progress, cancel, resume, corrupt file.
// loading   : cold start: the worker needs LOWK_QA_LOAD_DELAY_MS to load its model. Record shows the
//             «Загружаем модель распознавания…» state in the overlay AND the main window, subtitles start the moment
//             the model is up, and the audio captured meanwhile is transcribed (journal timestamps from second 0).
// bundle    : the built Lowk.app carries the worker (Contents/MacOS, runs); a qa binary laid out like a bundle
//             (worker only next to the executable, no env, dev fallback off) starts a REAL session and gets subtitles.
// Output: docs/qa/m1/results.json + PNG + copies of the call .md.
import { spawn, spawnSync } from 'node:child_process';
import { copyFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, statSync, truncateSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..', '..');
const out = join(root, 'docs', 'qa', 'm1');
const targetDir = process.env.CARGO_TARGET_DIR ?? join(root, 'target', 'qa');
const bin = join(targetDir, 'debug', 'lowk');
const cap = join(tmpdir(), 'lowk-qa-cap');
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const arg = (k) => { const i = process.argv.indexOf(k); return i >= 0 ? process.argv[i + 1] : undefined; };
const stages = (arg('--stage') ?? 'live,noengine,install,loading,bundle,warmup').split(',');
const checks = [];
const add = (c) => { checks.push(c); console.log(`${c.ok ? 'PASS' : 'FAIL'}  ${c.name}  ${c.detail ?? ''}`); };

mkdirSync(out, { recursive: true });
const env0 = { ...process.env, PATH: `${process.env.HOME}/.cargo/bin:${process.env.HOME}/.local/node/current/bin:${process.env.HOME}/.local/bin:${process.env.PATH}` };
if (!process.argv.includes('--no-build')) {
  for (const [c, a] of [['pnpm', ['--dir', 'ui', 'build']], ['cargo', ['build', '-q', '-p', 'lowk-app', '--features', 'qa,custom-protocol']]]) {
    const r = spawnSync(c, a, { cwd: root, env: { ...env0, CARGO_TARGET_DIR: targetDir }, stdio: 'inherit' });
    if (r.status !== 0) { console.error('build failed', c, a.join(' ')); process.exit(2); }
  }
}
if (!existsSync(bin)) { console.error('missing', bin); process.exit(2); }
spawnSync('swiftc', ['-O', '-o', cap, join(root, 'tools/qa/qa-cap.swift')], { stdio: 'ignore' });

const worker = join(root, 'workers/lowk-stt-worker/target/release/lowk-stt-worker');
const audio = join(root, 'spikes/stt-bench/audio');

/** Starts one QA instance. `home` overrides $HOME (app data, models, keychain lookups). */
async function launch({ home, env = {}, settings = '', exe = bin }) {
  const work = mkdtempSync(join(tmpdir(), 'lowk-m1-'));
  const data = join(work, 'data');
  mkdirSync(data, { recursive: true });
  writeFileSync(join(data, 'settings.toml'), `[onboarding]\ndone = true\nrecording_notice_ack = true\n[vault]\npath = "${work}/vault"\n${settings}`);
  const port = 19000 + Math.floor(Math.random() * 900);
  const token = [...crypto.getRandomValues(new Uint8Array(16))].map((b) => b.toString(16).padStart(2, '0')).join('');
  const penv = { ...env0, LOWK_QA_MULTI: '1', LOWK_DATA_DIR: data, LOWK_QA_PORT: String(port), LOWK_QA_TOKEN: token, ...env };
  for (const k of Object.keys(penv)) if (penv[k] === null) delete penv[k]; // null = unset
  if (home) penv.HOME = home;
  const app = spawn(exe, [], { env: penv, stdio: ['ignore', 'ignore', 'ignore'] });
  const cmd = async (line) => (await fetch(`http://127.0.0.1:${port}/cmd`, { method: 'POST', headers: { 'X-Lowk-Qa-Token': token }, body: line })).text();
  const ev = async (win, expr) => {
    const t = await cmd(`eval ${win} ${expr}`);
    const j = JSON.parse(t);
    if (!j.ok) throw new Error(`eval ${win}: ${j.v}`);
    return j.v;
  };
  for (let i = 0; i < 150; i++) { try { await cmd('pid'); break; } catch { await sleep(100); } }
  await sleep(2500);
  const shot = async (win, name) => {
    const r = spawnSync(cap, ['shot', await cmd('pid'), win, join(out, name)], { encoding: 'utf8' });
    return r.status === 0 ? name : null;
  };
  const kill = async () => { try { await cmd('exclusion on'); await cmd('quit'); } catch {} await sleep(600); try { app.kill(); } catch {} };
  return { work, data, vault: join(work, 'vault'), cmd, ev, shot, kill, app };
}

const findMd = (dir) => {
  const hits = [];
  const walk = (d) => { for (const e of readdirSync(d, { withFileTypes: true })) { const p = join(d, e.name); if (e.isDirectory()) walk(p); else if (e.name.endsWith('.md')) hits.push(p); } };
  if (existsSync(dir)) walk(dir);
  return hits.filter((p) => !p.includes('/.lowk/') && !p.endsWith('README.md'));
};
const pct = (a, q) => { const s = [...a].sort((x, y) => x - y); return s.length ? s[Math.min(s.length - 1, Math.ceil(s.length * q) - 1)] : NaN; };

// ---------------------------------------------------------------------------------------------
async function stageLive(o = {}) {
  // o.audible: real capture (no LOWK_QA_*_FILE); o.beforeRecord(a) starts the players; o.prefix names the evidence files
  const P = o.prefix ?? '';
  const sys = join(audio, '03_them_q.m4a'), me = join(audio, '03_me_q.m4a');
  const a = await launch({
    env: o.audible ? { LOWK_STT_WORKER: worker } : { LOWK_QA_SYSTEM_FILE: sys, LOWK_QA_ME_FILE: me, LOWK_STT_WORKER: worker },
    settings: '[stt]\nengine = "nemotron-local"\nlanguages = ["ru", "en"]\nbackground_correction = true\n',
  });
  const log = [];
  try {
    await a.cmd('nav #/');
    await sleep(1500);
    await a.cmd('exclusion off');
    // record every transcript event as the main webview receives it (real IPC path)
    await a.ev('main', `(()=>{window.__ev=[];const T=window.__TAURI_INTERNALS__;const h=T.transformCallback(e=>window.__ev.push({t:performance.now(),p:e.payload}));return T.invoke('plugin:event|listen',{event:'stt://segment',target:{kind:'Any'},handler:h})})()`);
    await a.ev('main', `(()=>{window.__ended=[];const T=window.__TAURI_INTERNALS__;const h=T.transformCallback(e=>window.__ended.push({t:Math.round(performance.now()),p:e.payload}));return T.invoke('plugin:event|listen',{event:'session://ended',target:{kind:'Any'},handler:h})})()`);
    await a.ev('main', `(()=>{window.__llm=[];const T=window.__TAURI_INTERNALS__;for(const n of ['start','delta','done','error']){const h=T.transformCallback(e=>window.__llm.push({t:performance.now(),n,p:e.payload}));T.invoke('plugin:event|listen',{event:'llm://'+n,target:{kind:'Any'},handler:h})}return 1})()`);

    if (o.beforeRecord) await o.beforeRecord(a);
    const t0 = Date.now();
    await a.cmd('bind toggle_record down');
    await a.cmd('bind toggle_record up');
    const st0 = JSON.parse(await a.ev('main', `JSON.stringify(await window.__TAURI_INTERNALS__.invoke('session_state'))`));
    add({ name: P + 'record bind starts a REAL call (not the demo) with the file sources', ok: st0.phase === 'recording' && st0.demo === false, detail: JSON.stringify(st0) });

    const probe = `(()=>{const q=s=>document.querySelector(s);const sub=[...document.querySelectorAll('.sub .txt')].pop();return {sub:sub?sub.innerText:null,caret:!!q('.sub .caret'),my:(q('.my .tx.ph')?'':((q('.my .tx')||{}).innerText||'')),ans:(q('.ans .body')||{}).innerText||'',wait:(q('.sub .wait')||{}).innerText||null}})()`;
    const samples = [];
    let firstSub = null, askAt = null, ask1 = null, ask2 = null, shots = [];
    let phase = 'pre';
    while (Date.now() - t0 < (o.endMs ? o.endMs + 40_000 : 110_000)) {
      const t = Date.now() - t0;
      const s = await a.ev('overlay', probe).catch(() => null);
      if (s) samples.push({ t, ...s });
      if (s?.sub && firstSub === null) { firstSub = t; const n = await a.shot('overlay', P + 'live-subtitle.png'); if (n) shots.push(n); }
      // ask #1 while the System utterance is still being spoken (a partial with the caret)
      if (!ask1 && t > 9000 && s?.caret && /^(Система|System)/i.test(s.sub ?? '')) {
        const before = s.sub;
        const tA = Date.now();
        await a.cmd('bind ask down'); await a.cmd('bind ask up');
        ask1 = { t, tA: tA - t0, shown: before };
        phase = 'ask1';
      }
      // ask #2 about 35 s later, again on a System partial
      if (ask1 && !ask2 && s && s.my.length > 0 && t > ask1.t + 20_000 && (s.caret || t > ask1.t + 45_000)) {
        const myBefore = s.my;
        await a.cmd('bind ask down'); await a.cmd('bind ask up');
        ask2 = { t, myBefore, shown: s.sub };
        await sleep(400);
        const s2 = await a.ev('overlay', probe).catch(() => ({}));
        ask2.myAfter = s2.my;
        const n = await a.shot('overlay', P + 'live-after-ask2.png'); if (n) shots.push(n);
      }
      if (ask1 && !shots.includes(P + 'live-answer.png') && t > ask1.t + 6000) { const n = await a.shot('overlay', P + 'live-answer.png'); if (n) shots.push(n); }
      const done = (await a.ev('main', `JSON.stringify(await window.__TAURI_INTERNALS__.invoke('session_state'))`).then(JSON.parse).catch(() => ({}))).phase;
      if (t > 15000 && (o.noMic || (ask2 && t > ask2.t + 12_000)) && done === 'recording' && samples.length > 50) {
        // keep going until both files have been played (about 60-70 s of audio)
        const evn = await a.ev('main', 'window.__ev.length').catch(() => 1);
        if (t > (o.endMs ?? 75_000) && evn > 0) break;
      }
      await sleep(120);
    }
    // my-answer growth between the two asks
    const between = samples.filter((s) => ask1 && ask2 && s.t > ask1.t + 4000 && s.t < ask2.t).map((s) => s.my);
    const lens = between.map((m) => m.length);
    const monotonic = lens.every((l, i) => i === 0 || l >= lens[i - 1] - 0); // never shrinks (no close) before ask #2
    const grew = lens.length && lens[lens.length - 1] > lens[0];

    const tStop = Date.now();
    await a.cmd('nav #/settings/general');
    await a.ev('overlay', `(()=>{const b=document.querySelector('button.stop');if(!b)return false;b.click();return true})()`).catch(() => false);
    let stopMs = null;
    for (let i = 0; i < 150; i++) {
      const s = await a.ev('main', `JSON.stringify(await window.__TAURI_INTERNALS__.invoke('session_state'))`).then(JSON.parse).catch(() => ({}));
      if (s.phase === 'idle') { stopMs = Date.now() - tStop; break; }
      await sleep(100);
    }
    await sleep(800);
    const w = JSON.parse(await a.cmd('win main'));
    let hash = await a.ev('main', 'location.hash').catch(() => '?');
    let homeAfter = 800;
    for (; hash !== '#/' && hash !== '' && homeAfter < 5000; homeAfter += 250) { await sleep(250); hash = await a.ev('main', 'location.hash').catch(() => '?'); }
    add({ name: P + 'stop (overlay button) ends the call and opens the main window on library home', ok: stopMs !== null && w.visible && (hash === '#/' || hash === ''), detail: `idle after ${stopMs} ms; main visible=${w.visible} hash=${hash} (home reached ${homeAfter} ms after idle); session://ended events seen by the main webview: ${JSON.stringify(await a.ev('main', 'window.__ended').catch(() => null))}` });

    const events = await a.ev('main', 'window.__ev');
    const llm = await a.ev('main', 'window.__llm');
    writeFileSync(join(out, P + 'live-events.json'), JSON.stringify({ events, llm, samples, ask1, ask2, t0, timeOrigin: await a.ev('main', 'performance.timeOrigin').catch(() => null) }, null, 1));

    // ---- subtitles ----
    const subs = samples.filter((s) => s.sub);
    const distinct = new Set(subs.map((s) => s.sub)).size;
    const labels = new Set(subs.map((s) => (s.sub.match(/^(Я|Система|Me|System)/i) ?? [''])[0].toLowerCase()));
    add({ name: P + 'live subtitles appear in the overlay as the file plays (both speakers)', ok: firstSub !== null && distinct > 20 && labels.has('система') && (labels.has('я') || !!o.noMic), detail: `first subtitle after ${firstSub} ms; ${distinct} distinct lines; speakers ${[...labels].join('/')}`, shots });

    // ---- bus -> DOM delay (real WKWebView) ----
    const first = new Map();
    for (const s of samples) if (s.sub && !first.has(s.sub)) first.set(s.sub, s.t);
    const t0m = await a.ev('main', 'performance.now()').catch(() => 0);
    void t0m;
    // ---- journal ----
    const mds = findMd(a.vault);
    const md = mds.length ? readFileSync(mds[0], 'utf8') : '';
    if (mds.length) copyFileSync(mds[0], join(out, P + 'live-call.md'));
    const lines = md.split('\n');
    const tsLines = lines.filter((l) => /^\*{0,2}\[?\d{1,2}:\d{2}/.test(l) || /^- \*\*\d{2}:\d{2}/.test(l) || /\d{2}:\d{2}(:\d{2})?\]?\s*\*{0,2}(Я|Система)/.test(l));
    const meL = lines.filter((l) => /^\[\d\d:\d\d:\d\d\] \*\*(Я|Me):\*\*/.test(l)).length;
    const sysL = lines.filter((l) => /^\[\d\d:\d\d:\d\d\] \*\*(Система|System):\*\*/.test(l)).length;
    add({ name: P + 'finals land in the call .md with timestamps and Я / Система speakers', ok: md.length > 500 && (meL > 3 || !!o.noMic) && sysL > 0, detail: `${mds.length} md; Я lines ${meL}, Система lines ${sysL}; sample: ${tsLines.slice(0, 2).join(' | ').slice(0, 160)}`, shots: [] });

    // ---- correction ----
    const byUtt = new Map();
    for (const { p } of events) { if (p.kind !== 'final') continue; const k = `${p.stream}:${p.utt}`; const arr = byUtt.get(k) ?? []; arr.push(p); byUtt.set(k, arr); }
    const corrected = [...byUtt.values()].filter((v) => v.length > 1);
    let inMd = 0, differs = 0;
    for (const v of corrected) {
      const last = v.sort((x, y) => x.rev - y.rev).at(-1), first1 = v[0];
      if (last.text !== first1.text) {
        differs++;
        if (md.includes(last.text.trim().slice(0, 30))) inMd++;
      }
    }
    add({ name: P + 'background correction revises stored text (the .md carries the corrected version, not the live one)', ok: corrected.length > 3 && differs > 3 && inMd >= Math.floor(differs * 0.7), detail: `${byUtt.size} final utterances, ${corrected.length} re-emitted by Whisper, ${differs} changed, ${inMd} of them stored corrected in the .md` });

    // ---- ask on a partial ----
    const qa = llm.filter((e) => e.n === 'start');
    const qline = lines.filter((l) => /lowk-qa|\[!qa|вопрос|Question/i.test(l)).slice(0, 3);
    const askMd = md;
    const qText = (askMd.match(/\*\*(?:Вопрос|Question):\*\* ([^\n]+)/) ?? [])[1] ?? '';
    const shownTxt = (ask1?.shown ?? '').replace(/^(СИСТЕМА|SYSTEM)/i, '').toLowerCase();
    add({ name: P + 'Ask sent while the System utterance was still a partial: the question is that live text (snapshot_for_ask includes the partial)', ok: !!ask1 && qa.length >= 1 && qText.length > 0 && qText.toLowerCase().startsWith(shownTxt.slice(0, 3)), detail: `ask #1 at ${ask1?.tA} ms with on screen: "${(ask1?.shown ?? '').slice(0, 80)}"; question stored: "${qText.slice(0, 120)}"; qa blocks ${qline.length}` });

    // ---- my answer ----
    add({ name: P + '«Мой ответ» keeps collecting until the NEXT Ask (not closed by time or System speech)', ok: !!o.noMic || !!ask1 && !!ask2 && monotonic && (grew || !!o.audible) && (ask2.myBefore ?? '').length > 0 && (ask2.myAfter ?? '').length < (ask2.myBefore ?? '').length, detail: `between asks: ${lens.length} samples, length ${lens[0]} -> ${lens.at(-1)}, never shrank=${monotonic}; at ask #2 ${ask2?.myBefore?.length} chars -> new link ${ask2?.myAfter?.length} chars` });
    const hasMy = /Мой ответ|My answer|my-answer|lowk-my/i.test(md);
    add({ name: P + 'closed «Мой ответ» is saved in the .md', ok: hasMy || !!o.noMic, detail: o.noMic ? 'SKIPPED (no mic speech: speaker output is muted, the mic hears nothing)' : hasMy ? 'present' : 'missing in md' });

    // ---- the live page in the main window ----
    return { md, events, t0, samples, ask1, ask2, llm, timeOrigin: await a.ev('main', 'performance.timeOrigin').catch(() => null) };
  } finally {
    writeFileSync(join(out, P + 'live-checks.partial.json'), JSON.stringify(log));
    await a.kill();
  }
}

// ---------------------------------------------------------------------------------------------
async function stageNoEngine() {
  const home = mkdtempSync(join(tmpdir(), 'lowk-m1-home-'));
  const a = await launch({ home, env: { LOWK_STT_WORKER: '' } });
  try {
    await a.cmd('nav #/settings/providers');
    await sleep(1200);
    const card = await a.ev('main', `(()=>{const c=document.querySelector('[data-testid=engine-card]');return c?c.innerText:null})()`);
    const n = await a.shot('main', 'noengine-providers.png');
    add({ name: 'no engine installed: Settings > Providers shows a download card that says what happens without it', ok: !!card && /Not installed|Не установлена/.test(card) && /demo|демо/i.test(card), detail: JSON.stringify(card)?.slice(0, 200), shots: n ? [n] : [] });
    await a.cmd('bind toggle_record down'); await a.cmd('bind toggle_record up');
    await sleep(1500);
    const st = JSON.parse(await a.ev('main', `JSON.stringify(await window.__TAURI_INTERNALS__.invoke('session_state'))`));
    add({ name: 'no engine installed: the record bind falls back to the scripted demo', ok: st.demo === true, detail: JSON.stringify(st) });
    await a.cmd('bind toggle_record down'); await a.cmd('bind toggle_record up');
  } finally { await a.kill(); rmSync(home, { recursive: true, force: true }); }
}

async function stageInstall() {
  const home = mkdtempSync(join(tmpdir(), 'lowk-m1-home-'));
  const models = join(home, 'Library/Application Support/Lowk/models/nemotron-local');
  const a = await launch({ home, settings: '[stt]\nengine = "nemotron-local"\nbackground_correction = false\n' });
  const card = () => a.ev('main', `(()=>{const c=document.querySelector('[data-testid=engine-card]');return c?{text:c.innerText,bar:(c.querySelector('[role=progressbar]')||{}).ariaValueNow??null}:null})()`);
  const click = (label) => a.ev('main', `(()=>{const b=[...document.querySelectorAll('[data-testid=engine-card] button')].find(b=>/${label}/.test(b.innerText));if(!b)return false;b.click();return true})()`);
  const gguf = join(models, 'nemotron-3.5-asr-streaming-0.6b-Q8_0.part');
  try {
    await a.cmd('nav #/settings/providers');
    await sleep(1200);
    const c0 = await card();
    add({ name: 'engine install: card offers the download with size and licence', ok: !!c0 && /Download|Скачать/.test(c0.text) && /OpenMDW|MIT|Licen|Лиценз/.test(c0.text), detail: JSON.stringify(c0?.text).slice(0, 220) });
    // start, let it download a bit, cancel
    const started = await click('Download|Скачать');
    let prog = [];
    for (let i = 0; i < 40; i++) { await sleep(500); const c = await card(); if (c?.bar !== null && c) prog.push(Number(c.bar)); if (existsSync(gguf) && statSync(gguf).size > 20_000_000) break; }
    const sz1 = existsSync(gguf) ? statSync(gguf).size : 0;
    const shotP = await a.shot('main', 'install-progress.png');
    await click('Cancel|Отмена');
    await sleep(2500);
    const sz1b = existsSync(gguf) ? statSync(gguf).size : 0;
    const cAfterCancel = await card();
    add({ name: 'engine install: progress is shown and Cancel stops it, keeping the partial file', ok: started === true && sz1 > 0 && sz1b >= sz1 && !!cAfterCancel && /Download|Скачать|Try again|Повторить/.test(cAfterCancel.text) && !/failed|не удалось/i.test(cAfterCancel.text), detail: `part ${sz1} -> ${sz1b} bytes; progress ticks ${prog.length}; card after cancel: ${JSON.stringify(cAfterCancel?.text).slice(0, 120)}`, shots: shotP ? [shotP] : [] });
    // resume
    await click('Download|Скачать|Try again|Повторить');
    await sleep(4000);
    const sz2 = existsSync(gguf) ? statSync(gguf).size : 0;
    await click('Cancel|Отмена');
    await sleep(2500);
    add({ name: 'engine install: a second start resumes the .part file (Range) instead of starting over', ok: sz2 > sz1b, detail: `before ${sz1b}, after resume ${sz2} bytes` });
    // corrupt complete file -> checksum failure with a readable message
    const total = 751_094_240;
    writeFileSync(gguf, '');
    truncateSync(gguf, total);
    await click('Download|Скачать|Try again|Повторить');
    let msg = null;
    for (let i = 0; i < 80; i++) { await sleep(500); const c = await card(); if (c && /integrity|целостност/.test(c.text)) { msg = c.text; break; } }
    const shotC = await a.shot('main', 'install-checksum.png');
    add({ name: 'engine install: a corrupt (full-size) file fails the SHA-256 check with a readable message, the file is removed, retry is offered', ok: !!msg && !existsSync(gguf) && /Try again|Повторить/.test(msg), detail: JSON.stringify(msg)?.slice(0, 200), shots: shotC ? [shotC] : [] });
  } finally { await a.kill(); rmSync(home, { recursive: true, force: true }); }
}

/** Waits for the first overlay subtitle; returns ms since `t0` (null on timeout). Also records the loading text. */
async function watchStart(a, t0, maxMs) {
  const probe = `(()=>{const q=s=>document.querySelector(s);const l=q('[data-testid=stt-loading]');const sub=[...document.querySelectorAll('.sub .txt')].pop();const w=q('.sub .wait');return {loading:l?l.innerText:null,sub:sub?sub.innerText:null,wait:w?w.innerText:null}})()`;
  const seen = { loadingAt: null, loadingText: null, subAt: null, loadingAfterSub: false, mainLoading: null };
  while (Date.now() - t0 < maxMs) {
    const s = await a.ev('overlay', probe).catch(() => null);
    const t = Date.now() - t0;
    seen.last = s;
    if (s?.loading && seen.loadingAt === null) { seen.loadingAt = t; seen.loadingText = s.loading; }
    if (s?.loading && seen.mainLoading === null) {
      seen.mainLoading = await a.ev('main', `(()=>{const l=document.querySelector('[data-testid=stt-loading]');return l?l.innerText:null})()`).catch(() => null);
    }
    if (s?.sub && seen.subAt === null) { seen.subAt = t; }
    if (s?.sub && s?.loading) seen.loadingAfterSub = true;
    if (seen.subAt !== null) break;
    await sleep(100);
  }
  return seen;
}
const fileEnv = () => ({ LOWK_QA_SYSTEM_FILE: join(audio, '03_them_q.m4a'), LOWK_QA_ME_FILE: join(audio, '03_me_q.m4a') });
const nemotronSettings = '[stt]\nengine = "nemotron-local"\nlanguages = ["ru", "en"]\nbackground_correction = false\n';

async function stageLoading() {
  const DELAY = 6000;
  const a = await launch({ env: { ...fileEnv(), LOWK_STT_WORKER: worker, LOWK_QA_LOAD_DELAY_MS: String(DELAY), LOWK_NO_WARMUP: '1' }, settings: nemotronSettings });
  try {
    await a.cmd('nav #/');
    await sleep(1200);
    await a.cmd('exclusion off');
    const t0 = Date.now();
    await a.cmd('bind toggle_record down'); await a.cmd('bind toggle_record up');
    const seen = await watchStart(a, t0, 30_000);
    const main = seen.mainLoading;
    const shot = await a.shot('overlay', 'loading-overlay.png');
    add({ name: 'cold start: Record before the model is ready shows «Загружаем модель распознавания…» in the overlay (not a silent «Слушаю…»)', ok: seen.loadingAt !== null && seen.loadingAt < 4000 && /Загружаем|Loading the speech/.test(seen.loadingText ?? ''), detail: `loading text after ${seen.loadingAt} ms: "${seen.loadingText}"`, shots: shot ? [shot] : [] });
    add({ name: 'cold start: the main window shows the same loading state while recording', ok: !!main && /Загружаем|Loading the speech/.test(main), detail: JSON.stringify(main) });
    add({ name: 'cold start: subtitles start when the model is ready (not before), and the loading text is gone', ok: seen.subAt !== null && seen.subAt >= DELAY - 1500 && seen.subAt < DELAY + 12_000 && !seen.loadingAfterSub, detail: `first subtitle after ${seen.subAt} ms (model delay ${DELAY} ms)` });
    // finish the recording and read the journal
    await sleep(5000);
    await a.cmd('bind toggle_record down'); await a.cmd('bind toggle_record up');
    await sleep(4000);
    const mds = findMd(a.vault);
    const md = mds.length ? readFileSync(mds[0], 'utf8') : '';
    const stamps = [...md.matchAll(/^\[(\d\d):(\d\d):(\d\d)\] \*\*/gm)].map((m) => +m[1] * 3600 + +m[2] * 60 + +m[3]);
    add({ name: 'cold start: audio captured while the model loaded is buffered and transcribed (journal has lines stamped from the first seconds, not only after the load)', ok: stamps.length > 0 && Math.min(...stamps) < DELAY / 1000 - 1, detail: `${stamps.length} journal lines; earliest stamp ${Math.min(...stamps)} s; model ready at ~${DELAY / 1000} s` });
  } finally { await a.kill(); }
}

async function stageBundle() {
  // 1) the product: the app bundle built by `just build`
  const app = join(root, 'target/release/bundle/macos/Lowk.app/Contents/MacOS');
  const bw = join(app, 'lowk-stt-worker');
  if (existsSync(bw)) {
    const v = spawnSync(bw, ['--version'], { encoding: 'utf8' });
    add({ name: 'bundle: Lowk.app ships the STT worker next to the executable and it runs', ok: v.status === 0 && /lowk-stt-worker/.test(v.stdout) && existsSync(join(app, 'lowk')), detail: `${(statSync(bw).size / 1048576).toFixed(1)} MB; ${v.stdout.trim()}` });
  } else {
    add({ name: 'bundle: Lowk.app ships the STT worker next to the executable and it runs', ok: false, detail: `missing ${bw} (run: just build)` });
  }
  // 2) the lookup: a qa binary laid out like the bundle, worker ONLY beside the executable, no env hints
  const lay = mkdtempSync(join(tmpdir(), 'lowk-bundle-'));
  const mac = join(lay, 'Lowk.app/Contents/MacOS');
  mkdirSync(mac, { recursive: true });
  copyFileSync(bin, join(mac, 'lowk'));
  copyFileSync(existsSync(bw) ? bw : worker, join(mac, 'lowk-stt-worker'));
  spawnSync('chmod', ['+x', join(mac, 'lowk'), join(mac, 'lowk-stt-worker')]);
  const a = await launch({ exe: join(mac, 'lowk'), env: { ...fileEnv(), LOWK_STT_WORKER: null, LOWK_NO_DEV_WORKER: '1', LOWK_NO_WARMUP: '1' }, settings: nemotronSettings });
  try {
    await a.cmd('nav #/settings/providers');
    await sleep(1200);
    const card = await a.ev('main', `(()=>{const c=document.querySelector('[data-testid=engine-card]');return c?c.innerText:null})()`);
    const missing = await a.ev('main', `!!document.querySelector('[data-testid=engine-runtime-missing]')`);
    add({ name: 'bundle: the engine card finds the bundled worker (no «component missing» warning)', ok: !!card && missing === false, detail: JSON.stringify(card)?.slice(0, 160) });
    await a.cmd('nav #/');
    await sleep(800);
    await a.cmd('exclusion off');
    const t0 = Date.now();
    await a.cmd('bind toggle_record down'); await a.cmd('bind toggle_record up');
    const seen = await watchStart(a, t0, 40_000);
    const st = JSON.parse(await a.ev('overlay', `JSON.stringify(await window.__TAURI_INTERNALS__.invoke('session_state'))`));
    add({ name: 'bundle: a session started from the bundle layout is REAL (not the demo) and live subtitles arrive from the bundled worker', ok: st.phase === 'recording' && st.demo === false && seen.subAt !== null, detail: `demo=${st.demo}; first subtitle after ${seen.subAt} ms; last overlay state ${JSON.stringify(seen.last)}` });
    await a.cmd('bind toggle_record down'); await a.cmd('bind toggle_record up');
    await sleep(2500);
  } finally { await a.kill(); rmSync(lay, { recursive: true, force: true }); }
}

async function stageWarmup() {
  // The installed engine is loaded once in the background right after launch (low priority), so the first Record is fast.
  const a = await launch({ env: { ...fileEnv(), LOWK_STT_WORKER: worker }, settings: nemotronSettings });
  try {
    let seen = null;
    for (let i = 0; i < 80 && !seen; i++) {
      const r = spawnSync('ps', ['-axo', 'pid=,nice=,command='], { encoding: 'utf8' }).stdout.split('\n').find((l) => /^\d+\s+\d+\s+\S*lowk-stt-worker warm --model/.test(l.trim()));
      if (r) seen = r.trim();
      else await sleep(250);
    }
    add({ name: 'warm-up: shortly after launch the installed engine is preloaded by a background-priority worker', ok: !!seen, detail: seen ? seen.slice(0, 120) : 'no warm worker seen within 20 s' });
  } finally { await a.kill(); }
}

// ---------------------------------------------------------------------------------------------
// AUDIBLE stages (real capture, real sound through the speakers; opt-in: --stage audible-app,audible-system).
// The system volume is never touched. The display must be awake (a sleeping display gives AudioQueueStart -66681).
//   audible-app    : the "call" is an afplay process playing the them recordings; a second afplay (the owner's own voice,
//                    02_me) plays through the speakers and reaches the real microphone. Source = that one process (per-app tap).
//   audible-system : whole-system tap; the app itself plays a DIFFERENT clip in its own webview (must be excluded).
const wavOf = (src, dst) => spawnSync('afconvert', ['-f', 'WAVE', '-d', 'LEI16@16000', '-c', '1', src, dst]).status === 0;
function readWav(f) { // 16 kHz mono LE16 -> Float32Array
  const b = readFileSync(f); let o = 12;
  while (o < b.length - 8) { const id = b.toString('ascii', o, o + 4), n = b.readUInt32LE(o + 4); if (id === 'data') { const d = b.subarray(o + 8, o + 8 + n); const x = new Float32Array(d.length >> 1); for (let i = 0; i < x.length; i++) x[i] = d.readInt16LE(i * 2) / 32768; return x; } o += 8 + n + (n & 1); }
  throw new Error('no data chunk');
}
function writeWav(f, x) {
  const d = Buffer.alloc(x.length * 2); for (let i = 0; i < x.length; i++) d.writeInt16LE(Math.max(-32768, Math.min(32767, Math.round(x[i] * 32767))), i * 2);
  const h = Buffer.alloc(44); h.write('RIFF', 0); h.writeUInt32LE(36 + d.length, 4); h.write('WAVEfmt ', 8); h.writeUInt32LE(16, 16); h.writeUInt16LE(1, 20); h.writeUInt16LE(1, 22); h.writeUInt32LE(16000, 24); h.writeUInt32LE(32000, 28); h.writeUInt16LE(2, 32); h.writeUInt16LE(16, 34); h.write('data', 36); h.writeUInt32LE(d.length, 40);
  writeFileSync(f, Buffer.concat([h, d]));
}
/** Concatenates clips with a lead-in and gaps. Returns the file, the reference text and the speech end of every utterance (s from file start). */
function buildClip(dir, name, clips, leadS = 4, gapS = 2) {
  const parts = [new Float32Array(16000 * leadS)]; const ends = []; let t = leadS; const refs = [];
  for (const c of clips) {
    const w = join(dir, `${c}.wav`); wavOf(join(audio, `${c}.m4a`), w);
    const x = readWav(w); refs.push(readFileSync(join(audio, `${c}.txt`), 'utf8').trim());
    const base = t; parts.push(x); t += x.length / 16000;
    let last = -1, quiet = 0; // an utterance ends after >= 300 ms of silence following speech (20 ms rms frames)
    for (let i = 0; i + 320 <= x.length; i += 320) {
      let e = 0; for (let k = 0; k < 320; k++) e += x[i + k] * x[i + k];
      if (Math.sqrt(e / 320) > 0.01) { last = i / 16000; quiet = 0; } else if (last >= 0 && ++quiet === 15) { ends.push(base + last + 0.02); last = -1; }
    }
    if (last >= 0) ends.push(base + last + 0.02);
    parts.push(new Float32Array(16000 * gapS)); t += gapS;
  }
  const all = new Float32Array(parts.reduce((n, p) => n + p.length, 0)); let o = 0; for (const p of parts) { all.set(p, o); o += p.length; }
  const f = join(dir, name); writeWav(f, all);
  return { file: f, ref: refs.join(' '), ends, dur: all.length / 16000 };
}
const normW = (s) => s.toLowerCase().replace(/ё/g, 'е').replace(/[^\p{L}\p{N}]+/gu, ' ').trim().split(' ').filter(Boolean);
const lev = (a, b) => {
  let prev = Array.from({ length: b.length + 1 }, (_, i) => i);
  for (let i = 1; i <= a.length; i++) { const cur = [i]; for (let j = 1; j <= b.length; j++) cur[j] = Math.min(prev[j] + 1, cur[j - 1] + 1, prev[j - 1] + (a[i - 1] === b[j - 1] ? 0 : 1)); prev = cur; }
  return prev[b.length];
};
const cer = (hyp, ref) => { const h = normW(hyp).join(' '), r = normW(ref).join(' '); return lev(h, r) / Math.max(1, r.length); };
const wer = (hyp, ref) => { const h = normW(hyp), r = normW(ref); return lev(h, r) / Math.max(1, r.length); };
const bigrams = (w) => new Set(w.slice(0, -1).map((x, i) => x + ' ' + w[i + 1]));
const mdLines = (md, who) => md.split('\n').map((l) => l.match(new RegExp(`^\\[(\\d\\d):(\\d\\d):(\\d\\d)\\] \\*\\*(?:${who}):\\*\\* (.*)$`))).filter(Boolean).map((m) => ({ t: +m[1] * 3600 + +m[2] * 60 + +m[3], text: m[4] }));
const players = [];
const play = (file) => { const p = spawn('afplay', ['-v', '1', file], { stdio: 'ignore' }); players.push(p); return p; };
const stopPlayers = () => { for (const p of players.splice(0)) try { p.kill('SIGKILL'); } catch {} };

/** Plays a wav inside the app's own webview (WebAudio): proves our own sounds stay out of the System stream. */
async function webviewPlay(a, wavFile) {
  const b64 = readFileSync(wavFile).toString('base64');
  await a.ev('main', 'window.__w=""');
  for (let i = 0; i < b64.length; i += 200_000) await a.ev('main', `(window.__w+=${JSON.stringify(b64.slice(i, i + 200_000))},1)`);
  return a.ev('main', `(async()=>{const u=Uint8Array.from(atob(window.__w),c=>c.charCodeAt(0));const ctx=new AudioContext();await ctx.resume();const buf=await ctx.decodeAudioData(u.buffer.slice(0));const s=ctx.createBufferSource();s.buffer=buf;s.connect(ctx.destination);s.start();window.__ctx=ctx;return ctx.state+' '+buf.duration.toFixed(1)+'s'})()`);
}

function audibleMetrics(tag, md, ref, ends, playAt, evs, timeOrigin) {
  const sys = mdLines(md, 'Система|System'), me = mdLines(md, 'Я|Me');
  const sysText = sys.map((x) => x.text).join(' ');
  const m = { tag, sysLines: sys.length, meLines: me.length, sysCER: cer(sysText, ref), sysWER: wer(sysText, ref) };
  const sysEv = evs.filter(({ p }) => p.stream === 'system').map(({ t, p }) => ({ at: timeOrigin + t, p })).sort((x, y) => x.at - y.at);
  const lagTo = (list) => ends.map((e) => { const spoken = playAt + e * 1000; const hit = list.find((x) => x.at >= spoken - 150); return hit ? hit.at - spoken : null; }).filter((x) => x !== null);
  const lp = lagTo(sysEv), lf = lagTo(sysEv.filter((x) => x.p.kind === 'final'));
  const emit = sysEv.map(({ p }) => (p.t_emit_ns - p.t_audio_end_ns) / 1e6).filter((x) => x >= 0 && x < 5000);
  m.endOfSpeechToAnyText = { n: lp.length, p50: pct(lp, 0.5), p95: pct(lp, 0.95) };
  m.endOfSpeechToFinal = { n: lf.length, p50: pct(lf, 0.5), p95: pct(lf, 0.95) };
  m.audioToEmit = { n: emit.length, p50: pct(emit, 0.5), p95: pct(emit, 0.95) };
  m.me = me;
  return m;
}

async function stageAudibleApp() {
  const dir = mkdtempSync(join(tmpdir(), 'lowk-audible-'));
  const them = buildClip(dir, 'call.wav', ['02_them_q', '03_them_q']);
  const mine = buildClip(dir, 'mine.wav', ['02_me_q'], 9, 0);
  let playAt = 0, callProc;
  try {
    const r = await stageLive({
      audible: true, noMic: process.env.LOWK_AUDIBLE_NO_MIC === '1', prefix: 'audible-app-', endMs: process.env.LOWK_AUDIBLE_NO_MIC === '1' ? 62_000 : 78_000,
      beforeRecord: async (a) => {
        callProc = play(them.file);
        await sleep(600);
        const patch = JSON.stringify({ audio: { default_source: { kind: 'app', id: String(callProc.pid) } } });
        const sr = await a.ev('main', `JSON.stringify(await window.__TAURI_INTERNALS__.invoke('settings_patch',{patch:${patch}}).then(s=>s.audio.default_source))`);
        add({ name: 'audible-app: source = the afplay "call" process only', ok: /app/.test(sr), detail: sr });
        play(mine.file); // the owner's voice: reaches the real mic through the speakers
        playAt = Date.now() - 600;
      },
    });
    const m = audibleMetrics('app', r.md, them.ref, them.ends, playAt, r.events, r.timeOrigin);
    const sysBi = bigrams(normW(mdLines(r.md, 'Система|System').map((x) => x.text).join(' ')));
    const ownRef = readFileSync(join(audio, '02_me_q.txt'), 'utf8'), meBi = bigrams(normW(ownRef));
    let dupLines = 0; const per = [];
    for (const l of m.me) { const b = bigrams(normW(l.text)); if (!b.size) continue; let inSys = 0, inMe = 0; for (const x of b) { if (sysBi.has(x)) inSys++; if (meBi.has(x)) inMe++; } per.push({ text: l.text.slice(0, 90), overlapWithSystemText: +(inSys / b.size).toFixed(2), overlapWithOwnVoiceRef: +(inMe / b.size).toFixed(2) }); if (inSys / b.size >= 0.4) dupLines++; }
    const meText = m.me.map((x) => x.text).join(' ');
    m.echo = { meLines: m.me.length, linesRepeatingSystem: dupLines, meCERvsOwnVoice: cer(meText, ownRef), meCERvsSystemRef: cer(meText, them.ref), per };
    add({ name: 'audible-app: System transcript = ONLY the chosen app (CER/WER vs the them reference text)', ok: m.sysCER < 0.45 && m.sysLines >= 1, detail: `CER ${m.sysCER.toFixed(3)} WER ${m.sysWER.toFixed(3)} (${m.sysLines} lines)` });
    let lk = 0; for (const x of sysBi) if (meBi.has(x)) lk++;
    add({ name: 'audible-app: the other process (own-voice clip) does not reach System', ok: lk <= 2, detail: `${lk} bigrams of the own-voice clip found in System text (of ${sysBi.size})` });
    add({ name: 'audible-app: real-capture latency (speech end -> text in the webview)', ok: m.endOfSpeechToAnyText.n > 3, detail: JSON.stringify({ toAnyText: m.endOfSpeechToAnyText, toFinal: m.endOfSpeechToFinal, audioToEmit: m.audioToEmit }) });
    add({ name: 'audible-app: echo (System speech repeated as «Я») measured', ok: true, detail: `${dupLines}/${m.me.length} «Я» lines repeat System text (bigram overlap >= 40%); «Я» CER vs own-voice ref ${m.echo.meCERvsOwnVoice.toFixed(3)}, vs System ref ${m.echo.meCERvsSystemRef.toFixed(3)}` });
    delete m.me;
    writeFileSync(join(out, 'audible-app-metrics.json'), JSON.stringify(m, null, 1));
  } finally { stopPlayers(); rmSync(dir, { recursive: true, force: true }); }
}

async function stageAudibleSystem() {
  const dir = mkdtempSync(join(tmpdir(), 'lowk-audible-'));
  const them = buildClip(dir, 'call.wav', ['02_them_q', '03_them_q']);
  const own = join(dir, 'own.wav'); wavOf(join(audio, '04_them_q.m4a'), own);
  const a = await launch({ env: { LOWK_STT_WORKER: worker }, settings: '[stt]\nengine = "nemotron-local"\nlanguages = ["ru", "en"]\nbackground_correction = false\n[audio]\ndefault_source = { kind = "whole_system" }\n' });
  try {
    await a.cmd('nav #/'); await sleep(1500); await a.cmd('exclusion off');
    await a.ev('main', `(()=>{window.__ev=[];const T=window.__TAURI_INTERNALS__;const h=T.transformCallback(e=>window.__ev.push({t:performance.now(),p:e.payload}));return T.invoke('plugin:event|listen',{event:'stt://segment',target:{kind:'Any'},handler:h})})()`);
    await a.cmd('bind toggle_record down'); await a.cmd('bind toggle_record up');
    await sleep(1500);
    const playAt = Date.now(); play(them.file);
    const wv = await webviewPlay(a, own).catch((e) => `error ${e}`);
    const wvAt = Date.now();
    add({ name: 'audible-system: the app plays its OWN sound (a different clip, in its webview) during the whole-system capture', ok: /running/.test(String(wv)), detail: String(wv) });
    await sleep(them.dur * 1000 + 4000 - (Date.now() - playAt));
    const shot = await a.shot('overlay', 'audible-system-overlay.png');
    await a.cmd('bind toggle_record down'); await a.cmd('bind toggle_record up');
    await sleep(3000);
    const mds = findMd(a.vault);
    const md = mds.length ? readFileSync(mds[0], 'utf8') : '';
    if (mds.length) copyFileSync(mds[0], join(out, 'audible-system-call.md'));
    const events = await a.ev('main', 'window.__ev').catch(() => []);
    const timeOrigin = await a.ev('main', 'performance.timeOrigin').catch(() => 0);
    const m = audibleMetrics('system', md, them.ref, them.ends, playAt, events, timeOrigin);
    const sysW = normW(mdLines(md, 'Система|System').map((x) => x.text).join(' '));
    const ownRef = readFileSync(join(audio, '04_them_q.txt'), 'utf8');
    const ownBi = bigrams(normW(ownRef)); let lk = 0; for (const x of bigrams(sysW)) if (ownBi.has(x)) lk++;
    const refW = new Set(normW(them.ref));
    const distinct = [...new Set(normW(ownRef))].filter((w) => w.length > 5 && !refW.has(w));
    const hitW = distinct.filter((w) => sysW.includes(w));
    add({ name: 'audible-system: whole-system capture contains the playing app clip (CER vs reference)', ok: m.sysCER < 0.5, detail: `CER ${m.sysCER.toFixed(3)} WER ${m.sysWER.toFixed(3)}, ${m.sysLines} System lines`, shots: shot ? [shot] : [] });
    add({ name: 'audible-system: our own app sound is excluded (distinctive words of the webview clip never appear in System)', ok: hitW.length === 0 && lk <= 1, detail: `${hitW.length}/${distinct.length} distinctive words (${hitW.join(',')}) and ${lk} bigrams of the excluded clip found; webview clip started ${wvAt - playAt} ms after the call clip` });
    m.exclusion = { distinctWords: distinct.length, leakedWords: hitW, leakedBigrams: lk };
    delete m.me;
    writeFileSync(join(out, 'audible-system-metrics.json'), JSON.stringify(m, null, 1));
  } finally { stopPlayers(); await a.kill(); rmSync(dir, { recursive: true, force: true }); }
}

try {
  if (stages.includes('live')) await stageLive();
  if (stages.includes('noengine')) await stageNoEngine();
  if (stages.includes('install')) await stageInstall();
  if (stages.includes('loading')) await stageLoading();
  if (stages.includes('bundle')) await stageBundle();
  if (stages.includes('warmup')) await stageWarmup();
  if (stages.includes('audible-app')) await stageAudibleApp();
  if (stages.includes('audible-system')) await stageAudibleSystem();
} catch (e) {
  add({ name: 'm1-e2e harness error', ok: false, detail: String(e?.stack ?? e).slice(0, 400) });
}
writeFileSync(join(out, arg('--stage') ? `results-${stages.join('+')}.json` : 'results.json'), JSON.stringify(checks, null, 1));
console.log(`m1-e2e: ${checks.filter((c) => c.ok).length}/${checks.length} passed`);
process.exit(checks.every((c) => c.ok) ? 0 : 1);
