// Registry of KNOWN product bugs (docs/M0-FEEDBACK.md). A lint issue that matches an entry is
// reported as "known Fx" (orange in the gallery) instead of failing the generic state test; the
// matching expected-fail test in known-bugs.spec.ts proves the detector still sees it.
// When a fix lands: delete the entry here AND remove `test.fail()` from the F-test.
import type { Issue } from './lib/lint';

export interface Known {
  fid: string;
  rule: Issue['rule'] | 'contrast';
  /** matches the issue selector string */
  sel: RegExp;
  /** matches `${group}/${stateId}` */
  state?: RegExp;
}

export const KNOWN: Known[] = [
  // --- owner feedback (docs/M0-FEEDBACK.md) ---
  { fid: 'F2', rule: 'flex-gap', sel: /button\.btn > span\.lbl/ }, // dot / label / kbd touch in the primary button
  { fid: 'F13', rule: 'overflow', sel: /div\.ov > div\.(toast|badge)/, state: /^overlay\// }, // toast + move badge hang below the 384px window
  { fid: 'F11', rule: 'contrast', sel: /.*/, state: /^overlay\// }, // overlay text over busy/light content (< 3:1 at 60% opacity); see also the F11 worst-case test
  // --- new findings from the first QA run (not in the owner list; fix agents: see docs/QA.md) ---
  { fid: 'N1', rule: 'contrast', sel: /.*/, state: /^main\// }, // --tx3 / --tx2 text on --bg/--panel below 3:1
  { fid: 'N2', rule: 'text-overlap', sel: /div\.hd > h1\.lbl {2}<> {2}button\.btn > span\.lbl/ }, // long call title runs under the header button
  { fid: 'N3', rule: 'text-overlap', sel: /div\.t > small\.lbl {2}<> {2}div\.perm > span\.state/ }, // permissions row: hint overlaps state badge
  { fid: 'N4', rule: 'grid', sel: /div\.apps > button\.app/ }, // onboarding app tiles are 42px high
  { fid: 'N6', rule: 'text-overlap', sel: /div\.ctl > code\.path\.lbl {2}<> {2}button\.btn > span\.lbl/ }, // storage settings: vault path runs under the buttons at ~1100px
  { fid: 'N7', rule: 'overflow', sel: /span\.lbl > kbd\.onprimary/ }, // primary button's kbd chip clipped by its own label at 960px with a long title
];

export function classify(rule: Known['rule'], sel: string, stateKey: string): string | undefined {
  return KNOWN.find((k) => k.rule === rule && k.sel.test(sel) && (!k.state || k.state.test(stateKey)))?.fid;
}
