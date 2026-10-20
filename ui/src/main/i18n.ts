// Main-window string layer: every key lives in `lib/i18n` (`m.*` namespace plus the shared ones).
import { locale, t } from '../lib/i18n/locale.svelte';
import { dicts } from '../lib/i18n';
import type { Key, Params } from '../lib/i18n';

export type MKey = Key;

/** Translate a key; reactive in templates (reads the current locale). */
export function tm(key: Key, params: Params = {}): string {
  return t(key, params);
}

/** Plural for keys stored as `one|few|many|other` in one string (`other` last). */
export function tn(key: Key, n: number): string {
  const parts = (dicts[locale.lang][key] ?? key).split('|');
  const order = { one: 0, few: 1, many: 2, other: 3 } as const;
  const s = parts[Math.min(order[locale.plural(n)], parts.length - 1)] ?? key;
  return s.replace('{n}', String(n));
}

/** Kept for tests that enumerate the namespace. */
export const ownKeys = dicts;
