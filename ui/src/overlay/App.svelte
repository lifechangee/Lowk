<script lang="ts">
  import '../lib/tokens.css';
  import { onMount } from 'svelte';
  import { commands } from '../lib/ipc';
  import { resolveLang, setLang } from '../lib/i18n';
  import { OverlayStore } from './store.svelte';
  import { TauriSource } from './tauri';
  import Overlay from './Overlay.svelte';

  const inTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
  const store = new OverlayStore();
  store.isMac = /mac/i.test(navigator.platform);
  const source = new TauriSource();
  // Dev harness (mock demo + test backgrounds) is tree-shaken from release builds.
  const dev = import.meta.env.DEV && !inTauri ? import('./DevStage.svelte') : null;

  onMount(() => {
    if (!inTauri) return;
    source.start(store);
    void commands
      .localeGet()
      .then((l) => {
        store.lang = resolveLang('auto', l);
        setLang(store.lang);
      })
      .catch(() => {});
    return () => source.stop();
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
