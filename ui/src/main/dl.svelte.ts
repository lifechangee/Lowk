import type { EngineProgress } from '../lib/ipc';
import { DlTracker, type DlView } from './dl';

/** Reactive wrapper for the model cards: `feed` every progress event of one model, read `view`. */
export class Dl {
  #t = new DlTracker();
  view = $state<DlView>(this.#t.view);
  feed(p: EngineProgress, now: number = Date.now()): void {
    const v = this.#t.feed(p, now);
    if (v) this.view = v;
  }
  reset(): void { this.#t.reset(); this.view = this.#t.view; }
}
