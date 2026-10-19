import { describe, expect, it } from 'vitest';
import todo from './i18n-keys.todo.json';
import { localDict } from './i18n';
import { PRESETS, kFromTransparency, menuRgb, normalizeRgb, toneToVars, varsToStyle } from './tone';
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

describe('i18n todo', () => {
  it('matches the local dictionaries', () => {
    expect(todo.ru).toEqual(localDict.ru);
    expect(todo.en).toEqual(localDict.en);
    expect(Object.keys(localDict.ru)).toEqual(Object.keys(localDict.en));
  });
});
