import type { QaUpdate } from '../lib/ipc/bindings';

export type MyAnswerView = { title: 'before' | 'listening' | 'paused' | 'limit' | 'closed'; body: 'hotkey' | 'text' | 'paused' | 'limit' | 'empty'; elapsed: string; dimmed: boolean; dot: boolean };

export function myAnswerView(state: QaUpdate['state'], elapsedMs: number): MyAnswerView {
  const seconds = Math.floor(Math.max(0, elapsedMs) / 1000);
  const elapsed = `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, '0')}`;
  switch (state) {
    case 'listening': return { title: 'listening', body: 'text', elapsed, dimmed: false, dot: true };
    case 'paused': return { title: 'paused', body: 'paused', elapsed: '', dimmed: false, dot: false };
    case 'limit': return { title: 'limit', body: 'limit', elapsed: '', dimmed: false, dot: false };
    case 'closed': return { title: 'closed', body: 'text', elapsed: '', dimmed: true, dot: false };
    default: return { title: 'before', body: 'hotkey', elapsed: '', dimmed: false, dot: false };
  }
}
