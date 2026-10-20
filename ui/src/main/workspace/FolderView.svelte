<script lang="ts">
  import Button from '../../lib/components/Button.svelte';
  import Icon from '../../lib/components/Icon.svelte';
  import Modal from '../../lib/components/Modal.svelte';
  import Tag from '../../lib/components/Tag.svelte';
  import { tPlural } from '../../lib/i18n/locale.svelte';
  import { api, folderKey, SYS_TRASH, SYS_UNSORTED } from '../api';
  import { duration, when } from '../fmt';
  import { tm } from '../i18n';
  import { hrefs } from '../router.svelte';
  import { callRow } from '../dnd.svelte';
  import { library, ui } from '../state.svelte';

  let { folder }: { folder: string } = $props();

  const isTrash = $derived(folder === SYS_TRASH);
  const isUnsorted = $derived(folder === SYS_UNSORTED);
  const meta = $derived(library.folder(folder));
  const title = $derived(isTrash ? tm('main.trash') : isUnsorted ? tm('main.unsorted') : (meta?.title ?? ''));
  const calls = $derived(library.callsIn(folder));
  let confirmEmpty = $state(false);

  async function restore(id: string) { await api.callRestore(id); await library.reload(); ui.notify(tm('m.restored')); }
  async function emptyTrash() { await api.trashEmpty(); await library.reload(); confirmEmpty = false; }
</script>

<div class="page">
  <div class="cin chead">
    <div class="row1">
      <div class="hd">
        <h1 class="lbl">{title}</h1>
        <div class="meta">
          <span>{tPlural('main.calls', calls.length)}</span>
          {#each meta?.tags ?? [] as t (t)}<Tag text={'# ' + t} />{/each}
        </div>
      </div>
      {#if isTrash}
        <Button variant="danger" icon="trash" disabled={calls.length === 0} onclick={() => (confirmEmpty = true)}>{tm('m.emptyTrash')}</Button>
      {/if}
    </div>
    {#if isTrash}<p class="note">{tm('m.trashNote')}</p>{:else if isUnsorted}<p class="note">{tm('m.unsortedNote')}</p>{/if}
  </div>
  <div class="doc">
    <div class="cin">
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
          <div class="emptyrow"><Icon name={isTrash ? 'trash' : 'folder'} size={20} /><span>{isTrash ? tm('m.trashEmpty') : tm('m.noCallsYet')}</span></div>
        {/each}
      </div>
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
  .note { color: var(--tx2); font-size: 13px; margin: var(--s3) 0 0; }
  .doc { flex: 1; overflow: auto; padding: var(--s2) 0 64px; min-height: 0; }
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
  .emptyrow { display: flex; gap: var(--s3); align-items: center; justify-content: center; padding: var(--s8); color: var(--tx3); }
  .warn { margin: 0; color: var(--tx2); }
</style>
