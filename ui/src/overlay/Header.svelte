<script lang="ts">
  import type { OverlayStore } from './store.svelte';
  import type { OverlaySource } from './source';
  import { formatElapsed } from './store.svelte';
  import { ot } from './i18n';

  let { store, source }: { store: OverlayStore; source: OverlaySource } = $props();
  const lang = $derived(store.lang);
  const modeKey = $derived(
    store.mode === 'interactive'
      ? 'overlay.mode.interactive'
      : store.mode === 'moving'
        ? 'overlay.mode.moving'
        : store.mode === 'hover'
          ? 'overlay.mode.hover'
          : 'overlay.mode.passive',
  );
  const tplName = $derived(store.template ? store.template.names[lang] : '');
  const slotKey = (i: number) => (store.isMac ? '⌥' : 'Alt+') + (i + 1);
  // G6: hold-Ctrl (hover) makes the header as clickable as the typing mode, minus the keyboard.
  const interactive = $derived(store.mode === 'interactive' || store.mode === 'hover');

  function toggle() {
    if (interactive) store.menuOpen = !store.menuOpen;
  }
  function pick(id: string) {
    store.selectTemplate(id);
    source.setTemplate(id);
  }
  function outside(e: PointerEvent) {
    if (store.menuOpen && !(e.target as HTMLElement).closest('.tpl-wrap')) store.menuOpen = false;
  }
</script>

<svelte:window onpointerdown={outside} />

<div class="head" data-drag>
  {#if store.phase === 'recording'}
    <div class="rec" data-blk><i></i><span class="lbl">{formatElapsed(store.elapsedMs)}</span></div>
  {:else}
    <div class="rec off" data-blk><i></i><span class="lbl">{ot(store.phase === 'paused' ? 'overlay.state.paused' : 'overlay.state.idle', lang)}</span></div>
  {/if}
  {#if store.phase === 'recording' || store.phase === 'paused'}
    <!-- F8: one click, no confirmation. Click-through in passive mode; the record bind keeps working. -->
    <button class="stop" data-blk type="button" title={ot('overlay.action.stop', lang)} aria-label={ot('overlay.action.stop', lang)} onclick={() => source.stopRecording?.()}>
      <svg width="10" height="10" viewBox="0 0 8 8" aria-hidden="true"><rect width="8" height="8" rx="1.5" fill="currentColor" /></svg>
    </button>
  {/if}
  <span class="div"></span>
  <div class="tpl-wrap" data-blk>
    <button class="tpl" type="button" aria-haspopup="menu" aria-expanded={store.menuOpen} aria-label={ot('overlay.template', lang)} onclick={toggle}>
      <span class="lbl">{tplName}</span>
      <svg class="caret-d" width="9" height="9" viewBox="0 0 10 10" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"><path d="M2 3.5 5 6.5 8 3.5" /></svg>
    </button>
    {#if store.menuOpen}
      <div class="menu" role="menu">
        {#each store.templates as tpl, i (tpl.id)}
          <button type="button" role="menuitem" class:sel={tpl.id === store.templateId} onclick={() => pick(tpl.id)}>
            <span class="ck">{tpl.id === store.templateId ? '✓' : ''}</span>
            <span class="nm lbl">{tpl.names[lang]}</span>
            {#if tpl.needs_my_answer}<span class="new">{ot('overlay.new', lang)}</span>{/if}
            <small>{slotKey(i)}</small>
          </button>
        {/each}
      </div>
    {/if}
  </div>
  <span class="sp"></span>
  {#if store.shots > 0}
    <span class="chip" data-blk>
      <svg width="13" height="13" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"><path d="M2 5V3.5A1.5 1.5 0 0 1 3.5 2H5M11 2h1.5A1.5 1.5 0 0 1 14 3.5V5M14 11v1.5a1.5 1.5 0 0 1-1.5 1.5H11M5 14H3.5A1.5 1.5 0 0 1 2 12.5V11" /><circle cx="8" cy="8" r="2" /></svg>
      <span class="shots-wrap">{#each store.thumbs as t, i (i)}{#if t.startsWith('data:')}<img class="thumb" src={t} alt="" />{:else}<i class="thumb t{i % 3}"></i>{/if}{/each}</span>
      <span class="lbl">{ot('overlay.shots', lang, { n: store.shots })}</span>
      <button class="clr" type="button" title={ot('overlay.shots.clear', lang)} aria-label={ot('overlay.shots.clear', lang)} onclick={() => source.clearQueue?.()}>
        <svg width="8" height="8" viewBox="0 0 8 8" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" aria-hidden="true"><path d="M1 1l6 6M7 1 1 7" /></svg>
      </button>
    </span>
  {/if}
  <span class="mode" data-blk><i></i><span class="lbl">{ot(modeKey, lang)}</span></span>
</div>
