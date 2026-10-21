import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';
import { en, ru } from '../lib/i18n';
import { mergePatch } from './api/mock';
import { bindFor, bindWarnings, defaultBinds, formatTrigger, orphanTemplateBinds, templateSlot, triggerEquals, triggerFromKeyEvent, triggerFromMouse, withBind } from './binds';
import { parse } from './router.svelte';
import { greeting } from './fmt';
import type { SourceInfo } from '../lib/ipc';
import { nameFromId, presetInfo, presetsFor, resolveSource } from './source';

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
    expect(formatTrigger(mac.find((x) => x.action === 'screenshot')!.trigger, 'macos', names)).toEqual(['⇧', '⌘', 'S']);
    const win = defaultBinds('windows');
    expect(formatTrigger(win.find((x) => x.action === 'screenshot')!.trigger, 'windows', names)).toEqual(['Ctrl', 'Shift', 'S']);
    expect(formatTrigger(win.find((x) => x.action === 'move_hold')!.trigger, 'windows', names)).toEqual(['Alt']);
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
    const at = (a: string) => b.find((x) => x.action === a)!;
    expect(bindWarnings(at('ask_brief'), b, 'macos')).toContain('global_enter');
    expect(bindWarnings(at('screenshot'), b, 'macos')).toContain('save_as');
    const dup = [...b, { action: 'cancel_answer' as const, trigger: at('ask_brief').trigger }];
    expect(bindWarnings(dup.at(-1)!, dup, 'macos')).toContain('duplicate');
    expect(triggerEquals(at('ask_brief').trigger, at('ask_brief').trigger)).toBe(true);
  });
});

describe('infra', () => {
  it('merge patch', () => {
    expect(mergePatch({ a: { b: 1, c: 2 }, d: [1] }, { a: { b: 5 }, d: [2, 3] })).toEqual({ a: { b: 5, c: 2 }, d: [2, 3] });
  });
  it('router parse', () => {
    expect(parse('#/call/abc')).toEqual({ name: 'call', id: 'abc', at: null });
    expect(parse('#/call/abc?t=1424')).toEqual({ name: 'call', id: 'abc', at: 1424 });
    expect(parse('#/folder/sys%3Atrash')).toEqual({ name: 'folder', id: 'sys:trash', tab: 'calls' });
    expect(parse('#/folder/f1/documents')).toEqual({ name: 'folder', id: 'f1', tab: 'documents' });
    expect(parse('#/folder/f1/nope')).toEqual({ name: 'folder', id: 'f1', tab: 'calls' });
    expect(parse('#/doc/d1?p=14')).toEqual({ name: 'doc', id: 'd1', page: 14 });
    expect(parse('#/search?q=EBITDA&folder=f1&kind=doc')).toEqual({ name: 'search', q: 'EBITDA', folder: 'f1', tag: '', from: '', to: '', kind: 'doc' });
    expect(parse('#/settings/knowledge')).toEqual({ name: 'settings', page: 'knowledge' });
    expect(parse('#/settings/nope')).toEqual({ name: 'settings', page: 'general' });
    expect(parse('#/onboarding/99')).toEqual({ name: 'onboarding', step: 7 });
    expect(parse('')).toEqual({ name: 'home' });
  });
});

describe('G6 default binds', () => {
  it('binds the brief and complement templates and the hold keys', () => {
    const b = defaultBinds('macos');
    expect(b.find((x) => x.action === 'ask_brief')?.trigger).toMatchObject({ kind: 'combo', key: 'Enter', mods: { meta: true } });
    expect(b.find((x) => x.action === 'ask_complement')?.trigger).toMatchObject({ kind: 'combo', key: 'Enter', mods: { meta: true, shift: true } });
    expect(b.find((x) => x.action === 'click_hold')?.trigger).toEqual({ kind: 'modifier_only', mods: { ctrl: true } });
    expect(b.find((x) => x.action === 'move_hold')?.trigger).toEqual({ kind: 'modifier_only', mods: { alt: true } });
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
  const src = (id: string, name: string, producing: boolean): SourceInfo => ({ id, name, bundle_id: id, pid: 0, icon_png_b64: null, producing_audio: producing, kind: 'app' });
  const apps = [src('zoom', 'Zoom', true), src('teams', 'Teams', false)];
  it('follows the stored choice, else the first app playing audio, else the whole system', () => {
    expect(resolveSource({ kind: 'app', id: 'teams' }, apps)).toBe('app:teams');
    expect(resolveSource({ kind: 'whole_system' }, apps)).toBe('system');
    expect(resolveSource(null, apps)).toBe('app:zoom');
    expect(resolveSource({ kind: 'app', id: 'gone' }, apps)).toBe('app:gone'); // a stored choice need not be running
    expect(resolveSource(null, [apps[1]!])).toBe('system');
    expect(resolveSource(null, [])).toBe('system');
  });
});

describe('G3 source presets', () => {
  const app = (id: string, name: string): SourceInfo => ({ id, name, bundle_id: id, pid: 1, icon_png_b64: null, producing_audio: true, kind: 'app' });
  it('lists the seven presets (FaceTime only on macOS) and prefers the real running entry', () => {
    expect(presetsFor('macos').map((p) => p.key)).toEqual(['zoom', 'meet', 'teams', 'discord', 'telegram', 'slack', 'facetime']);
    expect(presetsFor('windows').map((p) => p.key)).not.toContain('facetime');
    const zoom = presetsFor('macos')[0]!;
    expect(presetInfo(zoom, 'macos', [app('us.zoom.xos', 'zoom.us')]).pid).toBe(1);
    const stat = presetInfo(zoom, 'macos', []);
    expect(stat).toMatchObject({ id: 'us.zoom.xos', name: 'Zoom', producing_audio: false });
    expect(presetInfo(presetsFor('windows')[0]!, 'windows', []).id).toBe('Zoom.exe');
    expect(presetInfo(presetsFor('macos')[1]!, 'macos', [app('com.google.Chrome', 'Google Chrome')]).name).toBe('Google Meet (Chrome)');
  });
});

describe('G3 names', () => {
  it('derives a display name from an id', () => {
    expect(nameFromId('C:\\\\Apps\\\\Zoom.exe')).toBe('Zoom');
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

describe('template binds', () => {
  const k = (key: string) => ({ kind: 'combo' as const, mods: { meta: true }, key });
  it('a custom template gets its own slot next to the built-in actions', () => {
    expect(templateSlot('brief')).toEqual({ action: 'ask_brief' });
    expect(templateSlot('my-tpl')).toEqual({ action: 'ask_template', template: 'my-tpl' });
    let b = withBind(defaultBinds('macos'), 'ask_template', k('KeyJ'), 'my-tpl');
    b = withBind(b, 'ask_template', k('KeyK'), 'other');
    expect(bindFor(b, 'ask_template', 'my-tpl')?.trigger).toMatchObject({ key: 'KeyJ' });
    expect(bindFor(b, 'ask_template', 'other')?.trigger).toMatchObject({ key: 'KeyK' });
    expect(bindFor(b, 'ask_template', 'nope')).toBeUndefined();
    // rebinding one template leaves the other alone; clearing removes only its slot
    b = withBind(b, 'ask_template', k('KeyL'), 'my-tpl');
    expect(b.filter((x) => x.action === 'ask_template')).toHaveLength(2);
    b = withBind(b, 'ask_template', null, 'my-tpl');
    expect(b.filter((x) => x.action === 'ask_template').map((x) => x.template)).toEqual(['other']);
  });
  it('the same trigger on two templates is a duplicate; different slots are not', () => {
    const b = withBind(withBind([], 'ask_template', k('KeyJ'), 'a'), 'ask_template', k('KeyJ'), 'b');
    expect(bindWarnings(b[0]!, b, 'macos')).toContain('duplicate');
    const c = withBind(withBind([], 'ask_template', k('KeyJ'), 'a'), 'ask_template', k('KeyK'), 'b');
    expect(bindWarnings(c[0]!, c, 'macos')).not.toContain('duplicate');
  });
  it('a bind of a deleted template is reported as an orphan', () => {
    const b = withBind(withBind([], 'ask_template', k('KeyJ'), 'a'), 'ask_template', k('KeyK'), 'gone');
    expect(orphanTemplateBinds(b, ['a']).map((x) => x.template)).toEqual(['gone']);
  });
});
