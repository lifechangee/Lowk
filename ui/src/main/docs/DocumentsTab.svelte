<script lang="ts">
  // Folder tab «Документы»: import (button, drag-and-drop), status per file with progress, warnings and errors,
  // retry / cancel, star = pinned (always sent to the model) with the budget meter, slow-file warning.
  import { onMount } from 'svelte';
  import type { Document, IngestProgress, PinnedUsage } from '../../lib/ipc';
  import Button from '../../lib/components/Button.svelte';
  import Icon from '../../lib/components/Icon.svelte';
  import Modal from '../../lib/components/Modal.svelte';
  import { tPlural } from '../../lib/i18n/locale.svelte';
  import { api, folderKey, type PickedFile } from '../api';
  import { errText } from '../errors';
  import { tm } from '../i18n';
  import { library, prefs, ui } from '../state.svelte';
  import DocRow from './DocRow.svelte';
  import { fmtSize, fmtTokens, pinnedOverflow } from './util';

  let { folder, oncount }: { folder: string; oncount?: (n: number) => void } = $props();

  let docs = $state<Document[]>([]);
  let usage = $state<PinnedUsage | null>(null);
  let progress = $state<Record<string, IngestProgress>>({});
  let loaded = $state(false);
  let loadError = $state('');
  let dragging = $state(false);
  let slow = $state<{ files: PickedFile[]; big: PickedFile[] } | null>(null);
  let reloadTimer: ReturnType<typeof setTimeout> | undefined;

  const folderTitle = $derived(library.folder(folder)?.title ?? '');
  const budget = $derived(usage?.budget_tokens ?? prefs.value.docs?.pinned_budget_tokens ?? 6000);
  const overflow = $derived(new Set(pinnedOverflow(docs, budget)));
  const pinnedReady = $derived(docs.filter((d) => d.pinned && d.status === 'ready'));
  const pct = $derived(Math.min(100, Math.round(((usage?.tokens ?? 0) / Math.max(1, budget)) * 100)));
  const sorted = $derived([...docs].sort((a, b) => Number(b.pinned) - Number(a.pinned) || b.added.localeCompare(a.added)));

  async function load() {
    try {
      const [d, u] = await Promise.all([api.docList(folder), api.docsPinnedUsage(folder)]);
      docs = d;
      usage = u;
      loadError = '';
      oncount?.(d.length);
    } catch (e) { loadError = errText(e); }
    loaded = true;
  }
  const reloadSoon = () => { clearTimeout(reloadTimer); reloadTimer = setTimeout(() => void load(), 120); };
  $effect(() => { folder; progress = {}; void load(); });

  onMount(() => {
    const offProgress = api.onDocProgress((p) => {
      if (folderKey(p.folder) !== folder) return;
      progress = { ...progress, [p.doc]: p };
      const i = docs.findIndex((d) => d.id === p.doc);
      if (i < 0) return reloadSoon(); // a file that appeared from outside (watcher) or a late import
      docs[i] = { ...docs[i]!, status: p.status, error: p.error };
      if (p.status !== 'converting' && p.status !== 'queued') reloadSoon(); // pages, tokens, warnings are final now
    });
    const offDrop = api.onFileDrop((files) => void add(files));
    return () => { offProgress(); offDrop(); clearTimeout(reloadTimer); };
  });

  async function add(files: PickedFile[]) {
    if (!files.length) return;
    const limit = (prefs.value.docs?.warn_slow_mb ?? 50) * 1048576;
    const big = files.filter((f) => f.size_bytes > limit);
    if (big.length) { slow = { files, big }; return; }
    await importNow(files);
  }
  async function importNow(files: PickedFile[]) {
    slow = null;
    try {
      const added = await api.docImport(folder, files);
      docs = [...added, ...docs];
      oncount?.(docs.length);
    } catch (e) { ui.notify(errText(e)); }
  }
  async function choose() {
    try {
      const files = await api.pickDocuments();
      if (files) await add(files);
    } catch (e) { ui.notify(errText(e)); }
  }
  function ondrop(e: DragEvent) {
    e.preventDefault();
    dragging = false;
    const list = [...(e.dataTransfer?.files ?? [])].map((f) => ({ name: f.name, path: (f as File & { path?: string }).path ?? f.name, size_bytes: f.size }));
    void add(list);
  }
  const hasFiles = (e: DragEvent) => !!e.dataTransfer && [...e.dataTransfer.types].includes('Files');

  async function pin(d: Document) {
    try { await api.docPatch(d.id, { pinned: !d.pinned }); await load(); } catch (e) { ui.notify(errText(e)); }
  }
  async function cancel(d: Document) {
    try { await api.docCancel(d.id); } catch (e) { ui.notify(errText(e)); }
  }
  async function retry(d: Document) {
    try { await api.docRetry(d.id); await load(); } catch (e) { ui.notify(errText(e)); }
  }
  async function remove(d: Document) {
    try {
      await api.docDelete(d.id);
      await load();
      ui.notify(tm('docs.deleted', { name: d.title }), { label: tm('docs.undo'), run: () => void api.docRestore(d.id).then(load).catch((e) => ui.notify(errText(e))) });
    } catch (e) { ui.notify(errText(e)); }
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="tab" class:drag={dragging} data-testid="docs-tab"
  ondragover={(e) => { if (hasFiles(e)) { e.preventDefault(); dragging = true; } }}
  ondragleave={(e) => { if (e.currentTarget === e.target) dragging = false; }}
  {ondrop}>
  {#if dragging}<div class="dropmsg" role="status"><Icon name="upload" size={20} /><span class="lbl">{tm('docs.dropHere', { folder: folderTitle })}</span></div>{/if}

  {#if loadError}
    <p class="err" role="alert"><Icon name="alert" size={14} /><span>{loadError}</span><Button size="sm" icon="refresh" onclick={load}>{tm('common.retry')}</Button></p>
  {:else if !loaded}
    <p class="muted">{tm('common.loading')}</p>
  {:else if docs.length === 0}
    <section class="empty" data-testid="docs-empty">
      <div class="eic"><Icon name="upload" size={20} /></div>
      <h2>{tm('docs.empty.title')}</h2>
      <p>{tm('docs.empty.body')}</p>
      <Button variant="primary" icon="plus" onclick={choose}>{tm('docs.add')}</Button>
      <small class="lbl">{tm('docs.formats')}</small>
    </section>
  {:else}
    <div class="bar">
      <div class="pin" data-testid="pinned-meter">
        <div class="pt"><Icon name="star-fill" size={14} /><b class="lbl">{tm('docs.pinned.title')}</b>
          <span class="lbl">{tPlural('main.documents', pinnedReady.length)} · {tm('docs.pinned.usage', { used: fmtTokens(usage?.tokens ?? 0), budget: fmtTokens(budget) })}</span></div>
        <div class="meter" role="progressbar" aria-valuemin="0" aria-valuemax={budget} aria-valuenow={Math.min(usage?.tokens ?? 0, budget)} aria-label={tm('docs.pinned.title')}><i class:over={usage?.over_budget} style:width="{pct}%"></i></div>
        {#if overflow.size}
          <p class="over" role="status"><Icon name="alert" size={14} /><span>{tm('docs.pinned.overflow', { n: overflow.size })}</span><a href="#/settings/knowledge">{tm('docs.pinned.raise')}</a></p>
        {:else}
          <small class="hint">{tm('docs.pinned.hint')}</small>
        {/if}
      </div>
      <Button icon="plus" onclick={choose}>{tm('docs.add')}</Button>
    </div>
    <div class="list">
      {#each sorted as d (d.id)}
        <DocRow doc={d} progress={progress[d.id]} overflow={overflow.has(d.id)} onpin={() => pin(d)} oncancel={() => cancel(d)} onretry={() => retry(d)} ondelete={() => remove(d)} onreveal={() => api.revealPath(d.original_path)} />
      {/each}
    </div>
    <p class="drophint lbl">{tm('docs.dropHint')}</p>
  {/if}
</div>

<Modal open={slow !== null} title={tm('docs.slow.title')} width={460} closeLabel={tm('common.close')} onclose={() => (slow = null)}>
  {#if slow}
    <p class="slowp">{tm('docs.slow.body', { mb: prefs.value.docs?.warn_slow_mb ?? 50 })}</p>
    <ul class="slowl">{#each slow.big as f (f.name)}<li><span class="lbl">{f.name}</span><b>{fmtSize(f.size_bytes)}</b></li>{/each}</ul>
  {/if}
  {#snippet footer()}
    <Button onclick={() => (slow = null)}>{tm('common.cancel')}</Button>
    <Button variant="primary" onclick={() => slow && importNow(slow.files)}>{tm('docs.slow.go')}</Button>
  {/snippet}
</Modal>

<style>
  .tab { position: relative; display: flex; flex-direction: column; gap: var(--s4); min-height: 240px; }
  .tab.drag { outline: 2px dashed var(--ac); outline-offset: 8px; border-radius: var(--r-lg); }
  .dropmsg { position: absolute; inset: 0; z-index: 2; display: flex; align-items: center; justify-content: center; gap: var(--s3); background: var(--acbg); border-radius: var(--r-lg); color: var(--ac); font-weight: 600; pointer-events: none; }
  .bar { display: flex; gap: var(--s4); align-items: flex-start; justify-content: space-between; }
  .pin { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: var(--s2); border: 1px solid var(--line); background: var(--panel); border-radius: var(--r-lg); padding: var(--s3) var(--s4); }
  .pt { display: flex; align-items: center; gap: var(--s2); min-width: 0; font-size: 13px; color: var(--tx2); }
  .pt :global(svg) { color: var(--ac); }
  .pt b { color: var(--tx); font-weight: 600; flex: none; }
  .meter { height: 6px; border-radius: 99px; background: var(--hv); overflow: hidden; }
  .meter i { display: block; height: 100%; background: var(--ac); border-radius: 99px; }
  .meter i.over { background: var(--move); }
  .hint { color: var(--tx3); font-size: 12px; line-height: 16px; }
  .over { display: flex; gap: var(--s2); align-items: center; margin: 0; color: var(--tx); font-size: 12px; line-height: 16px; flex-wrap: wrap; }
  .over :global(svg) { color: var(--move); flex: none; }
  .over a { color: var(--ac); }
  .list { border: 1px solid var(--line); border-radius: var(--r-lg); background: var(--panel); overflow: hidden; }
  .drophint { color: var(--tx3); font-size: 12px; text-align: center; margin: 0; }
  .empty { display: flex; flex-direction: column; align-items: center; text-align: center; gap: var(--s3); border: 1px dashed var(--line-strong); border-radius: var(--r-xl); padding: var(--s12) var(--s8); }
  .eic { width: 44px; height: 44px; border-radius: var(--r-lg); background: var(--hv); border: 1px solid var(--line); display: grid; place-items: center; color: var(--tx2); }
  .empty h2 { margin: 0; font: 600 var(--fs-lg) var(--font); }
  .empty p { margin: 0; max-width: 460px; color: var(--tx2); }
  .empty small { color: var(--tx3); font-size: 12px; max-width: 100%; }
  .muted { color: var(--tx3); margin: var(--s6) 0; }
  .err { display: flex; gap: var(--s3); align-items: center; margin: 0; padding: var(--s3) var(--s4); border: 1px solid rgba(255, 77, 79, 0.4); border-radius: var(--r-lg); color: #ff8f90; }
  .err span { flex: 1; min-width: 0; }
  .slowp { margin: 0 0 var(--s3); color: var(--tx2); }
  .slowl { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: var(--s1); }
  .slowl li { display: flex; justify-content: space-between; gap: var(--s3); font-size: 13px; }
  .slowl b { font-weight: 500; color: var(--tx2); flex: none; }
</style>
