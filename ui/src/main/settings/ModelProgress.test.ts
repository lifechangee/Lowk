import { describe, expect, it } from 'vitest';
import { progressPercent } from './ModelProgress.svelte';

describe('aggregate model progress', () => {
  it('keeps the aggregate percentage while one file is verifying', () => {
    const aggregatePct = 54;
    expect(progressPercent(aggregatePct)).toBe(54);
  });
});
