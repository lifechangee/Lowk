import { describe, expect, it } from 'vitest';
import { stealthLevel } from './stealth.svelte';

const ok = { overlay_hidden: true, main_visible: false, details: '' };

describe('stealthLevel', () => {
  it('is safe only when the OS flag is verified and the screen-grab (if it ran) did not see the overlay', () => {
    expect(stealthLevel({ state: 'verified' })).toBe('safe');
    expect(stealthLevel({ state: 'verified' }, ok)).toBe('safe');
  });
  it('is risk when the flag is degraded, unavailable, unknown, or the screen-grab saw the overlay', () => {
    expect(stealthLevel({ state: 'degraded', reason: 'x' })).toBe('risk');
    expect(stealthLevel({ state: 'unavailable', reason: 'x' })).toBe('risk');
    expect(stealthLevel(null)).toBe('risk');
    expect(stealthLevel({ state: 'verified' }, { ...ok, overlay_hidden: false })).toBe('risk');
  });
});
