// Overlay strings: all keys live in lib/i18n (`overlay.*`).
import { t as libT, type Key, type Lang } from '../lib/i18n';

export type OKey = Key;

export function ot(key: Key, lang: Lang, params: Record<string, string | number> = {}): string {
  return libT(key, params, lang);
}
