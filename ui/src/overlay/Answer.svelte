<script lang="ts">
  import type { OverlayStore } from './store.svelte';
  import { splitBold } from './store.svelte';
  import { ot } from './i18n';

  let { store }: { store: OverlayStore } = $props();
  const lang = $derived(store.lang);
  const parts = $derived(splitBold(store.answer));
  const order = ['question', 'hint', 'my'] as const;
  const idx = $derived(order.indexOf(store.stage));
  const labels = ['overlay.chain.question', 'overlay.chain.hint', 'overlay.chain.my'] as const;
  let body: HTMLDivElement | undefined = $state();

  // Keep the newest text visible while streaming (fixed-size area, no scrollbar).
  $effect(() => {
    store.answer;
    if (body) body.scrollTop = body.scrollHeight;
  });
</script>

<div class="ans" data-blk>
  <div class="chain">
    {#each labels as l, i (l)}
      {#if i}<em>›</em>{/if}
      <span class:on={i === idx} class:done={i < idx}>{ot(l, lang)}</span>
    {/each}
    <b class="mdl lbl">{store.model}</b>
  </div>
  <div class="body" bind:this={body}>
    {#if store.answerError}
      <span class="err">{ot('overlay.answer.error', lang)}</span>
    {:else if !store.hasAnswered}
      <span class="empty">{ot('overlay.answer.empty', lang, { key: store.keys.ask })}</span>
    {:else}
      {#each parts as p, i (i)}{#if p.bold}<b>{p.text}</b>{:else}{p.text}{/if}{/each}{#if store.streaming}<span class="caret"></span>{/if}
    {/if}
  </div>
</div>
