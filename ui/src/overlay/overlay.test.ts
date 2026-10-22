import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import { PRESETS, blockTone, kFromTransparency, menuRgb, normalizeRgb, parseGrid, toneToVars, varsToStyle, type ToneEvent } from './tone';
import { OverlayStore, formatElapsed, splitBold } from './store.svelte';

describe('tone', () => {
  it('normalizes colour formats', () => {
    expect(normalizeRgb('#0c0c0f')).toBe('12,12,15');
    expect(normalizeRgb('rgb(12, 12, 15)')).toBe('12,12,15');
    expect(normalizeRgb('nope')).toBeNull();
  });
  it('falls back to the preset on bad input', () => {
    const v = toneToVars({ tone: 'dark', rgb: 'x', fg: 'y', inv: '', int: '', mv: '' });
    expect(v.rgb).toBe(PRESETS.dark.rgb);
  });
  it('maps transparency to k and clamps', () => {
    expect(kFromTransparency(60)).toBe(0.4);
    expect(kFromTransparency(0)).toBe(1);
    expect(kFromTransparency(500)).toBe(0.1);
  });
  it('emits every input variable', () => {
    const s = varsToStyle({ ...PRESETS.light, mrgb: menuRgb(PRESETS.light.rgb, PRESETS.light.fg) }, 0.4, 13);
    for (const v of ['--rgb', '--fg', '--inv', '--int', '--mv', '--mrgb', '--k']) expect(s).toContain(v + ':');
  });
});

describe('store', () => {
  it('collects "My answer" only after an AI answer', () => {
    const s = new OverlayStore();
    const stt = (stream: 'me' | 'system', text: string) => ({ stream, utt: 1, rev: 0, kind: 'final' as const, text, lang: null, t_start_ns: 0, t_audio_end_ns: 0, t_emit_ns: 0 });
    s.applyStt(stt('me', 'before'));
    expect(s.myAnswer).toBe('');
    s.applyLlm({ type: 'start', req_id: 'r', qa_id: 'q' });
    s.applyLlm({ type: 'delta', req_id: 'r', text: 'Hi' });
    s.applyLlm({ type: 'done', req_id: 'r', truncated: false, usage: null });
    s.applyStt(stt('me', 'my words'));
    expect(s.answer).toBe('Hi');
    expect(s.myAnswer).toBe('my words');
    expect(s.stage).toBe('my');
  });
  it('leaves the menu closed outside interactive mode', () => {
    const s = new OverlayStore();
    s.menuOpen = true;
    s.setMode('passive');
    expect(s.menuOpen).toBe(false);
  });
  it('formats time and bold', () => {
    expect(formatElapsed((47 * 60 + 12) * 1000)).toBe('00:47:12');
    expect(splitBold('a **b** c')).toEqual([{ text: 'a ', bold: false }, { text: 'b', bold: true }, { text: ' c', bold: false }]);
  });
});

describe('legibility', () => {
  it('passes the contrast floor of the backdrop through as --smin', () => {
    const v = toneToVars({ tone: 'light', rgb: '255,255,255', fg: '10,11,15', inv: '255,255,255', int: '14,130,62', mv: '166,96,0', scrim_min: 0.4 });
    expect(v.smin).toBe(0.4);
    expect(varsToStyle(v, 0.1, 13)).toContain('--smin:0.4');
  });
  it('keeps text opaque at 90% transparency', () => {
    const css = readFileSync(new URL('./overlay.css', import.meta.url), 'utf8');
    const m = /--tx: rgba\(var\(--fg\), calc\(([\d.]+) \+ var\(--k\) \* ([\d.]+)\)\)/.exec(css)!;
    expect(Number(m[1]) + 0.1 * Number(m[2])).toBeGreaterThanOrEqual(0.9);
  });
});

const toneOf = (k: 'dark' | 'light' | 'color') => ({ tone: k, rgb: PRESETS[k].rgb, fg: PRESETS[k].fg, inv: PRESETS[k].inv, int: PRESETS[k].int, mv: PRESETS[k].mv, luma: null, busy: null });

describe('adaptive grid (F5)', () => {
  const ev = (cells: ('dark' | 'light' | 'color')[]): ToneEvent => ({ ...toneOf('dark'), grid: { cols: 4, rows: 3, cells: cells.map(toneOf) } });
  it('falls back to the global tone when the grid is absent or malformed', () => {
    expect(parseGrid(toneOf('dark'))).toBeNull();
    expect(parseGrid({ ...toneOf('dark'), grid: { cols: 2, rows: 2, cells: [] } })).toBeNull();
    const s = new OverlayStore();
    s.applyTone(toneOf('light'));
    expect(s.grid).toBeNull();
    expect(s.cells).toHaveLength(12);
    expect(s.cells[7]!.rgb).toBe(PRESETS.light.rgb);
  });
  it('picks the text colour from the cells under each block', () => {
    const cells = Array.from({ length: 12 }, (_, i) => (i % 4 < 2 ? 'dark' : 'light')) as ('dark' | 'light')[];
    const s = new OverlayStore();
    s.applyTone(ev(cells));
    expect(s.grid).toHaveLength(12);
    const left = blockTone(s.grid!, { x0: 0, y0: 0, x1: 0.4, y1: 0.3 });
    const right = blockTone(s.grid!, { x0: 0.6, y0: 0, x1: 1, y1: 0.3 });
    const both = blockTone(s.grid!, { x0: 0, y0: 0, x1: 1, y1: 0.3 });
    expect(left.fg).toBe(PRESETS.dark.fg);
    expect(left.mixed).toBe(false);
    expect(right.fg).toBe(PRESETS.light.fg);
    expect(both.mixed).toBe(true);
  });
});

describe('streamed answer is applied once (F10)', () => {
  const start = { type: 'start', req_id: 'r1', qa_id: 'q' } as const;
  const delta = (text: string, seq?: number) => ({ type: 'delta', req_id: 'r1', text, seq }) as const;
  it('drops deltas repeated by seq and a repeated start', () => {
    const s = new OverlayStore();
    s.applyLlm(start);
    s.applyLlm(start);
    for (const [i, w] of ['Это ', 'подсказка-', 'заглушка'].entries()) {
      s.applyLlm(delta(w, i));
      s.applyLlm(delta(w, i));
    }
    s.applyLlm(start);
    expect(s.answer).toBe('Это подсказка-заглушка');
  });
  it('ignores deltas of another or finished request, and a second done', () => {
    const s = new OverlayStore();
    s.applyLlm(start);
    s.applyLlm(delta('a', 0));
    s.applyLlm({ type: 'delta', req_id: 'other', text: 'X' });
    s.applyLlm({ type: 'done', req_id: 'r1', truncated: false, usage: null });
    s.applyLlm(delta('b', 1));
    s.applyLlm({ type: 'done', req_id: 'r1', truncated: false, usage: null });
    expect(s.answer).toBe('a');
    expect(s.streaming).toBe(false);
  });
  it('a new request replaces the text', () => {
    const s = new OverlayStore();
    s.applyLlm(start);
    s.applyLlm(delta('a', 0));
    s.applyLlm({ type: 'start', req_id: 'r2', qa_id: 'q2' });
    s.applyLlm({ type: 'delta', req_id: 'r2', text: 'b', seq: 0 });
    expect(s.answer).toBe('b');
  });
});

describe('text size (F6)', () => {
  it('reaches the overlay root as --ov-fs', () => {
    expect(varsToStyle(PRESETS.dark, 0.4, 16)).toContain('--ov-fs:16px');
  });
});

describe('S5 audio notice', () => {
  it('shows a notice only while the app audio is lost, in both languages', () => {
    const hints = readFileSync(new URL('./Hints.svelte', import.meta.url), 'utf8');
    expect(hints).toContain('store.audioLost');
    for (const l of ['ru', 'en']) {
      expect(readFileSync(new URL(`../lib/i18n/${l}/overlay.ts`, import.meta.url), 'utf8')).toContain("'overlay.audioLost'");
    }
    const s = new OverlayStore();
    expect(s.audioLost).toBe(false);
  });
});

describe('sources and refusals (M2)', () => {
  it('keeps the sources of the current answer and clears them on the next ask', () => {
    const s = new OverlayStore();
    const src = { kind: 'doc', id: 'd1', folder: { kind: 'unsorted' }, title: 'Spec.pdf', at_s: null, page: 12, heading_path: [], chunk_id: null, pinned: false } as never;
    s.applyLlm({ type: 'start', req_id: 'r1', qa_id: 'q1' });
    s.applyLlm({ type: 'sources', req_id: 'other', sources: [src], withheld: 0 });
    expect(s.sources).toHaveLength(0);
    s.applyLlm({ type: 'sources', req_id: 'r1', sources: [src], withheld: 2 });
    expect(s.sources).toHaveLength(1);
    expect(s.withheld).toBe(2);
    s.applyLlm({ type: 'start', req_id: 'r2', qa_id: 'q2' });
    expect(s.sources).toHaveLength(0);
  });

  it('shows why an ask was refused; a cancel is not an error', () => {
    const s = new OverlayStore();
    s.applyLlm({ type: 'start', req_id: 'r1', qa_id: 'q1' });
    s.applyLlm({ type: 'error', req_id: 'r1', error: { code: 'api_key_missing', params: {}, detail: null } });
    expect(s.answerError).toBe(true);
    expect(s.errorCode).toBe('api_key_missing');
    s.applyLlm({ type: 'start', req_id: 'r2', qa_id: 'q2' });
    s.applyLlm({ type: 'error', req_id: 'r2', error: { code: 'llm_cancelled', params: {}, detail: null } });
    expect(s.answerError).toBe(false);
    expect(s.streaming).toBe(false);
  });
});

describe('subtitle history (O6)', () => {
  const stt = (stream: 'me' | 'system', utt: number, kind: 'partial' | 'final', text: string, rev = 0) => ({ stream, utt, rev, kind, text, lang: null, t_start_ns: 0, t_audio_end_ns: 0, t_emit_ns: 0 });
  it('always ends with the newest line, corrections update in place', () => {
    const s = new OverlayStore();
    s.applyStt(stt('system', 5, 'final', 'старая'));
    s.applyStt(stt('system', 6, 'partial', 'нов'));
    s.applyStt(stt('system', 5, 'final', 'старая (исправлено)', 1)); // late Whisper correction
    expect(s.lines.map((l) => l.text)).toEqual(['старая (исправлено)', 'нов']);
    expect(s.sub?.text).toBe('нов');
    s.applyStt(stt('system', 6, 'final', 'новая реплика', 1));
    expect(s.sub).toMatchObject({ text: 'новая реплика', partial: false });
  });
  it('an engine restart (lower utterance ids) is not dropped', () => {
    const s = new OverlayStore();
    s.applyStt(stt('system', 1_000_005, 'final', 'до рестарта'));
    s.applyStt(stt('system', 1, 'final', 'после рестарта'));
    expect(s.sub?.text).toBe('после рестарта');
  });
  it('keeps a bounded history and a superseded partial loses its caret', () => {
    const s = new OverlayStore();
    s.applyStt(stt('me', 1, 'partial', 'а'));
    for (let i = 2; i < 200; i++) s.applyStt(stt('system', i, 'final', `r${i}`));
    expect(s.lines.length).toBe(60);
    expect(s.lines.every((l) => !l.partial)).toBe(true);
    expect(s.sub?.text).toBe('r199');
  });
});
