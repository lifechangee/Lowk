#!/usr/bin/env node
// Builds docs/qa/report.html: every screenshot (chromium | webkit | real app) next to the matching
// v3 mockup frame, failures and known bugs highlighted. No dependencies.
//   node tools/qa/report.mjs
import { copyFileSync, existsSync, mkdirSync, readdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..', '..');
const out = join(root, 'docs', 'qa');
const e2e = join(root, 'ui', 'e2e', 'out');
const realDir = join(out, 'real');
const shotsOut = join(out, 'shots');
rmSync(shotsOut, { recursive: true, force: true });
mkdirSync(shotsOut, { recursive: true });

const esc = (s) => String(s).replace(/[&<>"]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' })[c]);
const readJson = (p) => (existsSync(p) ? JSON.parse(readFileSync(p, 'utf8')) : null);
const copy = (src, name) => {
  if (!existsSync(src)) return null;
  const dst = join(shotsOut, name);
  mkdirSync(dirname(dst), { recursive: true });
  copyFileSync(src, dst);
  return 'shots/' + name;
};

// ---- browser suite records ----
const recDir = join(e2e, 'records');
const records = existsSync(recDir) ? readdirSync(recDir).map((f) => readJson(join(recDir, f))) : [];
const byId = new Map();
for (const r of records) {
  const k = `${r.group}|${r.id}`;
  if (!byId.has(k)) byId.set(k, { id: r.id, group: r.group, title: r.title, lang: r.lang, size: r.size, mockup: r.mockup, per: {} });
  byId.get(k).per[r.project] = r;
}
const mock = {};
for (const k of ['ov', 'mn', 'ob']) mock[k] = copy(join(e2e, 'mockups', `${k}.png`), `mockups/${k}.png`);

// ---- playwright results: F-id table ----
const results = readJson(join(e2e, 'results.json'));
const tests = [];
(function walk(suite) {
  for (const s of suite.suites ?? []) walk(s);
  for (const sp of suite.specs ?? []) for (const t of sp.tests ?? []) tests.push({ title: sp.title, project: t.projectName, status: t.status, expected: t.expectedStatus, file: sp.file });
})({ suites: results?.suites ?? [] });
const ftests = tests.filter((t) => /^F\d/.test(t.title));
const fRows = {};
for (const t of ftests) {
  const fid = t.title.match(/^F\d+/)[0];
  (fRows[fid] ??= []).push(t);
}
const verdict = (t) => (t.expected === 'failed' && t.status === 'expected' ? 'caught' : t.expected === 'failed' && t.status === 'unexpected' ? 'FIXED? (passes now: remove test.fail)' : t.status === 'expected' ? 'pass' : t.status === 'unexpected' ? 'FAIL' : t.status);

// ---- real-app records ----
const real = existsSync(realDir) ? readdirSync(realDir).filter((f) => f.endsWith('.json')).map((f) => readJson(join(realDir, f))) : [];

const issueBadge = (i) => `<li class="${i.fid ? 'known' : 'bad'}">${i.fid ? `<b>${esc(i.fid)}</b> ` : ''}<code>${esc(i.rule)}</code> ${esc(i.sel)} - ${esc(i.msg ?? '')}</li>`;
const contrastBadge = (c) => `<li class="${c.fid ? 'known' : c.ratio < 3 ? 'bad' : 'warn'}">${c.fid ? `<b>${esc(c.fid)}</b> ` : ''}<code>contrast</code> ${esc(c.sel)} "${esc(c.text)}" ${c.ratio}:1 (need ${c.need})</li>`;

let nBad = 0, nKnown = 0, nCards = 0;
const cards = [];
for (const g of [...byId.values()].sort((a, b) => (a.group + a.id).localeCompare(b.group + b.id))) {
  const projs = Object.keys(g.per).sort();
  const all = projs.flatMap((p) => [...g.per[p].issues, ...g.per[p].contrast.filter((c) => c.ratio < 3 || c.fid)]);
  const bad = all.filter((i) => !i.fid).length, known = all.filter((i) => i.fid).length;
  nBad += bad ? 1 : 0;
  nKnown += known ? 1 : 0;
  nCards++;
  const imgs = projs.map((p) => {
    const r = g.per[p];
    const src = copy(join(e2e, r.shot), r.shot);
    return `<figure><figcaption>${esc(p)}</figcaption>${src ? `<a href="${src}"><img loading="lazy" src="${src}"></a>` : '(no shot)'}</figure>`;
  });
  if (g.mockup && mock[g.mockup]) imgs.push(`<figure class="mk"><figcaption>v3 mockup (${g.mockup})</figcaption><a href="${mock[g.mockup]}"><img loading="lazy" src="${mock[g.mockup]}"></a></figure>`);
  const lines = projs.flatMap((p) => [...g.per[p].issues.map(issueBadge), ...g.per[p].contrast.map(contrastBadge)].map((l) => l.replace('<li ', `<li data-p="${p}" `)));
  const uniq = [...new Set(lines)];
  cards.push(`<section class="card ${bad ? 'fail' : known ? 'knownc' : 'ok'}" data-g="${g.group}" data-state="${bad ? 'fail' : known ? 'known' : 'ok'}"><h3>${esc(g.title)} <small>${esc(g.group)} / ${esc(g.lang)} / ${esc(g.size)}</small></h3><div class="shots">${imgs.join('')}</div>${uniq.length ? `<ul>${uniq.join('')}</ul>` : ''}</section>`);
}

const realCards = real.flatMap((r) =>
  (r.checks ?? []).map((c) => `<section class="card ${c.ok ? 'ok' : c.fid ? 'knownc' : 'fail'}" data-state="${c.ok ? 'ok' : c.fid ? 'known' : 'fail'}"><h3>${esc(c.name)} <small>real app</small></h3><div class="shots">${(c.shots ?? []).map((s) => { const src = copy(join(realDir, s), 'real/' + s); return src ? `<figure><a href="${src}"><img loading="lazy" src="${src}"></a></figure>` : ''; }).join('')}</div><ul><li class="${c.ok ? 'okl' : c.fid ? 'known' : 'bad'}">${c.fid ? `<b>${esc(c.fid)}</b> ` : ''}${esc(c.detail ?? '')}</li></ul></section>`),
);

const fSummary = Object.entries(fRows).sort().map(([fid, ts]) => `<tr><td><b>${fid}</b></td><td>${esc(ts[0].title)}</td><td>${[...new Set(ts.map((t) => `${t.project}: ${verdict(t)}`))].join('<br>')}</td></tr>`).join('');
const realF = real.flatMap((r) => (r.checks ?? []).filter((c) => c.fid).map((c) => `<tr><td><b>${esc(c.fid)}</b></td><td>${esc(c.name)} <i>(real app)</i></td><td>${c.ok ? 'passes (fixed? remove the expected-fail)' : 'caught'}</td></tr>`)).join('');
const total = tests.length, passed = tests.filter((t) => t.status === 'expected').length;

const html = `<!doctype html><html lang="en"><meta charset="utf-8"><title>Lowk QA report</title>
<style>
:root{color-scheme:dark;--bg:#111113;--p:#17171a;--l:#27272b;--t:#ececee;--t2:#8e8e96}
body{margin:0;background:var(--bg);color:var(--t);font:13px/1.45 -apple-system,system-ui,sans-serif;padding:24px}
h1{font-size:20px}h2{margin-top:32px;font-size:15px}h3{font-size:13px;margin:0 0 8px}h3 small{color:var(--t2);font-weight:400}
table{border-collapse:collapse}td,th{border:1px solid var(--l);padding:4px 10px;text-align:left;vertical-align:top}
.bar{position:sticky;top:0;background:var(--bg);padding:8px 0;z-index:5;display:flex;gap:8px;flex-wrap:wrap}
button{background:var(--p);color:var(--t);border:1px solid var(--l);border-radius:6px;padding:4px 10px;cursor:pointer}
.card{background:var(--p);border:1px solid var(--l);border-radius:10px;padding:12px;margin:12px 0}
.card.fail{border-color:#ff4d4f;box-shadow:0 0 0 1px #ff4d4f}.card.knownc{border-color:#d9a441}
.shots{display:flex;gap:8px;overflow-x:auto}figure{margin:0;flex:none}figcaption{color:var(--t2);font-size:11px}
img{height:300px;border:1px solid var(--l);display:block}figure.mk img{border-color:#4a7bd0}
ul{margin:8px 0 0;padding-left:18px;font-size:12px}li.bad{color:#ff8f90}li.known{color:#e8c070}li.warn{color:var(--t2)}li.okl{color:#6fcf97}code{color:var(--t2)}
</style>
<h1>Lowk QA report <small style="color:var(--t2);font-weight:400">${new Date().toISOString().slice(0, 16)}Z</small></h1>
<p>Browser suite: ${passed}/${total} tests as expected. Cards: ${nCards} (<b style="color:#ff8f90">${nBad} with new failures</b>, <b style="color:#e8c070">${nKnown} with known issues</b>). Red = new failure, amber = known bug (F-id from docs/M0-FEEDBACK.md or N-id from docs/QA.md), blue frame = v3 mockup reference.</p>
<h2>Owner-reported bugs: detector status</h2>
<table><tr><th>ID</th><th>Check</th><th>Result</th></tr>${fSummary}${realF}</table>
<div class="bar"><button data-f="all">all</button><button data-f="fail">new failures</button><button data-f="known">known</button><button data-f="ok">ok</button></div>
${realCards.length ? `<h2>Real app (qa build)</h2>${realCards.join('')}` : '<h2>Real app</h2><p>No real-app run found (docs/qa/real). Run <code>just qa-app</code>.</p>'}
<h2>UI visual suite</h2>
${cards.join('')}
<script>document.querySelectorAll('.bar button').forEach(b=>b.onclick=()=>{const f=b.dataset.f;document.querySelectorAll('.card').forEach(c=>c.style.display=f==='all'||c.dataset.state===f?'':'none')})</script>
</html>`;
writeFileSync(join(out, 'report.html'), html);
console.log(`wrote ${join(out, 'report.html')} (${nCards} cards, ${nBad} failing, ${nKnown} known)`);
