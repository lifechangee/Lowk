// Download progress for the model cards. The backend reports per-file byte counts and may emit them out of
// order (several files, retries after a hiccup): the shown figure is the sum of every file's highest count so
// far, so it never goes backwards, is repainted at most 4 times a second, and carries a smoothed speed and ETA.
import type { EngineProgress } from '../lib/ipc';

export interface DlView {
  phase: 'idle' | 'downloading' | 'verifying' | 'ready' | 'failed';
  doneBytes: number;
  totalBytes: number;
  /** 0..100, monotonic. */
  pct: number;
  /** Bytes per second (smoothed); 0 until two samples exist. */
  speed: number;
  /** Seconds left; null while unknown. */
  eta: number | null;
  message: string | null;
}

export const DL_MIN_INTERVAL_MS = 250;
const EMPTY: DlView = { phase: 'idle', doneBytes: 0, totalBytes: 0, pct: 0, speed: 0, eta: null, message: null };

export class DlTracker {
  #files = new Map<string, { done: number; total: number }>();
  #view: DlView = { ...EMPTY };
  #shownAt = -Infinity;
  #prevDone = 0;
  #prevAt = 0;
  #speed = 0;

  get view(): DlView { return this.#view; }

  reset(): void {
    this.#files.clear();
    this.#view = { ...EMPTY };
    this.#shownAt = -Infinity;
    this.#prevDone = 0;
    this.#prevAt = 0;
    this.#speed = 0;
  }

  /**
   * Feeds one event; returns the view when the display should repaint, else null. `now` in ms.
   * `ready` is per file (the model is ready when the install call returns), so it only completes that file.
   */
  feed(p: Pick<EngineProgress, 'file' | 'phase' | 'done_bytes' | 'total_bytes' | 'message'>, now: number): DlView | null {
    if (p.phase === 'failed') {
      this.#view = { ...this.#view, phase: 'failed', message: p.message, eta: null, speed: 0 };
      this.#shownAt = now;
      return this.#view;
    }
    this.#touch(p);
    if (p.phase === 'verifying') {
      this.#view = { ...this.#paint(now), phase: 'verifying', eta: null };
      return this.#view;
    }
    if (p.phase !== 'ready' && now - this.#shownAt < DL_MIN_INTERVAL_MS) return null;
    return this.#paint(now);
  }

  #touch(p: Pick<EngineProgress, 'file' | 'done_bytes' | 'total_bytes'>): void {
    const f = this.#files.get(p.file) ?? { done: 0, total: 0 };
    f.done = Math.max(f.done, p.done_bytes);
    f.total = Math.max(f.total, p.total_bytes);
    this.#files.set(p.file, f);
  }

  #paint(now: number): DlView {
    let done = 0;
    let total = 0;
    for (const f of this.#files.values()) { done += Math.min(f.done, f.total || f.done); total += Math.max(f.total, f.done); }
    done = Math.max(done, this.#view.doneBytes);
    total = Math.max(total, done);
    if (this.#prevAt > 0 && now > this.#prevAt) {
      const inst = Math.max(0, (done - this.#prevDone) / ((now - this.#prevAt) / 1000));
      this.#speed = this.#speed > 0 ? this.#speed * 0.7 + inst * 0.3 : inst;
    }
    this.#prevDone = done;
    this.#prevAt = now;
    const eta = this.#speed > 1024 && total > done ? (total - done) / this.#speed : null;
    const pct = Math.max(this.#view.pct, total > 0 ? Math.min(100, Math.floor((done / total) * 100)) : 0);
    this.#shownAt = now;
    this.#view = { phase: 'downloading', doneBytes: done, totalBytes: total, pct, speed: this.#speed, eta, message: null };
    return this.#view;
  }
}
