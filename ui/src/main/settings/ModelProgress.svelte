<script lang="ts">
  // Body under a model's chip: size + time estimate (not downloaded), a smoothed bar with speed and ETA (downloading),
  // «Verifying», or the readable error. The retry/cancel buttons stay with the card.
  import { untrack } from 'svelte';
  import type { DlView } from '../dl';
  import { estimateSeconds, fmtEta, fmtMbBytes, fmtSpeed } from '../fmt';
  import { tm } from '../i18n';
  import type { ModelState } from './ModelChip.svelte';

  let { state: card, view, reason = '', sizeMb = 0 }: { state: ModelState; view: DlView; reason?: string; sizeMb?: number } = $props();
  // The backend sends nothing while a connection is silent (up to its 20 s stall limit), so the bar would
  // show a decaying speed and a fake ETA. Bytes that stopped moving read as reconnecting at once.
  let movedAt = $state(Date.now());
  let lastDone = -1;
  let now = $state(Date.now());
  $effect(() => {
    const d = view.doneBytes;
    untrack(() => {
      if (d !== lastDone) {
        lastDone = d;
        movedAt = Date.now();
      }
    });
  });
  $effect(() => {
    if (card !== 'downloading') return;
    const t = setInterval(() => (now = Date.now()), 1000);
    return () => clearInterval(t);
  });
  // Why the bar stands still: a dropped or silent connection (resumes by itself) or a damaged part fetched again.
  const note = $derived(
    view.phase === 'retrying' || (card === 'downloading' && isStalled(view.doneBytes, movedAt, now))
      ? tm('m.model.retrying')
      : view.phase === 'repairing'
        ? tm('m.model.repairing')
        : '',
  );
</script>

{#if card === 'downloading' || card === 'verifying'}
  <div class="bar" role="progressbar" aria-valuemin="0" aria-valuemax="100" aria-valuenow={progressPercent(view.pct)} aria-label={tm('m.engine.downloading')}><i style:width="{progressPercent(view.pct)}%"></i></div>
  <div class="line" data-testid="model-progress">
    <span class="lbl main">{card === 'verifying' ? tm('m.model.verifying') : tm('m.model.downloading', { done: fmtMbBytes(view.doneBytes), total: fmtMbBytes(view.totalBytes) })}</span>
    {#if card === 'downloading' && note}<span class="lbl sub" data-testid="model-note" role="status">{note}</span>
    {:else if card === 'downloading' && view.speed > 0}<span class="lbl sub" data-testid="model-speed">{fmtSpeed(view.speed)}{#if view.eta !== null} · {tm('m.model.eta', { eta: fmtEta(view.eta) })}{/if}</span>{/if}
  </div>
{:else if card === 'error'}
  <p class="err" role="alert" data-testid="model-error">{tm('m.model.error', { reason })}</p>
{:else if card === 'missing' && sizeMb > 0}
  <p class="note" data-testid="model-estimate">{tm('m.model.estimate', { size: fmtMbBytes(sizeMb * 1048576), eta: fmtEta(estimateSeconds(sizeMb)) })}</p>
{/if}

<script module lang="ts">
  export function progressPercent(aggregatePct: number): number {
    return aggregatePct;
  }

  /** No new byte for this long during a download = say «reconnecting» instead of a speed. */
  export const STALL_NOTE_MS = 3000;

  export function isStalled(doneBytes: number, movedAt: number, now: number): boolean {
    return doneBytes > 0 && now - movedAt >= STALL_NOTE_MS;
  }
</script>

<style>
  .bar { height: 8px; border-radius: 99px; background: var(--hv); overflow: hidden; }
  .bar i { display: block; height: 100%; background: var(--ac); }
  .line { display: flex; justify-content: space-between; gap: var(--s3); min-width: 0; font-size: 12px; color: var(--tx3); }
  .sub { flex: none; max-width: 55%; }
  .err { margin: 0; color: var(--warn); font-size: 12px; line-height: 16px; overflow-wrap: anywhere; }
  .note { margin: 0; color: var(--tx3); font-size: 12px; line-height: 16px; overflow-wrap: anywhere; }
</style>
