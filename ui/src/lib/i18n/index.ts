import { dict as ruDict } from './ru';
import { dict as enDict } from './en';

export type Lang = 'ru' | 'en';
/** Stored setting: `auto` follows the system locale. */
export type LangSetting = 'auto' | Lang;
/** Flat key union; `ru` is the source of truth. */
export type Key = keyof typeof ruDict;
export type Params = Record<string, string | number>;
export type PluralCategory = 'one' | 'few' | 'many' | 'other';
/** A plural family: `main.calls` for keys `main.calls.one|few|many|other`. */
export type PluralBase = { [K in Key]: K extends `${infer B}.other` ? B : never }[Key];

// `en` is typed against `ru`'s keys, so a missing key is a compile error.
export const ru: Record<Key, string> = ruDict;
export const en: Record<Key, string> = enDict;
export const dicts: Record<Lang, Record<string, string>> = { ru, en };

/** Anything starting with `ru` is Russian; everything else English. */
export function resolveLang(setting: LangSetting, systemLocale: string): Lang {
  if (setting !== 'auto') return setting;
  return systemLocale.trim().toLowerCase().startsWith('ru') ? 'ru' : 'en';
}

function format(s: string, params: Params): string {
  return s.replace(/\{(\w+)\}/g, (m, k: string) => (k in params ? String(params[k]) : m));
}

/**
 * Module-level current language, used when a call omits `lang`. Kept for
 * non-reactive callers; templates should use the reactive `t` from
 * `locale.svelte.ts`, which keeps this value in sync.
 */
let current: Lang = 'en';
export function setLang(l: Lang): void { current = l; }
export function getLang(): Lang { return current; }

/** `t('common.items', { n: 3 }, 'en')`; `{name}` placeholders. Missing keys return the key. */
export function t(key: Key, params: Params = {}, lang: Lang = current): string {
  return format(dicts[lang][key] ?? key, params);
}

/** Plural category for `n` in `lang` (Intl.PluralRules: one, few, many, other). */
export function pluralCategory(n: number, lang: Lang = current): PluralCategory {
  return new Intl.PluralRules(lang).select(n) as PluralCategory;
}

/**
 * Plural lookup: `tPlural('main.calls', 5, {}, 'ru')` -> "5 созвонов".
 * `{n}` is filled automatically. Falls back to `.other`, then to the base key.
 */
export function tPlural(base: PluralBase, n: number, params: Params = {}, lang: Lang = current): string {
  const d = dicts[lang];
  const s = d[`${base}.${pluralCategory(n, lang)}`] ?? d[`${base}.other`] ?? base;
  return format(s, { n, ...params });
}

/** Locale-aware number formatting (grouping, decimal comma in RU). */
export function formatNumber(n: number, lang: Lang, options?: Intl.NumberFormatOptions): string {
  return new Intl.NumberFormat(lang, options).format(n);
}
