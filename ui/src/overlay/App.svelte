<script lang="ts">
  import '../lib/tokens.css';
  import { onMount } from 'svelte';
  import { OverlayStore } from './store.svelte';
  import { LazyTauriSource } from './lazy';
  import { restoreSnapshot, saveSnapshot } from './snapshot';
  import Overlay from './Overlay.svelte';

  const inTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
  const store = new OverlayStore();
  store.isMac = /mac/i.test(navigator.platform);
  // First paint must not wait for the backend: start from the last known look, then the bridge fills in.
  store.pillWindow = location.hash === '#pill';
  restoreSnapshot(store);
  const source = new LazyTauriSource();
  // Dev harness (mock demo + test backgrounds) is tree-shaken from release builds.
  const dev = import.meta.env.DEV && !inTauri ? import('./DevStage.svelte') : null;

  onMount(() => {
    // First content is on screen after the next frame; the marker is for tests and diagnostics.
    requestAnimationFrame(() => {
      performance.mark('overlay-first-content');
      document.documentElement.dataset.ready = '1';
    });
    if (!inTauri) return;
    source.start(store);
    const save = setInterval(() => saveSnapshot(store), 2000);
    const onHide = () => saveSnapshot(store);
    addEventListener('pagehide', onHide);
    return () => {
      clearInterval(save);
      removeEventListener('pagehide', onHide);
      source.stop();
    };
  });
</script>

{#if dev}
  {#await dev then m}
    <m.default />
  {/await}
{:else}
  <div class="root"><Overlay {store} {source} /></div>
{/if}

<style>
  .root { position: fixed; inset: 0; }
</style>
