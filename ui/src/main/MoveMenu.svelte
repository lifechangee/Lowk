<script lang="ts">
  // «Переместить в…»: the keyboard / context-menu alternative to drag & drop (F9).
  import { tick } from 'svelte';
  import Icon from '../lib/components/Icon.svelte';
  import { folderKey, SYS_UNSORTED } from './api';
  import { tm } from './i18n';
  import { library, ui } from './state.svelte';

  const m = $derived(ui.moveMenu);
  const call = $derived(m ? library.call(m.id) : undefined);
  const items = $derived(
    call
      ? [
          ...library.folders.map((f) => ({ key: folderKey(f.id), label: f.title, icon: 'folder' as const })),
          { key: SYS_UNSORTED, label: tm('main.unsorted'), icon: 'inbox' as const },
        ].filter((i) => i.key !== folderKey(call.folder))
      : [],
  );
  let el = $state<HTMLElement | null>(null);
  let pos = $state({ x: 0, y: 0 });

  $effect(() => {
    if (!m) return;
    const z = Number(getComputedStyle(document.documentElement).getPropertyValue('--z')) || 1;
    pos = { x: m.x / z, y: m.y / z };
    tick().then(() => {
      if (!el) return;
      const vw = innerWidth / z, vh = innerHeight / z;
      pos = { x: Math.max(8, Math.min(m.x / z, vw - el.offsetWidth - 8)), y: Math.max(8, Math.min(m.y / z, vh - el.offsetHeight - 8)) };
      el.querySelector<HTMLElement>('[role=menuitem]')?.focus();
    });
  });

  function close() { ui.moveMenu = null; }
  async function pick(key: string) {
    const id = m!.id;
    close();
    await library.move(id, key);
  }
  function onkey(e: KeyboardEvent) {
    if (!m) return;
    const btns = [...(el?.querySelectorAll<HTMLElement>('[role=menuitem]') ?? [])];
    const i = btns.indexOf(document.activeElement as HTMLElement);
    if (e.key === 'Escape') { e.preventDefault(); close(); }
    else if (e.key === 'ArrowDown') { e.preventDefault(); btns[(i + 1) % btns.length]?.focus(); }
    else if (e.key === 'ArrowUp') { e.preventDefault(); btns[(i - 1 + btns.length) % btns.length]?.focus(); }
    else if (e.key === 'Tab') close();
  }
  function onpointerdown(e: PointerEvent) { if (m && el && !el.contains(e.target as Node)) close(); }
</script>

<svelte:window onkeydown={onkey} onpointerdowncapture={onpointerdown} onblur={close} />

{#if m && call}
  <div class="mm" role="menu" aria-label={tm('m.moveMenu')} bind:this={el} style:left="{pos.x}px" style:top="{pos.y}px">
    <div class="h">{tm('m.moveMenu')}</div>
    {#each items as i (i.key)}
      <button role="menuitem" class="it" onclick={() => pick(i.key)}><Icon name={i.icon} size={14} /><span class="lbl">{i.label}</span></button>
    {/each}
  </div>
{/if}

<style>
  .mm { position: fixed; z-index: 70; min-width: 208px; max-width: 280px; max-height: 320px; overflow: auto; padding: var(--s1); background: var(--panel); border: 1px solid var(--line-strong); border-radius: var(--r-lg); box-shadow: var(--sh); }
  .h { padding: var(--s1) var(--s2); font: 600 var(--fs-xs) var(--font); letter-spacing: 0.08em; text-transform: uppercase; color: var(--tx3); }
  .it { all: unset; box-sizing: border-box; width: 100%; display: flex; align-items: center; gap: var(--s2); height: 32px; padding: 0 var(--s2); border-radius: var(--r); cursor: pointer; font-size: 13px; color: var(--tx); }
  .it :global(svg) { color: var(--tx2); }
  .it:hover, .it:focus-visible { background: var(--sel); }
  .it:focus-visible { outline: 2px solid var(--ac); outline-offset: -2px; }
</style>
