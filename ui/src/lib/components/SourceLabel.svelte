<script lang="ts">
  // One «source» chip under an AI answer: document p. N, call mm:ss, screenshot, folder instructions.
  // A link when `href` is given (main window), a plain chip otherwise (overlay: the click is handled there).
  import type { SourceRef } from '../ipc';
  import { t } from '../i18n/locale.svelte';
  import { sourceHint, sourceHref, sourceText } from '../sourceLabel';
  import Icon, { type IconName } from './Icon.svelte';

  let { source, onopen }: { source: SourceRef; onopen?: (s: SourceRef) => void } = $props();
  const icons: Record<SourceRef['kind'], IconName> = { call: 'mic', doc: 'doc', image: 'monitor', instructions: 'pencil' };
  const text = $derived(sourceText(source, t));
</script>

{#if onopen}
  <button class="src" data-kind={source.kind} title={sourceHint(source) || text} onclick={() => onopen(source)}>
    <Icon name={icons[source.kind]} size={12} /><span class="lbl">{text}</span>{#if source.pinned}<Icon name="star-fill" size={10} />{/if}
  </button>
{:else}
  <a class="src" data-kind={source.kind} href={sourceHref(source)} title={sourceHint(source) || text}>
    <Icon name={icons[source.kind]} size={12} /><span class="lbl">{text}</span>{#if source.pinned}<Icon name="star-fill" size={10} />{/if}
  </a>
{/if}

<style>
  .src { all: unset; box-sizing: border-box; cursor: pointer; display: inline-flex; align-items: center; gap: var(--s1); height: 24px; max-width: 240px; padding: 0 var(--s2); border-radius: var(--r-sm); border: 1px solid var(--line); background: var(--hv); color: var(--tx2); font-size: 12px; line-height: 16px; min-width: 0; text-decoration: none; }
  .src:hover, .src:focus-visible { border-color: var(--ac); color: var(--tx); }
  .src :global(svg) { flex: none; }
  .src :global(svg:last-child:not(:first-child)) { color: var(--ac); }
</style>
