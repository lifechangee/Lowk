#!/usr/bin/env node
// `just qa-bundle`: checks the PRODUCT, the signed bundle `target/release/bundle/macos/Lowk.app`, not a dev build.
// Dev builds inherit Microphone / Screen Recording / Input Monitoring / Accessibility from the host app, so every
// permission, signing and Info.plist bug is invisible there (a real-app test found it). Verdicts:
//   PASS         checked and fine
//   FAIL         broken (exit 1)
//   NEEDS-GRANT  cannot be checked until the tester grants the permission to the bundle in System Settings;
//                this script never touches TCC and never counts it as a pass.
//
//   node tools/qa/bundle-qa.mjs [--no-build] [--no-run] [--silent]
//     --no-run = static checks only (no launch); --silent = launch in the background and synthesize no key presses
//     (no sound is ever played by this script; --silent keeps the keyboard and the foreground window alone too)
import { spawnSync, spawn } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { homedir, tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..', '..');
const APP = join(root, 'target/release/bundle/macos/Lowk.app');
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const has = (f) => process.argv.includes(f);
const results = [];
const rec = (verdict, name, detail = '') => {
  results.push({ verdict, name, detail });
  console.log(`${verdict.padEnd(11)} ${name}${detail ? '  ' + detail : ''}`);
};
const pass = (n, d) => rec('PASS', n, d);
const fail = (n, d) => rec('FAIL', n, d);
const grant = (n, d) => rec('NEEDS-GRANT', n, d);
const check = (ok, n, d) => (ok ? pass(n, ok === true ? d : ok) : fail(n, d));
const run = (c, a, o = {}) => spawnSync(c, a, { encoding: 'utf8', cwd: root, ...o });

if (process.platform !== 'darwin') { console.log('bundle QA is macOS only'); process.exit(0); }

// The privacy keys the app really needs: every TCC prompt we can trigger. A missing one is not a bad message,
// it is an immediate abort of the process when the prompt is requested (the onboarding crash).
const PRIVACY_KEYS = ['NSMicrophoneUsageDescription', 'NSAudioCaptureUsageDescription', 'NSScreenCaptureUsageDescription'];

// ---- 0. a running Lowk would hand our launch over to itself (single instance) and the rebuild replaces its files
// Only THIS checkout's bundle is ever touched. A blanket `pkill -x lowk` would also kill a running Lowk of the user
// (SIGTERM, no crash report).
const EXE_RE = `^${join(APP, 'Contents/MacOS/lowk').replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}( |$)`;
const wasRunning = run('pgrep', ['-f', EXE_RE]).status === 0;
let restoreExe = null;
if (wasRunning) {
  const ps = run('pgrep', ['-f', EXE_RE]).stdout.trim().split('\n')[0];
  restoreExe = run('ps', ['-p', ps, '-o', 'comm=']).stdout.trim();
  run('pkill', ['-f', EXE_RE]);
  await sleep(1500);
}
const restore = () => {
  if (restoreExe) {
    const app = restoreExe.includes('.app/') ? restoreExe.slice(0, restoreExe.indexOf('.app/') + 4) : APP;
    run('open', [app]);
  }
};

// ---- 1. build
if (!has('--no-build')) {
  const b = spawnSync('just', ['build'], { cwd: root, stdio: 'inherit', env: { ...process.env, PATH: `${homedir()}/.cargo/bin:${homedir()}/.local/node/current/bin:${homedir()}/.local/bin:/opt/homebrew/bin:${process.env.PATH}` } });
  if (b.status !== 0) { fail('just build', `exit ${b.status}`); restore(); process.exit(1); }
}
if (!existsSync(APP)) { fail('bundle exists', APP); restore(); process.exit(1); }

// ---- 2. static checks
const plist = (k) => run('/usr/bin/plutil', ['-extract', k, 'raw', join(APP, 'Contents/Info.plist')]);
for (const k of PRIVACY_KEYS) {
  const r = plist(k);
  check(r.status === 0 && r.stdout.trim().length > 10, `Info.plist has ${k}`, r.status === 0 ? '' : 'missing');
}
const bid = plist('CFBundleIdentifier').stdout.trim();
const cs = run('/usr/bin/codesign', ['-dv', '--verbose=4', APP]);
const ident = /^Identifier=(.*)$/m.exec(cs.stderr)?.[1];
check(ident === bid && bid.length > 0, `codesign Identifier == bundle id (${bid})`, `Identifier=${ident}`);
check(!/Info.plist=not bound/.test(cs.stderr), 'Info.plist is bound into the signature (the bundle is signed as a bundle)');
const sig = /^Signature=(.*)$/m.exec(cs.stderr)?.[1] ?? (/Authority=/.test(cs.stderr) ? 'certificate' : 'unknown');
const ver = run('/usr/bin/codesign', ['--verify', '--strict', '--verbose=1', APP]);
check(ver.status === 0, 'codesign --verify --strict', ver.stderr.trim().split('\n').pop());
const dr = run('/usr/bin/codesign', ['-d', '-r-', APP]);
const drText = (dr.stdout + dr.stderr).split('\n').find((l) => l.startsWith('designated')) ?? '';
check(drText.includes(`identifier "${bid}"`), 'designated requirement is pinned to the bundle id (stable across rebuilds, not a cdhash)', drText);
if (sig === 'adhoc') rec('INFO', 'signing identity: ad-hoc', 'set LOWK_SIGN_IDENTITY="Lowk Dev" for a stable certificate identity (docs/setup/macos-signing.md)');
else rec('INFO', `signing identity: ${sig}`);
const worker = join(APP, 'Contents/MacOS/lowk-stt-worker');
check(existsSync(worker) && run('/usr/bin/codesign', ['--verify', worker]).status === 0, 'STT worker is inside the bundle and signed');
const exe = join(APP, 'Contents/MacOS/lowk');
const bin = readFileSync(exe);
check(!bin.includes('X-Lowk-Qa-Token'), 'no QA control channel in the shipped binary');
// S4: the shipped binary contains WebRTC AEC3 (macOS bundles are built with the `aec` feature).
check(bin.includes('EchoCanceller3') || bin.includes('echo_canceller3'), 'S4: WebRTC AEC3 is linked into the bundle');

// ---- 2b. documents: the embedder sidecar and the document / search / Ask pipeline, run by the BUNDLE'S OWN binary
// (`lowk --selfcheck-m2`, headless, no window, no sound): ingest PDF + XLSX (+ DOCX, PPTX, CSV, MD, scanned PDF via
// Apple Vision), search finds them, the index rebuilds from the vault, semantic search with the bundled
// llama-server sidecar (when the pinned EmbeddingGemma file is on this Mac), local-only folders never reach the
// cloud (LLM, embeddings, retrieval), the Ask flow with sources and the custom template against a stub server,
// summary + tags, folder export/import, and one real OpenRouter ask when a key is in ~/.lowk-test-keys.
const embedWorker = join(APP, 'Contents/MacOS/lowk-embed-worker');
check(existsSync(embedWorker) && run('/usr/bin/codesign', ['--verify', embedWorker]).status === 0, 'embedder sidecar is inside the bundle and signed');
{
  const mb = (f) => (statSync(f).size / 1048576).toFixed(1);
  const du = run('du', ['-sk', APP]).stdout.split('\t')[0];
  rec('INFO', `bundle size ${(Number(du) / 1024).toFixed(0)} MB`, `lowk ${mb(exe)} MB, stt worker ${mb(worker)} MB, embed worker ${existsSync(embedWorker) ? mb(embedWorker) : '-'} MB`);
}
{
  const PINNED_SHA = '2188ac1deca4b77dffefd603c2776a9d76d9d74ec01841392982ebb840b09135';
  const modelCandidates = [process.env.LOWK_EMBED_MODEL, join(homedir(), 'Library/Application Support/DigUp/Models/embeddinggemma-2-Q8_0.gguf')].filter(Boolean);
  const model = modelCandidates.find((f) => existsSync(f) && run('shasum', ['-a', '256', f]).stdout.startsWith(PINNED_SHA));
  let liveKey = process.env.OPENROUTER_API_KEY;
  if (!liveKey && existsSync(join(homedir(), '.lowk-test-keys'))) liveKey = /^(?:export )?OPENROUTER_API_KEY=(.*)$/m.exec(readFileSync(join(homedir(), '.lowk-test-keys'), 'utf8'))?.[1]?.trim().replace(/^["']|["']$/g, '');
  const sh = mkdtempSync(join(tmpdir(), 'lowk-selfcheck-home-'));
  const args = ['--selfcheck-m2', '--fixtures', join(root, 'fixtures/ingest'), '--ocr', '--require-worker'];
  if (model) args.push('--model', model);
  if (liveKey && !has('--no-live')) args.push('--live');
  const r = spawnSync(exe, args, { encoding: 'utf8', timeout: 900000, env: { ...process.env, HOME: sh, LOWK_DATA_DIR: join(sh, 'data'), LOWK_DEV_KEYS: '1', OPENROUTER_API_KEY: liveKey ?? '' } });
  const out = (r.stdout ?? '') + (r.stderr ?? '');
  const lines = [...out.matchAll(/^CHECK (PASS|FAIL|SKIP) (.*?) :: (.*)$/gm)];
  for (const [, v, name, detail] of lines) {
    const d = detail.replace(liveKey ?? '\u0000', '<key>').slice(0, 160);
    if (v === 'PASS') pass(`docs ${name}`, d);
    else if (v === 'FAIL') fail(`docs ${name}`, d);
    else rec('SKIP', `docs ${name}`, d);
  }
  check(lines.length >= 25 && /^SELFCHECK \d+ checks, 0 failed/m.test(out) && r.status === 0, 'documents self-check of the bundle ran to the end', `exit ${r.status}, ${lines.length} checks`);
  if (!model) grant('documents semantic search with the bundled sidecar', 'no pinned EmbeddingGemma file on this Mac (set LOWK_EMBED_MODEL or install the model from the app)');
  rmSync(sh, { recursive: true, force: true });
}

// ---- 2c. Audio path, silent, by the BUNDLE'S OWN binary (`lowk --selfcheck-audio`): the Me and System streams are
// fed from FILES as raw 48 kHz device signals (a clipping hot mic with a strong echo of the speakers) through the real
// ring -> resampler -> WebRTC AEC3 -> session -> BUNDLED STT worker for 60 s (regression: "I spoke, it recognised
// and closed"), then the real tap is built/rebuilt: its aggregate must have no output stream and none may be left
// behind (regression: capture must never alter playback). Nothing is played.
{
  const models = process.env.LOWK_MODELS ?? join(homedir(), 'Library/Application Support/Lowk/models');
  const benchAudio = [process.env.LOWK_BENCH_AUDIO, join(root, 'fixtures/stt')].filter(Boolean).find((d) => existsSync(join(d, '02_them_q.m4a')));
  if (!existsSync(join(models, 'nemotron-local')) || !benchAudio) {
    grant('audio self-check (60 s mic+system+AEC from files)', `no engine models in ${models} or no fixtures/stt recordings (reference texts only are tracked) (set LOWK_BENCH_AUDIO)`);
  } else {
    const sh = mkdtempSync(join(tmpdir(), 'lowk-audio-selfcheck-'));
    const r = spawnSync(exe, ['--selfcheck-audio', '--me', join(benchAudio, '01_me_q.m4a'), '--system', join(benchAudio, '02_them_q.m4a'), '--models', models, '--seconds', '60', '--hot', '--require-aec'], { encoding: 'utf8', timeout: 900000, env: { ...process.env, HOME: sh, LOWK_DATA_DIR: join(sh, 'data'), LOWK_STT_WORKER: worker } });
    const out = (r.stdout ?? '') + (r.stderr ?? '');
    const lines = [...out.matchAll(/^CHECK (PASS|FAIL|SKIP) (.*?) :: (.*)$/gm)];
    for (const [, v, name, detail] of lines) {
      if (v === 'PASS') pass(`audio ${name}`, detail.slice(0, 160));
      else if (v === 'FAIL') fail(`audio ${name}`, detail.slice(0, 160));
      else rec('SKIP', `audio ${name}`, detail.slice(0, 160));
    }
    check(/^SELFCHECK-AUDIO ok/m.test(out) && r.status === 0 && lines.length >= 14, 'audio self-check of the bundle ran to the end (the process did not die)', `exit ${r.status}${r.signal ? ` signal ${r.signal}` : ''}, ${lines.length} checks`);
    rmSync(sh, { recursive: true, force: true });
  }
}

if (has('--no-run')) { summary(); restore(); process.exit(results.some((r) => r.verdict === 'FAIL') ? 1 : 0); }

// ---- 3. launch the bundle (clean profile: empty data dir AND empty HOME, so nothing from the dev machine leaks in)
const work = mkdtempSync(join(tmpdir(), 'lowk-bundle-qa-'));
const home = join(work, 'home');
const data = join(work, 'data');
mkdirSync(home, { recursive: true });
mkdirSync(data, { recursive: true });
// S2: the profile starts with the screenshot binds SWAPPED (what an old build wrote); the bundle must repair them.
const bindToml = (action, key) => `[[binds]]\naction = "${action}"\nswallow = false\n[binds.trigger]\nkind = "combo"\nkey = "${key}"\n[binds.trigger.mods]\nalt = false\nctrl = false\nmeta = true\nshift = true\n`;
writeFileSync(join(data, 'settings.toml'), `[onboarding]\ndone = true\nrecording_notice_ack = true\n[vault]\npath = "${work}/vault"\n` + bindToml('screenshot', 'KeyA') + bindToml('screenshot_area', 'KeyS') + bindToml('toggle_record', 'KeyR'));
const since = Date.now();
const o = run('open', ['-n', ...(has('--silent') ? ['-g'] : []), '--env', `LOWK_DATA_DIR=${data}`, '--env', `HOME=${home}`, '--env', 'LOWK_NO_WARMUP=1', APP]);
if (o.status !== 0) fail('open the bundle', o.stderr);
await sleep(9000);
const alive = run('pgrep', ['-f', EXE_RE]).status === 0;
check(alive, 'the bundle stays alive after launch (no TCC/privacy abort)');
const crash = existsSync(join(homedir(), 'Library/Logs/DiagnosticReports'))
  ? readdirSync(join(homedir(), 'Library/Logs/DiagnosticReports')).filter((f) => /^lowk.*\.(ips|crash)$/i.test(f) && statSync(join(homedir(), 'Library/Logs/DiagnosticReports', f)).mtimeMs > since)
  : [];
check(crash.length === 0, 'no crash report written during the run', crash.join(', '));

const logPath = join(data, 'lowk.log');
const log = existsSync(logPath) ? readFileSync(logPath, 'utf8') : '';
check(log.length > 0, 'the bundle writes lowk.log (diagnostics for a Finder-launched app)', logPath);

const eng = /startup: engine.*/.exec(log)?.[0] ?? '';
check(/demo_allowed=false/.test(eng), 'user-facing build cannot run the demo (demo_allowed=false)', eng.replace(/\x1b\[[0-9;]*m/g, '').slice(-160));
check(/engine_ready=false/.test(eng), 'clean profile: no engine ready -> Record must show the download prompt, never fake data');

const permLine = /startup: permissions.*perms="?([^\n]*)/.exec(log)?.[1] ?? '';
for (const m of permLine.matchAll(/(\w+)=(\w+)/g)) {
  const [, kind, state] = m;
  if (state === 'Granted') pass(`permission ${kind} granted to the bundle`);
  else grant(`permission ${kind} is ${state}`, 'grant it to Lowk in System Settings > Privacy & Security, then relaunch');
}

const applied = /binds applied.*total=(\d+).*active=(\d+)/s.exec(log);
const notActive = [...log.matchAll(/bind not fully active.*?action=(\w+) status=([^\n]*)/g)].map((m) => ({ a: m[1], s: m[2].replace(/\x1b\[[0-9;]*m/g, '') }));
check(!!applied, 'binds were applied at startup', applied ? `${applied[2]} of ${applied[1]} active` : 'no "binds applied" line');
const HOLD = new Set(['MoveHold', 'ClickHold']);
const plainBad = notActive.filter((x) => !HOLD.has(x.a));
check(plainBad.length === 0, 'plain combos (global shortcuts) work with no extra permission', plainBad.map((x) => `${x.a}:${x.s}`).join('; '));
const holdBad = notActive.filter((x) => HOLD.has(x.a));
if (holdBad.length) grant('hold / modifier-only binds (move, click-through) need Input Monitoring', holdBad.map((x) => `${x.a}:${x.s}`).join('; '));
else if (applied) pass('hold / modifier-only binds are active');

// A real key press through the OS (System Events) into the bundle: the global shortcut for Record. On a clean
// profile there is no engine, so the app must refuse and open the prompt (never start a demo call).
const logNow = () => (existsSync(logPath) ? readFileSync(logPath, 'utf8') : '');
const key = has('--silent')
  ? { status: 1, stderr: '--silent: no key presses' }
  : run('osascript', ['-e', 'tell application "System Events" to key code 15 using {command down, shift down}']);
if (has('--silent')) {
  rec('SKIP', 'real key presses (⌘⇧R / ⌘⇧S / ⌘⇧A)', '--silent: run without it when the keyboard may be used');
} else if (key.status !== 0) {
  grant('real key press into the bundle (⌘⇧R, Record)', `this terminal/host app has no Accessibility, so it cannot synthesize keys: ${key.stderr.trim().slice(0, 100)}`);
} else {
  await sleep(2500);
  const l = logNow();
  check(/bind event.*action=ToggleRecord phase=Down/.test(l), 'real ⌘⇧R press reaches the bundle (global shortcut works with no extra permission)');
  check(/record refused: no speech engine ready/.test(l), 'Record without an engine opens the download/cloud prompt (no demo)');
  check(!/Demo \d/.test(l) && !/lowk-demo/.test(l), 'no demo call was started');
}

// S2: the swapped binds were repaired on disk, and the real keys fire the right actions.
{
  const st = existsSync(join(data, 'settings.toml')) ? readFileSync(join(data, 'settings.toml'), 'utf8') : '';
  const keyOf = (action) => new RegExp(`action = "${action}"[\\s\\S]*?key = "(\\w+)"`).exec(st)?.[1];
  check(keyOf('screenshot') === 'KeyS' && keyOf('screenshot_area') === 'KeyA', 'S2: swapped screenshot binds are migrated (⌘⇧S = Скриншот, ⌘⇧A = область)', `screenshot=${keyOf('screenshot')} area=${keyOf('screenshot_area')}`);
  if (key.status === 0 && !has('--silent')) {
    // Only the log lines written AFTER each press count, and the exact action name is asserted
    // (the other screenshot action must NOT fire).
    const press = async (code) => {
      const before = logNow().length;
      run('osascript', ['-e', `tell application "System Events" to key code ${code} using {command down, shift down}`]);
      await sleep(1500);
      return [...logNow().slice(before).matchAll(/bind event action=(\w+) phase=Down/g)].map((m) => m[1]);
    };
    const s = await press(1);
    check(s.length === 1 && s[0] === 'Screenshot', 'S2: real ⌘⇧S fires exactly Screenshot (all monitors)', `fired: ${s.join(',') || 'nothing'}`);
    const a = await press(0);
    check(a.length === 1 && a[0] === 'ScreenshotArea', 'S2: real ⌘⇧A fires exactly ScreenshotArea', `fired: ${a.join(',') || 'nothing'}`);

    // B: binds are PHYSICAL keys. Repeat under every keyboard layout this Mac has enabled (users type in
    // «Ру»: kVK 1 prints «ы», not "s"). The layout is switched through TIS for this test only and restored.
    const kbd = join(work, 'kbd');
    const built = run('swiftc', ['-O', '-o', kbd, join(root, 'tools/qa/kbd.swift')]);
    if (built.status !== 0) {
      grant('B: ⌘⇧S / ⌘⇧A under every keyboard layout', `kbd helper did not compile: ${(built.stderr ?? '').slice(0, 100)}`);
    } else {
      const orig = run(kbd, ['current']).stdout.trim();
      const all = run(kbd, ['list']).stdout.split('\n').map((x) => x.trim()).filter(Boolean);
      const wanted = [...new Set([orig, ...all.filter((x) => /ABC|US|Russian|Dvorak|Colemak|German|French/i.test(x))])].slice(0, 5);
      try {
        for (const id of wanted) {
          if (run(kbd, ['select', id]).status !== 0) { grant(`B: layout ${id} cannot be selected here`); continue; }
          await sleep(400);
          const before = logNow().length;
          run(kbd, ['press', '1', 'cmd,shift']);
          await sleep(1200);
          const s2 = [...logNow().slice(before).matchAll(/bind event action=(\w+) phase=Down/g)].map((m) => m[1]);
          const before2 = logNow().length;
          run(kbd, ['press', '0', 'cmd,shift']);
          await sleep(1200);
          const a2 = [...logNow().slice(before2).matchAll(/bind event action=(\w+) phase=Down/g)].map((m) => m[1]);
          check(s2.join() === 'Screenshot' && a2.join() === 'ScreenshotArea', `B: layout ${id.replace('com.apple.keylayout.', '')}: physical S key = Screenshot, A key = ScreenshotArea`, `S -> ${s2.join(',') || 'nothing'}, A -> ${a2.join(',') || 'nothing'}`);
        }
      } finally {
        if (orig) run(kbd, ['select', orig]);
      }
    }
  }
}

// Call languages (per-call language selection) against the BUNDLED worker, from files (silent): the
// language set of a call limits the decoder and the allowed letters. Needs the engine models of this Mac
// (read only); without them the check is reported, not passed.
{
  const models = process.env.LOWK_MODELS ?? join(homedir(), 'Library/Application Support/Lowk/models');
  if (!existsSync(join(models, 'nemotron-local')) || run('which', ['afconvert']).status !== 0) {
    grant('call languages replay', `no engine models in ${models} (or no afconvert): install the engine, then rerun`);
  } else {
    const py = run('python3', [join(root, 'tools/qa/stt-lang-check.py'), '--quick', 'ru,en', 'pl,en'], { env: { ...process.env, LOWK_STT_WORKER: worker, LOWK_MODELS: models } });
    const out = (py.stdout ?? '') + (py.stderr ?? '');
    const sets = [...out.matchAll(/^SET (\S+)\s+mean CER ([\d.]+).*letters outside the set: (\d+)/gm)].map((m) => ({ set: m[1], cer: Number(m[2]), bad: Number(m[3]) }));
    check(py.status === 0 && sets.length === 2, 'call languages: bundled worker, ru+en and pl+en, no letter outside the selected set', sets.map((x) => `${x.set} cer=${x.cer} foreign=${x.bad}`).join('; ') || out.slice(-200));
    const ruen = sets.find((x) => x.set === 'ru+en');
    check(!!ruen && ruen.cer < 0.5, 'call languages: ru+en recognises the reference recordings (CER < 0.5)', ruen ? `cer=${ruen.cer}` : '');
  }
}

// A kill signal is not a crash: the app says so in its log, finishes the call and exits by itself (2026-10-22: an
// external pkill looked like "the app closed while I was speaking" because nothing was logged).
{
  const pids = run('pgrep', ['-f', EXE_RE]).stdout.trim().split('\n').filter(Boolean);
  if (pids.length) {
    run('kill', ['-TERM', pids[0]]);
    await sleep(3500);
    check(run('pgrep', ['-f', EXE_RE]).status !== 0, 'SIGTERM ends the bundle by itself within 3.5 s');
    check(/stopped by a signal \(kill\/pkill\/launchd\), not a crash/.test(logNow()), 'SIGTERM is named in lowk.log (not a silent disappearance)');
  }
}
run('pkill', ['-f', EXE_RE]);
await sleep(1000);
rmSync(work, { recursive: true, force: true });
summary();
restore();
process.exit(results.some((r) => r.verdict === 'FAIL') ? 1 : 0);

function summary() {
  const n = (v) => results.filter((r) => r.verdict === v).length;
  console.log(`\nbundle QA: ${n('PASS')} pass, ${n('FAIL')} fail, ${n('NEEDS-GRANT')} need a grant (not counted as passes)`);
  const g = results.filter((r) => r.verdict === 'NEEDS-GRANT');
  if (g.length) console.log('needs grant:\n' + g.map((r) => `  - ${r.name}`).join('\n'));
}
