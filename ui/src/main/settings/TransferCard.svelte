<script lang="ts">
  // Export / import: the whole vault and single folders as zip. Import never overwrites (ids are reassigned on collision).
  import type { ImportSummary } from '../../lib/ipc';
  import Button from '../../lib/components/Button.svelte';
  import Icon from '../../lib/components/Icon.svelte';
  import SettingRow from '../../lib/components/SettingRow.svelte';
  import { api } from '../api';
  import { errText } from '../errors';
  import { tm } from '../i18n';
  import { library } from '../state.svelte';

  type Result = { ok: true; text: string; summary?: ImportSummary } | { ok: false; text: string };
  let busy = $state<'' | 'export' | 'import' | 'folder'>('');
  let result = $state<Result | null>(null);

  async function exportAll() {
    busy = 'export'; result = null;
    try {
      const dest = await api.pickSavePath('Lowk-vault.zip');
      if (!dest) { busy = ''; return; }
      const n = await api.exportVault(dest);
      result = { ok: true, text: tm('transfer.exported', { n, path: dest }) };
    } catch (e) { result = { ok: false, text: errText(e) }; }
    busy = '';
  }
  async function importAll(kind: 'import' | 'folder') {
    busy = kind; result = null;
    try {
      const src = await api.pickOpenPath('zip');
      if (!src) { busy = ''; return; }
      const summary = kind === 'import' ? await api.importVault(src) : await api.folderImport(src);
      await library.reload();
      result = { ok: true, text: tm('transfer.imported', { n: summary.files_copied }), summary };
    } catch (e) { result = { ok: false, text: errText(e) }; }
    busy = '';
  }
</script>

<div data-testid="transfer-card">
  <SettingRow label={tm('transfer.exportVault')} hint={tm('transfer.exportVaultHint')}>
    <Button icon="download" disabled={busy !== ''} onclick={exportAll}>{busy === 'export' ? tm('common.loading') : tm('transfer.exportBtn')}</Button>
  </SettingRow>
  <SettingRow label={tm('transfer.importVault')} hint={tm('transfer.importHint')}>
    <Button icon="upload" disabled={busy !== ''} onclick={() => importAll('import')}>{busy === 'import' ? tm('common.loading') : tm('transfer.importBtn')}</Button>
  </SettingRow>
  <SettingRow label={tm('transfer.importFolder')} hint={tm('transfer.importFolderHint')}>
    <Button icon="upload" disabled={busy !== ''} onclick={() => importAll('folder')}>{busy === 'folder' ? tm('common.loading') : tm('transfer.importBtn')}</Button>
  </SettingRow>
  {#if result}
    <div class="res" class:bad={!result.ok} role={result.ok ? 'status' : 'alert'} data-testid="transfer-result">
      <Icon name={result.ok ? 'check' : 'alert'} size={14} />
      <div>
        <p class="lbl2">{result.text}</p>
        {#if result.ok && result.summary}
          <ul>
            <li>{tm('transfer.sum.copied', { n: result.summary.files_copied })}</li>
            <li>{tm('transfer.sum.skipped', { n: result.summary.skipped })}</li>
            <li>{tm('transfer.sum.ids', { n: result.summary.ids_reassigned })}</li>
            <li>{tm('transfer.sum.merged', { n: result.summary.folders_merged })}</li>
          </ul>
        {/if}
      </div>
    </div>
  {/if}
</div>

<style>
  .res { display: flex; gap: var(--s3); align-items: flex-start; margin-top: var(--s3); padding: var(--s3) var(--s4); border: 1px solid var(--line); background: var(--panel); border-radius: var(--r-lg); font-size: 13px; }
  .res :global(svg) { margin-top: 3px; color: var(--ac); flex: none; }
  .res.bad { border-color: rgba(255, 77, 79, 0.4); color: #ff8f90; }
  .res.bad :global(svg) { color: #ff8f90; }
  .lbl2 { margin: 0; overflow-wrap: anywhere; }
  ul { margin: var(--s1) 0 0; padding-left: var(--s4); color: var(--tx2); font-size: 12px; line-height: 16px; }
</style>
