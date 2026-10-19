<script lang="ts">
  import type { Snippet } from 'svelte';
  // Radio-style selectable card (onboarding, appearance).
  let { selected, title, hint, badge, onselect, children }: { selected: boolean; title: string; hint?: string; badge?: string; onselect: () => void; children?: Snippet } = $props();
</script>

<div class="opt" class:on={selected} role="radio" aria-checked={selected} tabindex="0" onclick={onselect} onkeydown={(e) => (e.key === 'Enter' || e.key === ' ') && (e.preventDefault(), onselect())}>
  <span class="rd"></span>
  <div class="body"><b>{title}</b>{#if hint}<small>{hint}</small>{/if}{#if children}{@render children()}{/if}</div>
  {#if badge}<span class="badge lbl">{badge}</span>{/if}
</div>

<style>
  .opt { display: flex; gap: var(--s3); align-items: flex-start; border: 1px solid var(--line); background: var(--panel); border-radius: var(--r-lg); padding: var(--s3) var(--s4); cursor: pointer; margin-bottom: var(--s2); transition: 0.15s; min-width: 0; }
  .opt:hover { border-color: var(--tx3); }
  .opt:focus-visible { outline: 2px solid var(--ac); }
  .on { border-color: var(--ac); background: var(--acbg); }
  .rd { width: 16px; height: 16px; border-radius: 50%; border: 1.5px solid var(--tx3); flex: none; margin-top: 2px; position: relative; }
  .on .rd { border-color: var(--ac); }
  .on .rd::after { content: ''; position: absolute; inset: 3px; border-radius: 50%; background: var(--ac); }
  .body { flex: 1; min-width: 0; }
  b { display: block; font-weight: 600; }
  small { display: block; color: var(--tx2); font-size: 12px; line-height: 16px; margin-top: 2px; }
  .badge { flex: none; max-width: 40%; font-size: 11px; line-height: 16px; padding: 0 var(--s2); border-radius: 99px; background: var(--acbg); color: var(--ac); font-weight: 600; }
</style>
