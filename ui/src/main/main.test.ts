import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';
import { en, ru } from '../lib/i18n';
import { mergePatch } from './api/mock';
import { bindWarnings, defaultBinds, formatTrigger, triggerEquals, triggerFromKeyEvent, triggerFromMouse, withBind } from './binds';
import { parse } from './router.svelte';
import { greeting } from './fmt';
import { resolveSource } from './source';

const names = { middle: 'Wheel', back: 'Back', forward: 'Forward', other: (n: number) => `Btn ${n}` };

describe('main i18n keys', () => {
  const mKeys = Object.keys(ru).filter((k) => k.startsWith('m.'));
  it('has the same placeholders in both languages', () => {
    const ph = (s: string) => (s.match(/\{\w+\}/g) ?? []).sort().join();
    for (const k of mKeys as (keyof typeof ru)[]) expect(ph(en[k])).toBe(ph(ru[k]));
  });
  it('every literal tm(...) key used in sources exists', () => {
    const all = new Set(Object.keys(ru));
    const walk = (d: string): string[] => readdirSync(d).flatMap((f) => (statSync(join(d, f)).isDirectory() ? walk(join(d, f)) : [join(d, f)]));
    const missing: string[] = [];
    for (const f of walk(new URL('.', import.meta.url).pathname).filter((x) => /\.(svelte|ts)$/.test(x) && !x.endsWith('.test.ts'))) {
      for (const m of readFileSync(f, 'utf8').matchAll(/\btm\('([\w.]+)'/g)) if (!all.has(m[1]!)) missing.push(`${f}: ${m[1]}`);
    }
    expect(missing).toEqual([]);
  });
});

describe('binds', () => {
  it('formats mac and windows triggers', () => {
    const mac = defaultBinds('macos');
    expect(formatTrigger(mac[0]!.trigger, 'macos', names)).toEqual(['⌘', '↵']);
    expect(formatTrigger(mac[1]!.trigger, 'macos', names)).toEqual(['⇧', '⌘', 'S']);
    const win = defaultBinds('windows');
    expect(formatTrigger(win[1]!.trigger, 'windows', names)).toEqual(['Ctrl', 'Shift', 'S']);
    expect(formatTrigger(win.at(-1)!.trigger, 'windows', names)).toEqual(['Alt']);
  });
  it('records keys, ignores bare modifiers, maps mouse side buttons', () => {
    const e = { code: 'KeyR', metaKey: true, ctrlKey: false, altKey: false, shiftKey: true };
    expect(triggerFromKeyEvent(e)).toEqual({ kind: 'combo', mods: { meta: true, shift: true }, key: 'KeyR' });
    expect(triggerFromKeyEvent({ ...e, code: 'ShiftLeft' })).toBeNull();
    expect(triggerFromMouse(0, {})).toBeNull();
    expect(triggerFromMouse(3, {})).toEqual({ kind: 'mouse', button: { kind: 'back' }, mods: {} });
    expect(triggerFromMouse(4, {})).toEqual({ kind: 'mouse', button: { kind: 'forward' }, mods: {} });
    expect(triggerFromMouse(5, {})).toEqual({ kind: 'mouse', button: { kind: 'other', n: 6 }, mods: {} });
  });
  it('replaces, clears and warns', () => {
    const b = defaultBinds('macos');
    const next = withBind(b, 'ask', { kind: 'mouse', button: { kind: 'forward' }, mods: {} });
    expect(next.find((x) => x.action === 'ask')?.swallow).toBe(true);
    expect(withBind(b, 'ask', null).some((x) => x.action === 'ask')).toBe(false);
    expect(bindWarnings(b[0]!, b, 'macos')).toContain('global_enter');
    expect(bindWarnings(b[1]!, b, 'macos')).toContain('save_as');
    const dup = [...b, { action: 'cancel_answer' as const, trigger: b[0]!.trigger }];
    expect(bindWarnings(dup.at(-1)!, dup, 'macos')).toContain('duplicate');
    expect(triggerEquals(b[0]!.trigger, b[0]!.trigger)).toBe(true);
  });
});

describe('infra', () => {
  it('merge patch', () => {
    expect(mergePatch({ a: { b: 1, c: 2 }, d: [1] }, { a: { b: 5 }, d: [2, 3] })).toEqual({ a: { b: 5, c: 2 }, d: [2, 3] });
  });
  it('router parse', () => {
    expect(parse('#/call/abc')).toEqual({ name: 'call', id: 'abc' });
    expect(parse('#/folder/sys%3Atrash')).toEqual({ name: 'folder', id: 'sys:trash' });
    expect(parse('#/settings/nope')).toEqual({ name: 'settings', page: 'general' });
    expect(parse('#/onboarding/99')).toEqual({ name: 'onboarding', step: 7 });
    expect(parse('')).toEqual({ name: 'home' });
  });
});

describe('F7 default binds', () => {
  it('has no panic bind; backslash toggles the overlay', () => {
    for (const os of ['macos', 'windows'] as const) {
      const b = defaultBinds(os);
      expect(b.some((x) => x.action === 'panic_hide')).toBe(false);
      expect(b.find((x) => x.action === 'toggle_overlay')?.trigger).toMatchObject({ kind: 'combo', key: 'Backslash' });
    }
  });
});

describe('F12 capture source default', () => {
  const apps = [{ id: 'zoom', name: 'Zoom', producing_audio: true, pids: [] }, { id: 'teams', name: 'Teams', producing_audio: false, pids: [] }];
  it('follows the stored choice, else the recommended app, else the whole system', () => {
    expect(resolveSource({ kind: 'app', id: 'teams' }, apps)).toBe('app:teams');
    expect(resolveSource({ kind: 'whole_system' }, apps)).toBe('system');
    expect(resolveSource(null, apps)).toBe('app:zoom');
    expect(resolveSource({ kind: 'app', id: 'gone' }, apps)).toBe('app:zoom');
    expect(resolveSource(null, [])).toBe('system');
  });
});

describe('greeting', () => {
  it('uses the local hour', () => {
    const g = (h: number) => greeting(new Date(2026, 9, 20, h, 30));
    expect(new Set([g(2), g(9), g(14), g(20)]).size).toBe(4);
    expect(g(4)).toBe(g(0));
    expect(g(5)).toBe(g(11));
    expect(g(12)).toBe(g(17));
    expect(g(18)).toBe(g(23));
  });
});
