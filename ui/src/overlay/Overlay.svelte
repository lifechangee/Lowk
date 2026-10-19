<script lang="ts">
  import './overlay.css';
  import type { OverlayStore } from './store.svelte';
  import type { OverlaySource } from './source';
  import { varsToStyle } from './tone';
  import { keyLabels } from './keys';
  import { ot } from './i18n';
  import Header from './Header.svelte';
  import Subtitle from './Subtitle.svelte';
  import Answer from './Answer.svelte';
  import MyAnswer from './MyAnswer.svelte';
  import InputRow from './InputRow.svelte';
  import Hints from './Hints.svelte';
  import Pill from './Pill.svelte';

  let { store, source }: { store: OverlayStore; source: OverlaySource } = $props();
  const style = $derived(varsToStyle(store.tone, store.k, store.fontPx));
</script>

{#if store.showPill}
  <div class="ovp" {style} style:width="100%" style:height="100%"><Pill {store} {source} /></div>
{:else}
  <div class="ov" class:interactive={store.mode === 'interactive'} class:moving={store.mode === 'moving'} {style} data-mode={store.mode}>
    <Header {store} {source} />
    <Subtitle {store} />
    <Answer {store} />
    <MyAnswer {store} />
    {#if store.mode === 'interactive'}<InputRow {store} {source} />{/if}
    <Hints {store} />
    {#if store.mode === 'moving'}
      <div class="badge">{ot('overlay.move.badge', store.lang, { key: keyLabels(store.isMac).move })}</div>
    {/if}
    {#if store.toast}<div class="toast">{store.toast}</div>{/if}
  </div>
{/if}
