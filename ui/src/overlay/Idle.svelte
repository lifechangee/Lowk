<script lang="ts">
  import type { OverlayStore } from './store.svelte';
  import type { OverlaySource } from './source';
  import { ot } from './i18n';

  let { store, source }: { store: OverlayStore; source: OverlaySource } = $props();
  const lang = $derived(store.lang);
  const key = $derived(store.keys.record);
  const hint = $derived(key ? ot('overlay.idle.start', lang, { key }) : ot('overlay.pill.start', lang));
</script>

<!-- Nothing is recorded: one honest line and a way to start. Same call as the record bind and the pill. -->
<div class="idle-body" data-testid="ov-idle" data-blk>
  <span class="lbl" data-blk>{hint}</span>
  <button class="stop go" data-blk type="button" data-testid="ov-start" onclick={() => source.toggleRecord()}>{ot('overlay.idle.button', lang)}</button>
</div>
