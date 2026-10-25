<script lang="ts">
  import { onMount } from 'svelte';
  import { installCtrlWheel } from './wheel';
  onMount(installCtrlWheel);
  import './overlay.css';
  import type { OverlayStore } from './store.svelte';
  import type { OverlaySource } from './source';
  import { GRID_ROWS, SCRIM_TRANSITION, blockTone, cellsToStyle, registerCellProps, scrimRowStyle, textGradient, varsToStyle } from './tone';
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
  registerCellProps();
  const scrimStyle = $derived(`${cellsToStyle(store.cells)};transition:${SCRIM_TRANSITION}`);
  const rowStyles = Array.from({ length: GRID_ROWS }, (_, r) => scrimRowStyle(r));
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
  // Long text (`[data-gt]`: subtitle lines, answer, «Мой ответ») takes the colour of the cell behind EACH glyph:
  // a hard-stop gradient of the cell text colours, clipped to the text (see overlay.css `[data-grad]`).
  function measureText() {
    if (!root) return;
    const r = root.getBoundingClientRect();
    for (const el of root.querySelectorAll<HTMLElement>('[data-gt], [data-blk][data-mixed] .lbl, [data-blk][data-mixed]:not(:has(*))')) {
      if (!store.grid || !r.width || !r.height) {
        el.removeAttribute('data-grad');
        el.style.removeProperty('background-image');
        el.style.removeProperty('background-size');
        el.style.removeProperty('background-position');
        continue;
      }
      const b = el.getBoundingClientRect();
      const g = textGradient(store.grid, { w: r.width, h: r.height }, { x: b.left - r.left, y: b.top - r.top }, { w: b.width, h: b.height });
      el.style.backgroundImage = g.image;
      el.style.backgroundSize = g.size;
      el.style.backgroundPosition = g.position;
      el.setAttribute('data-grad', '');
    }
  }
  let raf = 0;
  const soon = () => {
    if (raf) return;
    raf = requestAnimationFrame(() => {
      raf = 0;
      measureText();
    });
  };
  $effect(() => {
    store.grid;
    store.mode;
    store.fontPx;
    store.phase;
    store.lang;
    if (!root) return;
    void tick().then(() => {
      measure();
      measureText();
    });
    const ro = new ResizeObserver(() => {
      measure();
      measureText();
    });
    ro.observe(root);
    // streamed text, new subtitle lines and scrolling move the text relative to the overlay
    const mo = new MutationObserver(soon);
    mo.observe(root, { childList: true, subtree: true, characterData: true });
    root.addEventListener('scroll', soon, true);
    const el = root;
    return () => {
      ro.disconnect();
      mo.disconnect();
      el.removeEventListener('scroll', soon, true);
    };
  });

  // Interactive header drag: pointer deltas in screen points; the backend moves the window.
  let drag: { x: number; y: number } | null = null;
  function down(e: PointerEvent) {
    const t = e.target as HTMLElement;
    if ((store.mode !== 'interactive' && store.mode !== 'hover') || e.button !== 0 || !t.closest('[data-drag]') || t.closest('button, .menu')) return;
    drag = { x: e.screenX, y: e.screenY };
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    source.dragStart?.();
  }
  function move(e: PointerEvent) {
    if (drag) source.dragTo?.(e.screenX - drag.x, e.screenY - drag.y);
  }
  // macOS WebKit turns Ctrl+click into a context menu and dispatches no `click`. While the Ctrl hold (hover mode)
  // is the thing making the overlay clickable, a primary press on a button must still act: if the native click does
  // not arrive right after the release, fire it ourselves; never show the context menu.
  let pressBtn: HTMLElement | null = null;
  let nativeClick = false;
  function press(e: PointerEvent) {
    pressBtn = store.mode === 'hover' && e.ctrlKey ? (e.target as HTMLElement).closest('button') : null;
    nativeClick = false;
  }
  function seen() {
    nativeClick = true;
  }
  function up() {
    if (drag) source.dragEnd?.();
    drag = null;
    const b = pressBtn;
    pressBtn = null;
    if (b) setTimeout(() => nativeClick || b.click(), 40);
  }
  function key(e: KeyboardEvent) {
    if (e.key === 'Escape' && store.mode === 'interactive') source.escape?.();
  }
</script>

<svelte:window onkeydown={key} />

{#if store.showPill}
  <div class="ovp" {style} style:width="100%" style:height="100%"><Pill {store} {source} /></div>
{:else}
  <div role="presentation" bind:this={root} onpointerdown={(e) => { press(e); down(e); }} onclickcapture={seen} oncontextmenu={(e) => e.preventDefault()} onpointermove={move} onpointerup={up} onpointercancel={up} class="ov" class:interactive={store.mode === 'interactive'} class:moving={store.mode === 'moving'} class:hover={store.mode === 'hover'} {style} data-mode={store.mode}>
    <div class="scrim" style={scrimStyle} aria-hidden="true">{#each rowStyles as rs}<i style={rs}></i>{/each}</div>
    <Header {store} {source} />
    <Subtitle {store} />
    <Answer {store} {source} />
    <MyAnswer {store} />
    {#if store.mode === 'interactive'}<InputRow {store} {source} />{/if}
    <Hints {store} />
    {#if store.sentinel}<div class="sentinel"></div>{/if}
  </div>
{/if}
