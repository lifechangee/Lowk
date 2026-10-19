import { describe, expect, it } from 'vitest';
import { en, resolveLang, ru, t } from './index';

describe('i18n', () => {
  it('ru and en have identical key sets and no empty values', () => {
    expect(Object.keys(en).sort()).toEqual(Object.keys(ru).sort());
    for (const d of [ru, en]) for (const v of Object.values(d)) expect(v.trim()).not.toBe('');
  });
  it('resolves locale', () => {
    expect(resolveLang('auto', 'ru-RU')).toBe('ru');
    expect(resolveLang('auto', 'de-DE')).toBe('en');
    expect(resolveLang('ru', 'en-US')).toBe('ru');
  });
  it('interpolates', () => {
    expect(t('common.items', { n: 3 }, 'en')).toBe('3 items');
  });
});
