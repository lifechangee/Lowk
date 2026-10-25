<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { area as ruA } from '../lib/i18n/ru/area';
  import { area as enA } from '../lib/i18n/en/area';
  import { clamp, isClick, normalize, type Pt } from './geometry';

  const dicts: Record<string, Record<string, string>> = { ru: ruA, en: enA };
  let lang = $state(navigator.language.toLowerCase().startsWith('ru') ? 'ru' : 'en');
  let keys = $state(true);
  const tr = (k: string, p: Record<string, string | number> = {}) =>
    ((dicts[lang]?.[k]) ?? k).replace(/\{(\w+)\}/g, (m, n: string) => (n in p ? String(p[n]) : m));

  let start = $state<Pt | null>(null);
  let cur = $state<Pt>([0, 0]);
  // After a confirm / cancel the page clears itself at once (the native window hides a moment later).
  let done = $state(false);
  // Own crosshair, drawn inside this capture-excluded window: the OS cursor stays a plain arrow because the
  // system cursor (any shape change) is composited into screen shares.
  let ptr = $state<Pt | null>(null);
  let vw = $state(innerWidth);
  let vh = $state(innerHeight);

  const box = $derived(start ? clamp(normalize(start, cur), vw, vh) : null);
  const size = $derived(box ? tr('area.size', { w: Math.round(box.w * devicePixelRatio), h: Math.round(box.h * devicePixelRatio) }) : '');
  // The size label sits below the rectangle, above it when the bottom edge is too close.
  const labelTop = $derived(box ? (box.y + box.h + 34 > vh ? box.y - 30 : box.y + box.h + 6) : 0);
  const labelLeft = $derived(box ? Math.min(Math.max(box.x, 4), Math.max(4, vw - 120)) : 0);

  function reset() {
    start = null;
    done = false;
  }
  function cancel() {
    if (done) return;
    done = true;
    start = null;
    void invoke('area_cancel').catch(() => {});
  }
  function confirm() {
    if (done || !box || box.w < 1 || box.h < 1) return;
    const r = { ...box };
    done = true;
    start = null;
    void invoke('area_confirm', { rect: r }).catch(() => {});
  }
  const pt = (e: PointerEvent): Pt => [e.clientX, e.clientY];

  function down(e: PointerEvent) {
    if (done) return;
    ptr = pt(e);
    if (e.button !== 0) return cancel();
    start = pt(e);
    cur = start;
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
  }
  function move(e: PointerEvent) {
    ptr = pt(e);
    if (start) cur = pt(e);
  }
  function up(e: PointerEvent) {
    if (!start || e.button !== 0) return;
    cur = pt(e);
    if (isClick(start, cur)) cancel();
    else confirm();
  }
  function key(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      e.preventDefault();
      cancel();
    } else if (e.key === 'Enter') {
      e.preventDefault();
      confirm();
    }
  }

  onMount(() => {
    const un = listen<{ lang: string; keys: boolean }>('area://open', (e) => {
      if (e.payload.lang in dicts) lang = e.payload.lang;
      keys = e.payload.keys;
      vw = innerWidth;
      vh = innerHeight;
      reset();
    });
    return () => void un.then((f) => f());
  });
</script>

<svelte:window onkeydown={key} onresize={() => ((vw = innerWidth), (vh = innerHeight))} />

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="sel"
  class:done
  role="application"
  aria-label={tr('area.aria')}
  onpointerdown={down}
  onpointermove={move}
  onpointerup={up}
  oncontextmenu={(e) => (e.preventDefault(), cancel())}
>
  {#if ptr && !done}
    <div class="ch ch-v" data-testid="crosshair-v" style="left:{ptr[0]}px"></div>
    <div class="ch ch-h" data-testid="crosshair-h" style="top:{ptr[1]}px"></div>
  {/if}
  {#if box}
    <div class="rect" data-testid="rect" style="left:{box.x}px;top:{box.y}px;width:{box.w}px;height:{box.h}px"></div>
    <div class="size" data-testid="size" style="left:{labelLeft}px;top:{labelTop}px">{size}</div>
  {:else}
    <div class="dim"></div>
    <div class="hint" data-testid="hint">{tr(keys ? 'area.hint' : 'area.hintMouse')}</div>
  {/if}
</div>

<style>
  .sel { position: fixed; inset: 0; cursor: default; touch-action: none; }
  .ch { position: absolute; pointer-events: none; background: rgba(255, 255, 255, 0.85); box-shadow: 0 0 0 1px rgba(0, 0, 0, 0.5); }
  .ch-v { top: 0; bottom: 0; width: 1px; }
  .ch-h { left: 0; right: 0; height: 1px; }
  .sel.done { visibility: hidden; }
  .dim { position: absolute; inset: 0; background: rgba(0, 0, 0, 0.4); }
  .rect {
    position: absolute;
    box-sizing: border-box;
    border: 1px solid #fff;
    outline: 1px solid rgba(0, 0, 0, 0.55);
    box-shadow: 0 0 0 100vmax rgba(0, 0, 0, 0.4);
  }
  .size, .hint {
    position: absolute;
    padding: 3px 8px;
    border-radius: 6px;
    background: rgba(20, 20, 22, 0.92);
    color: #fff;
    font: 12px/18px -apple-system, 'Segoe UI', system-ui, sans-serif;
    white-space: nowrap;
    pointer-events: none;
  }
  .hint { left: 50%; top: 50%; transform: translate(-50%, -50%); font-size: 14px; padding: 6px 12px; }
</style>
