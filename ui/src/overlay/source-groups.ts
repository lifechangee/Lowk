import type { SourceRef } from '../lib/ipc/bindings';
import type { Lang } from '../lib/i18n/types';
import { mmss } from '../lib/sourceLabel';
import { ot } from './i18n';

/** Presentation only: preserve retrieval order and the first ref's opening target. */
export function groupSources(sources: readonly SourceRef[], lang: Lang): { ref: SourceRef; label: string }[] {
  const groups = new Map<string, { ref: SourceRef; pages: Set<number>; times: Set<number> }>();
  for (const ref of sources) {
    const key = ref.kind === 'instructions' ? 'instructions' : JSON.stringify([ref.kind, ref.id]);
    let group = groups.get(key);
    if (!group) {
      group = { ref, pages: new Set(), times: new Set() };
      groups.set(key, group);
    }
    if (ref.page !== null) group.pages.add(ref.page);
    if (ref.at_s !== null) group.times.add(ref.at_s);
  }
  const list = (values: string[]) => values.slice(0, 3).join(', ') + (values.length > 3 ? '…' : '');
  return [...groups.values()].map(({ ref, pages, times }) => {
    if (ref.kind === 'instructions') return { ref, label: ot('overlay.src.instructions', lang) };
    const where = pages.size
      ? ot('overlay.src.page', lang, { n: list([...pages].map(String)) })
      : list([...times].map(mmss));
    return { ref, label: ref.title + (where ? ` · ${where}` : '') };
  });
}
