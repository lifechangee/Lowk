// UI -> backend side of the capability check: every IPC command the UI can call must be in the app's command list
// (crates/lowk-app/build.rs), which is what gets an `allow-<command>` permission. The live mic button was refused
// in the bundle because its command was missing there; the Rust test covers the capability files, this one the UI.
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';

const here = new URL('.', import.meta.url).pathname;
const src = join(here, '..', '..');
const buildRs = readFileSync(join(src, '..', '..', 'crates', 'lowk-app', 'build.rs'), 'utf8');
const registered = new Set([...buildRs.slice(buildRs.indexOf('const COMMANDS'), buildRs.indexOf('];')).matchAll(/"([a-z0-9_]+)"/g)].map((m) => m[1]!));

const walk = (d: string): string[] => readdirSync(d).flatMap((f) => (statSync(join(d, f)).isDirectory() ? walk(join(d, f)) : [join(d, f)]));

describe('ui -> backend commands', () => {
  it('parses the command list', () => {
    expect(registered.size).toBeGreaterThan(50);
    expect(registered.has('settings_get')).toBe(true);
  });
  it('every command in bindings.ts is in build.rs COMMANDS', () => {
    const bindings = readFileSync(join(here, 'bindings.ts'), 'utf8');
    const used = [...bindings.matchAll(/__TAURI_INVOKE\("([a-z0-9_]+)"/g)].map((m) => m[1]!);
    expect(used.length).toBeGreaterThan(50);
    expect(used.filter((c) => !registered.has(c))).toEqual([]);
  });
  it('every direct invoke("...") outside bindings.ts is in build.rs COMMANDS (plugins excluded)', () => {
    const missing: string[] = [];
    for (const f of walk(src).filter((x) => /\.(svelte|ts)$/.test(x) && !x.endsWith('bindings.ts') && !x.endsWith('.test.ts'))) {
      for (const m of readFileSync(f, 'utf8').matchAll(/\binvoke(?:<[^>(]*>)?\(\s*['"]([^'"]+)['"]/g)) {
        if (m[1]!.startsWith('plugin:')) continue;
        if (!registered.has(m[1]!)) missing.push(`${f.slice(src.length)}: ${m[1]}`);
      }
    }
    expect(missing).toEqual([]);
  });
  it('the command list has no duplicates', () => {
    const all = [...buildRs.slice(buildRs.indexOf('const COMMANDS'), buildRs.indexOf('];')).matchAll(/"([a-z0-9_]+)"/g)].map((m) => m[1]!);
    expect(all.length).toBe(registered.size);
  });
});
