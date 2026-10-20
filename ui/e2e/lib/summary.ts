// Prints a digest of e2e/out/records (what failed, grouped by rule). Used while developing.
import { readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
const dir = join(process.cwd(), 'e2e', 'out', 'records');
const by: Record<string, Record<string, number>> = {};
const ex: Record<string, string> = {};
for (const f of readdirSync(dir)) {
  const r = JSON.parse(readFileSync(join(dir, f), 'utf8'));
  for (const i of r.issues) {
    const k = `${i.rule} | ${i.fid ?? '-'} | ${i.sel}`;
    (by[r.group] ??= {})[k] = (by[r.group]![k] ?? 0) + 1;
    ex[k] ??= `${i.msg}  [${r.id}]`;
  }
  for (const c of r.contrast) {
    const k = `contrast | ${c.fid ?? '-'} | ${c.sel}`;
    (by[r.group] ??= {})[k] = (by[r.group]![k] ?? 0) + 1;
    ex[k] ??= `${c.ratio}<${c.need} "${c.text}" [${r.id}]`;
  }
}
for (const [g, m] of Object.entries(by)) {
  console.log('==', g);
  for (const [k, n] of Object.entries(m).sort((a, b) => b[1] - a[1]).slice(0, Number(process.argv[2] ?? 40))) console.log(String(n).padStart(4), k, '\n       ', ex[k]);
}
