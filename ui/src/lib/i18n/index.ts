import { dict as ruDict } from './ru';
import { dict as enDict } from './en';

export type Lang = 'ru' | 'en';
export type Key = keyof typeof ruDict;
export const ru: Record<Key, string> = ruDict;
// `en` must have the same key set as `ru` (enforced by the type and by i18n.test.ts).
export const en: Record<Key, string> = enDict;
const dicts: Record<Lang, Record<Key, string>> = { ru, en };

/** Anything starting with `ru` is Russian; everything else English. */
export function resolveLang(setting: 'auto' | Lang, systemLocale: string): Lang {
  if (setting !== 'auto') return setting;
  return systemLocale.toLowerCase().startsWith('ru') ? 'ru' : 'en';
}

let current: Lang = 'en';
export function setLang(l: Lang): void { current = l; }
export function getLang(): Lang { return current; }

/** `t('common.items', { n: 3 })`; `{name}` placeholders. */
export function t(key: Key, params: Record<string, string | number> = {}, lang: Lang = current): string {
  const s = dicts[lang][key] ?? key;
  return s.replace(/\{(\w+)\}/g, (m, k: string) => (k in params ? String(params[k]) : m));
}

/** Plural category via Intl.PluralRules (`one`, `few`, `many`, `other`). */
export function plural(n: number, lang: Lang = current): Intl.LDMLPluralRule {
  return new Intl.PluralRules(lang).select(n);
}
