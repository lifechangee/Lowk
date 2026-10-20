<script lang="ts">
  import type { OverlayStore } from './store.svelte';
  import type { OverlaySource } from './source';
  import { ot } from './i18n';

  let { store, source }: { store: OverlayStore; source: OverlaySource } = $props();
  const vertical = $derived(store.pillEdge === 'left' || store.pillEdge === 'right');
  const label = $derived(ot('overlay.pill.start', store.lang));

  // G2: drag the pill anywhere (the ⌥ magnet works too, from the backend). A press that moves more than
  // a few pixels is a drag and must not start a recording; a plain click still does.
  const THRESHOLD = 4;
  let down: { x: number; y: number; dragging: boolean } | null = null;
  let suppress = false;

  function pdown(e: PointerEvent) {
    if (e.button !== 0) return;
    down = { x: e.screenX, y: e.screenY, dragging: false };
    try {
      (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    } catch {
      /* synthetic pointers (QA) have no capturable id */
    }
  }
  function pmove(e: PointerEvent) {
    if (!down) return;
    const dx = e.screenX - down.x;
    const dy = e.screenY - down.y;
    if (!down.dragging && Math.hypot(dx, dy) >= THRESHOLD) {
      down.dragging = true;
      source.pillDragStart?.();
    }
    if (down.dragging) source.pillDragTo?.(dx, dy);
  }
  function pup() {
    if (down?.dragging) {
      source.pillDragEnd?.();
      suppress = true;
      setTimeout(() => (suppress = false), 0);
    }
    down = null;
  }
  function click() {
    if (!suppress) source.toggleRecord();
  }
</script>

<button class="pill" class:v={vertical} type="button" title={label} aria-label={label} onpointerdown={pdown} onpointermove={pmove} onpointerup={pup} onpointercancel={pup} onclick={click}>
  <i></i>
  {#if !vertical}<span class="lbl">{label}</span>{/if}
</button>
