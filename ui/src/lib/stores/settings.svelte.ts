import { commands, type Settings } from '../ipc';

class SettingsStore {
  value = $state<Settings | null>(null);
  async load(): Promise<void> {
    this.value = await commands.settingsGet();
  }
}
export const settings = new SettingsStore();
