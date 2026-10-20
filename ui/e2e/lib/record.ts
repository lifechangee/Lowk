import { mkdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import type { Issue } from './lint';

export const OUT = join(process.cwd(), 'e2e', 'out');

export interface Record_ {
  id: string;
  group: string;
  project: string;
  title: string;
  lang?: string;
  size?: string;
  /** Mockup key in docs/design/lowk-mockups-v3.html (ov | mn | ob) or none. */
  mockup?: 'ov' | 'mn' | 'ob';
  shot: string;
  issues: (Issue & { fid?: string })[];
  contrast: { sel: string; text: string; ratio: number; need: number; fid?: string }[];
  extra?: string[];
}

export function writeRecord(r: Record_): void {
  mkdirSync(join(OUT, 'records'), { recursive: true });
  writeFileSync(join(OUT, 'records', `${r.project}__${r.id}.json`.replace(/[^\w.\-]+/g, '_')), JSON.stringify(r));
}

export function shotPath(project: string, id: string): { abs: string; rel: string } {
  const rel = `shots/${project}/${id}.png`.replace(/[^\w./\-]+/g, '_');
  mkdirSync(join(OUT, 'shots', project), { recursive: true });
  return { abs: join(OUT, rel), rel };
}
