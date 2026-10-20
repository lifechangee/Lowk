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
  { fid: 'F13', rule: 'overflow', sel: /div\.ov > div\.(toast|badge)/, state: /^overlay\// }, // toast + move badge hang below the 384px window
  { fid: 'F11', rule: 'contrast', sel: /.*/, state: /^overlay\// }, // overlay text over busy/light content (< 3:1 at 60% opacity); see also the F11 worst-case test
];

export function classify(rule: Known['rule'], sel: string, stateKey: string): string | undefined {
  return KNOWN.find((k) => k.rule === rule && k.sel.test(sel) && (!k.state || k.state.test(stateKey)))?.fid;
}
