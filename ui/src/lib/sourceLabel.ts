// Labels for `SourceRef` (what an AI answer was grounded on): document p. N, call mm:ss, screenshot, instructions.
// Shared by the main window (call page, search) and, in M3, the overlay (`llm://sources`).
import type { SourceRef } from './ipc';
import type { Key, Params } from './i18n';

type T = (key: Key, params?: Params) => string;

/** 83 -> "1:23", 3725 -> "1:02:05". */
export function mmss(totalSeconds: number): string {
  const s = Math.max(0, Math.floor(totalSeconds));
  const h = Math.floor(s / 3600);
  const m = Math.floor(s / 60) % 60;
  const sec = String(s % 60).padStart(2, '0');
  return h > 0 ? `${h}:${String(m).padStart(2, '0')}:${sec}` : `${m}:${sec}`;
}

/** The visible text of a source label. */
export function sourceText(ref: SourceRef, t: T): string {
  switch (ref.kind) {
    case 'instructions':
      return t('source.instructions');
    case 'doc':
      return ref.page ? `${ref.title} · ${t('source.page', { n: ref.page })}` : ref.title;
    case 'image':
      return ref.at_s !== null ? `${ref.title} · ${t('source.screenshot')} ${mmss(ref.at_s)}` : `${ref.title} · ${t('source.screenshot')}`;
    default:
      return ref.at_s !== null ? `${ref.title} · ${mmss(ref.at_s)}` : ref.title;
  }
}

/** Tooltip: the heading path inside the document, if any. */
export function sourceHint(ref: SourceRef): string {
  return ref.heading_path.join(' › ');
}

/** Where a source opens in the main window (hash route). */
export function sourceHref(ref: SourceRef): string {
  const id = encodeURIComponent(ref.id);
  switch (ref.kind) {
    case 'doc':
      return ref.page ? `#/doc/${id}?p=${ref.page}` : `#/doc/${id}`;
    case 'instructions':
      return `#/folder/${id}/instructions`;
    default:
      return ref.at_s !== null ? `#/call/${id}?t=${Math.floor(ref.at_s)}` : `#/call/${id}`;
  }
}
