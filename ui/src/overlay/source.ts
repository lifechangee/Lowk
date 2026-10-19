// The overlay talks to the world through one small interface, so the same UI runs against the
// mock demo (vite dev) and the Tauri events (app). Real commands arrive with typed IPC (WP6/WP7).
import type { OverlayStore } from './store.svelte';

export interface OverlaySource {
  start(store: OverlayStore): void;
  stop(): void;
  /** Ask bind or header action. */
  ask(): void;
  screenshot(): void;
  /** Interactive-mode input row submit. */
  submitText(text: string): void;
  setTemplate(id: string): void;
  /** Pill click / record bind. */
  toggleRecord(): void;
}
