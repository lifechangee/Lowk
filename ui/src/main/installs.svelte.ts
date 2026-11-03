// Where model downloads live. ONE place knows which downloads are running, so every screen that shows a model
// (settings card, onboarding, the «a call cannot start» prompt) tells the same story: a download that is already
// running is never offered again. The old cards kept this in component state, so a card that opened while the
// download ran (the new-call prompt) looked idle and a click started a second writer on the same files.
import type { EngineProgress } from '../lib/ipc';
import { api } from './api';
import { Dl } from './dl.svelte';

class Installs {
  #dls = new Map<string, Dl>();
  #off: (() => void) | null = null;
  #cancelled = new Set<string>();
  #polling = new Set<string>();
  /** Engines whose download is running right now (started here or found running in the backend). */
  active = $state<Record<string, boolean>>({});
  /** The last failure per engine (the backend's reason text), `null`/missing = none. */
  failed = $state<Record<string, string | null>>({});
  /** Bumped whenever a download ends, so the cards re-read the engine status. */
  ended = $state(0);

  /** Starts listening to `engine://progress` (idempotent). Called at app start, so progress is tracked even with no card open. */
  ensure(): void {
    if (this.#off) return;
    this.#off = api.onEngineProgress((p: EngineProgress) => {
      if (p.engine === 'embedder') return;
      this.dl(p.engine).feed(p);
      if (p.phase === 'failed' && !this.#cancelled.has(p.engine)) this.failed[p.engine] = p.message ?? '';
    });
  }

  dl(engine: string): Dl {
    this.ensure();
    let d = this.#dls.get(engine);
    if (!d) this.#dls.set(engine, (d = new Dl()));
    return d;
  }

  /** Starts a download unless one of this engine is already running (then it just follows that one). */
  async start(engine: string): Promise<void> {
    if (this.active[engine]) return;
    this.ensure();
    this.#cancelled.delete(engine);
    this.failed[engine] = null;
    this.dl(engine).reset();
    this.active[engine] = true;
    try {
      await api.engineInstall(engine);
    } catch (e) {
      if (!this.#cancelled.has(engine) && this.failed[engine] == null) this.failed[engine] = e instanceof Error ? e.message : String(e);
    } finally {
      this.active[engine] = false;
      this.ended++;
    }
  }

  cancel(engine: string): void {
    this.#cancelled.add(engine);
    this.failed[engine] = null;
    void api.engineInstallCancel();
  }

  /** A download the backend reports as running that this window did not start (window reloaded, started elsewhere): follow it. */
  adopt(engine: string, p: EngineProgress): void {
    this.dl(engine).feed(p);
    if (this.active[engine] || this.#polling.has(engine)) return;
    this.active[engine] = true;
    this.failed[engine] = null;
    this.#polling.add(engine);
    const tick = async () => {
      const running = (await api.engineStatus().catch(() => [])).find((i) => i.descriptor.id === engine)?.installing;
      if (running) { this.dl(engine).feed(running); setTimeout(() => void tick(), 1500); return; }
      this.#polling.delete(engine);
      this.active[engine] = false;
      this.ended++;
    };
    setTimeout(() => void tick(), 1500);
  }
}

export const installs = new Installs();
