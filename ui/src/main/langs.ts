// Call languages (pure helpers). Mirrors `crates/lowk-types/src/langpolicy.rs`: the codes are the
// primary BCP-47 tags the local engine (Nemotron 3.5) can decode; the Rust side re-validates every
// set against the engine that really runs, so this list only decides what the UI offers.
import type { EngineInfo } from '../lib/ipc';

/** Order = the order of the full list in the picker (after the user's own picks). */
export const ALL_LANGS = [
  'ru', 'en', 'uk', 'de', 'fr', 'es', 'it', 'pt', 'nl', 'pl', 'cs', 'sk', 'sl', 'hr', 'sv', 'da', 'no', 'fi', 'hu', 'ro', 'tr',
  'lt', 'lv', 'et', 'vi', 'bg', 'el', 'he', 'ar', 'hi', 'th', 'ja', 'ko', 'zh',
] as const;
export type LangCode = (typeof ALL_LANGS)[number];

/** Shown as chips without opening the full list. */
export const COMMON_LANGS: string[] = ['ru', 'en', 'uk', 'de', 'fr', 'es', 'pl'];

export function primary(tag: string): string {
  const p = (tag.split(/[-_.@]/)[0] ?? '').trim().toLowerCase();
  return p === 'nb' || p === 'nn' ? 'no' : p;
}

export function normalize(codes: readonly string[]): string[] {
  const out: string[] = [];
  for (const c of codes) {
    const p = primary(c);
    if (p && !out.includes(p)) out.push(p);
  }
  return out;
}

/** Keeps `codes` the engine supports (`supported` empty = no limit); never returns an empty set. */
export function resolve(codes: readonly string[] | null | undefined, fallback: readonly string[], supported: readonly string[]): string[] {
  const keep = (v: readonly string[]) => normalize(v).filter((c) => supported.length === 0 || supported.includes(c));
  let r = keep(codes ?? []);
  if (r.length === 0) r = keep(fallback);
  if (r.length === 0) r = supported.length === 0 || supported.includes('en') ? ['en'] : [supported[0]!];
  return r;
}

/** System language + English (`ru-RU` -> ru+en, `en-US` -> en, `de` -> de+en); English only when the engine lacks the system language. */
export function systemDefault(locale: string, supported: readonly string[]): string[] {
  const sys = primary(locale);
  const ok = (c: string) => supported.length === 0 || supported.includes(c);
  const v: string[] = [];
  if (sys && ok(sys)) v.push(sys);
  if (sys !== 'en' && ok('en')) v.push('en');
  return v.length ? v : ['en'];
}

/** Languages the engine can decode; mock/cloud descriptors may say `*` (= everything we know). */
export function supportedBy(engines: readonly EngineInfo[], engineId: string | undefined): string[] {
  const e = engines.find((i) => i.descriptor.id === engineId) ?? engines.find((i) => i.recommended) ?? engines[0];
  const l = e?.descriptor.languages ?? [];
  if (l.length === 0 || l.includes('*')) return [...ALL_LANGS];
  return normalize(l);
}

export const sameSet = (a: readonly string[], b: readonly string[]): boolean => a.length === b.length && a.every((x) => b.includes(x));
