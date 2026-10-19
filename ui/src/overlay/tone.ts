// Adaptive tone -> CSS variable inputs. See docs/ARCHITECTURE.md 7.4.
// The overlay root receives only these inputs; every other token is calc() over them in overlay.css.
import type { Tone, ToneKind } from '../lib/ipc/bindings';

export interface ToneVars {
  rgb: string;
  fg: string;
  inv: string;
  int: string;
  mv: string;
  /** Menu surface colour (menus are more opaque than the shell). */
  mrgb: string;
}

/** Fallback palettes (also used when `tone_mode` forces a fixed palette). Values from mockups v3. */
export const PRESETS: Record<ToneKind, ToneVars> = {
  dark: { rgb: '12,12,15', fg: '247,247,249', inv: '0,0,0', int: '94,224,138', mv: '255,194,74', mrgb: '26,26,31' },
  light: { rgb: '255,255,255', fg: '10,11,15', inv: '255,255,255', int: '14,130,62', mv: '166,96,0', mrgb: '252,252,253' },
  color: { rgb: '40,14,80', fg: '255,255,255', inv: '30,0,60', int: '90,255,150', mv: '255,214,102', mrgb: '48,22,90' },
};

/** Accepts `12,12,15`, `12 12 15`, `rgb(12, 12, 15)` or `#0c0c0f`; returns `r,g,b` or null. */
export function normalizeRgb(s: string): string | null {
  const hex = /^#?([0-9a-f]{6})$/i.exec(s.trim());
  if (hex) {
    const n = parseInt(hex[1]!, 16);
    return `${(n >> 16) & 255},${(n >> 8) & 255},${n & 255}`;
  }
  const m = s.match(/\d+(\.\d+)?/g);
  if (!m || m.length < 3) return null;
  return m
    .slice(0, 3)
    .map((x) => Math.min(255, Math.max(0, Math.round(Number(x)))))
    .join(',');
}

function parts(rgb: string): number[] {
  return rgb.split(',').map(Number);
}

/** Menu colour: the scrim colour nudged 6% towards the text colour so menus read as raised. */
export function menuRgb(rgb: string, fg: string): string {
  const a = parts(rgb);
  const b = parts(fg);
  return a.map((v, i) => Math.round(v + ((b[i] ?? v) - v) * 0.06)).join(',');
}

export function toneToVars(t: Pick<Tone, 'tone' | 'rgb' | 'fg' | 'inv' | 'int' | 'mv'>): ToneVars {
  const base = PRESETS[t.tone];
  const pick = (v: string, fb: string) => normalizeRgb(v) ?? fb;
  const rgb = pick(t.rgb, base.rgb);
  const fg = pick(t.fg, base.fg);
  return { rgb, fg, inv: pick(t.inv, base.inv), int: pick(t.int, base.int), mv: pick(t.mv, base.mv), mrgb: menuRgb(rgb, fg) };
}

/** `settings.overlay.opacity` is the 0..90 transparency slider (mockup "Прозрачность"): k = 1 - v/100. */
export function kFromTransparency(percent: number): number {
  const p = Math.min(90, Math.max(0, Number.isFinite(percent) ? percent : 60));
  return Math.round((1 - p / 100) * 1000) / 1000;
}

export function varsToStyle(v: ToneVars, k: number, fontPx: number): string {
  return `--rgb:${v.rgb};--fg:${v.fg};--inv:${v.inv};--int:${v.int};--mv:${v.mv};--mrgb:${v.mrgb};--k:${k};--ov-fs:${fontPx}px`;
}
