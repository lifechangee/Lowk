// Update state of the main window: the backend owns it, this store polls the cheap `update_status` command.
import type { UpdateStatus } from '../lib/ipc';
import { api } from './api';

const IDLE_MS = 60_000;
const BUSY_MS = 700;

class Updates {
  status = $state<UpdateStatus | null>(null);
  /** «Позже»: the notice for this version stays hidden until the app restarts. */
  dismissed = $state<string | null>(null);
  checking = $state(false);
  #timer: ReturnType<typeof setTimeout> | undefined;
  #on = false;

  start(): () => void {
    if (this.#on) return () => {};
    this.#on = true;
    void this.#tick();
    return () => { this.#on = false; clearTimeout(this.#timer); };
  }

  async #tick(): Promise<void> {
    if (!this.#on) return;
    try { this.status = await api.updateStatus(); } catch { /* the next tick retries */ }
    const busy = this.status?.phase === 'downloading' || this.status?.phase === 'checking';
    this.#timer = setTimeout(() => void this.#tick(), busy ? BUSY_MS : IDLE_MS);
  }

  async #run(f: () => Promise<UpdateStatus>): Promise<void> {
    try { this.status = await f(); } catch { /* shown through the next status */ }
    clearTimeout(this.#timer);
    void this.#tick();
  }

  async check(): Promise<void> {
    this.checking = true;
    this.dismissed = null;
    try { await this.#run(() => api.updateCheck(true)); } finally { this.checking = false; }
  }
  install = () => this.#run(() => api.updateInstall());
  restart = () => this.#run(() => api.updateRestart());
  skip = (v: string) => this.#run(() => api.updateSkip(v));
  later(v: string): void { this.dismissed = v; }
}

export const updates = new Updates();
