<script lang="ts">
  import type { OverlayStore } from './store.svelte';
  import type { OverlaySource } from './source';
  import type { SourceRef } from '../lib/ipc/bindings';
  import { splitBold } from './store.svelte';
  import { ot } from './i18n';

  let { store, source }: { store: OverlayStore; source?: OverlaySource } = $props();
  const errKey = $derived(store.errorCode === 'api_key_missing' ? 'overlay.answer.noKey' : store.errorCode === 'local_only_blocked' ? 'overlay.answer.localOnly' : 'overlay.answer.error');
  function label(s: SourceRef): string {
    if (s.kind === 'instructions') return ot('overlay.src.instructions', lang);
    const where = s.page != null ? `, ${ot('overlay.src.page', lang, { n: s.page })}` : s.at_s != null ? `, ${Math.floor(s.at_s / 60)}:${String(s.at_s % 60).padStart(2, '0')}` : '';
    return `${s.title}${where}`;
  }
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
      <span class="err">{ot(errKey, lang)}</span>
      {#if store.errorCode === 'api_key_missing'}<button class="lnk" onclick={() => source?.openSettings?.()}>{ot('overlay.answer.openSettings', lang)}</button>{/if}
    {:else if !store.hasAnswered}
      <span class="empty">{ot('overlay.answer.empty', lang, { key: store.keys.ask })}</span>
    {:else}
      {#each parts as p, i (i)}{#if p.bold}<b>{p.text}</b>{:else}{p.text}{/if}{/each}{#if store.streaming}<span class="caret"></span>{/if}
    {/if}
  </div>
  {#if store.sources.length || store.withheld}
    <div class="srcs">
      {#each store.sources as s, i (i)}<button class="chip lbl" title={s.title} onclick={() => source?.openSource?.(s)}>{label(s)}</button>{/each}
      {#if store.withheld}<span class="chip held">{ot('overlay.src.withheld', lang, { n: store.withheld })}</span>{/if}
    </div>
  {/if}
</div>
