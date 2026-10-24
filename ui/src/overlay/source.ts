// The overlay talks to the world through one small interface, so the same UI runs against the
// mock demo (vite dev) and the Tauri events (app).
import type { OverlayStore } from './store.svelte';

export interface OverlaySource {
  start(store: OverlayStore): void;
  stop(): void;
  /** Ask bind or header action. */
  ask(): void;
  screenshot(): void;
  /** G7: empty the screenshot queue (header chip). */
  clearQueue?(): void;
  /** Interactive-mode input row submit. */
  submitText(text: string): void;
  setTemplate(id: string): void;
  /** A source chip: open the call / document in the main window. */
  openSource?(s: import('../lib/ipc/bindings').SourceRef): void;
  /** «No API key» link: main window on Settings > Providers. */
  openSettings?(): void;
  /** Pill click / record bind. */
  toggleRecord(): void;
  /** Stop-recording button in the header (`session_stop`). */
  stopRecording?(): void;
  /** Microphone on/off while recording (`session_set_mic`). */
  setMic?(on: boolean): void;
  /** Esc in interactive mode: back to click-through. */
  escape?(): void;
  /** Drag the overlay by its header (interactive mode): pointer deltas in screen points. */
  dragStart?(): void;
  dragTo?(dx: number, dy: number): void;
  dragEnd?(): void;
  /** G2: the pill window is draggable too (same pointer-delta protocol). */
  pillDragStart?(): void;
  pillDragTo?(dx: number, dy: number): void;
  pillDragEnd?(): void;
}
