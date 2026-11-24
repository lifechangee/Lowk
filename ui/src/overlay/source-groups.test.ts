import { describe, expect, it } from 'vitest';
import type { SourceRef } from '../lib/ipc/bindings';
import { groupSources } from './source-groups';

const doc = (page: number | null = null, extra: Partial<SourceRef> = {}): SourceRef => ({
  kind: 'doc', id: 'doc-1', folder: { kind: 'unsorted' }, title: '01_SHPARGALKA_zakrepit',
  at_s: null, page, heading_path: [], chunk_id: null, pinned: false, ...extra,
});

describe('overlay source groups', () => {
  for (const lang of ['ru', 'en'] as const) {
    it(`groups the B19 instructions + five pageless chunks (${lang}) without changing refs`, () => {
      const refs = [doc(null, { kind: 'instructions', id: 'folder-1', title: 'Инструкции папки' }),
        ...Array.from({ length: 5 }, (_, i) => doc(null, { chunk_id: `chunk-${i}` }))];
      const before = structuredClone(refs);
      const groups = groupSources(refs, lang);
      expect(groups.map((g) => g.label)).toEqual([
        lang === 'ru' ? 'Инструкции папки' : 'Folder instructions', '01_SHPARGALKA_zakrepit',
      ]);
      expect(groups[1]!.ref).toBe(refs[1]);
      expect(refs).toEqual(before);
    });

    it(`merges distinct pages and limits them to three (${lang})`, () => {
      expect(groupSources([3, 5, 5, 9, 12].map((p) => doc(p)), lang)[0]!.label)
        .toBe(`01_SHPARGALKA_zakrepit · ${lang === 'ru' ? 'стр.' : 'p.'} 3, 5, 9…`);
      expect(groupSources([doc(3), doc(5)], lang)[0]!.label)
        .toBe(`01_SHPARGALKA_zakrepit · ${lang === 'ru' ? 'стр.' : 'p.'} 3, 5`);
    });
  }

  it('merges call times, keeps kinds/ids separate, and combines instructions into one chip', () => {
    const refs = [83, 83, 125, 3725, 4000].map((at_s) => doc(null, { kind: 'call', at_s }));
    refs.push(doc(), doc(null, { id: 'doc-2' }),
      doc(null, { kind: 'instructions', id: 'folder-1' }), doc(null, { kind: 'instructions', id: 'folder-2' }));
    expect(groupSources(refs, 'en').map((g) => g.label)).toEqual([
      '01_SHPARGALKA_zakrepit · 1:23, 2:05, 1:02:05…',
      '01_SHPARGALKA_zakrepit', '01_SHPARGALKA_zakrepit', 'Folder instructions',
    ]);
    expect(groupSources([], 'ru')).toEqual([]);
  });
});
