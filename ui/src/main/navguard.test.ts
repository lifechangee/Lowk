// Unsaved document edits: any navigation away from the document is held back; the same document is not "away".
import { describe, expect, it } from 'vitest';
import { blocksNav } from './router.svelte';

const DOC = '01JC8Z3K6Q9X2M4N7R5T0V0301';

describe('blocksNav', () => {
  it('lets everything through when nothing is guarded or nothing is unsaved', () => {
    expect(blocksNav(null, '#/')).toBe(false);
    expect(blocksNav({ doc: DOC, dirty: () => false }, '#/')).toBe(false);
  });
  it('holds back a page, a call, a folder or settings while the edit is unsaved', () => {
    const g = { doc: DOC, dirty: () => true };
    for (const h of ['#/', '#/call/abc', '#/folder/f1/documents', '#/settings/general', '#/search?q=x', '#/doc/other']) expect(blocksNav(g, h)).toBe(true);
  });
  it('another page of the same document is not leaving it', () => {
    expect(blocksNav({ doc: DOC, dirty: () => true }, `#/doc/${DOC}?p=3`)).toBe(false);
  });
});
