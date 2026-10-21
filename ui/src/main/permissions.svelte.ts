import type { PermKind, PermState } from '../lib/ipc';
import { api } from './api';

export const PERM_ORDER: PermKind[] = ['microphone', 'system_audio', 'screen_recording', 'input_monitoring', 'accessibility'];

/** macOS reads these three once per process: a grant made while Lowk runs is applied only after a relaunch. */
const RESTART_KINDS: PermKind[] = ['screen_recording', 'input_monitoring', 'accessibility'];

class Perms {
  state = $state<Record<PermKind, PermState>>({ microphone: 'unknown', system_audio: 'unknown', screen_recording: 'unknown', input_monitoring: 'unknown', accessibility: 'unknown' });
  /** Kinds the user asked for / opened Settings for during this run; if one is still not granted after the window got focus back, it needs a relaunch. */
  #asked = $state<Set<PermKind>>(new Set());
  #returned = $state<Set<PermKind>>(new Set());
  #timer: ReturnType<typeof setInterval> | null = null;

  /** `fromFocus`: the window just got focus back (the user may be returning from System Settings). */
  async refresh(fromFocus = false): Promise<void> {
    this.state = await api.permStatus();
    if (fromFocus) this.#returned = new Set(this.#asked);
    for (const k of PERM_ORDER) if (this.state[k] === 'granted') { this.#asked.delete(k); this.#returned.delete(k); }
  }
  /** Live status while a permissions screen is open: focus events plus a light poll. */
  watch(): () => void {
    void this.refresh();
    this.#timer = setInterval(() => void this.refresh(), 2000);
    const onFocus = () => void this.refresh(true);
    window.addEventListener('focus', onFocus);
    return () => {
      if (this.#timer) clearInterval(this.#timer);
      window.removeEventListener('focus', onFocus);
    };
  }
  async request(kind: PermKind): Promise<void> {
    this.#asked = new Set(this.#asked).add(kind);
    this.state[kind] = await api.permRequest(kind);
    if (this.state[kind] === 'granted') this.#asked.delete(kind);
  }
  /** Ask for every permission that applies and is not granted yet (onboarding step 4). */
  async requestAll(): Promise<void> {
    for (const k of PERM_ORDER) if (this.state[k] !== 'granted' && this.state[k] !== 'not_applicable') await this.request(k);
  }
  open(kind: PermKind): Promise<void> {
    this.#asked = new Set(this.#asked).add(kind);
    return api.permOpenSettings(kind);
  }
  /** «Перезапустите Lowk, чтобы применить»: asked, came back to the window, still not granted. */
  needsRestart(kind: PermKind): boolean {
    return RESTART_KINDS.includes(kind) && this.#returned.has(kind) && this.state[kind] !== 'granted';
  }
  get anyNeedsRestart(): boolean { return RESTART_KINDS.some((k) => this.needsRestart(k)); }
  relaunch(): Promise<void> { return api.appRelaunch(); }
  get missing(): boolean { return PERM_ORDER.some((k) => this.state[k] === 'denied' || this.state[k] === 'not_determined'); }
}
export const perms = new Perms();
