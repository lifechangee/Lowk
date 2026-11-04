import { describe, expect, it, vi } from 'vitest';
import { completeEngineNeeded, isDownloadActive, isEngineReady } from './EngineNeededDialog.svelte';

describe('EngineNeededDialog install state', () => {
  it('uses a live completed state over the opening snapshot', () => {
    expect(isDownloadActive(false, true)).toBe(false);
  });

  it('uses the opening snapshot until live state is known', () => {
    expect(isDownloadActive(undefined, true)).toBe(true);
  });

  it('closes as ready only when the selected local engine is fully installed', () => {
    expect(isEngineReady({ installed: true, runtime_missing: false, extra_mb: 0 })).toBe(true);
    expect(isEngineReady({ installed: true, runtime_missing: false, extra_mb: 20 })).toBe(false);
    expect(isEngineReady({ installed: false, runtime_missing: false })).toBe(false);
    expect(isEngineReady(undefined)).toBe(false);
  });

  it('closes the missing-engine dialog and offers the ready toast action', () => {
    const close = vi.fn();
    const toast = vi.fn();
    expect(completeEngineNeeded(true, close, toast)).toBe(true);
    expect(close).toHaveBeenCalledOnce();
    expect(toast).toHaveBeenCalledOnce();
    expect(completeEngineNeeded(false, close, toast)).toBe(false);
    expect(close).toHaveBeenCalledOnce();
    expect(toast).toHaveBeenCalledOnce();
  });
});
