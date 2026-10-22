<script lang="ts">
  import type { OverlayStore } from './store.svelte';
  import { ot } from './i18n';

  let { store }: { store: OverlayStore } = $props();
  const lang = $derived(store.lang);
</script>

<div class="my" data-blk>
  <span class="t lbl" data-gt>↳ {ot('overlay.my.title', lang)}</span>
  <span class="tx" data-gt class:ph={!store.myAnswer}>{store.myAnswer || ot('overlay.my.empty', lang)}</span>
</div>
