import { describe, expect, it } from 'vitest';
import { needsEngineInstall } from './EngineCard.svelte';

describe('EngineCard retry availability', () => {
  it('offers Retry when the primary model installed but a correction asset failed', () => {
    expect(needsEngineInstall(true, 420, true)).toBe(true);
  });

  it('does not offer install when every required asset is present', () => {
    expect(needsEngineInstall(true, 0, false)).toBe(false);
  });
});
