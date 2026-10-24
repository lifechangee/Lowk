// Overlay strings: only the `overlay.*` namespace is bundled here (not the whole app dictionary, which was
// ~100 KB of main-window text in the overlay's critical path). Keys live in lib/i18n/{ru,en}/overlay.ts.
import { overlay as ruO } from '../lib/i18n/ru/overlay';
import { overlay as enO } from '../lib/i18n/en/overlay';
import type { Lang } from '../lib/i18n/types';

export type OKey = keyof typeof ruO;
const dicts: Record<Lang, Record<string, string>> = { ru: ruO, en: enO };

export function ot(key: OKey, lang: Lang, params: Record<string, string | number> = {}): string {
  return (dicts[lang][key] ?? key).replace(/\{(\w+)\}/g, (m, k: string) => (k in params ? String(params[k]) : m));
}

/** Same rule as lib/i18n `resolveLang`, without importing the full dictionary. */
export function resolveOverlayLang(setting: 'auto' | Lang, systemLocale: string): Lang {
  if (setting !== 'auto') return setting;
  return systemLocale.trim().toLowerCase().startsWith('ru') ? 'ru' : 'en';
}
