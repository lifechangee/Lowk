<script lang="ts">
  import Button from '../../lib/components/Button.svelte';
  import SettingRow from '../../lib/components/SettingRow.svelte';
  import { api } from '../api';
  import { tm } from '../i18n';
  import { prefs, ui } from '../state.svelte';

  const path = $derived(prefs.value.vault?.path ?? '~/Documents/Lowk');
  async function change() {
    const p = await api.pickVaultDir();
    if (p) { await prefs.patch({ vault: { path: p } }); ui.notify(tm('m.vaultChanged')); }
  }
</script>

<h2>{tm('settings.storage')}</h2>
<SettingRow stack label={tm('m.vaultPath')} hint={tm('m.vaultPathHint')}>
  <code class="path lbl" title={path}>{path}</code>
  <Button onclick={change}>{tm('m.change')}</Button>
  <Button icon="external" onclick={() => api.revealPath(path)}>{tm('m.reveal')}</Button>
</SettingRow>
<SettingRow label={tm('main.unsorted')} hint={tm('m.unsortedDirHint')}><code class="dir lbl">{prefs.value.vault?.unsorted_dir ?? 'Unsorted'}</code></SettingRow>
<SettingRow label={tm('main.trash')} hint={tm('m.trashDirHint')}><code class="dir lbl">.lowk/{prefs.value.vault?.trash_dir ?? 'trash'}</code></SettingRow>
<p class="note">{tm('m.vaultNote')}</p>

<style>
  h2 { font: 600 var(--fs-lg) var(--font); margin: 0 0 var(--s2); }
  .path { flex: 1; font: 12px var(--mono); background: var(--bg); border: 1px solid var(--line); border-radius: var(--r); padding: 0 var(--s3); height: 32px; line-height: 30px; }
  .dir { font: 12px var(--mono); color: var(--tx2); }
  .note { color: var(--tx2); font-size: 12px; margin: var(--s3) 0 0; }
</style>
