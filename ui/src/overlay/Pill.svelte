<script lang="ts">
  import type { OverlayStore } from './store.svelte';
  import type { OverlaySource } from './source';
  import { ot } from './i18n';

  let { store, source }: { store: OverlayStore; source: OverlaySource } = $props();
  const vertical = $derived(store.pillEdge === 'left' || store.pillEdge === 'right');
  const label = $derived(ot('overlay.pill.start', store.lang));
</script>

<button class="pill" class:v={vertical} type="button" title={label} aria-label={label} onclick={() => source.toggleRecord()}>
  <i></i>
  {#if !vertical}<span class="lbl">{label}</span>{/if}
</button>
