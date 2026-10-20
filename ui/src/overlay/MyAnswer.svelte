<script lang="ts">
  import type { OverlayStore } from './store.svelte';
  import { ot } from './i18n';

  let { store }: { store: OverlayStore } = $props();
  const lang = $derived(store.lang);
</script>

<div class="my" data-blk>
  <span class="t lbl">↳ {ot('overlay.my.title', lang)}</span>
  <span class="tx" class:ph={!store.myAnswer}>{store.myAnswer || ot('overlay.my.empty', lang)}</span>
</div>
