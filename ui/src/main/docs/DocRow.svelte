<script lang="ts">
  import type { Document, IngestProgress } from '../../lib/ipc';
  import Button from '../../lib/components/Button.svelte';
  import Icon from '../../lib/components/Icon.svelte';
  import Menu from '../../lib/components/Menu.svelte';
  import { when } from '../fmt';
  import { appErrorText } from '../errors';
  import { tm } from '../i18n';
  import { hrefs, router } from '../router.svelte';
  import { fmtEta, fmtSize, KIND_LABEL, unitCount, warningText } from './util';

  type Props = {
    doc: Document;
    progress?: IngestProgress;
    /** Pinned, ready, but does not fit the pinned budget: not sent to the model. */
    overflow?: boolean;
    onpin: () => void;
    oncancel: () => void;
    onretry: () => void;
    ondelete: () => void;
    onreveal: () => void;
  };
  let { doc, progress, overflow = false, onpin, oncancel, onretry, ondelete, onreveal }: Props = $props();

  const busy = $derived(doc.status === 'queued' || doc.status === 'converting' || doc.status === 'indexing');
  const total = $derived(progress?.total ?? 0);
  const done = $derived(progress?.done ?? 0);
  const pct = $derived(total > 0 ? Math.min(100, Math.round((done / total) * 100)) : 0);
  const statusKey = $derived(`docs.status.${doc.status}` as 'docs.status.ready');
  const meta = $derived(
    [doc.pages ? unitCount(doc.kind, doc.pages) : '', fmtSize(doc.size_bytes), when(doc.added, new Date(), false)].filter(Boolean).join(' · '),
  );
</script>

<div class="row" class:failed={doc.status === 'failed'} data-doc={doc.id} data-status={doc.status}>
  <span class="ext" aria-hidden="true">{KIND_LABEL[doc.kind]}</span>
  <div class="main">
    <div class="t1">
      <a class="ttl lbl" href={hrefs.doc(doc.id)} title={doc.file_name}>{doc.title}</a>
      {#if overflow}<span class="chip warn lbl" title={tm('docs.pinned.overflowHint')}>{tm('docs.pinned.notSent')}</span>{/if}
    </div>
    <div class="meta lbl">{meta}</div>

    {#if busy}
      <div class="prog">
        <div class="bar" role="progressbar" aria-valuemin="0" aria-valuemax="100" aria-valuenow={total > 0 ? pct : undefined} aria-label={tm(statusKey)} class:ind={total === 0}><i style:width="{total > 0 ? pct : 40}%"></i></div>
        <span class="pl lbl">{#if doc.status === 'converting' && total > 0}{done} / {unitCount(doc.kind, total)}{#if fmtEta(progress?.eta_s ?? null)} · {fmtEta(progress?.eta_s ?? null)}{/if}{:else}{tm(statusKey)}{/if}</span>
      </div>
    {/if}

    {#if doc.status === 'failed' && doc.error}
      <p class="err" role="alert"><Icon name="alert" size={14} /><span>{appErrorText(doc.error)}</span></p>
    {/if}
    {#if doc.warnings.length}
      <ul class="warns" aria-label={tm('docs.warnings')}>
        {#each doc.warnings as w, i (i)}<li><Icon name="info" size={12} /><span>{warningText(w)}</span></li>{/each}
      </ul>
    {/if}
  </div>

  <div class="side">
    {#if doc.status === 'ready'}
      <button class="star" class:on={doc.pinned} aria-pressed={doc.pinned} aria-label={doc.pinned ? tm('docs.unpin') : tm('docs.pin')} title={doc.pinned ? tm('docs.unpin') : tm('docs.pin')} onclick={onpin}><Icon name={doc.pinned ? 'star-fill' : 'star'} size={16} /></button>
    {/if}
    {#if busy && doc.status !== 'indexing'}
      <Button size="sm" onclick={oncancel}>{tm('common.cancel')}</Button>
    {:else if doc.status === 'failed' || doc.status === 'cancelled'}
      <Button size="sm" icon="refresh" onclick={onretry}>{tm('common.retry')}</Button>
    {/if}
    <span class="chip lbl" class:ok={doc.status === 'ready'} class:bad={doc.status === 'failed'}>{tm(statusKey)}</span>
    <Menu label={tm('docs.actions')} items={[
      { id: 'open', label: tm('common.open'), icon: 'doc', onselect: () => router.go(hrefs.doc(doc.id)) },
      { id: 'reveal', label: tm('docs.revealOriginal'), icon: 'external', onselect: onreveal },
      { sep: true },
      { id: 'delete', label: tm('docs.delete'), icon: 'trash', danger: true, onselect: ondelete },
    ]} />
  </div>
</div>

<style>
  .row { display: grid; grid-template-columns: 40px minmax(0, 1fr) auto; gap: var(--s3); align-items: start; padding: var(--s3) var(--s4); border-top: 1px solid var(--line); }
  .row:first-child { border-top: none; }
  .row:hover { background: var(--hv); }
  .ext { white-space: nowrap; overflow: hidden; font: 700 9px/16px var(--font); margin-top: var(--s0); padding: var(--s0) var(--s1); border-radius: var(--s1); background: var(--hv); border: 1px solid var(--line); color: var(--tx2); letter-spacing: 0.04em; text-align: center; }
  .main { min-width: 0; display: flex; flex-direction: column; gap: var(--s1); }
  .t1 { display: flex; align-items: center; gap: var(--s2); min-width: 0; }
  .ttl { color: var(--tx); text-decoration: none; font-weight: 550; min-width: 0; }
  .ttl:hover { text-decoration: underline; }
  .meta { color: var(--tx2); font-size: 12px; line-height: 16px; }
  .prog { display: flex; align-items: center; gap: var(--s3); margin-top: var(--s1); min-width: 0; }
  .bar { flex: 1; height: 6px; border-radius: 99px; background: var(--hv); overflow: hidden; min-width: 48px; }
  .bar i { display: block; height: 100%; background: var(--ac); border-radius: 99px; }
  .bar.ind i { opacity: 0.5; }
  .pl { color: var(--tx2); font-size: 12px; line-height: 16px; flex: none; max-width: 60%; }
  .err { display: flex; gap: var(--s2); align-items: flex-start; margin: var(--s1) 0 0; color: #ff8f90; font-size: 12px; line-height: 16px; }
  .err :global(svg) { margin-top: 1px; }
  .warns { list-style: none; margin: var(--s1) 0 0; padding: 0; display: flex; flex-direction: column; gap: var(--s1); }
  .warns li { display: flex; gap: var(--s2); align-items: flex-start; color: var(--tx2); font-size: 12px; line-height: 16px; }
  .warns li :global(svg) { margin-top: 2px; color: var(--tx3); }
  .side { display: flex; align-items: center; gap: var(--s2); flex: none; }
  .chip { display: inline-flex; align-items: center; height: 20px; padding: 0 var(--s2); border-radius: 99px; background: var(--hv); border: 1px solid var(--line); color: var(--tx2); font-size: 11px; line-height: 18px; max-width: 160px; }
  .chip.ok { background: var(--acbg); border-color: transparent; color: var(--ac); }
  .chip.bad { border-color: rgba(255, 77, 79, 0.45); color: #ff8f90; }
  .chip.warn { border-color: var(--line-strong); color: var(--tx); flex: none; }
  .star { all: unset; box-sizing: border-box; cursor: pointer; width: 32px; height: 32px; display: grid; place-items: center; border-radius: var(--r); color: var(--tx3); }
  .star:hover, .star:focus-visible { background: var(--sel); color: var(--tx); }
  .star.on { color: var(--ac); }
</style>
