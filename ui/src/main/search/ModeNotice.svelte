<script lang="ts">
  // «Keyword only»: the semantic half of the index is off, missing or still building. Said out loud, never silent.
  import Icon from '../../lib/components/Icon.svelte';
  import { tm } from '../i18n';
  import { hrefs } from '../router.svelte';
  import { index } from '../state.svelte';
  import { keywordReason } from './util';

  let { compact = false }: { compact?: boolean } = $props();
  const st = $derived(index.status?.semantic ?? null);
  const reason = $derived(keywordReason(st));
  const text = $derived(
    st?.state === 'building'
      ? tm('search.mode.building', { done: st.done, total: st.total })
      : reason === 'off' ? tm('search.mode.off')
      : reason === 'model_missing' ? tm('search.mode.missing')
      : reason === 'failed' ? tm('search.mode.failed')
      : tm('search.mode.keyword'),
  );
</script>

<span class="mn" class:compact role="status" data-testid="mode-notice" title={tm('search.mode.hint')}>
  <Icon name="info" size={12} /><span class="lbl">{compact ? tm('search.mode.keyword') : text}</span>
  {#if !compact}<a href={hrefs.settings('knowledge')}>{tm('search.mode.settings')}</a>{/if}
</span>

<style>
  .mn { display: inline-flex; align-items: center; gap: var(--s2); min-width: 0; max-width: 100%; height: 24px; padding: 0 var(--s2); border-radius: var(--r-sm); border: 1px solid var(--line-strong); color: var(--tx2); font-size: 12px; line-height: 16px; }
  .mn :global(svg) { flex: none; color: var(--move); }
  a { color: var(--ac); flex: none; text-decoration: none; }
  a:hover { text-decoration: underline; }
</style>
