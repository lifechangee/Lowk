<script lang="ts">
  import type { OverlayStore } from './store.svelte';
  import { ot } from './i18n';

  let { store }: { store: OverlayStore } = $props();
  const lang = $derived(store.lang);
</script>

<div class="sub" data-blk>
  {#if store.sub}
    <div class="txt">
      <span class="spk {store.sub.stream === 'me' ? 'me' : 'sys'}">{ot(store.sub.stream === 'me' ? 'overlay.me' : 'overlay.system', lang)}</span>{store.sub.text}{#if store.sub.partial}<span class="caret"></span>{/if}
    </div>
  {:else}
    <span class="wait">{ot('overlay.waiting', lang)}</span>
  {/if}
</div>
