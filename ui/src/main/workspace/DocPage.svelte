<script lang="ts">
  // Document viewer + Markdown editor. Opens at a page when a search hit or a source label points there (`#/doc/ID?p=N`).
  import { onMount, tick } from 'svelte';
  import type { Document, IngestProgress } from '../../lib/ipc';
  import Button from '../../lib/components/Button.svelte';
  import Icon from '../../lib/components/Icon.svelte';
  import Menu from '../../lib/components/Menu.svelte';
  import Modal from '../../lib/components/Modal.svelte';
  import { api } from '../api';
  import { fmtSize, fmtTokens, KIND_LABEL, splitFrontmatter, splitPages, unitCount, unitOf, warningText } from '../docs/util';
  import MdEditor from '../docs/MdEditor.svelte';
  import { appErrorText, errText } from '../errors';
  import { when } from '../fmt';
  import { tm } from '../i18n';
  import Md from '../Md.svelte';
  import { hrefs, router } from '../router.svelte';
  import { library, ui } from '../state.svelte';

  let { id, page }: { id: string; page: number | null } = $props();

  let doc = $state<Document | null>(null);
  let missing = $state(false);
  let loadError = $state('');
  let md = $state('');
  let saved = $state('');
  let editing = $state(false);
  let saving = $state(false);
  let progress = $state<IngestProgress | null>(null);
  let body: HTMLElement | undefined = $state();
  let hit = $state<number | null>(null);

  const dirty = $derived(editing && md !== saved);
  const pages = $derived(splitPages(splitFrontmatter(saved).body));
  const unit = $derived(doc ? unitOf(doc.kind) : 'page');
  const folderTitle = $derived(doc ? (doc.folder.kind === 'user' ? (library.folder(doc.folder.id)?.title ?? '') : tm('main.unsorted')) : '');
  const busy = $derived(doc ? ['queued', 'converting', 'indexing'].includes(doc.status) : false);

  async function load() {
    try {
      doc = await api.docGet(id);
      missing = !doc;
      if (doc && doc.status === 'ready') { md = saved = await api.docGetMd(id); await tick(); jump(); }
    } catch (e) { loadError = errText(e); }
  }
  $effect(() => { id; void load(); });
  $effect(() => { page; if (!editing) void tick().then(jump); });

  function jump() {
    if (page === null || !body) return;
    const el = body.querySelector<HTMLElement>(`[data-page="${page}"]`);
    if (!el) return;
    el.scrollIntoView({ block: 'start' });
    hit = page;
    setTimeout(() => { if (hit === page) hit = null; }, 2600);
  }

  onMount(() =>
    api.onDocProgress((p) => {
      if (p.doc !== id) return;
      progress = p;
      if (doc) doc = { ...doc, status: p.status, error: p.error };
      if (p.status === 'ready') void load();
    }),
  );

  async function save() {
    if (!doc || saving || md === saved) return;
    saving = true;
    try {
      doc = await api.docSaveMd(id, md);
      saved = md;
      ui.notify(tm('docs.saved'));
    } catch (e) { ui.notify(errText(e)); }
    saving = false;
  }
  function revert() { md = saved; }
  let discard = $state(false);
  function toggleEdit() {
    if (editing && dirty) { discard = true; return; }
    editing = !editing;
  }
  function discardNow() { md = saved; discard = false; editing = false; }
  async function pin() {
    if (!doc) return;
    try { doc = await api.docPatch(id, { pinned: !doc.pinned }); } catch (e) { ui.notify(errText(e)); }
  }
  async function remove() {
    if (!doc) return;
    const d = doc;
    try {
      await api.docDelete(id);
      ui.notify(tm('docs.deleted', { name: d.title }), { label: tm('docs.undo'), run: () => void api.docRestore(d.id).catch((e) => ui.notify(errText(e))) });
      router.go(hrefs.folder(d.folder.kind === 'user' ? d.folder.id : 'sys:unsorted', 'documents'));
    } catch (e) { ui.notify(errText(e)); }
  }
  async function retry() {
    if (!doc) return;
    try { doc = await api.docRetry(id); } catch (e) { ui.notify(errText(e)); }
  }
  const back = $derived(doc ? hrefs.folder(doc.folder.kind === 'user' ? doc.folder.id : 'sys:unsorted', 'documents') : hrefs.home);
</script>

<svelte:window onbeforeunload={(e) => { if (dirty) e.preventDefault(); }} />

{#if missing}
  <div class="state"><Icon name="doc" size={20} /><b>{tm('docs.notFound')}</b><a href={hrefs.home}>{tm('main.nav.home')}</a></div>
{:else if loadError}
  <div class="state" role="alert"><Icon name="alert" size={20} /><b>{loadError}</b></div>
{:else if !doc}
  <p class="state muted">{tm('common.loading')}</p>
{:else}
  <div class="page">
    <div class="cin chead">
      <a class="back" href={back}><Icon name="chevron-left" size={14} /><span class="lbl">{folderTitle}</span></a>
      <div class="row1">
        <div class="hd">
          <h1 class="lbl" title={doc.file_name}>{doc.title}</h1>
          <div class="meta">
            <span class="ext">{KIND_LABEL[doc.kind]}</span>
            {#if doc.pages}<span>{unitCount(doc.kind, doc.pages)}</span><i class="d"></i>{/if}
            {#if doc.size_bytes}<span>{fmtSize(doc.size_bytes)}</span><i class="d"></i>{/if}
            {#if doc.tokens}<span>{tm('docs.tokens', { n: fmtTokens(doc.tokens) })}</span><i class="d"></i>{/if}
            <span>{when(doc.added, new Date(), false)}</span>
            <span class="chip lbl" class:ok={doc.status === 'ready'} class:bad={doc.status === 'failed'}>{tm(`docs.status.${doc.status}` as 'docs.status.ready')}</span>
          </div>
        </div>
        <div class="acts">
          {#if doc.status === 'ready'}
            <Button size="sm" icon={doc.pinned ? 'star-fill' : 'star'} pressed={doc.pinned || undefined} onclick={pin}>{doc.pinned ? tm('docs.pinned.on') : tm('docs.pin')}</Button>
            <Button size="sm" icon="pencil" pressed={editing || undefined} onclick={toggleEdit}>{editing ? tm('docs.view') : tm('docs.edit')}</Button>
          {/if}
          <Menu label={tm('docs.actions')} items={[
            { id: 'reveal', label: tm('docs.revealOriginal'), icon: 'external', onselect: () => api.revealPath(doc!.original_path) },
            { sep: true },
            { id: 'delete', label: tm('docs.delete'), icon: 'trash', danger: true, onselect: remove },
          ]} />
        </div>
      </div>
    </div>

    <div class="scroll" bind:this={body}>
      <div class="cin">
        {#if doc.status === 'failed' && doc.error}
          <div class="banner bad" role="alert"><Icon name="alert" size={16} /><span>{appErrorText(doc.error)}</span><Button size="sm" icon="refresh" onclick={retry}>{tm('common.retry')}</Button></div>
        {:else if doc.status === 'cancelled'}
          <div class="banner"><Icon name="info" size={16} /><span>{tm('docs.status.cancelled')}</span><Button size="sm" icon="refresh" onclick={retry}>{tm('common.retry')}</Button></div>
        {:else if busy}
          <div class="banner" role="status" data-testid="doc-converting">
            <Icon name="refresh" size={16} />
            <span>{progress && progress.total > 0 ? tm('docs.converting', { done: progress.done, total: progress.total }) : tm(`docs.status.${doc.status}` as 'docs.status.queued')}</span>
            {#if doc.status !== 'indexing'}<Button size="sm" onclick={() => api.docCancel(id)}>{tm('common.cancel')}</Button>{/if}
          </div>
        {:else}
          {#each doc.warnings as w, i (i)}<div class="banner soft"><Icon name="info" size={16} /><span>{warningText(w)}</span></div>{/each}
          {#if editing}
            <div class="editbar">
              <span class="hint">{tm('docs.editHint')}</span>
              <span class="sp"></span>
              {#if dirty}<span class="dirty lbl" role="status">{tm('docs.unsaved')}</span>{/if}
              <Button size="sm" disabled={!dirty} onclick={revert}>{tm('docs.revert')}</Button>
              <Button size="sm" variant="primary" disabled={!dirty || saving} onclick={save}>{tm('common.save')}</Button>
            </div>
            <div class="edwrap"><MdEditor value={md} onchange={(v) => (md = v)} onsave={save} /></div>
          {:else}
            <div class="pages" data-testid="doc-pages">
              {#each pages as p (p.page)}
                <section class="pg" class:hit={hit === p.page} data-page={p.page}>
                  {#if p.page > 0}<div class="pn"><span>{tm(`docs.unit.${unit}` as 'docs.unit.page', { n: p.page })}</span></div>{/if}
                  <Md md={p.text} />
                </section>
              {/each}
            </div>
          {/if}
        {/if}
      </div>
    </div>
  </div>
{/if}

<Modal open={discard} title={tm('docs.discard.title')} width={420} closeLabel={tm('common.close')} onclose={() => (discard = false)}>
  <p class="dp">{tm('docs.discard.body')}</p>
  {#snippet footer()}
    <Button onclick={() => (discard = false)}>{tm('docs.discard.keep')}</Button>
    <Button variant="danger" onclick={discardNow}>{tm('docs.discard.go')}</Button>
  {/snippet}
</Modal>

<style>
  .dp { margin: 0; color: var(--tx2); }
  .page { display: flex; flex-direction: column; min-height: 0; flex: 1; }
  .cin { max-width: 768px; width: 100%; margin: 0 auto; padding: 0 var(--s8); }
  .chead { padding-top: var(--s4); padding-bottom: var(--s4); border-bottom: 1px solid var(--line); flex: none; }
  .back { display: inline-flex; align-items: center; gap: var(--s1); max-width: 100%; color: var(--tx2); font-size: 12px; line-height: 20px; text-decoration: none; margin-bottom: var(--s1); }
  .back:hover { color: var(--tx); }
  .row1 { display: flex; justify-content: space-between; gap: var(--s4); align-items: flex-start; }
  .hd { min-width: 0; flex: 1; }
  h1 { font-size: 24px; line-height: 32px; letter-spacing: -0.025em; margin: 0; font-weight: 650; }
  .meta { display: flex; flex-wrap: wrap; gap: var(--s1) var(--s3); align-items: center; color: var(--tx2); font-size: 12px; line-height: 20px; margin-top: var(--s1); }
  .d { width: 3px; height: 3px; border-radius: 50%; background: var(--tx3); }
  .ext { font: 700 10px/16px var(--font); padding: 0 var(--s1); border-radius: var(--s1); background: var(--hv); border: 1px solid var(--line); letter-spacing: 0.04em; }
  .chip { display: inline-flex; align-items: center; height: 20px; padding: 0 var(--s2); border-radius: 99px; background: var(--hv); border: 1px solid var(--line); font-size: 11px; line-height: 18px; max-width: 160px; }
  .chip.ok { background: var(--acbg); border-color: transparent; color: var(--ac); }
  .chip.bad { border-color: rgba(255, 77, 79, 0.45); color: #ff8f90; }
  .acts { display: flex; gap: var(--s2); align-items: center; flex: none; }
  .scroll { flex: 1; overflow: auto; padding: var(--s6) 0 64px; min-height: 0; }
  .banner { display: flex; align-items: center; gap: var(--s3); border: 1px solid var(--line); background: var(--panel); border-radius: var(--r-lg); padding: var(--s3) var(--s4); margin-bottom: var(--s3); font-size: 13px; }
  .banner span { flex: 1; min-width: 0; }
  .banner.bad { border-color: rgba(255, 77, 79, 0.4); color: #ff8f90; }
  .banner.soft { color: var(--tx2); padding: var(--s2) var(--s3); }
  .editbar { display: flex; gap: var(--s2); align-items: center; margin-bottom: var(--s3); }
  .hint { color: var(--tx3); font-size: 12px; line-height: 16px; min-width: 0; }
  .sp { flex: 1; }
  .dirty { color: var(--tx); font-size: 12px; font-weight: 600; }
  .edwrap { height: calc(100vh / var(--z, 1) - 320px); min-height: 320px; }
  .pg { padding: var(--s3) 0; border-top: 1px solid var(--line); border-radius: 0; scroll-margin-top: var(--s4); }
  .pg:first-child { border-top: none; }
  .pg.hit { background: var(--acbg); box-shadow: -12px 0 0 var(--acbg), 12px 0 0 var(--acbg); }
  .pn { font: 600 var(--fs-xs) var(--font); letter-spacing: 0.08em; text-transform: uppercase; color: var(--tx3); margin-bottom: var(--s2); }
  .state { display: flex; flex-direction: column; align-items: center; gap: var(--s3); padding: var(--s12) var(--s8); color: var(--tx2); }
  .state a { color: var(--ac); }
  .muted { color: var(--tx3); }
</style>
