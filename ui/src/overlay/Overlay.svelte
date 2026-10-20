<script lang="ts">
  import './overlay.css';
  import type { OverlayStore } from './store.svelte';
  import type { OverlaySource } from './source';
  import { blockTone, cellsToStyle, varsToStyle } from './tone';
  import { tick } from 'svelte';
    import Header from './Header.svelte';
  import Subtitle from './Subtitle.svelte';
  import Answer from './Answer.svelte';
  import MyAnswer from './MyAnswer.svelte';
  import InputRow from './InputRow.svelte';
  import Hints from './Hints.svelte';
  import Pill from './Pill.svelte';

  let { store, source }: { store: OverlayStore; source: OverlaySource } = $props();
  const style = $derived(varsToStyle(store.tone, store.k, store.fontPx));
  const scrimStyle = $derived(cellsToStyle(store.cells));
  let root: HTMLDivElement | undefined = $state();

  // F5: every text block (header, subtitle, answer, «Мой ответ», input, hints) picks its text colour
  // from the grid cells under it. Without a grid the global tone applies untouched.
  let first = true;
  function measure() {
    if (!root) return;
    // The very first grid paint must not fade in from the global tone (no start-up flash of the wrong colours).
    const r = root.getBoundingClientRect();
    if (!r.width || !r.height) return;
    const snap = first && !!store.grid;
    if (snap) root.classList.add('nofade');
    for (const el of root.querySelectorAll<HTMLElement>('[data-blk]')) {
      if (!store.grid) {
        el.style.removeProperty('--fg');
        el.style.removeProperty('--inv');
        el.style.removeProperty('--brgb');
        el.style.removeProperty('--int');
        el.style.removeProperty('--mv');
        el.removeAttribute('data-mixed');
        continue;
      }
      const b = el.getBoundingClientRect();
      const t = blockTone(store.grid, { x0: (b.left - r.left) / r.width, y0: (b.top - r.top) / r.height, x1: (b.right - r.left) / r.width, y1: (b.bottom - r.top) / r.height });
      el.style.setProperty('--fg', t.fg);
      el.style.setProperty('--inv', t.inv);
      el.style.setProperty('--brgb', t.rgb);
      el.style.setProperty('--int', t.int);
      el.style.setProperty('--mv', t.mv);
      el.toggleAttribute('data-mixed', t.mixed);
    }
    if (snap) {
      first = false;
      void root.offsetWidth;
      requestAnimationFrame(() => root?.classList.remove('nofade'));
    }
  }
  $effect(() => {
    store.grid;
    store.mode;
    store.fontPx;
    store.phase;
    store.lang;
    if (!root) return;
    void tick().then(measure);
    const ro = new ResizeObserver(measure);
    ro.observe(root);
    return () => ro.disconnect();
  });

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
  <div role="presentation" bind:this={root} onpointerdown={down} onpointermove={move} onpointerup={up} onpointercancel={up} class="ov" class:interactive={store.mode === 'interactive'} class:moving={store.mode === 'moving'} {style} data-mode={store.mode}>
    <div class="scrim" style={scrimStyle} aria-hidden="true"><i class="r0"></i><i class="r1"></i><i class="r2"></i></div>
    <Header {store} {source} />
    <Subtitle {store} />
    <Answer {store} />
    <MyAnswer {store} />
    {#if store.mode === 'interactive'}<InputRow {store} {source} />{/if}
    <Hints {store} />
    {#if store.sentinel}<div class="sentinel"></div>{/if}
  </div>
{/if}
