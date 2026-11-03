// The one-time notice after a crash: which call was recovered, that it was, and how much was saved.
import type { RecoveredCall } from '../lib/ipc';
import { duration } from './fmt';
import { tm } from './i18n';
import { hrefs } from './router.svelte';

export type RecoveryNotice = { text: string; href: string };

const saved = (s: number): string => (s < 60 ? tm('m.recovered.sec', { n: s }) : duration(s));

/** The text and the link of the notice; with several calls it names the last one and the count. */
export function recoveryNotice(list: RecoveredCall[]): RecoveryNotice | null {
  const last = list[list.length - 1];
  if (!last) return null;
  const title = last.title.trim() || tm('m.recovered.untitled');
  const len = saved(last.duration_s);
  const text = list.length === 1 ? tm('m.recovered.one', { title, len }) : tm('m.recovered.many', { n: list.length, title, len });
  return { text, href: hrefs.call(last.id) };
}
