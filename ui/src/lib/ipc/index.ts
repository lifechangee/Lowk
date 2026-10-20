// Typed IPC. `bindings.ts` is generated (`just bindings`); never edit it by hand.
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import type { AppEvent } from './bindings';

export * from './bindings';

/** Tauri event names; mirrors `lowk_types::events`. */
export const EVENTS = {
  sttSegment: 'stt://segment',
  sessionState: 'session://state',
  sessionEnded: 'session://ended',
  audioLevel: 'audio://level',
  overlayMode: 'overlay://mode',
  overlayTone: 'overlay://tone',
  stealthStatus: 'stealth://status',
  queueChanged: 'queue://changed',
  permChanged: 'perm://changed',
  storeChanged: 'store://changed',
  settingsChanged: 'settings://changed',
  sttError: 'stt://error',
  sttLoading: 'stt://loading',
  engineProgress: 'engine://progress',
  myAnswer: 'qa://my-answer',
} as const;

export function on<T>(name: string, handler: (payload: T) => void): Promise<UnlistenFn> {
  return listen<T>(name, (e) => handler(e.payload));
}

export type { AppEvent };
