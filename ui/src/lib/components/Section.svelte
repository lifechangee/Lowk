<script lang="ts">
  import type { Snippet } from 'svelte';
  // Small uppercase section label (the mockup's `.h6` / `.sec h3`).
  let { title, children }: { title: string; children?: Snippet } = $props();
</script>

<div class="sec"><h3 class="lbl">{title}</h3>{#if children}<span class="act">{@render children()}</span>{/if}</div>

<style>
  .sec { display: flex; justify-content: space-between; align-items: baseline; gap: var(--s3); margin: 0 0 var(--s3); min-width: 0; }
  h3 { margin: 0; font-size: 11px; line-height: 16px; letter-spacing: 0.08em; text-transform: uppercase; color: var(--tx3); font-weight: 600; }
  .act { font-size: 12px; color: var(--tx2); flex: none; }
</style>
