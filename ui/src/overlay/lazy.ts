// The overlay renders first; the Tauri bridge (typed IPC bindings, event API, bind labels) is a separate chunk
// loaded right after the first paint. Until it arrives, user actions are queued and replayed in order.
import type { OverlayStore } from './store.svelte';
import type { OverlaySource } from './source';
import type { TauriSource } from './tauri';

type M = Exclude<keyof OverlaySource, 'start' | 'stop'>;
const ACTIONS: M[] = ['ask', 'screenshot', 'clearQueue', 'submitText', 'setTemplate', 'openSource', 'openSettings', 'toggleRecord', 'stopRecording', 'setMic', 'escape', 'dragStart', 'dragTo', 'dragEnd', 'pillDragStart', 'pillDragTo', 'pillDragEnd'];

export class LazyTauriSource implements OverlaySource {
  #real: TauriSource | undefined;
  #pending: Array<() => void> = [];
  #want = false;
  #loaded: Promise<void> | undefined;
  ask!: () => void;
  screenshot!: () => void;
  submitText!: (text: string) => void;
  setTemplate!: (id: string) => void;
  toggleRecord!: () => void;

  constructor() {
    for (const m of ACTIONS) {
      (this as unknown as Record<string, unknown>)[m] = (...a: unknown[]) => {
        const call = () => ((this.#real as unknown as Record<string, (...x: unknown[]) => void>)[m]!)(...a);
        if (this.#real) call();
        else this.#pending.push(call);
      };
    }
  }

  start(store: OverlayStore): void {
    this.#want = true;
    this.#loaded ??= import('./tauri').then(({ TauriSource }) => {
      this.#real = new TauriSource();
    });
    void this.#loaded.then(() => {
      if (!this.#want || !this.#real) return;
      this.#real.start(store);
      const p = this.#pending;
      this.#pending = [];
      p.forEach((f) => f());
    });
  }

  stop(): void {
    this.#want = false;
    this.#real?.stop();
  }
}
