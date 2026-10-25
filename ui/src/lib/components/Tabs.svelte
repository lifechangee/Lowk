<script lang="ts" generics="T extends string">
  // Page tabs (folder: Calls / Documents / Instructions; call: Transcript / Summary / ...). Arrow keys move between tabs.
  import type { Snippet } from 'svelte';
  import { t } from '../i18n/locale.svelte';
  type Tab = { id: T; label: string; count?: number | null };
  let { tabs, value, label, onselect, trailing }: { tabs: Tab[]; value: T; label: string; onselect: (id: T) => void; trailing?: Snippet } = $props();

  function key(e: KeyboardEvent, i: number) {
    const d = e.key === 'ArrowRight' ? 1 : e.key === 'ArrowLeft' ? -1 : 0;
    if (!d) return;
    e.preventDefault();
    const n = tabs[(i + d + tabs.length) % tabs.length]!;
    onselect(n.id);
    queueMicrotask(() => (e.currentTarget as HTMLElement | null)?.parentElement?.querySelector<HTMLElement>(`[data-tab="${n.id}"]`)?.focus());
  }

  // Many tabs scroll sideways (scrollbar hidden): keep the open tab in view and fade the edge that hides more tabs.
  let list = $state<HTMLElement>();
  let more = $state({ left: false, right: false });
  function edges() {
    if (!list) return;
    more = { left: list.scrollLeft > 1, right: list.scrollLeft + list.clientWidth < list.scrollWidth - 1 };
  }
  /** A mouse wheel only turns vertically: on an overflowing tab bar it scrolls sideways (trackpads already send deltaX). */
  function wheel(e: WheelEvent) {
    if (!list || list.scrollWidth <= list.clientWidth || Math.abs(e.deltaY) <= Math.abs(e.deltaX)) return;
    e.preventDefault();
    list.scrollLeft += e.deltaY;
  }
  const nudge = (d: number) => list?.scrollBy({ left: d * Math.max(120, list.clientWidth * 0.6), behavior: 'smooth' });
  $effect(() => {
    void value;
    list?.querySelector<HTMLElement>(`[data-tab="${value}"]`)?.scrollIntoView({ block: 'nearest', inline: 'nearest' });
    edges();
  });
</script>

<svelte:window onresize={edges} />

<div class="tabsbar">
  <div class="cin tabrow">
    {#if more.left || more.right}<button class="arr" type="button" data-testid="tabs-prev" aria-label={t('m.tabs.prev')} disabled={!more.left} onclick={() => nudge(-1)}>‹</button>{/if}
    <div class="tabs" class:fl={more.left} class:fr={more.right} role="tablist" aria-label={label} bind:this={list} onscroll={edges} onwheel={wheel}>
      {#each tabs as tab, i (tab.id)}
        <button class="tab" class:on={value === tab.id} role="tab" data-tab={tab.id} aria-selected={value === tab.id} tabindex={value === tab.id ? 0 : -1} onclick={() => onselect(tab.id)} onkeydown={(e) => key(e, i)}>
          <span>{tab.label}</span>{#if tab.count !== undefined && tab.count !== null}<small>{tab.count}</small>{/if}
        </button>
      {/each}
    </div>
    {#if more.left || more.right}<button class="arr" type="button" data-testid="tabs-next" aria-label={t('m.tabs.next')} disabled={!more.right} onclick={() => nudge(1)}>›</button>{/if}
    {#if trailing}<div class="tools">{@render trailing()}</div>{/if}
  </div>
</div>

<style>
  .tabsbar { border-bottom: 1px solid var(--line); flex: none; }
  .cin { max-width: 768px; width: 100%; margin: 0 auto; padding: 0 var(--s8); }
  .tabrow { display: flex; align-items: center; height: 44px; gap: var(--s4); }
  .tabs { display: flex; gap: var(--s1); height: 100%; min-width: 0; overflow-x: auto; scrollbar-width: none; }
  .tabs.fr { mask-image: linear-gradient(to right, #000 calc(100% - 40px), transparent); }
  .tabs.fl { mask-image: linear-gradient(to left, #000 calc(100% - 40px), transparent); }
  .tabs.fl.fr { mask-image: linear-gradient(to right, transparent, #000 40px, #000 calc(100% - 40px), transparent); }
  .tab { all: unset; box-sizing: border-box; cursor: pointer; display: inline-flex; align-items: center; gap: var(--s2); height: 44px; padding: 0 var(--s3); color: var(--tx2); font-size: 13px; border-bottom: 2px solid transparent; margin-bottom: -1px; white-space: nowrap; flex: none; }
  .tab:hover { color: var(--tx); }
  .tab:focus-visible { outline: 2px solid var(--focus); outline-offset: -2px; border-radius: var(--r); }
  .tab.on { color: var(--tx); border-color: var(--ac); font-weight: 550; }
  small { font-size: 11px; line-height: 16px; color: var(--tx3); font-weight: 400; }
  .arr { all: unset; box-sizing: border-box; cursor: pointer; flex: none; width: 24px; height: 24px; border-radius: var(--r); display: inline-flex; align-items: center; justify-content: center; color: var(--tx2); font-size: 18px; line-height: 1; }
  .arr:hover:not(:disabled) { background: var(--hv); color: var(--tx); }
  .arr:disabled { visibility: hidden; }
  .arr:focus-visible { outline: 2px solid var(--focus); }
  .tools { margin-left: auto; display: flex; gap: var(--s2); flex: none; }
</style>
