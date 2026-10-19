import { locale } from '../lib/i18n/locale.svelte';
import { tm } from './i18n';

export function pad(n: number): string { return String(n).padStart(2, '0'); }

/** 3725 -> "01:02:05". */
export function clock(totalSeconds: number): string {
  const s = Math.max(0, Math.floor(totalSeconds));
  return `${pad(Math.floor(s / 3600))}:${pad(Math.floor(s / 60) % 60)}:${pad(s % 60)}`;
}

/** 4320 -> "1 ч 12 мин" / "1 h 12 min". */
export function duration(totalSeconds: number | null): string {
  if (totalSeconds === null) return '';
  const m = Math.round(totalSeconds / 60);
  const h = Math.floor(m / 60);
  const rest = m % 60;
  if (h === 0) return tm('m.dur.min', { n: Math.max(1, rest) });
  return rest === 0 ? tm('m.dur.h', { n: h }) : `${tm('m.dur.h', { n: h })} ${tm('m.dur.min', { n: pad(rest) })}`;
}

const sameDay = (a: Date, b: Date) => a.toDateString() === b.toDateString();

/** «Сегодня, 15:00» / «Вчера» / «16 окт». */
export function when(iso: string, now: Date = new Date(), withTime = true): string {
  const d = new Date(iso);
  const time = new Intl.DateTimeFormat(locale.lang, { hour: '2-digit', minute: '2-digit' }).format(d);
  if (sameDay(d, now)) return withTime ? `${tm('m.today')}, ${time}` : tm('m.today');
  const y = new Date(now);
  y.setDate(y.getDate() - 1);
  if (sameDay(d, y)) return tm('m.yesterday');
  return new Intl.DateTimeFormat(locale.lang, { day: 'numeric', month: 'short', ...(d.getFullYear() !== now.getFullYear() ? { year: 'numeric' } : {}) }).format(d).replace('.', '');
}

export function fullDate(iso: string): string {
  return new Intl.DateTimeFormat(locale.lang, { day: 'numeric', month: 'long', year: 'numeric' }).format(new Date(iso));
}

export function greeting(now: Date = new Date()): string {
  const h = now.getHours();
  return tm(h < 5 ? 'm.greet.night' : h < 12 ? 'm.greet.morning' : h < 18 ? 'm.greet.day' : 'm.greet.evening');
}

export function langTag(langs: string[]): string {
  return langs.map((l) => l.toUpperCase()).join(' + ');
}

export function download(name: string, text: string): void {
  const url = URL.createObjectURL(new Blob([text], { type: 'text/markdown' }));
  const a = document.createElement('a');
  a.href = url;
  a.download = name;
  a.click();
  URL.revokeObjectURL(url);
}
