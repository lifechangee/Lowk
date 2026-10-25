<script lang="ts">
  // Status chip of a downloadable model: Not downloaded -> Downloading -> Verifying -> Ready (or Error).
  import Icon from '../../lib/components/Icon.svelte';
  import { tm } from '../i18n';

  export type ModelState = 'missing' | 'downloading' | 'verifying' | 'ready' | 'error';
  let { state }: { state: ModelState } = $props();
  const text = $derived(state === 'missing' ? tm('m.model.missing') : state === 'downloading' ? tm('m.model.chip.downloading') : state === 'verifying' ? tm('m.model.verifying') : state === 'ready' ? tm('m.model.ready') : tm('m.model.chip.error'));
</script>

<span class="chip" class:on={state === 'ready'} class:bad={state === 'error'} data-testid="model-chip" data-state={state}>{#if state === 'ready'}<Icon name="check" size={12} />{/if}<span>{text}</span></span>

<style>
  .chip { display: inline-flex; align-items: center; justify-content: center; gap: var(--s1); flex: none; box-sizing: border-box; height: 20px; padding: 0 var(--s2); border-radius: 99px; background: var(--hv); color: var(--tx3); font-size: 12px; line-height: 20px; white-space: nowrap; }
  .chip.on { background: var(--acbg); color: var(--ac); }
  .chip.bad { color: var(--warn); }
  .chip span { line-height: 20px; }
</style>
