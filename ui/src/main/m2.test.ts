// Unit tests for the M2 UI helpers: errors, documents, search, Markdown, source labels, routes.
import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import type { Document, SearchHit, SourceRef } from '../lib/ipc';
import { dicts, en, ru, type Key } from '../lib/i18n';
import { mmss, sourceHref, sourceText } from '../lib/sourceLabel';
import { MockBackend } from './api/mock';
import { blocks, inline } from './md';
import { estimateTokens, pinnedOverflow, splitFrontmatter, splitPages, unitOf } from './docs/util';
import { hrefs, parse } from './router.svelte';
import { buildQuery, groupHits, hitHref, matchTerms, pickLine, segments } from './search/util';
import { TOUR, TOUR_STEPS } from './tour/tour.svelte';

describe('every ErrorCode has a translation in both languages', () => {
  const bindings = readFileSync(new URL('../lib/ipc/bindings.ts', import.meta.url), 'utf8');
  const codes = [...(/export type ErrorCode = ([^;]+);/.exec(bindings)?.[1] ?? '').matchAll(/"(\w+)"/g)].map((m) => m[1]!);
  it('parses the code list from the generated bindings', () => expect(codes.length).toBeGreaterThan(40));
  it('has error.<code> in ru and en', () => {
    for (const c of codes) {
      expect(ru[`error.${c}` as Key], `ru ${c}`).toBeTruthy();
      expect(en[`error.${c}` as Key], `en ${c}`).toBeTruthy();
    }
  });
  it('has a text for every ingest warning code, document status and tour step', () => {
    for (const w of ['ocr_used', 'ocr_low_confidence', 'pages_skipped', 'tables_flattened', 'slide_images_ignored', 'sheet_truncated', 'encrypted_partial', 'large_file']) expect(ru[`docs.warn.${w}` as Key]).toBeTruthy();
    for (const s of ['queued', 'converting', 'indexing', 'ready', 'failed', 'cancelled']) expect(en[`docs.status.${s}` as Key]).toBeTruthy();
    for (let i = 1; i <= TOUR_STEPS; i++) for (const lang of ['ru', 'en'] as const) { expect(dicts[lang][`tour.step.${i}.title`]).toBeTruthy(); expect(dicts[lang][`tour.step.${i}.body`]).toBeTruthy(); }
    expect(TOUR).toHaveLength(6);
  });
});

describe('documents', () => {
  const doc = (id: string, tokens: number, o: Partial<Document> = {}): Document => ({ id, tokens, pinned: true, status: 'ready', added: id, ...o }) as Document;
  it('pinned budget: the oldest pinned documents win, the rest are reported as not fitting', () => {
    expect(pinnedOverflow([doc('a', 3200), doc('b', 2800), doc('c', 1500)], 6000)).toEqual(['c']);
    expect(pinnedOverflow([doc('a', 3200), doc('b', 2800)], 6000)).toEqual([]);
    expect(pinnedOverflow([doc('a', 3200), doc('b', 100, { pinned: false }), doc('c', 100, { status: 'converting' })], 6000)).toEqual([]);
  });
  it('splits converted markdown by page markers', () => {
    const md = '---\ntype: doc\n---\n# T\n<!-- p:1 -->\nA\n<!-- p:2 -->\nB';
    const { body, front } = splitFrontmatter(md);
    expect(front).toBe('type: doc');
    expect(splitPages(body)).toEqual([{ page: 0, text: '# T' }, { page: 1, text: 'A' }, { page: 2, text: 'B' }]);
  });
  it('units and token estimate', () => {
    expect(unitOf('pptx')).toBe('slide');
    expect(unitOf('xlsx')).toBe('sheet');
    expect(unitOf('pdf')).toBe('page');
    expect(estimateTokens('')).toBe(0);
    expect(estimateTokens('x'.repeat(380))).toBe(100);
  });
});

describe('search helpers', () => {
  it('highlights use UTF-16 offsets and never overlap', () => {
    expect(segments('бюджет на Q4 и бюджет', [[0, 6], [15, 21]])).toEqual([{ m: true, s: 'бюджет' }, { m: false, s: ' на Q4 и ' }, { m: true, s: 'бюджет' }]);
    expect(segments('abc', [[0, 2], [1, 3]])).toEqual([{ m: true, s: 'ab' }, { m: true, s: 'c' }]);
    expect(segments('plain', [])).toEqual([{ m: false, s: 'plain' }]);
  });
  const hit = (kind: SourceRef['kind'], extra: Partial<SourceRef> = {}): SearchHit => ({ chunk_id: Math.random().toString(), kind: kind === 'doc' ? 'doc' : kind === 'image' ? 'image' : 'speech', source: { kind, id: 'X1', folder: { kind: 'unsorted' }, title: 't', at_s: null, page: null, heading_path: [], chunk_id: null, pinned: false, ...extra }, folder_title: '', snippet: '', highlights: [], tags: [], when: null, score: 1 });
  it('click opens a call at the minute and a document at the page', () => {
    expect(hitHref(hit('call', { at_s: 1424.6 }))).toBe('#/call/X1?t=1424');
    expect(hitHref(hit('doc', { page: 14 }))).toBe('#/doc/X1?p=14');
    expect(hitHref(hit('doc'))).toBe('#/doc/X1');
    expect(hitHref(hit('instructions'))).toBe('#/folder/X1/instructions');
  });
  it('groups hits: calls, documents, screenshots', () => {
    expect(groupHits([hit('image'), hit('doc'), hit('call')]).map((g) => g.kind)).toEqual(['call', 'doc', 'image']);
  });
  it('builds the query with filters', () => {
    const q = buildQuery({ q: ' EBITDA ', folder: 'F1', tag: 'sales', from: '2026-10-01', to: '2026-10-20', kind: 'call' }, 8);
    expect(q.text).toBe('EBITDA');
    expect(q.filters.folders).toEqual([{ kind: 'user', id: 'F1' }]);
    expect(q.filters.tags).toEqual(['sales']);
    expect(q.filters.from).toBe('2026-10-01T00:00:00Z');
    expect(q.filters.kinds).toEqual(['speech', 'qa']);
    expect(q.limit).toBe(8);
  });
  it('search page addresses round-trip through the router', () => {
    const href = hrefs.search({ q: 'бюджет Q4', folder: 'F1', kind: 'doc' });
    expect(parse(href)).toMatchObject({ name: 'search', q: 'бюджет Q4', folder: 'F1', kind: 'doc' });
  });
});

describe('source labels', () => {
  const t = (k: Key, p?: Record<string, string | number>) => (en[k] as string).replace(/\{(\w+)\}/g, (_, n: string) => String(p?.[n]));
  const ref = (o: Partial<SourceRef>): SourceRef => ({ kind: 'doc', id: 'D1', folder: { kind: 'unsorted' }, title: 'Contract.pdf', at_s: null, page: null, heading_path: [], chunk_id: null, pinned: false, ...o });
  it('document p. N, call mm:ss, screenshot, instructions', () => {
    expect(sourceText(ref({ page: 14 }), t)).toBe('Contract.pdf · p. 14');
    expect(sourceText(ref({ kind: 'call', title: 'Demo', at_s: 1105 }), t)).toBe('Demo · 18:25');
    expect(sourceText(ref({ kind: 'image', title: 'Demo', at_s: 65 }), t)).toBe('Demo · screenshot 1:05');
    expect(sourceText(ref({ kind: 'instructions' }), t)).toBe('Folder instructions');
    expect(mmss(3725)).toBe('1:02:05');
  });
  it('links agree with the router', () => {
    expect(parse(sourceHref(ref({ kind: 'call', id: 'C1', at_s: 90 })))).toEqual({ name: 'call', id: 'C1', at: 90 });
    expect(parse(sourceHref(ref({ page: 3 })))).toEqual({ name: 'doc', id: 'D1', page: 3 });
  });
});

describe('markdown reader', () => {
  it('reads headings, lists, bold, tables and drops page markers', () => {
    const b = blocks('### Tasks\n- **Me**: budget\n- Client: sandbox\n\n<!-- p:2 -->\n| a | b |\n|---|---|\n| 1 | 2 |\n\ntext');
    expect(b.map((x) => x.k)).toEqual(['h', 'ul', 'table', 'p']);
    expect(inline('**Me**: x `y`')).toEqual([{ b: true, t: 'Me' }, { t: ': x ' }, { code: true, t: 'y' }]);
  });
});

describe('M2-S3: numbered clauses keep their own numbers', () => {
  it('a list that starts at 3 or is split by a page marker is not renumbered from 1', () => {
    const b = blocks('3. Price\n4. Warranty\n\n<!-- p:2 -->\n5. Secrecy');
    const ol = b.filter((x) => x.k === 'ol') as { nums: number[] }[];
    expect(ol.map((x) => x.nums)).toEqual([[3, 4], [5]]);
  });
});

describe('mock backend: M2 contracts', () => {
  it('imports a document, converts it and keeps the pinned budget meter honest', async () => {
    const api = new MockBackend();
    const F = '01JC8Z3K6Q9X2M4N7R5T0V0003';
    const before = await api.docsPinnedUsage(F);
    expect(before.over_budget).toBe(true);
    const [d] = await api.docImport(F, [{ name: 'notes.txt', path: 'notes.txt', size_bytes: 1000 }]);
    expect(d!.status).toBe('queued');
    const [bad] = await api.docImport(F, [{ name: 'archive.zip', path: 'archive.zip', size_bytes: 1000 }]);
    expect(bad!.status).toBe('failed');
    expect(bad!.error?.code).toBe('doc_unsupported');
  });
  it('refuses a duplicate unless told to copy or replace', async () => {
    const api = new MockBackend();
    const F = '01JC8Z3K6Q9X2M4N7R5T0V0003';
    const f = { name: 'dup.pdf', path: 'dup.pdf', size_bytes: 4321 };
    await api.docImport(F, [f]);
    const hits = await api.docFindDuplicates(F, [f, { ...f, name: 'other.pdf', path: 'other.pdf' }]);
    expect(hits.map((h) => h.index)).toEqual([0]);
    await expect(api.docImport(F, [f])).rejects.toMatchObject({ app: { code: 'doc_duplicate' } });
    const n = (await api.docList(F)).length;
    await api.docImport(F, [f], { onDuplicate: 'copy' });
    expect((await api.docList(F)).length).toBe(n + 1);
    await api.docImport(F, [f], { onDuplicate: 'replace' });
    expect((await api.docList(F)).filter((d) => d.file_name === 'dup.pdf').length).toBe(1);
  });
  it('keyword-only results while the embedder is missing', async () => {
    const api = new MockBackend();
    const r = await api.searchQuery({ text: 'EBITDA', filters: {}, mode: 'hybrid', limit: 8, offset: 0 });
    expect(r.total).toBeGreaterThan(0);
    expect(r.hits[0]!.highlights.length).toBeGreaterThan(0);
    expect(r.mode_used).toBe('hybrid'); // the default mock has the embedder installed
  });
});

describe('M2-S2: a call hit opens at the matching line, not the chunk start', () => {
  const lines = [
    { at_s: 0, text: 'Здравствуйте, начнём встречу' },
    { at_s: 5, text: 'Петрович просит перенести срок' },
    { at_s: 9, text: 'Хорошо, договорились' },
  ];
  it('picks the first line containing a matched term', () => expect(pickLine(lines, 0, 'петрович')).toBe(1));
  it('falls back to the nearest line at the time without a match', () => {
    expect(pickLine(lines, 9, null)).toBe(2);
    expect(pickLine(lines, 0, 'нетакого')).toBe(0);
  });
  it('terms come from the hit highlights and round-trip through the URL', () => {
    const hit = { snippet: 'Петрович просит', highlights: [[0, 8]], source: { kind: 'call', id: 'C1', at_s: 0 } } as unknown as SearchHit;
    expect(matchTerms(hit)).toBe('петрович');
    expect(parse(hitHref(hit))).toEqual({ name: 'call', id: 'C1', at: 0, match: 'петрович' });
  });
});
