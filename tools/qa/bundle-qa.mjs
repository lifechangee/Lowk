#!/usr/bin/env node
// `just qa-bundle`: checks the PRODUCT, the signed bundle `target/release/bundle/macos/Lowk.app`, not a dev build.
// Dev builds inherit Microphone / Screen Recording / Input Monitoring / Accessibility from the host app, so every
// permission, signing and Info.plist bug is invisible there (M1 owner test). Verdicts:
//   PASS         checked and fine
//   FAIL         broken (exit 1)
//   NEEDS-GRANT  cannot be checked until the OWNER grants the permission to the bundle in System Settings;
//                this script never touches TCC and never counts it as a pass.
//
//   node tools/qa/bundle-qa.mjs [--no-build] [--no-run]     --no-run = static checks only (no launch)
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
const wasRunning = run('pgrep', ['-x', 'lowk']).status === 0;
let restoreExe = null;
if (wasRunning) {
  const ps = run('pgrep', ['-x', 'lowk']).stdout.trim().split('\n')[0];
  restoreExe = run('ps', ['-p', ps, '-o', 'comm=']).stdout.trim();
  run('pkill', ['-x', 'lowk']);
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
const o = run('open', ['-n', '--env', `LOWK_DATA_DIR=${data}`, '--env', `HOME=${home}`, '--env', 'LOWK_NO_WARMUP=1', APP]);
if (o.status !== 0) fail('open the bundle', o.stderr);
await sleep(9000);
const alive = run('pgrep', ['-x', 'lowk']).status === 0;
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
  else grant(`permission ${kind} is ${state}`, 'owner must grant it to Lowk in System Settings > Privacy & Security, then relaunch');
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
const key = run('osascript', ['-e', 'tell application "System Events" to key code 15 using {command down, shift down}']);
if (key.status !== 0) {
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
  if (key.status === 0) {
    run('osascript', ['-e', 'tell application "System Events" to key code 1 using {command down, shift down}']);
    await sleep(1200);
    check(/bind event.*action=Screenshot phase=Down/.test(logNow()), 'S2: real ⌘⇧S fires Screenshot (all monitors)');
    run('osascript', ['-e', 'tell application "System Events" to key code 0 using {command down, shift down}']);
    await sleep(1200);
    check(/bind event.*action=ScreenshotArea phase=Down/.test(logNow()), 'S2: real ⌘⇧A fires ScreenshotArea');
  }
}

run('pkill', ['-x', 'lowk']);
await sleep(1000);
rmSync(work, { recursive: true, force: true });
summary();
restore();
process.exit(results.some((r) => r.verdict === 'FAIL') ? 1 : 0);

function summary() {
  const n = (v) => results.filter((r) => r.verdict === v).length;
  console.log(`\nbundle QA: ${n('PASS')} pass, ${n('FAIL')} fail, ${n('NEEDS-GRANT')} need an owner grant (not counted as passes)`);
  const g = results.filter((r) => r.verdict === 'NEEDS-GRANT');
  if (g.length) console.log('needs owner grant:\n' + g.map((r) => `  - ${r.name}`).join('\n'));
}
