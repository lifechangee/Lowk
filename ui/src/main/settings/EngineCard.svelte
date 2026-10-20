<script lang="ts">
  import { onMount } from 'svelte';
  import type { EngineInfo, EngineProgress } from '../../lib/ipc';
  import Button from '../../lib/components/Button.svelte';
  import Icon from '../../lib/components/Icon.svelte';
  import { api } from '../api';
  import { tm } from '../i18n';

  let { engine }: { engine: string } = $props();

  let info = $state<EngineInfo | null>(null);
  let busy = $state(false);
  let prog = $state<EngineProgress | null>(null);
  let error = $state<string | null>(null);
  let cancelled = false;

  const refresh = async () => { info = (await api.engineStatus()).find((i) => i.descriptor.id === engine) ?? null; };
  onMount(() => {
    void refresh();
    return api.onEngineProgress((p) => {
      if (p.engine !== engine) return;
      prog = p;
      if (p.phase === 'failed' && !cancelled) error = p.message ?? '';
    });
  });
  $effect(() => { engine; prog = null; error = null; void refresh(); });

  const mb = (b: number) => Math.round(b / 1048576);
  const pct = $derived(prog && prog.total_bytes > 0 ? Math.min(100, Math.round((prog.done_bytes / prog.total_bytes) * 100)) : 0);
  const reason = (m: string) => (m.includes('checksum') ? tm('m.engine.errChecksum') : m.includes('network') || m.includes('Network') ? tm('m.engine.errNetwork') : tm('m.engine.errGeneric'));

  async function install() {
    busy = true;
    cancelled = false;
    error = null;
    prog = null;
    try {
      await api.engineInstall(engine);
    } catch (e) {
      if (!error && !cancelled) error = e instanceof Error ? e.message : String(e);
      if (cancelled) error = null;
    }
    busy = false;
    await refresh();
  }
</script>

{#if info && info.descriptor.kind === 'local'}
  <div class="card" data-testid="engine-card">
    <div class="top">
      <b class="lbl">{tm('m.engine.title', { name: info.descriptor.name })}</b>
      <span class="pill lbl" class:on={info.installed}>{#if info.installed}<Icon name="check" size={12} />{tm('m.engine.installed')}{:else}{tm('m.engine.notInstalled')}{/if}</span>
    </div>
    {#if busy}
      <div class="bar" role="progressbar" aria-valuemin="0" aria-valuemax="100" aria-valuenow={pct} aria-label={tm('m.engine.downloading')}><i style:width="{pct}%"></i></div>
      <div class="acts">
        <span class="msg lbl">{prog?.phase === 'verifying' ? tm('m.engine.verifying') : tm('m.engine.progress', { done: mb(prog?.done_bytes ?? 0), total: mb(prog?.total_bytes ?? 0) })}</span>
        <Button size="sm" onclick={() => { cancelled = true; void api.engineInstallCancel(); }}>{tm('common.cancel')}</Button>
      </div>
    {:else if !info.installed}
      <p class="note">{tm('m.engine.needsDownload', { size: (info.descriptor.size_mb / 1000).toFixed(1) })}</p>
      <div class="acts">
        <Button variant="primary" size="sm" onclick={install}>{error ? tm('m.engine.retry') : tm('m.engine.install', { size: (info.descriptor.size_mb / 1000).toFixed(1) })}</Button>
        {#if error !== null}<span class="msg lbl err" role="alert">{reason(error)}</span>{/if}
      </div>
      <p class="note">{tm('m.engine.license', { license: info.descriptor.license })} · {info.license_url}</p>
    {:else if info.runtime_missing}
      <p class="note msg err" role="alert" data-testid="engine-runtime-missing">{tm('m.engine.runtimeMissing')}</p>
    {:else}
      <p class="note">{tm('m.engine.ready')}</p>
    {/if}
  </div>
{/if}

<style>
  .card { border: 1px solid var(--line); background: var(--panel); border-radius: var(--r-lg); padding: var(--s4); margin: var(--s3) 0; display: flex; flex-direction: column; gap: var(--s3); }
  .top { display: flex; justify-content: space-between; align-items: center; gap: var(--s3); min-width: 0; }
  .pill { display: inline-flex; align-items: center; gap: var(--s1); font-size: 12px; padding: 0 var(--s2); height: 20px; border-radius: 99px; background: var(--hv); color: var(--tx3); max-width: 60%; }
  .pill.on { background: var(--acbg); color: var(--ac); }
  .bar { height: 8px; border-radius: 99px; background: var(--hv); overflow: hidden; }
  .bar i { display: block; height: 100%; background: var(--ac); }
  .acts { display: flex; gap: var(--s2); align-items: center; justify-content: space-between; min-width: 0; }
  .msg { font-size: 12px; color: var(--tx3); }
  .msg.err { color: var(--warn); }
  .note { margin: 0; color: var(--tx3); font-size: 12px; line-height: 16px; overflow-wrap: anywhere; }
</style>
