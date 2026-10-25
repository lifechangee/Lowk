<script lang="ts">
  // Search by meaning: the embedder (local EmbeddingGemma by default, cloud as an option, or off = keyword only).
  // The licence is shown BEFORE the download; model change = background rebuild while keyword search keeps working.
  import { onMount } from 'svelte';
  import type { EmbedderInfo, EmbedderMode } from '../../lib/ipc';
  import Button from '../../lib/components/Button.svelte';
  import Icon from '../../lib/components/Icon.svelte';
  import OptionCard from '../../lib/components/OptionCard.svelte';
  import { api } from '../api';
  import { Dl } from '../dl.svelte';
  import { fmtTokens } from '../docs/util';
  import { appErrorText, errCode, errText } from '../errors';
  import { tm } from '../i18n';
  import { hrefs } from '../router.svelte';
  import { index, prefs, ui } from '../state.svelte';
  import ModelChip, { type ModelState } from './ModelChip.svelte';
  import ModelProgress from './ModelProgress.svelte';

  let info = $state<EmbedderInfo | null>(null);
  let keyPresent = $state(false);
  const dl = new Dl();
  let busy = $state(false);
  let error = $state('');
  let cancelled = false;

  const mode = $derived((prefs.value.embedder?.mode ?? 'local') as EmbedderMode);
  const st = $derived(index.status);
  const sem = $derived(st?.semantic ?? null);
  const mst = $derived<ModelState>(busy ? (dl.view.phase === 'verifying' ? 'verifying' : 'downloading') : error ? 'error' : info?.installed ? 'ready' : 'missing');

  async function refresh() {
    try { info = await api.embedderInfo(); } catch (e) { error = errText(e); }
    void index.refresh();
  }
  onMount(() => {
    void refresh();
    void api.secretStatus('openrouter').then((v) => (keyPresent = v));
    const off = api.onEngineProgress((p) => {
      if (p.engine !== 'embedder') return;
      dl.feed(p);
      if (p.phase === 'failed' && !cancelled) error = p.message?.includes('checksum') ? tm('m.engine.errChecksum') : tm('m.engine.errGeneric');
    });
    const offIdx = api.onIndexStatus((s) => { index.status = s; });
    return () => { off(); offIdx(); };
  });

  async function setMode(m: EmbedderMode) { await prefs.patch({ embedder: { mode: m } }); void index.refresh(); }
  async function install() {
    busy = true; cancelled = false; error = ''; dl.reset();
    try { await api.embedderInstall(); }
    catch (e) { if (!cancelled && errCode(e) !== 'ingest_cancelled') error = error || errText(e); }
    busy = false;
    await refresh();
  }
  async function rebuild() {
    try { await api.indexRebuild({ kind: 'all' }); ui.notify(tm('knowledge.rebuilding')); } catch (e) { ui.notify(errText(e)); }
  }
  const openLicence = (e: Event) => { e.preventDefault(); if (info) window.open(info.license_url, '_blank', 'noopener'); };
</script>

<div class="card" data-testid="embedder-card">
  <div class="top">
    <b class="lbl">{tm('knowledge.embedder.title')}</b>
    <span class="pill" class:on={sem?.state === 'ready'}>
      {#if sem?.state === 'ready'}<Icon name="check" size={12} />{tm('knowledge.state.ready')}
      {:else if sem?.state === 'building'}{tm('knowledge.state.building')}
      {:else if sem?.state === 'off'}{tm('knowledge.state.off')}
      {:else if sem?.state === 'failed'}{tm('knowledge.state.failed')}
      {:else}{tm('knowledge.state.missing')}{/if}
    </span>
  </div>
  <p class="lead">{tm('knowledge.embedder.lead')}</p>

  <div class="opts" role="radiogroup" aria-label={tm('knowledge.embedder.title')}>
    <OptionCard selected={mode === 'local'} title={tm('knowledge.mode.local')} hint={tm('knowledge.mode.localHint')} badge={tm('knowledge.mode.recommended')} onselect={() => setMode('local')} />
    <OptionCard selected={mode === 'cloud'} title={tm('knowledge.mode.cloud')} hint={tm('knowledge.mode.cloudHint')} onselect={() => setMode('cloud')} />
    <OptionCard selected={mode === 'off'} title={tm('knowledge.mode.off')} hint={tm('knowledge.mode.offHint')} onselect={() => setMode('off')} />
  </div>

  {#if mode === 'local' && info}
    <div class="model" data-testid="embedder-model">
      <div class="mt"><b class="name">{info.name}</b><ModelChip state={mst} /></div>
      <ModelProgress state={mst} view={dl.view} reason={error} sizeMb={!info.installed ? info.size_mb : 0} />
      {#if busy}
        <div class="acts">
          <span></span>
          <Button size="sm" onclick={() => { cancelled = true; void api.embedderInstallCancel(); }}>{tm('common.cancel')}</Button>
        </div>
      {:else if !info.installed}
        <p class="lic">{tm('knowledge.licence.lead', { license: info.license })} <a href={info.license_url} onclick={openLicence}>{tm('knowledge.licence.read')}</a></p>
        <div class="acts">
          <Button variant="primary" size="sm" onclick={install}>{error ? tm('m.engine.retry') : tm('knowledge.install', { mb: info.size_mb })}</Button>
        </div>
      {:else}
        <p class="note"><Icon name="check" size={12} /> {tm('knowledge.model.installed', { license: info.license })}</p>
      {/if}
    </div>
  {:else if mode === 'cloud'}
    <div class="model">
      <p class="note">{tm('knowledge.cloud.note', { model: prefs.value.embedder?.cloud_model ?? '' })}</p>
      {#if !keyPresent}<p class="note warn" data-testid="embedder-nokey"><Icon name="key" size={12} /> {tm('summary.noKey')} <a href={hrefs.settings('providers')}>{tm('summary.openSettings')}</a></p>{/if}
    </div>
  {:else if mode === 'off'}
    <div class="model"><p class="note">{tm('knowledge.off.note')}</p></div>
  {/if}

  {#if mode !== 'off'}
    <div class="idx" data-testid="index-status">
      {#if sem?.state === 'building'}
        <div class="bar" role="progressbar" aria-valuemin="0" aria-valuemax={sem.total} aria-valuenow={sem.done} aria-label={tm('knowledge.state.building')}><i style:width="{Math.round((sem.done / Math.max(1, sem.total)) * 100)}%"></i></div>
        <span class="msg lbl">{tm('knowledge.building', { done: fmtTokens(sem.done), total: fmtTokens(sem.total) })}</span>
      {:else if sem?.state === 'failed'}
        <p class="note err" role="alert"><Icon name="alert" size={12} /> {appErrorText(sem.error)}</p>
      {/if}
      {#if st}<span class="msg lbl">{tm('knowledge.index', { chunks: fmtTokens(st.chunks), files: fmtTokens(st.files) })}{#if st.pending} · {tm('knowledge.pending', { n: st.pending })}{/if}</span>{/if}
      <Button size="sm" icon="refresh" onclick={rebuild}>{tm('knowledge.rebuild')}</Button>
    </div>
  {/if}
</div>

<style>
  .card { border: 1px solid var(--line); background: var(--panel); border-radius: var(--r-lg); padding: var(--s4); margin: var(--s3) 0; display: flex; flex-direction: column; gap: var(--s3); }
  .top { display: flex; justify-content: space-between; align-items: center; gap: var(--s3); min-width: 0; }
  .pill { display: inline-flex; align-items: center; justify-content: center; gap: var(--s1); flex: none; box-sizing: border-box; font-size: 12px; line-height: 20px; padding: 0 var(--s2); height: 20px; border-radius: 99px; background: var(--hv); color: var(--tx3); white-space: nowrap; }
  .pill.on { background: var(--acbg); color: var(--ac); }
  .lead { margin: 0; color: var(--tx2); font-size: 13px; }
  .opts :global(.opt) { margin-bottom: var(--s2); }
  .model { border: 1px solid var(--line); background: var(--bg); border-radius: var(--r); padding: var(--s3) var(--s4); display: flex; flex-direction: column; gap: var(--s2); }
  .mt { display: flex; justify-content: space-between; gap: var(--s3); align-items: center; min-width: 0; }
  .name { min-width: 0; overflow-wrap: anywhere; }
  .bar { height: 8px; border-radius: 99px; background: var(--hv); overflow: hidden; }
  .bar i { display: block; height: 100%; background: var(--ac); }
  .acts { display: flex; gap: var(--s2); align-items: center; justify-content: space-between; min-width: 0; }
  .msg { font-size: 12px; color: var(--tx3); }
  .note.err { color: #ff8f90; }
  .note, .lic { margin: 0; color: var(--tx3); font-size: 12px; line-height: 16px; overflow-wrap: anywhere; }
  .lic { color: var(--tx2); }
  .note.warn { color: var(--tx2); }
  a { color: var(--ac); }
  .idx { display: flex; flex-direction: column; gap: var(--s2); align-items: flex-start; }
  .idx .bar { width: 100%; }
</style>
