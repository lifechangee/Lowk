import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';

const css = readFileSync(fileURLToPath(new URL('./tokens.css', import.meta.url)), 'utf8');

/** Custom property names declared anywhere in tokens.css. */
const declared = new Set([...css.matchAll(/(--[\w-]+)\s*:/g)].map((m) => m[1]));

describe('tokens.css', () => {
  it('declares the core token set', () => {
    for (const name of [
      '--pg', '--bg', '--side', '--panel', '--line', '--tx', '--tx2', '--tx3',
      '--hv', '--sel', '--ac', '--acbg', '--acfg',
      '--rec', '--live', '--move',
      '--r-sm', '--r', '--r-lg', '--r-xl',
      '--s1', '--s2', '--s3', '--s4', '--s6',
      '--font', '--mono', '--fs', '--fs-sm', '--fs-xs',
    ]) {
      expect(declared, name).toContain(name);
    }
  });
  it('declares overlay tone inputs and derived tokens', () => {
    for (const name of ['--rgb', '--fg', '--inv', '--int', '--mv', '--k',
      '--bgc', '--bd', '--ln', '--chip', '--menu', '--shd', '--ts', '--intc', '--mvc', '--recc']) {
      expect(declared, name).toContain(name);
    }
  });
  it('spacing scale is on a 4px grid', () => {
    for (const m of css.matchAll(/--s(\d+):\s*(\d+)px/g)) {
      const px = Number(m[2]);
      if (Number(m[1]) === 0) continue; // 2px half step
      expect(px % 4, `--s${m[1]}`).toBe(0);
    }
  });
  it('light theme is opt-in and redefines the surface and text tokens', () => {
    expect(css).toMatch(/:root\[data-theme='light'\]\s*\{[^}]*--pg:/s);
    expect(css).toMatch(/:root\[data-theme='light'\]\s*\{[^}]*--tx:/s);
  });
});
