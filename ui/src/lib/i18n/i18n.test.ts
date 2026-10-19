import { describe, expect, it } from 'vitest';
import { dicts, en, pluralCategory, resolveLang, ru, t, tPlural, type Key, type PluralBase } from './index';
import { dict as ruDict } from './ru';
import { dict as enDict } from './en';

const placeholders = (s: string) => [...s.matchAll(/\{(\w+)\}/g)].map((m) => m[1]).sort().join(',');

describe('dictionary parity', () => {
  it('ru and en have identical key sets', () => {
    expect(Object.keys(enDict).sort()).toEqual(Object.keys(ruDict).sort());
  });
  it('no empty values in either language', () => {
    for (const d of [ru, en]) for (const v of Object.values(d)) expect(v.trim()).not.toBe('');
  });
  it('placeholders match between languages', () => {
    for (const k of Object.keys(ruDict) as Key[]) {
      expect(placeholders(en[k]), k).toBe(placeholders(ru[k]));
    }
  });
  it('every plural family has all four categories in both languages', () => {
    const bases = new Set(
      (Object.keys(ruDict) as Key[])
        .filter((k) => k.endsWith('.other'))
        .map((k) => k.slice(0, -'.other'.length)),
    );
    expect(bases.size).toBeGreaterThan(0);
    for (const b of bases) {
      for (const lang of ['ru', 'en'] as const) {
        for (const c of ['one', 'few', 'many', 'other']) {
          expect(dicts[lang][`${b}.${c}`], `${lang} ${b}.${c}`).toBeDefined();
        }
      }
    }
  });
});

describe('locale resolution', () => {
  it('auto follows the system locale, ru prefix is Russian, everything else English', () => {
    expect(resolveLang('auto', 'ru-RU')).toBe('ru');
    expect(resolveLang('auto', 'ru')).toBe('ru');
    expect(resolveLang('auto', 'de-DE')).toBe('en');
    expect(resolveLang('auto', '')).toBe('en');
  });
  it('an explicit setting overrides the system locale', () => {
    expect(resolveLang('ru', 'en-US')).toBe('ru');
    expect(resolveLang('en', 'ru-RU')).toBe('en');
  });
});

describe('t', () => {
  it('interpolates named params', () => {
    expect(t('common.items', { n: 3 }, 'en')).toBe('3 items');
    expect(t('onboarding.stepCounter', { n: 2, total: 8 }, 'ru')).toBe('Шаг 2 из 8');
  });
  it('returns the key for unknown keys', () => {
    expect(t('nope.missing' as Key, {}, 'en')).toBe('nope.missing');
  });
});

describe('Russian pluralization', () => {
  it('selects the right category', () => {
    expect(pluralCategory(1, 'ru')).toBe('one');
    expect(pluralCategory(2, 'ru')).toBe('few');
    expect(pluralCategory(5, 'ru')).toBe('many');
    expect(pluralCategory(11, 'ru')).toBe('many');
    expect(pluralCategory(21, 'ru')).toBe('one');
    expect(pluralCategory(22, 'ru')).toBe('few');
    expect(pluralCategory(0, 'ru')).toBe('many');
  });
  it('1 созвон / 2 созвона / 5 созвонов', () => {
    const base: PluralBase = 'main.calls';
    expect(tPlural(base, 1, {}, 'ru')).toBe('1 созвон');
    expect(tPlural(base, 2, {}, 'ru')).toBe('2 созвона');
    expect(tPlural(base, 5, {}, 'ru')).toBe('5 созвонов');
    expect(tPlural(base, 21, {}, 'ru')).toBe('21 созвон');
    expect(tPlural(base, 112, {}, 'ru')).toBe('112 созвонов');
  });
  it('English one / other', () => {
    expect(tPlural('main.calls', 1, {}, 'en')).toBe('1 call');
    expect(tPlural('main.calls', 3, {}, 'en')).toBe('3 calls');
    expect(tPlural('main.calls', 0, {}, 'en')).toBe('0 calls');
  });
});
