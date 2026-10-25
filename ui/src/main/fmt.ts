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

/** 15.923527 -> "16 ГБ" / "16 GB"; one decimal below 10 GB. */
export function fmtGb(gb: number): string {
  const n = new Intl.NumberFormat(locale.lang, { maximumFractionDigits: gb < 10 ? 1 : 0 }).format(gb);
  return `${n} ${tm('m.unit.gb')}`;
}

/** Bytes -> whole MB, grouped ("1 308"). */
export function fmtMbBytes(bytes: number): string {
  return new Intl.NumberFormat(locale.lang, { maximumFractionDigits: 0 }).format(Math.round(bytes / 1048576));
}

/** Seconds -> "40 с" / "3 мин" / "1 ч 5 мин"; rounds up so it never promises less than it takes. */
export function fmtEta(sec: number): string {
  const s = Math.max(1, Math.ceil(sec));
  if (s < 60) return tm('m.eta.sec', { n: Math.max(5, Math.ceil(s / 5) * 5) });
  const m = Math.ceil(s / 60);
  return m < 60 ? tm('m.eta.min', { n: m }) : duration(m * 60);
}

/** Bytes/s -> "3,2 МБ/с". */
export function fmtSpeed(bps: number): string {
  const v = bps / 1048576;
  return tm('m.model.speed', { speed: new Intl.NumberFormat(locale.lang, { maximumFractionDigits: v < 10 ? 1 : 0 }).format(v) });
}

/** Rough download time at 50 Mbit/s (6.25 MB/s) for a size in MB. */
export const estimateSeconds = (mb: number): number => mb / 6.25;
