// Backend facade. In a Tauri window, the commands that exist in the generated bindings are
// real; everything else is served by the in-memory mock until WP6 adds the commands
// (library_*, call_*, session_*, perm_*, secret_*, hw_*). In a plain browser all of it is mock.
import { commands } from '../../lib/ipc';
import type { Settings } from '../../lib/ipc';
import { MockBackend } from './mock';
import type { Backend } from './types';

export * from './types';

class TauriBackend extends MockBackend {
  override readonly isMock: boolean = false;
  override async localeGet(): Promise<string> { return commands.localeGet(); }
  override async settingsGet(): Promise<Settings> {
    const s = await commands.settingsGet();
    // Keep the mock's copy in sync so mock-only commands (bind report) see real binds.
    await super.settingsPatch(s);
    return s;
  }
  override async settingsPatch(patch: unknown): Promise<Settings> {
    const r = await commands.settingsPatch(patch);
    if (r.status === 'error') throw new Error(r.error.code);
    await super.settingsPatch(patch);
    return r.data;
  }
}

export const hasTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
export const api: Backend = hasTauri ? new TauriBackend() : new MockBackend();
