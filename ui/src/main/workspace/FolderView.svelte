<script lang="ts">
  // A folder is a workspace with tabs Calls / Documents / Instructions (variant A).
  // Unsorted and Trash are plain call lists. Page actions live in the ⋯ menu next to the title.
  import Button from '../../lib/components/Button.svelte';
  import Icon from '../../lib/components/Icon.svelte';
  import Menu from '../../lib/components/Menu.svelte';
  import Modal from '../../lib/components/Modal.svelte';
  import Tabs from '../../lib/components/Tabs.svelte';
  import TagEditor from '../../lib/components/TagEditor.svelte';
  import { tPlural } from '../../lib/i18n/locale.svelte';
  import { api, isSystemKey, SYS_TRASH, SYS_UNSORTED } from '../api';
  import DocumentsTab from '../docs/DocumentsTab.svelte';
  import InstructionsTab from '../docs/InstructionsTab.svelte';
  import { errText } from '../errors';
  import { duration, when } from '../fmt';
  import { tm } from '../i18n';
  import { hrefs, router, type FolderTab } from '../router.svelte';
  import { callRow } from '../dnd.svelte';
  import Select from '../../lib/components/Select.svelte';
  import type { MicPreset, RetxPreset } from '../../lib/ipc';
  import { library, ui } from '../state.svelte';

  let { folder, tab }: { folder: string; tab: FolderTab } = $props();

  const isTrash = $derived(folder === SYS_TRASH);
  const isUnsorted = $derived(folder === SYS_UNSORTED);
  const system = $derived(isSystemKey(folder));
  const meta = $derived(library.folder(folder));
  const title = $derived(isTrash ? tm('main.trash') : isUnsorted ? tm('main.unsorted') : (meta?.title ?? ''));
  const calls = $derived(library.callsIn(folder));
  const active = $derived(system ? 'calls' : tab);
  let confirmEmpty = $state(false);
  let docCount = $state<number | null>(null);

  $effect(() => {
    const k = folder;
    docCount = null;
    if (!isSystemKey(k)) void api.docList(k).then((d) => { if (k === folder) docCount = d.length; }).catch(() => undefined);
  });

  async function restore(id: string) { await api.callRestore(id); await library.reload(); ui.notify(tm('m.restored')); }
  async function emptyTrash() { await api.trashEmpty(); await library.reload(); confirmEmpty = false; }
  async function setTags(tags: string[]) {
    try { await api.folderUpdate(folder, { tags }); await library.reload(); } catch (e) { ui.notify(errText(e)); }
  }
  const micOptions: Array<{ value: MicPreset; label: string }> = $derived([
    { value: 'inherit', label: tm('m.folderMic.inherit') },
    { value: 'on', label: tm('m.folderMic.on') },
    { value: 'off', label: tm('m.folderMic.off') },
  ]);
  async function setMic(mic: MicPreset) {
    try { await api.folderUpdate(folder, { mic }); await library.reload(); } catch (e) { ui.notify(errText(e)); }
  }
  const retxOptions: Array<{ value: RetxPreset; label: string }> = $derived([
    { value: 'inherit', label: tm('m.folderMic.inherit') },
    { value: 'on', label: tm('m.folderRetx.on') },
    { value: 'off', label: tm('m.folderRetx.off') },
  ]);
  async function setRetx(retx: RetxPreset) {
    try { await api.folderUpdate(folder, { retx }); await library.reload(); } catch (e) { ui.notify(errText(e)); }
  }
  async function exportFolder() {
    try {
      const dest = await api.pickSavePath(`${title}.zip`);
      if (!dest) return;
      const n = await api.folderExport(folder, dest);
      ui.notify(tm('main.folderExported', { n }));
    } catch (e) { ui.notify(errText(e)); }
  }
  const tabs = $derived([
    { id: 'calls' as const, label: tm('main.tab.calls'), count: calls.length },
    { id: 'documents' as const, label: tm('main.tab.documents'), count: docCount },
    { id: 'instructions' as const, label: tm('main.tab.instructions'), count: null },
  ]);
</script>

<div class="page">
  <div class="cin chead">
    <div class="row1">
      <div class="hd">
        <h1 class="lbl">{title}</h1>
        <div class="meta">
          <span>{tPlural('main.calls', calls.length)}</span>
          {#if meta}<span class="micsel" data-testid="folder-mic"><span class="micl">{tm('m.folderMic')}</span><Select label={tm('m.folderMic')} value={meta.mic ?? 'inherit'} options={micOptions} onchange={setMic} /></span>{/if}
          {#if meta && !system}<span class="micsel" data-testid="folder-retx"><span class="micl">{tm('m.folderRetx')}</span><Select label={tm('m.folderRetx')} value={meta.retx ?? 'inherit'} options={retxOptions} onchange={setRetx} /></span>{/if}
          {#if meta}<TagEditor tags={meta.tags} addLabel={tm('main.addTag')} newLabel={tm('main.newTag')} removeLabel={(t) => tm('main.removeTag', { tag: t })} onchange={setTags} />{/if}
        </div>
      </div>
      {#if isTrash}
        <Button variant="danger" icon="trash" disabled={calls.length === 0} onclick={() => (confirmEmpty = true)}>{tm('m.emptyTrash')}</Button>
      {:else if !system}
        <Menu label={tm('main.folderActions')} testid="folder-menu" items={[
          { id: 'export', label: tm('main.exportFolder'), icon: 'download', onselect: exportFolder },
          { id: 'reveal', label: tm('main.revealFolder'), icon: 'external', onselect: () => meta && api.revealPath(meta.path) },
        ]} />
      {/if}
    </div>
    {#if isTrash}<p class="note">{tm('m.trashNote')}</p>{:else if isUnsorted}<p class="note">{tm('m.unsortedNote')}</p>{/if}
  </div>

  {#if !system}
    <Tabs {tabs} value={active} label={title} onselect={(t) => router.go(hrefs.folder(folder, t))} />
  {/if}

  <div class="doc">
    <div class="cin">
      {#if active === 'calls'}
        <div class="tbl">
          {#each calls as c (c.id)}
            <div class="tr" class:trash={isTrash} use:callRow={() => ({ id: c.id, title: c.title, movable: !isTrash && c.ended !== null })}>
              <a class="c1" href={isTrash ? undefined : hrefs.call(c.id)} draggable="false">
                <b class="lbl">{c.title}</b>
                <small class="lbl">{when(c.started)} · {duration(c.duration_s)}</small>
              </a>
              {#if isTrash}
                <Button size="sm" icon="restore" onclick={() => restore(c.id)}>{tm('m.restore')}</Button>
              {:else}
                <div class="rt"><span class="st lbl" class:ok={library.summarized.includes(c.id)}>{library.summarized.includes(c.id) ? tm('main.summaryReady') : tm('main.noSummary')}</span>
                {#if c.ended !== null}
                  <button class="more" aria-haspopup="menu" aria-label={tm('m.callActions')} title={tm('m.moveMenu')} onclick={(e) => { const r = e.currentTarget.getBoundingClientRect(); ui.openMove(c.id, r.right, r.bottom); }}><Icon name="more" size={14} /></button>
                {/if}</div>
              {/if}
            </div>
          {:else}
            <div class="emptyrow" data-testid="calls-empty">
              <div class="eic"><Icon name={isTrash ? 'trash' : isUnsorted ? 'inbox' : 'folder'} size={20} /></div>
              <b>{isTrash ? tm('main.empty.trashTitle') : isUnsorted ? tm('main.empty.unsortedTitle') : tm('main.empty.folderTitle')}</b>
              <p>{isTrash ? tm('main.empty.trashBody') : isUnsorted ? tm('main.empty.unsortedBody') : tm('main.empty.folderBody')}</p>
              {#if !system}
                <span data-np-trigger style="display: contents"><Button variant="primary" onclick={() => { ui.newCallFolder = folder; ui.newCallOpen = true; }}>{tm('main.newCallHere')}</Button></span>
              {/if}
            </div>
          {/each}
        </div>
      {:else if active === 'documents'}
        <DocumentsTab {folder} oncount={(n) => (docCount = n)} />
      {:else}
        <InstructionsTab {folder} />
      {/if}
    </div>
  </div>
</div>

<Modal open={confirmEmpty} title={tm('m.emptyTrash')} width={420} closeLabel={tm('common.close')} onclose={() => (confirmEmpty = false)}>
  <p class="warn">{tm('m.emptyTrashConfirm', { n: calls.length })}</p>
  {#snippet footer()}
    <Button onclick={() => (confirmEmpty = false)}>{tm('common.cancel')}</Button>
    <Button variant="danger" onclick={emptyTrash}>{tm('m.emptyTrash')}</Button>
  {/snippet}
</Modal>

<style>
  .page { display: flex; flex-direction: column; min-height: 0; flex: 1; }
  .cin { max-width: 768px; width: 100%; margin: 0 auto; padding: 0 var(--s8); }
  .chead { padding-top: var(--s6); padding-bottom: var(--s4); }
  .row1 { display: flex; justify-content: space-between; gap: var(--s4); align-items: flex-start; }
  .hd { min-width: 0; flex: 1; }
  h1 { font-size: 24px; line-height: 32px; margin: 0; font-weight: 650; letter-spacing: -0.025em; }
  .meta { display: flex; flex-wrap: wrap; gap: var(--s1) var(--s3); align-items: center; color: var(--tx2); font-size: 12px; line-height: 20px; margin-top: var(--s1); }
  .micsel { display: inline-flex; align-items: center; gap: var(--s2); }
  .micsel :global(.sel) { width: 150px; height: 26px; font-size: 12px; }
  .micl { color: var(--tx3); }
  .note { color: var(--tx2); font-size: 13px; margin: var(--s3) 0 0; }
  .doc { flex: 1; overflow: auto; padding: var(--s6) 0 64px; min-height: 0; }
  .tbl { border: 1px solid var(--line); border-radius: var(--r-lg); background: var(--panel); overflow: hidden; }
  .tr { display: grid; grid-template-columns: minmax(0, 1fr) auto; gap: var(--s4); align-items: center; padding: var(--s3) var(--s4); cursor: grab; border-top: 1px solid var(--line); }
  .tr:first-child { border-top: none; }
  .tr:hover { background: var(--hv); }
  .c1 { min-width: 0; color: inherit; text-decoration: none; display: block; }
  .c1 b { display: block; font-weight: 550; }
  .trash { cursor: default; }
  .trash .c1 { opacity: 0.7; }
  small { display: block; color: var(--tx2); font: var(--fs-sm) var(--font); }
  .st { font-size: 12px; color: var(--tx2); max-width: 128px; }
  .st.ok { color: var(--ac); }
  .rt { display: flex; align-items: center; gap: var(--s2); }
  .more { all: unset; box-sizing: border-box; width: 32px; height: 32px; display: grid; place-items: center; border-radius: var(--r); color: var(--tx3); cursor: pointer; }
  .more:hover, .more:focus-visible { background: var(--sel); color: var(--tx); }
  .emptyrow { grid-column: 1 / -1; display: flex; flex-direction: column; align-items: center; text-align: center; gap: var(--s2); padding: var(--s10) var(--s8); color: var(--tx2); }
  .emptyrow b { color: var(--tx); font-weight: 600; font-size: 16px; line-height: 24px; }
  .emptyrow p { margin: 0 0 var(--s2); max-width: 420px; }
  .eic { width: 44px; height: 44px; border-radius: var(--r-lg); background: var(--hv); border: 1px solid var(--line); display: grid; place-items: center; color: var(--tx2); margin-bottom: var(--s1); }
  .warn { margin: 0; color: var(--tx2); }
</style>
