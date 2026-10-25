import type { SelfTestReport, StealthStatus } from '../lib/ipc';
import { api } from './api';

/** green = the OS reports the overlay as excluded from capture; red = anything else (degraded, unavailable, or a failed self-test). */
export type StealthLevel = 'safe' | 'risk';

export function stealthLevel(s: StealthStatus | null, report: SelfTestReport | null = null): StealthLevel {
  if (!s || s.state !== 'verified') return 'risk';
  if (report && !report.overlay_hidden) return 'risk';
  return 'safe';
}

/** Overlay-protection state for the main window: read on start, kept live by `stealth://status` and window focus (display changes and overlay show re-verify in the backend and arrive as events). */
class Stealth {
  status = $state<StealthStatus | null>(null);
  report = $state<SelfTestReport | null>(null);
  busy = $state(false);
  error = $state(false);
  #off: (() => void) | null = null;

  level = $derived(stealthLevel(this.status, this.report));

  async refresh(): Promise<void> {
    try { this.status = await api.stealthStatus(); } catch { this.status = { state: 'unavailable', reason: 'status' }; }
  }
  /** The pixel-level self-test: paints a sentinel in the overlay and grabs the screen like a capturer. */
  async check(): Promise<void> {
    this.busy = true;
    this.error = false;
    try {
      this.report = await api.stealthSelftest();
    } catch {
      this.report = null;
      this.error = true;
    }
    await this.refresh();
    this.busy = false;
  }
  start(): () => void {
    void this.refresh();
    this.#off?.();
    const off = api.onStealthStatus((s) => { this.status = s; this.report = null; });
    const onFocus = () => void this.refresh();
    window.addEventListener('focus', onFocus);
    this.#off = () => { off(); window.removeEventListener('focus', onFocus); };
    return () => { this.#off?.(); this.#off = null; };
  }
}
export const stealth = new Stealth();
