// Pure helpers for the search UI (palette and results page).
import type { SearchFilters, SearchHit, SearchQuery, SearchResults, SemanticState } from '../../lib/ipc';
import { folderRef } from '../api';
import { hrefs, type SearchKind, type SearchParams } from '../router.svelte';

/** Marked and plain parts of a snippet. `highlights` are UTF-16 offsets (what JS string indexes are). */
export function segments(snippet: string, highlights: [number, number][]): { m: boolean; s: string }[] {
  const out: { m: boolean; s: string }[] = [];
  let at = 0;
  for (const [a0, b0] of [...highlights].sort((x, y) => x[0] - y[0])) {
    const a = Math.max(a0, at);
    const b = Math.min(b0, snippet.length);
    if (b <= a) continue;
    if (a > at) out.push({ m: false, s: snippet.slice(at, a) });
    out.push({ m: true, s: snippet.slice(a, b) });
    at = b;
  }
  if (at < snippet.length) out.push({ m: false, s: snippet.slice(at) });
  return out.length ? out : [{ m: false, s: snippet }];
}

/** Calls (speech, AI answers), documents (and folder instructions) and screenshots. */
export function groupOf(hit: SearchHit): SearchKind {
  return hit.source.kind === 'image' ? 'image' : hit.source.kind === 'call' ? 'call' : 'doc';
}
export const GROUP_ORDER: SearchKind[] = ['call', 'doc', 'image'];

export function groupHits(hits: SearchHit[]): { kind: SearchKind; hits: SearchHit[] }[] {
  return GROUP_ORDER.map((kind) => ({ kind, hits: hits.filter((h) => groupOf(h) === kind) })).filter((g) => g.hits.length > 0);
}

/** Click target of a hit: a call at the minute, a document at the page, a folder's instructions. */
export function hitHref(hit: SearchHit): string {
  const s = hit.source;
  if (s.kind === 'doc') return hrefs.doc(s.id, s.page);
  if (s.kind === 'instructions') return hrefs.folder(s.id, 'instructions');
  return hrefs.call(s.id, s.at_s);
}

export function buildQuery(p: SearchParams, limit: number, offset = 0): SearchQuery {
  const filters: SearchFilters = {};
  if (p.folder) filters.folders = [folderRef(p.folder)];
  if (p.tag) filters.tags = [p.tag];
  if (p.from) filters.from = `${p.from}T00:00:00Z`;
  if (p.to) filters.to = `${p.to}T23:59:59Z`;
  if (p.kind) filters.kinds = p.kind === 'call' ? ['speech', 'qa'] : p.kind === 'doc' ? ['doc'] : ['image'];
  return { text: p.q.trim(), filters, mode: 'hybrid', limit, offset };
}

export const hasFilters = (p: SearchParams): boolean => !!(p.folder || p.tag || p.from || p.to || p.kind);

/** Why the result is keyword-only (shown next to the notice): no silent downgrade. */
export function keywordReason(state: SemanticState | null | undefined): 'off' | 'model_missing' | 'building' | 'failed' | 'unknown' {
  if (!state) return 'unknown';
  return state.state === 'ready' ? 'unknown' : state.state;
}

export const isHybrid = (r: SearchResults | null): boolean => r?.mode_used === 'hybrid' || r?.mode_used === 'semantic';
