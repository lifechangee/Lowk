// WP8 string layer. New keys live in `i18n-keys.todo.json` ({ru, en}) until WP10 merges them
// into `lib/i18n`; keys that already exist there (main.*, onboarding.*, settings.*, common.*)
// are resolved through the reactive `t` from `lib/i18n/locale.svelte`.
import dict from './i18n-keys.todo.json';
import { locale, t } from '../lib/i18n/locale.svelte';
import type { Key, Params } from '../lib/i18n';

export type MKey = keyof typeof dict.ru;
const own: Record<'ru' | 'en', Record<string, string>> = dict;

/** Translate a WP8 key (falls through to the shared dictionary). Reactive in templates. */
export function tm(key: MKey | Key, params: Params = {}): string {
  const s = own[locale.lang][key];
  if (s === undefined) return t(key as Key, params);
  return s.replace(/\{(\w+)\}/g, (m, k: string) => (k in params ? String(params[k]) : m));
}

/** Plural for own keys stored as `one|few|many|other` in one string (`other` last). */
export function tn(key: MKey, n: number): string {
  const parts = (own[locale.lang][key] ?? key).split('|');
  const order = { one: 0, few: 1, many: 2, other: 3 } as const;
  const s = parts[Math.min(order[locale.plural(n)], parts.length - 1)] ?? key;
  return s.replace('{n}', String(n));
}

export const ownKeys = own;
