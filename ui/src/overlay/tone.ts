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
  /** Minimum scrim opacity (0..1) for WCAG 4.5:1 text over the worst backdrop pixel. */
  smin: number;
}

/** Fallback palettes (also used when `tone_mode` forces a fixed palette). Values from mockups v3. */
export const PRESETS: Record<ToneKind, ToneVars> = {
  dark: { rgb: '12,12,15', fg: '247,247,249', inv: '0,0,0', int: '94,224,138', mv: '255,194,74', mrgb: '26,26,31', smin: 0 },
  light: { rgb: '255,255,255', fg: '10,11,15', inv: '255,255,255', int: '14,130,62', mv: '166,96,0', mrgb: '252,252,253', smin: 0 },
  color: { rgb: '40,14,80', fg: '255,255,255', inv: '30,0,60', int: '90,255,150', mv: '255,214,102', mrgb: '48,22,90', smin: 0 },
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

export function toneToVars(t: Pick<Tone, 'tone' | 'rgb' | 'fg' | 'inv' | 'int' | 'mv'> & { scrim_min?: number | null }): ToneVars {
  const base = PRESETS[t.tone];
  const pick = (v: string, fb: string) => normalizeRgb(v) ?? fb;
  const rgb = pick(t.rgb, base.rgb);
  const fg = pick(t.fg, base.fg);
  return { rgb, fg, inv: pick(t.inv, base.inv), int: pick(t.int, base.int), mv: pick(t.mv, base.mv), mrgb: menuRgb(rgb, fg), smin: Math.min(0.95, Math.max(0, t.scrim_min ?? 0)) };
}

/** `settings.overlay.opacity` is the 0..90 transparency slider (mockup "Прозрачность"): k = 1 - v/100. */
export function kFromTransparency(percent: number): number {
  const p = Math.min(90, Math.max(0, Number.isFinite(percent) ? percent : 60));
  return Math.round((1 - p / 100) * 1000) / 1000;
}

/** Per-region tone grid (`overlay://tone` -> `grid`), row-major: `cells[row * cols + col]`. */
export interface ToneGrid {
  cols: number;
  rows: number;
  cells: Tone[];
}

export type ToneEvent = Tone & { grid?: ToneGrid | null };

export const GRID_COLS = 4;
export const GRID_ROWS = 3;

/** Reads a valid 4x3 grid off a tone event; anything else (absent, wrong size) means "use the global tone". */
export function parseGrid(t: ToneEvent): ToneVars[] | null {
  const g = t.grid;
  if (!g || g.cols !== GRID_COLS || g.rows !== GRID_ROWS || !Array.isArray(g.cells) || g.cells.length !== GRID_COLS * GRID_ROWS) return null;
  return g.cells.map((c) => toneToVars(c));
}

/** Relative luminance (0..1) of an `r,g,b` string. */
export function luma(rgb: string): number {
  const [r, g, b] = parts(rgb).map((v) => {
    const c = (v ?? 0) / 255;
    return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * r! + 0.7152 * g! + 0.0722 * b!;
}

export interface BlockTone {
  /** Text colour inputs for the block (same meaning as `--fg` / `--inv`). */
  fg: string;
  inv: string;
  /** Scrim colour of the representative cell, plus its interactive / moving accents. */
  rgb: string;
  int: string;
  mv: string;
  /** The cells under the block disagree (dark next to light): the block paints a local scrim so its single text colour stays legible. */
  mixed: boolean;
}

/**
 * Text colour for one text block from the grid cells under its rect (fractions of the overlay,
 * 0..1). The block follows the cell mix as a whole: the average scrim colour decides light vs dark
 * text, and the actual cell closest to that average supplies the exact fg/inv values, so a block
 * straddling a dark and a light cell never flips per word.
 */
export function blockTone(cells: ToneVars[], rect: { x0: number; y0: number; x1: number; y1: number }): BlockTone {
  const c0 = Math.min(GRID_COLS - 1, Math.max(0, Math.floor(rect.x0 * GRID_COLS)));
  const c1 = Math.min(GRID_COLS - 1, Math.max(c0, Math.ceil(rect.x1 * GRID_COLS) - 1));
  const r0 = Math.min(GRID_ROWS - 1, Math.max(0, Math.floor(rect.y0 * GRID_ROWS)));
  const r1 = Math.min(GRID_ROWS - 1, Math.max(r0, Math.ceil(rect.y1 * GRID_ROWS) - 1));
  const under: ToneVars[] = [];
  for (let r = r0; r <= r1; r++) for (let c = c0; c <= c1; c++) under.push(cells[r * GRID_COLS + c]!);
  const l = under.map((v) => luma(v.rgb));
  const avg = l.reduce((a, b) => a + b, 0) / l.length;
  let best = 0;
  for (let i = 1; i < under.length; i++) if (Math.abs(l[i]! - avg) < Math.abs(l[best]! - avg)) best = i;
  const v = under[best]!;
  return { fg: v.fg, inv: v.inv, rgb: v.rgb, int: v.int, mv: v.mv, mixed: Math.max(...l) - Math.min(...l) > 0.3 };
}

/** The 12 cell colours as custom properties for the scrim (`--c0`..`--c11`). */
export function cellsToStyle(cells: ToneVars[]): string {
  return cells.map((c, i) => `--c${i}:rgb(${c.rgb})`).join(';');
}

export function varsToStyle(v: ToneVars, k: number, fontPx: number): string {
  return `--rgb:${v.rgb};--fg:${v.fg};--inv:${v.inv};--int:${v.int};--mv:${v.mv};--mrgb:${v.mrgb};--smin:${v.smin};--k:${k};--ov-fs:${fontPx}px`;
}
