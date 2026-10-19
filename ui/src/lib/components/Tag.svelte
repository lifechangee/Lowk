<script lang="ts">
  import type { Snippet } from 'svelte';
  let { text, accent = false, dashed = false, onclick, children }: { text?: string; accent?: boolean; dashed?: boolean; onclick?: () => void; children?: Snippet } = $props();
</script>

{#if onclick}
  <button class="tag lbl" class:accent class:dashed {onclick}>{#if children}{@render children()}{:else}{text}{/if}</button>
{:else}
  <span class="tag lbl" class:accent class:dashed>{#if children}{@render children()}{:else}{text}{/if}</span>
{/if}

<style>
  .tag { all: unset; box-sizing: border-box; display: inline-block; max-width: 100%; height: 20px; padding: 0 var(--s2); border-radius: 99px; background: var(--hv); border: 1px solid var(--line); color: var(--tx2); font: var(--fs-xs) var(--font); line-height: 18px; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  button.tag { cursor: pointer; }
  button.tag:hover { color: var(--tx); }
  .accent { background: var(--acbg); border-color: transparent; color: var(--ac); }
  .dashed { border-style: dashed; }
  .tag:focus-visible { outline: 2px solid var(--ac); }
</style>
