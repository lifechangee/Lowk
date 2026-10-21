<script lang="ts">
  import type { OverlayStore } from './store.svelte';
  import { ot } from './i18n';

  let { store }: { store: OverlayStore } = $props();
  const lang = $derived(store.lang);
  const k = $derived(store.keys);
</script>

<div class="foot" data-blk>
  <div class="hints">
    {#if store.mode === 'interactive'}
      <span><kbd>Esc</kbd><span class="lbl">{ot('overlay.hint.exit', lang)}</span></span>
      <span><kbd>↵</kbd><span class="lbl">{ot('overlay.hint.send', lang)}</span></span>
    {:else}
      {#if k.click}<span><kbd>{k.click}</kbd><span class="lbl">{ot('overlay.hint.click', lang)}</span></span>{/if}
      {#if k.ask}<span><kbd>{k.ask}</kbd><span class="lbl">{ot('overlay.hint.ask', lang)}</span></span>{/if}
      {#if k.complement}<span><kbd>{k.complement}</kbd><span class="lbl">{ot('overlay.hint.complement', lang)}</span></span>{/if}
      {#if k.shot}<span><kbd>{k.shot}</kbd><span class="lbl">{ot('overlay.hint.shot', lang)}</span></span>{/if}
    {/if}
  </div>
  <!-- Moving badge and toast live in this row (in flow): they can never hang outside the window or cover «Мой ответ». -->
  <div class="hints r2">
    {#if store.toast}
      <span class="toast" role="status">{store.toast}</span>
    {:else if store.audioLost}
      <span class="toast" role="alert" data-testid="audio-lost">{ot('overlay.audioLost', lang)}</span>
    {:else if store.mode === 'moving'}
      <span class="badge" role="status">{ot('overlay.move.badge', lang, { key: k.move })}</span>
    {:else if store.holdDegraded}
      <span class="lbl warn" role="status" data-testid="hold-degraded">{ot('overlay.hint.degraded', lang, { key: k.move })}</span>
    {:else}
      <span class="lbl">{ot('overlay.hint.move', lang, { key: k.move, type: k.interactive })}</span>
    {/if}
  </div>
</div>
