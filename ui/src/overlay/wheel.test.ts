import { describe, expect, it } from 'vitest';
import { wheelPixels } from './wheel';

describe('wheelPixels', () => {
  it('handles pixel, line and page delta modes', () => {
    expect(wheelPixels(100, 0, 300)).toBe(100);
    expect(wheelPixels(3, 1, 300)).toBe(48);
    expect(wheelPixels(-1, 2, 300)).toBe(-300);
  });
});
