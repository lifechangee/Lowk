<script lang="ts">
  import './overlay.css';
  import type { OverlayStore } from './store.svelte';
  import type { OverlaySource } from './source';
  import { varsToStyle } from './tone';
  import { keyLabels } from './keys';
  import { ot } from './i18n';
  import Header from './Header.svelte';
  import Subtitle from './Subtitle.svelte';
  import Answer from './Answer.svelte';
  import MyAnswer from './MyAnswer.svelte';
  import InputRow from './InputRow.svelte';
  import Hints from './Hints.svelte';
  import Pill from './Pill.svelte';

  let { store, source }: { store: OverlayStore; source: OverlaySource } = $props();
  const style = $derived(varsToStyle(store.tone, store.k, store.fontPx));

  // Interactive header drag: pointer deltas in screen points; the backend moves the window.
  let drag: { x: number; y: number } | null = null;
  function down(e: PointerEvent) {
    const t = e.target as HTMLElement;
    if (store.mode !== 'interactive' || e.button !== 0 || !t.closest('[data-drag]') || t.closest('button, .menu')) return;
    drag = { x: e.screenX, y: e.screenY };
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    source.dragStart?.();
  }
  function move(e: PointerEvent) {
    if (drag) source.dragTo?.(e.screenX - drag.x, e.screenY - drag.y);
  }
  function up() {
    if (drag) source.dragEnd?.();
    drag = null;
  }
  function key(e: KeyboardEvent) {
    if (e.key === 'Escape' && store.mode === 'interactive') source.escape?.();
  }
</script>

<svelte:window onkeydown={key} />

{#if store.showPill}
  <div class="ovp" {style} style:width="100%" style:height="100%"><Pill {store} {source} /></div>
{:else}
  <div role="presentation" onpointerdown={down} onpointermove={move} onpointerup={up} onpointercancel={up} class="ov" class:interactive={store.mode === 'interactive'} class:moving={store.mode === 'moving'} {style} data-mode={store.mode}>
    <Header {store} {source} />
    <Subtitle {store} />
    <Answer {store} />
    <MyAnswer {store} />
    {#if store.mode === 'interactive'}<InputRow {store} {source} />{/if}
    <Hints {store} />
    {#if store.mode === 'moving'}
      <div class="badge">{ot('overlay.move.badge', store.lang, { key: keyLabels(store.isMac).move })}</div>
    {/if}
    {#if store.sentinel}<div class="sentinel"></div>{/if}
    {#if store.toast}<div class="toast">{store.toast}</div>{/if}
  </div>
{/if}
