<script lang="ts">
  import type { OverlayStore } from './store.svelte';
  import { keyLabels } from './keys';
  import { ot } from './i18n';

  let { store }: { store: OverlayStore } = $props();
  const lang = $derived(store.lang);
  const k = $derived(keyLabels(store.isMac));
</script>

<div class="foot" data-blk>
  <div class="hints">
    {#if store.mode === 'interactive'}
      <span><kbd>Esc</kbd><span class="lbl">{ot('overlay.hint.exit', lang)}</span></span>
      <span><kbd>↵</kbd><span class="lbl">{ot('overlay.hint.send', lang)}</span></span>
    {:else}
      <span><kbd>{k.interactive}</kbd><span class="lbl">{ot('overlay.hint.interactive', lang)}</span></span>
      <span><kbd>{k.ask}</kbd><span class="lbl">{ot('overlay.hint.ask', lang)}</span></span>
      <span><kbd>{k.shot}</kbd><span class="lbl">{ot('overlay.hint.shot', lang)}</span></span>
    {/if}
  </div>
  <!-- Moving badge and toast live in this row (in flow): they can never hang outside the window or cover «Мой ответ». -->
  <div class="hints r2">
    {#if store.toast}
      <span class="toast" role="status">{store.toast}</span>
    {:else if store.mode === 'moving'}
      <span class="badge" role="status">{ot('overlay.move.badge', lang, { key: k.move })}</span>
    {:else}
      <span class="lbl">{ot('overlay.hint.move', lang, { key: k.move })}</span>
    {/if}
  </div>
</div>
