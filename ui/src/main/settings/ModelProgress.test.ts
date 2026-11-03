import { describe, expect, it } from 'vitest';
import { STALL_NOTE_MS, isStalled, progressPercent } from './ModelProgress.svelte';

describe('aggregate model progress', () => {
  it('keeps the aggregate percentage while one file is verifying', () => {
    const aggregatePct = 54;
    expect(progressPercent(aggregatePct)).toBe(54);
  });
});

describe('a silent download', () => {
  it('reads as reconnecting once bytes stop moving, not as a falling speed', () => {
    expect(isStalled(552 * 2 ** 20, 1000, 1000 + STALL_NOTE_MS - 1)).toBe(false);
    expect(isStalled(552 * 2 ** 20, 1000, 1000 + STALL_NOTE_MS)).toBe(true);
  });
  it('is not stalled before the first byte (still connecting)', () => {
    expect(isStalled(0, 0, 60_000)).toBe(false);
  });
});
