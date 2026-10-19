import type { PermKind, PermState } from '../lib/ipc';
import { api } from './api';

export const PERM_ORDER: PermKind[] = ['microphone', 'system_audio', 'screen_recording', 'input_monitoring', 'accessibility'];

class Perms {
  state = $state<Record<PermKind, PermState>>({ microphone: 'unknown', system_audio: 'unknown', screen_recording: 'unknown', input_monitoring: 'unknown', accessibility: 'unknown' });
  async refresh(): Promise<void> { this.state = await api.permStatus(); }
  async request(kind: PermKind): Promise<void> {
    this.state[kind] = await api.permRequest(kind);
  }
  /** Ask for every permission that applies and is not granted yet (onboarding step 4). */
  async requestAll(): Promise<void> {
    for (const k of PERM_ORDER) if (this.state[k] !== 'granted' && this.state[k] !== 'not_applicable') await this.request(k);
  }
  open(kind: PermKind): Promise<void> { return api.permOpenSettings(kind); }
  get missing(): boolean { return PERM_ORDER.some((k) => this.state[k] === 'denied' || this.state[k] === 'not_determined'); }
}
export const perms = new Perms();
