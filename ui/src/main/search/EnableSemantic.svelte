<script lang="ts">
  // One-click «Turn on search by meaning» when the local embedding model is not installed (search page and home).
  // Reuses the embedder install API and its progress events (same as Settings → Documents & search).
  import { onMount } from 'svelte';
  import type { EmbedderInfo, EngineProgress } from '../../lib/ipc';
  import Button from '../../lib/components/Button.svelte';
  import Icon from '../../lib/components/Icon.svelte';
  import { api } from '../api';
  import { errCode, errText } from '../errors';
  import { tm } from '../i18n';
  import { index, prefs } from '../state.svelte';

  let info = $state<EmbedderInfo | null>(null);
  let prog = $state<EngineProgress | null>(null);
  let busy = $state(false);
  let error = $state('');
  let cancelled = false;

  const local = $derived((prefs.value.embedder?.mode ?? 'local') === 'local');
  const missing = $derived(index.status?.semantic.state === 'model_missing');
  const show = $derived(busy || (local && missing && !!info && !info.installed));
  const pct = $derived(prog && prog.total_bytes > 0 ? Math.min(100, Math.round((prog.done_bytes / prog.total_bytes) * 100)) : 0);
  const mb = (b: number) => Math.round(b / 1048576);

  onMount(() => {
    void api.embedderInfo().then((i) => (info = i)).catch(() => undefined);
    return api.onEngineProgress((p) => {
      if (p.engine !== 'embedder') return;
      prog = p;
      if (p.phase === 'failed' && !cancelled) error = tm('m.engine.errChecksum');
    });
  });

  async function install() {
    busy = true; cancelled = false; error = ''; prog = null;
    try { await api.embedderInstall(); }
    catch (e) { if (!cancelled && errCode(e) !== 'ingest_cancelled') error = error || errText(e); }
    busy = false;
    try { info = await api.embedderInfo(); } catch { /* keep the old info */ }
    void index.refresh();
  }
</script>

{#if show}
  <section class="es" data-testid="enable-semantic">
    <span class="ic"><Icon name="sparkle" size={16} /></span>
    <div class="body">
      <p class="lead">{tm('semantic.lead')}</p>
      {#if busy}
        <div class="bar" role="progressbar" aria-valuemin="0" aria-valuemax="100" aria-valuenow={pct} aria-label={tm('m.engine.downloading')}><i style:width="{pct}%"></i></div>
        <div class="acts">
          <span class="msg lbl">{prog?.phase === 'verifying' ? tm('m.engine.verifying') : tm('m.engine.progress', { done: mb(prog?.done_bytes ?? 0), total: mb(prog?.total_bytes ?? 0) })}</span>
          <Button size="sm" onclick={() => { cancelled = true; void api.embedderInstallCancel(); }}>{tm('common.cancel')}</Button>
        </div>
      {:else}
        <div class="acts">
          <Button variant="primary" size="sm" onclick={install}>{tm('semantic.enable', { mb: info?.size_mb ?? 310 })}</Button>
          {#if error}<span class="msg lbl err" role="alert">{error}</span>{/if}
        </div>
        {#if info}<p class="lic">{tm('knowledge.licence.lead', { license: info.license })}</p>{/if}
      {/if}
    </div>
  </section>
{/if}

<style>
  .es { display: flex; gap: var(--s3); align-items: flex-start; margin: var(--s4) 0; padding: var(--s3) var(--s4); border: 1px solid var(--line-strong); border-radius: var(--r-lg); background: var(--panel); }
  .ic { flex: none; color: var(--ac); padding-top: 2px; }
  .body { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: var(--s2); }
  .lead { margin: 0; color: var(--tx2); font-size: 13px; line-height: 20px; }
  .lic { margin: 0; color: var(--tx3); font-size: 12px; line-height: 16px; }
  .bar { height: 8px; border-radius: 99px; background: var(--hv); overflow: hidden; }
  .bar i { display: block; height: 100%; background: var(--ac); }
  .acts { display: flex; gap: var(--s3); align-items: center; justify-content: space-between; min-width: 0; }
  .acts :global(button:first-child:not(:last-child)) { flex: none; }
  .msg { font-size: 12px; color: var(--tx3); }
  .msg.err { color: #ff8f90; }
</style>
