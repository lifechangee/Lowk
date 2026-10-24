// Last known overlay look, persisted so the next start paints the right colours/size/language at once,
// before any backend answer (first paint never waits for IPC). Best effort: storage may be unavailable.
import type { OverlayStore } from './store.svelte';
import type { Lang } from '../lib/i18n/types';
import { PRESETS, type ToneVars } from './tone';

const KEY = 'lowk.overlay.snap.v1';

interface Snap {
  lang: Lang;
  transparency: number;
  fontPx: number;
  tone: ToneVars;
}

export function restoreSnapshot(store: OverlayStore): void {
  try {
    const raw = localStorage.getItem(KEY);
    if (!raw) return;
    const s = JSON.parse(raw) as Partial<Snap>;
    if (s.lang === 'ru' || s.lang === 'en') store.lang = s.lang;
    if (typeof s.transparency === 'number') store.transparency = s.transparency;
    if (typeof s.fontPx === 'number') store.fontPx = s.fontPx;
    const t = s.tone;
    if (t && typeof t === 'object' && Object.keys(PRESETS.dark).every((k) => k in t)) store.tone = t;
  } catch {
    /* no storage: defaults */
  }
}

export function saveSnapshot(store: OverlayStore): void {
  try {
    const s: Snap = { lang: store.lang, transparency: store.transparency, fontPx: store.fontPx, tone: store.tone };
    localStorage.setItem(KEY, JSON.stringify(s));
  } catch {
    /* ignore */
  }
}
