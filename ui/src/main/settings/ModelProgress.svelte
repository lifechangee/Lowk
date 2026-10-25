<script lang="ts">
  // Body under a model's chip: size + time estimate (not downloaded), a smoothed bar with speed and ETA (downloading),
  // «Verifying», or the readable error. The retry/cancel buttons stay with the card.
  import type { DlView } from '../dl';
  import { estimateSeconds, fmtEta, fmtMbBytes, fmtSpeed } from '../fmt';
  import { tm } from '../i18n';
  import type { ModelState } from './ModelChip.svelte';

  let { state, view, reason = '', sizeMb = 0 }: { state: ModelState; view: DlView; reason?: string; sizeMb?: number } = $props();
</script>

{#if state === 'downloading' || state === 'verifying'}
  <div class="bar" role="progressbar" aria-valuemin="0" aria-valuemax="100" aria-valuenow={state === 'verifying' ? 100 : view.pct} aria-label={tm('m.engine.downloading')}><i style:width="{state === 'verifying' ? 100 : view.pct}%"></i></div>
  <div class="line" data-testid="model-progress">
    <span class="lbl main">{state === 'verifying' ? tm('m.model.verifying') : tm('m.model.downloading', { done: fmtMbBytes(view.doneBytes), total: fmtMbBytes(view.totalBytes) })}</span>
    {#if state === 'downloading' && view.speed > 0}<span class="lbl sub" data-testid="model-speed">{fmtSpeed(view.speed)}{#if view.eta !== null} · {tm('m.model.eta', { eta: fmtEta(view.eta) })}{/if}</span>{/if}
  </div>
{:else if state === 'error'}
  <p class="err" role="alert" data-testid="model-error">{tm('m.model.error', { reason })}</p>
{:else if state === 'missing' && sizeMb > 0}
  <p class="note" data-testid="model-estimate">{tm('m.model.estimate', { size: fmtMbBytes(sizeMb * 1048576), eta: fmtEta(estimateSeconds(sizeMb)) })}</p>
{/if}

<style>
  .bar { height: 8px; border-radius: 99px; background: var(--hv); overflow: hidden; }
  .bar i { display: block; height: 100%; background: var(--ac); }
  .line { display: flex; justify-content: space-between; gap: var(--s3); min-width: 0; font-size: 12px; color: var(--tx3); }
  .sub { flex: none; max-width: 55%; }
  .err { margin: 0; color: var(--warn); font-size: 12px; line-height: 16px; overflow-wrap: anywhere; }
  .note { margin: 0; color: var(--tx3); font-size: 12px; line-height: 16px; overflow-wrap: anywhere; }
</style>
