// Backend facade. In a Tauri window every command in the generated bindings is real (tauri.ts);
// only deferred features (documents, index search, LLM summary) fall back to the mock.
// In a plain browser (vite dev, tests) everything is the in-memory mock.
import { MockBackend } from './mock';
import { TauriBackend } from './tauri';
import type { Backend } from './types';

export * from './types';

export const hasTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
export const api: Backend = hasTauri ? new TauriBackend() : new MockBackend();
