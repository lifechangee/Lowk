// Pure helpers for the documents UI (kept free of Svelte so they are unit-testable).
import type { DocKind, Document, IngestWarning } from '../../lib/ipc';
import { locale, tPlural } from '../../lib/i18n/locale.svelte';
import { tm } from '../i18n';

/** Rough token estimate for the instructions counter (the backend counts the real tokens). */
export function estimateTokens(text: string): number {
  return Math.ceil(text.length / 3.8);
}

export const KIND_LABEL: Record<DocKind, string> = { pdf: 'PDF', docx: 'DOCX', pptx: 'PPTX', keynote: 'KEY', xlsx: 'XLSX', csv: 'CSV', txt: 'TXT', md: 'MD', image: 'IMG' };

/** Page / slide / sheet: the unit of the `<!-- p:N -->` markers for a document kind. */
export function unitOf(kind: DocKind): 'page' | 'slide' | 'sheet' {
  return kind === 'pptx' || kind === 'keynote' ? 'slide' : kind === 'xlsx' || kind === 'csv' ? 'sheet' : 'page';
}

/** «24 страницы» / «18 slides» / «4 sheets». */
export function unitCount(kind: DocKind, n: number): string {
  const u = unitOf(kind);
  return tPlural(u === 'page' ? 'docs.pages' : u === 'slide' ? 'docs.slides' : 'docs.sheets', n);
}

export function fmtSize(bytes: number): string {
  if (bytes <= 0) return '';
  const unit = bytes >= 1048576 ? 'megabyte' : 'kilobyte';
  const v = bytes >= 1048576 ? bytes / 1048576 : Math.max(1, bytes / 1024);
  return new Intl.NumberFormat(locale.lang, { style: 'unit', unit, unitDisplay: 'short', maximumFractionDigits: v < 10 && unit === 'megabyte' ? 1 : 0 }).format(v);
}

export function fmtTokens(n: number): string {
  return new Intl.NumberFormat(locale.lang).format(n);
}

/** «~2 мин» / «~40 с». */
export function fmtEta(seconds: number | null): string {
  if (seconds === null || seconds <= 0) return '';
  return seconds >= 90 ? tm('docs.eta.min', { n: Math.round(seconds / 60) }) : tm('docs.eta.sec', { n: Math.max(5, Math.round(seconds / 5) * 5) });
}

export function warningText(w: IngestWarning): string {
  const base = tm(`docs.warn.${w.code}` as 'docs.warn.ocr_used');
  return w.page ? `${base} ${tm('docs.warn.page', { page: w.page })}` : base;
}

/**
 * Which pinned documents fit into the budget (oldest first, like the backend packs them): the ids that do NOT fit.
 * Only ready documents count; a pinned document that is still converting is not sent yet.
 */
export function pinnedOverflow(docs: Document[], budget: number): string[] {
  let used = 0;
  const out: string[] = [];
  for (const d of [...docs].filter((x) => x.pinned && x.status === 'ready').sort((a, b) => a.added.localeCompare(b.added))) {
    if (used + d.tokens <= budget) used += d.tokens;
    else out.push(d.id);
  }
  return out;
}

/** Split converted Markdown into pages by the `<!-- p:N -->` markers; text before the first marker is page 0 (frontmatter, title). */
export function splitPages(md: string): { page: number; text: string }[] {
  const parts = md.split(/<!-- p:(\d+) -->/);
  const out: { page: number; text: string }[] = [];
  const head = (parts[0] ?? '').trim();
  if (head) out.push({ page: 0, text: head });
  for (let i = 1; i < parts.length; i += 2) out.push({ page: Number(parts[i]), text: (parts[i + 1] ?? '').trim() });
  return out;
}

/** Cut the YAML frontmatter off; returns the body and the raw frontmatter lines. */
export function splitFrontmatter(md: string): { front: string; body: string } {
  const m = /^---\n([\s\S]*?)\n---\n?/.exec(md);
  return m ? { front: m[1]!, body: md.slice(m[0].length) } : { front: '', body: md };
}
