// Download progress for the model cards. The backend reports ONE figure for the whole install (done / total
// bytes over every file of the set, total known up front, never going down); this tracker keeps it monotonic
// even if events arrive out of order, repaints at most 4 times a second, and adds a speed averaged over the last
// few seconds (a stall shows as a falling average, not as a flicker) and an ETA.
import type { EngineProgress } from '../lib/ipc';

export interface DlView {
  phase: 'idle' | 'downloading' | 'retrying' | 'repairing' | 'verifying' | 'ready' | 'failed';
  doneBytes: number;
  totalBytes: number;
  /** 0..100, monotonic. */
  pct: number;
  /** Bytes per second (average over the last SPEED_WINDOW_MS); 0 until two samples exist. */
  speed: number;
  /** Seconds left; null while unknown. */
  eta: number | null;
  message: string | null;
}

export const DL_MIN_INTERVAL_MS = 250;
export const SPEED_WINDOW_MS = 8000;
const EMPTY: DlView = { phase: 'idle', doneBytes: 0, totalBytes: 0, pct: 0, speed: 0, eta: null, message: null };

export class DlTracker {
  #done = 0;
  #total = 0;
  #view: DlView = { ...EMPTY };
  #shownAt = -Infinity;
  #samples: { at: number; done: number }[] = [];

  get view(): DlView { return this.#view; }

  reset(): void {
    this.#done = 0;
    this.#total = 0;
    this.#view = { ...EMPTY };
    this.#shownAt = -Infinity;
    this.#samples = [];
  }

  /** Feeds one event; returns the view when the display should repaint, else null. `now` in ms. */
  feed(p: Pick<EngineProgress, 'file' | 'phase' | 'done_bytes' | 'total_bytes' | 'message'>, now: number): DlView | null {
    if (p.phase === 'failed') {
      this.#view = { ...this.#view, phase: 'failed', message: p.message, eta: null, speed: 0 };
      this.#shownAt = now;
      return this.#view;
    }
    this.#done = Math.max(this.#done, p.done_bytes);
    this.#total = Math.max(this.#total, p.total_bytes, this.#done);
    // Transient states are shown at once: the user sees WHY the bar stands still.
    if (p.phase === 'verifying' || p.phase === 'retrying' || p.phase === 'repairing') {
      const phase = p.phase;
      this.#view = { ...this.#paint(now), phase, eta: null, speed: phase === 'verifying' ? this.#view.speed : 0 };
      return this.#view;
    }
    if (p.phase !== 'ready' && now - this.#shownAt < DL_MIN_INTERVAL_MS) return null;
    return this.#paint(now);
  }

  #paint(now: number): DlView {
    const done = this.#done;
    const total = Math.max(this.#total, done);
    this.#samples.push({ at: now, done });
    while (this.#samples.length > 2 && now - this.#samples[0]!.at > SPEED_WINDOW_MS) this.#samples.shift();
    const first = this.#samples[0]!;
    const span = (now - first.at) / 1000;
    const speed = span >= 1 ? Math.max(0, (done - first.done) / span) : this.#view.speed;
    const eta = speed > 1024 && total > done ? (total - done) / speed : null;
    const pct = Math.max(this.#view.pct, total > 0 ? Math.min(100, Math.floor((done / total) * 100)) : 0);
    this.#shownAt = now;
    this.#view = { phase: 'downloading', doneBytes: done, totalBytes: total, pct, speed, eta, message: null };
    return this.#view;
  }
}
