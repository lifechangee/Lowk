<script lang="ts">
  import type { OverlayStore } from './store.svelte';
  import { ot } from './i18n';
  import { myAnswerView } from './my-answer';
  import type { OverlaySource } from './source';
  import type { OKey } from './i18n';

  let { store, source }: { store: OverlayStore; source: OverlaySource } = $props();
  const lang = $derived(store.lang);
  const view = $derived(myAnswerView(store.myAnswerState, store.myAnswerElapsedMs));
  const titleKeys: Record<'before' | 'listening' | 'paused' | 'limit' | 'closed', OKey> = {
    before: 'overlay.my.state.before', listening: 'overlay.my.state.listening', paused: 'overlay.my.state.paused',
    limit: 'overlay.my.state.limit', closed: 'overlay.my.state.closed',
  };
  let expanded = $state(false);
</script>

<div class="my" data-blk class:expanded class:dimmed={view.dimmed} class:interactive={store.mode === 'interactive'} class:limited={view.body === 'limit'} role={store.mode === 'interactive' ? 'group' : undefined} tabindex={store.mode === 'interactive' ? 0 : undefined} data-testid="my-answer" onclick={() => store.mode === 'interactive' && (expanded = !expanded)} onkeydown={(e) => {
  if (e.key === 'Escape' && store.mode === 'interactive' && store.myAnswerState === 'listening') {
    e.preventDefault(); e.stopPropagation(); source.doneMyAnswer?.();
  } else if (!((e.target as HTMLElement).closest('button')) && (e.key === 'Enter' || e.key === ' ')) {
    e.preventDefault(); expanded = !expanded;
  }
}}>
  <div class="my-content">
    <span class="t lbl" data-gt>{#if view.dot}<i class="my-dot"></i>{/if}{ot(titleKeys[view.title], lang, { elapsed: view.elapsed })}</span>
    <span class="tx" data-gt class:ph={!store.myAnswer && !store.myAnswerPartial}>
      {#if view.body === 'hotkey'}{ot('overlay.my.afterAsk', lang, { key: store.keys.ask })}
      {:else if view.body === 'paused'}{ot('overlay.my.paused', lang)}
      {:else}
        {store.myAnswer}{#if store.myAnswer && store.myAnswerPartial}{' '}{/if}<span class="partial">{store.myAnswerPartial}</span>
      {/if}
    </span>
    {#if view.body === 'limit'}<span class="limit-note" data-gt>{ot('overlay.my.limit', lang)}</span>{/if}
  </div>
  {#if store.myAnswerState === 'listening' || store.myAnswerState === 'paused'}
    <button type="button" class="answer-action my-done" data-testid="my-done" onclick={(e) => { e.stopPropagation(); source.doneMyAnswer?.(); }}>{ot('overlay.my.done', lang)}</button>
  {/if}
</div>
