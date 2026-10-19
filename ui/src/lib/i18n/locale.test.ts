import { describe, expect, it } from 'vitest';
import { getLang } from './index';
import { locale } from './locale.svelte';

describe('reactive locale', () => {
  it('keeps the non-reactive current language in sync', () => {
    locale.init('ru-RU', 'auto');
    expect(getLang()).toBe('ru');
    locale.setSetting('en');
    expect(getLang()).toBe('en');
  });
  it('follows system locale in auto mode and switches live', () => {
    locale.init('ru-RU', 'auto');
    expect(locale.lang).toBe('ru');
    expect(locale.t('main.newCall')).toBe('Новый созвон');

    locale.setSetting('en');
    expect(locale.lang).toBe('en');
    expect(locale.t('main.newCall')).toBe('New call');

    locale.setSetting('auto');
    locale.setSystemLocale('en-GB');
    expect(locale.lang).toBe('en');
  });
  it('plural helper uses the active language', () => {
    locale.init('ru-RU', 'ru');
    expect(locale.tPlural('main.calls', 2)).toBe('2 созвона');
    locale.setSetting('en');
    expect(locale.tPlural('main.calls', 2)).toBe('2 calls');
  });
});
