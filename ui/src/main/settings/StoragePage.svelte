<script lang="ts">
  import Button from '../../lib/components/Button.svelte';
  import SettingRow from '../../lib/components/SettingRow.svelte';
  import { api } from '../api';
  import { tm } from '../i18n';
  import { revealText } from '../os';
  import { prefs, ui } from '../state.svelte';
  import { onDestroy, onMount } from 'svelte';
  import type { DataDirInfo } from '../../lib/ipc';
  import TransferCard from './TransferCard.svelte';

  const path = $derived(prefs.value.vault?.path ?? '~/Documents/Lowk');
  async function change() {
    const p = await api.pickVaultDir();
    if (p) { await prefs.patch({ vault: { path: p } }); ui.notify(tm('m.vaultChanged')); }
  }

  // «Папка моделей»: models, engines and logs. A move runs in Rust; we poll its progress.
  let dd = $state<DataDirInfo | null>(null);
  let timer: ReturnType<typeof setInterval> | undefined;
  let wasMoving = false;
  async function refresh() {
    try { dd = await api.dataDirInfo(); } catch { return; }
    if (wasMoving && !dd.moving) {
      if (dd.error) ui.notify(tm('m.dataDirFailed', { error: dd.error }));
      else ui.notify(tm('m.dataDirMoved'));
    }
    wasMoving = !!dd.moving;
  }
  async function moveTo(p: string | null) {
    try { await api.dataDirMove(p); wasMoving = true; await refresh(); }
    catch (e) { ui.notify(tm('m.dataDirFailed', { error: e instanceof Error ? e.message : String(e) })); }
  }
  async function changeData() {
    const p = await api.pickVaultDir();
    if (p) await moveTo(p);
  }
  onMount(() => { void refresh(); timer = setInterval(refresh, 600); });
  onDestroy(() => clearInterval(timer));
</script>

<h2>{tm('settings.storage')}</h2>
<SettingRow stack label={tm('m.vaultPath')} hint={tm('m.vaultPathHint')}>
  <code class="path lbl" title={path}>{path}</code>
  <Button onclick={change}>{tm('m.change')}</Button>
  <Button icon="external" onclick={() => api.revealPath(path)}>{revealText()}</Button>
</SettingRow>
<SettingRow label={tm('main.unsorted')} hint={tm('m.unsortedDirHint')}><code class="dir lbl">{prefs.value.vault?.unsorted_dir ?? 'Unsorted'}</code></SettingRow>
<SettingRow label={tm('main.trash')} hint={tm('m.trashDirHint')}><code class="dir lbl">.lowk/{prefs.value.vault?.trash_dir ?? 'trash'}</code></SettingRow>
<p class="note">{tm('m.vaultNote')}</p>
{#if dd}
  <SettingRow stack label={tm('m.dataDir')} hint={tm('m.dataDirHint', { size: dd.used_mb })}>
    <code class="path lbl" title={dd.path} data-testid="data-dir">{dd.path}</code>
    <Button onclick={changeData} disabled={!!dd.moving}>{tm('m.change')}</Button>
    <Button icon="external" onclick={() => api.revealPath(dd!.path)}>{revealText()}</Button>
    {#if !dd.is_default && !dd.moving}<Button onclick={() => moveTo(null)}>{tm('m.dataDirDefault')}</Button>{/if}
  </SettingRow>
  {#if dd.moving}
    <p class="note" data-testid="data-dir-moving">{tm('m.dataDirMoving', { done: dd.moving.done_mb, total: dd.moving.total_mb })}</p>
    <progress max={Math.max(dd.moving.total_mb, 1)} value={dd.moving.done_mb}></progress>
  {/if}
  {#if dd.leftover}<p class="note">{tm('m.dataDirLeftover', { paths: dd.leftover })}</p>{/if}
{/if}
<SettingRow label={tm('m.logs')} hint={tm('m.logsHint')}>
  <Button icon="external" onclick={() => api.logsReveal().catch(() => ui.notify(tm('m.logsMissing')))}>{tm('m.logsOpen')}</Button>
</SettingRow>

<h2 class="gap">{tm('transfer.title')}</h2>
<TransferCard />

<style>
  h2 { font: 600 var(--fs-lg) var(--font); margin: 0 0 var(--s2); }
  .gap { margin-top: var(--s8); }
  .path { flex: 1; font: 12px var(--mono); background: var(--bg); border: 1px solid var(--line); border-radius: var(--r); padding: 0 var(--s3); height: 32px; line-height: 30px; }
  .dir { font: 12px var(--mono); color: var(--tx2); }
  progress { width: 100%; height: 6px; margin-top: var(--s2); }
  .note { color: var(--tx2); font-size: 12px; margin: var(--s3) 0 0; }
</style>
