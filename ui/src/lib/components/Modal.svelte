<script lang="ts">
  import type { Snippet } from 'svelte';
  import Icon from './Icon.svelte';
  type Props = { open: boolean; title?: string; width?: number; align?: 'top' | 'center'; closeLabel: string; onclose: () => void; children: Snippet; footer?: Snippet };
  let { open, title, width = 480, align = 'center', closeLabel, onclose, children, footer }: Props = $props();
</script>

<svelte:window onkeydown={(e) => open && e.key === 'Escape' && onclose()} />

{#if open}
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <div class="scrim" class:top={align === 'top'} onclick={(e) => e.target === e.currentTarget && onclose()}>
    <div class="modal" style:width="min({width}px, calc(100% - 32px))" role="dialog" aria-modal="true" aria-label={title}>
      {#if title}
        <div class="mh"><b class="lbl">{title}</b><button class="x" aria-label={closeLabel} onclick={onclose}><Icon name="close" size={14} /></button></div>
      {/if}
      <div class="mb">{@render children()}</div>
      {#if footer}<div class="mf">{@render footer()}</div>{/if}
    </div>
  </div>
{/if}

<style>
  .scrim { position: fixed; inset: 0; background: rgba(0, 0, 0, 0.45); display: flex; justify-content: center; align-items: center; z-index: 50; }
  .top { align-items: flex-start; padding-top: 96px; }
  .modal { background: var(--panel); border: 1px solid var(--line); border-radius: var(--r-xl); box-shadow: var(--sh); max-height: calc(100vh - 48px); display: flex; flex-direction: column; overflow: hidden; }
  .mh { display: flex; justify-content: space-between; align-items: center; gap: var(--s3); padding: var(--s4) var(--s6) 0; }
  .mh b { font-size: 16px; line-height: 24px; }
  .x { all: unset; cursor: pointer; width: 28px; height: 28px; display: grid; place-items: center; border-radius: var(--r); color: var(--tx2); flex: none; }
  .x:hover { background: var(--hv); color: var(--tx); }
  .mb { padding: var(--s4) var(--s6) var(--s6); overflow: auto; min-height: 0; }
  .mf { padding: var(--s3) var(--s6); border-top: 1px solid var(--line); display: flex; gap: var(--s2); justify-content: flex-end; }
</style>
