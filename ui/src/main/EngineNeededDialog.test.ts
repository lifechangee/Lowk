import { describe, expect, it } from 'vitest';
import { isDownloadActive } from './EngineNeededDialog.svelte';

describe('EngineNeededDialog install state', () => {
  it('uses a live completed state over the opening snapshot', () => {
    expect(isDownloadActive(false, true)).toBe(false);
  });

  it('uses the opening snapshot until live state is known', () => {
    expect(isDownloadActive(undefined, true)).toBe(true);
  });
});
