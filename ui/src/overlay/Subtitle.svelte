<script lang="ts">
  import type { OverlayStore } from './store.svelte';
  import { ot } from './i18n';
  import { follow } from './follow';

  let { store }: { store: OverlayStore } = $props();
  const lang = $derived(store.lang);
  let more = $state(false);
  let hist: HTMLDivElement | undefined = $state();
  const opts = { idleMs: 4000, onState: (s: { overflow: boolean; atEnd: boolean }) => (more = s.overflow && !s.atEnd) };
</script>

<div class="sub" data-blk>
  {#if store.lines.length}
    <!-- Short scrollable history; always follows the newest line. Scrolling is possible only while the overlay is clickable (hold ⌃ / ⌘⇧I). -->
    <div class="hist" bind:this={hist} use:follow={opts} data-testid="sub-hist">
      {#each store.lines as l (l.stream + ':' + l.utt)}
        <div class="txt ln" class:old={l !== store.sub} data-utt={l.utt}>
          <span class="spk {l.stream === 'me' ? 'me' : 'sys'}">{ot(l.stream === 'me' ? 'overlay.me' : 'overlay.system', lang)}</span>{l.text}{#if l.partial}<span class="caret"></span>{/if}
        </div>
      {/each}
    </div>
    {#if more}<button class="more" tabindex="-1" onwheel={(e) => hist?.scrollBy(0, e.deltaY)} onclick={() => hist?.dispatchEvent(new Event('lowk-jump'))}>{ot('overlay.more.sub', lang)}</button>{/if}
  {:else if store.sttLoading}
    <span class="wait" role="status" data-testid="stt-loading">{ot('overlay.sttLoading', lang)}</span>
  {:else if store.sttError}
    <span class="wait err" role="alert">{ot(store.sttError.recoverable ? 'overlay.sttRestarting' : 'overlay.sttFailed', lang)}</span>
  {:else}
    <span class="wait">{ot('overlay.waiting', lang)}</span>
  {/if}
</div>
