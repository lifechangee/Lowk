// Adaptive tone -> CSS variable inputs. See docs/architecture.md
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
  light: { rgb: '255,255,255', fg: '10,11,15', inv: '255,255,255', int: '0,113,49', mv: '130,91,0', mrgb: '252,252,253', smin: 0 },
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

export const GRID_COLS = 12;
export const GRID_ROWS = 8;
/** Half-width (px) of the colour transition at a cell edge: the scrim changes colour over 2 * BAND_PX = 8 px, never across a whole cell. */
export const BAND_PX = 4;

/** Reads a valid 12x8 grid off a tone event; anything else (absent, wrong size) means "use the global tone". */
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
 * Text colour for one text block from the grid cells under its rect (fractions of the overlay, 0..1).
 * No averaging: the block takes the exact fg/inv/accents of the cells of the text-colour group covering most of its area (a tie goes to
 * the darker scrim), so on a dark/light border the block follows the side it mostly sits on. Cells holding
 * under 10 % of the block do not count; `mixed` is set when the remaining cells disagree (dark next to light),
 * and the overlay then paints the block's text per cell like `[data-gt]` (see Overlay.svelte).
 */
export function blockTone(cells: ToneVars[], rect: { x0: number; y0: number; x1: number; y1: number }): BlockTone {
  const c0 = Math.min(GRID_COLS - 1, Math.max(0, Math.floor(rect.x0 * GRID_COLS)));
  const c1 = Math.min(GRID_COLS - 1, Math.max(c0, Math.ceil(rect.x1 * GRID_COLS - 1e-6) - 1));
  const r0 = Math.min(GRID_ROWS - 1, Math.max(0, Math.floor(rect.y0 * GRID_ROWS)));
  const r1 = Math.min(GRID_ROWS - 1, Math.max(r0, Math.ceil(rect.y1 * GRID_ROWS - 1e-6) - 1));
  const under: { v: ToneVars; area: number }[] = [];
  const ov = (a0: number, a1: number, b0: number, b1: number) => Math.max(0, Math.min(a1, b1) - Math.max(a0, b0));
  const total = Math.max(1e-9, (rect.x1 - rect.x0) * (rect.y1 - rect.y0));
  for (let r = r0; r <= r1; r++) {
    for (let c = c0; c <= c1; c++) {
      const area = rect.x1 > rect.x0 && rect.y1 > rect.y0 ? (ov(rect.x0, rect.x1, c / GRID_COLS, (c + 1) / GRID_COLS) * ov(rect.y0, rect.y1, r / GRID_ROWS, (r + 1) / GRID_ROWS)) / total : 1;
      under.push({ v: cells[r * GRID_COLS + c]!, area });
    }
  }
  const counted = under.filter((u) => u.area >= 0.1);
  const pool = counted.length ? counted : under;
  // Majority by text colour (= by polarity / tone), not by single cell: the group covering the most area wins, ties go to the darker scrim.
  const groups = new Map<string, { area: number; rep: { v: ToneVars; area: number } }>();
  for (const u of pool) {
    const g = groups.get(u.v.fg) ?? { area: 0, rep: u };
    g.area += u.area;
    if (u.area > g.rep.area) g.rep = u;
    groups.set(u.v.fg, g);
  }
  let best = [...groups.values()][0]!;
  for (const g of groups.values()) {
    if (g.area > best.area + 1e-9 || (Math.abs(g.area - best.area) <= 1e-9 && luma(g.rep.v.rgb) < luma(best.rep.v.rgb))) best = g;
  }
  const l = pool.map((u) => luma(u.v.rgb));
  const v = best.rep.v;
  return { fg: v.fg, inv: v.inv, rgb: v.rgb, int: v.int, mv: v.mv, mixed: Math.max(...l) - Math.min(...l) > 0.3 };
}

export interface TextGradient {
  image: string;
  size: string;
  position: string;
}

/**
 * Per-cell text colour for a long text element (subtitle line, answer): the cell text colours as hard-stop
 * gradient rows (one per grid row) laid over the whole overlay (`root`), shifted by the element's own offset
 * (`off`, px from the overlay's top-left), so every glyph takes the colour of the cell BEHIND it. Used with
 * `background-clip: text`; there is no per-row or per-block averaging and the stops are hard (0 px band).
 * With `box` (the element's size) the rows that do not overlap the element are skipped.
 */
export function textGradient(cells: ToneVars[], root: { w: number; h: number }, off: { x: number; y: number }, box?: { w: number; h: number }): TextGradient {
  const rowH = root.h / GRID_ROWS;
  const image: string[] = [];
  const size: string[] = [];
  const position: string[] = [];
  for (let r = 0; r < GRID_ROWS; r++) {
    if (box && ((r + 1) * rowH <= off.y - 1 || r * rowH >= off.y + box.h + 1)) continue;
    const stops = Array.from({ length: GRID_COLS }, (_, c) => `rgb(${cells[r * GRID_COLS + c]!.fg}) ${(c * 100) / GRID_COLS}% ${((c + 1) * 100) / GRID_COLS}%`);
    image.push(`linear-gradient(90deg, ${stops.join(', ')})`);
    size.push(`${root.w}px ${rowH}px`);
    position.push(`${-off.x}px ${r * rowH - off.y}px`);
  }
  if (!image.length) return textGradient(cells, root, off);
  return { image: image.join(', '), size: size.join(', '), position: position.join(', ') };
}

/** The grid cell colours as custom properties for the scrim (`--c0`..`--c95`), plus the fade of each. */
export function cellsToStyle(cells: ToneVars[]): string {
  return cells.map((c, i) => `--c${i}:rgb(${c.rgb})`).join(';');
}

/** Fade (s) of every adaptive colour change; the whole backdrop-to-paint budget is documented in docs/architecture.md */
export const FADE_S = 0.07;

/** `transition` value fading all cell colours (registered as `<color>` properties by [`registerCellProps`]) and the scrim opacity. */
export const SCRIM_TRANSITION = `${Array.from({ length: GRID_COLS * GRID_ROWS }, (_, i) => `--c${i} ${FADE_S}s`).join(',')},opacity ${FADE_S}s`;

/**
 * Horizontal scrim gradient of grid row `r`: each cell is flat up to `BAND_PX` before its right edge and
 * ramps to the next cell's colour over the 2 * BAND_PX around the edge (a sharp transition that follows the
 * backdrop border, never a blend across a whole cell). Static: it only references `var(--cN)`.
 */
export function scrimRowGradient(r: number): string {
  const edge = (k: number) => `calc(${k} * 100% / ${GRID_COLS}`;
  const stops: string[] = [];
  for (let c = 0; c < GRID_COLS; c++) {
    const v = `var(--c${r * GRID_COLS + c})`;
    const from = c === 0 ? '' : ` ${edge(c)} + var(--band))`;
    const to = c === GRID_COLS - 1 ? '' : ` ${edge(c + 1)} - var(--band))`;
    stops.push(`${v}${from}${to}`);
  }
  return `linear-gradient(90deg, ${stops.join(', ')})`;
}

/** Inline style of scrim row `r` (a strip of the overlay that fades in over the row above across 2 * BAND_PX). */
export function scrimRowStyle(r: number): string {
  const top = r === 0 ? '' : `top:calc(${r} * 100% / ${GRID_ROWS} - var(--band));-webkit-mask-image:linear-gradient(180deg,transparent,#000 calc(2 * var(--band)));mask-image:linear-gradient(180deg,transparent,#000 calc(2 * var(--band)));`;
  return `${top}background:${scrimRowGradient(r)}`;
}

let registered = false;
/** Registers `--c0..` as animatable `<color>` custom properties (idempotent; a no-op where `CSS.registerProperty` is missing: colours then switch without the fade). */
export function registerCellProps(): void {
  if (registered || typeof CSS === 'undefined' || !('registerProperty' in CSS)) return;
  registered = true;
  for (let i = 0; i < GRID_COLS * GRID_ROWS; i++) {
    try {
      CSS.registerProperty({ name: `--c${i}`, syntax: '<color>', inherits: true, initialValue: '#0c0c0f' });
    } catch {
      /* already registered (hot reload) */
    }
  }
}

export function varsToStyle(v: ToneVars, k: number, fontPx: number): string {
  return `--rgb:${v.rgb};--fg:${v.fg};--inv:${v.inv};--int:${v.int};--mv:${v.mv};--mrgb:${v.mrgb};--smin:${v.smin};--k:${k};--ov-fs:${fontPx}px`;
}
