import { describe, expect, it } from 'vitest';
import { ALL_LANGS, resolve, supportedBy, systemDefault } from './langs';
import type { EngineInfo } from '../lib/ipc';

const eng = (id: string, languages: string[], recommended = false) =>
  ({ descriptor: { id, languages }, recommended }) as unknown as EngineInfo;

describe('call languages', () => {
  it('system language + English', () => {
    expect(systemDefault('ru_RU.UTF-8', [...ALL_LANGS])).toEqual(['ru', 'en']);
    expect(systemDefault('en-US', [...ALL_LANGS])).toEqual(['en']);
    expect(systemDefault('de', [...ALL_LANGS])).toEqual(['de', 'en']);
    expect(systemDefault('nb-NO', [...ALL_LANGS])).toEqual(['no', 'en']);
    expect(systemDefault('sw-KE', [...ALL_LANGS])).toEqual(['en']);
  });
  it('only what the engine supports, never empty', () => {
    const sup = ['ru', 'en', 'pl'];
    expect(resolve(['pl', 'RU-ru', 'xx', 'pl'], ['en'], sup)).toEqual(['pl', 'ru']);
    expect(resolve(['xx'], ['ru'], sup)).toEqual(['ru']);
    expect(resolve([], ['xx'], sup)).toEqual(['en']);
  });
  it('supported list comes from the chosen engine, else the recommended one; * = all', () => {
    const l = [eng('a', ['ru', 'en']), eng('b', ['*'], true)];
    expect(supportedBy(l, 'a')).toEqual(['ru', 'en']);
    expect(supportedBy(l, 'zzz')).toEqual([...ALL_LANGS]);
    expect(supportedBy([], undefined)).toEqual([...ALL_LANGS]);
  });
});
