<script lang="ts">
  import type { Snippet } from 'svelte';
  // Label (+ optional hint) on the left, control on the right. Text is bounded in both languages.
  let { label, hint, children, stack = false }: { label: string; hint?: string; children?: Snippet; stack?: boolean } = $props();
</script>

<div class="row" class:stack>
  <div class="txt"><span class="l">{label}</span>{#if hint}<small>{hint}</small>{/if}</div>
  {#if children}<div class="ctl">{@render children()}</div>{/if}
</div>

<style>
  .row { display: flex; justify-content: space-between; align-items: center; min-height: 48px; padding: var(--s2) 0; border-bottom: 1px solid var(--line); gap: var(--s6); font-size: 13px; }
  .txt { min-width: 0; flex: 1; }
  .l { display: block; color: var(--tx); line-height: 20px; }
  small { display: block; color: var(--tx3); font-size: 12px; line-height: 16px; margin-top: 2px; }
  .ctl { flex: none; max-width: 55%; min-width: 0; display: flex; align-items: center; gap: var(--s2); justify-content: flex-end; }
  .stack { flex-direction: column; align-items: stretch; gap: var(--s2); }
  .stack .ctl { max-width: none; justify-content: flex-start; }
</style>
