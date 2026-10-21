<script lang="ts" module>
  export type MenuItem = { id: string; label: string; icon?: IconName; danger?: boolean; disabled?: boolean; onselect: () => void } | { sep: true };
</script>

<script lang="ts">
  // A ⋯ button with a popup menu (page header actions, document rows). Fixed positioning so that
  // scrolling or clipping containers never cut it off; keyboard: Enter/Space opens, arrows move, Esc closes.
  import { tick } from 'svelte';
  import Icon, { type IconName } from './Icon.svelte';

  let { items, label, icon = 'more', testid }: { items: MenuItem[]; label: string; icon?: IconName; testid?: string } = $props();

  let open = $state(false);
  let btn: HTMLButtonElement | undefined = $state();
  let menu: HTMLElement | undefined = $state();
  let pos = $state({ x: 0, y: 0 });

  async function show() {
    if (!btn) return;
    const z = Number(getComputedStyle(document.documentElement).getPropertyValue('--z')) || 1;
    const r = btn.getBoundingClientRect();
    pos = { x: r.right / z, y: r.bottom / z + 4 };
    open = true;
    await tick();
    if (menu) {
      const vw = innerWidth / z, vh = innerHeight / z;
      pos = { x: Math.max(8, Math.min(pos.x - menu.offsetWidth, vw - menu.offsetWidth - 8)), y: Math.max(8, Math.min(pos.y, vh - menu.offsetHeight - 8)) };
      menu.querySelector<HTMLElement>('[role=menuitem]:not([disabled])')?.focus();
    }
  }
  function close(refocus = false) { open = false; if (refocus) btn?.focus(); }
  function outside(e: PointerEvent) { if (open && !menu?.contains(e.target as Node) && !btn?.contains(e.target as Node)) close(); }
  function key(e: KeyboardEvent) {
    if (!open) return;
    const els = [...(menu?.querySelectorAll<HTMLElement>('[role=menuitem]:not([disabled])') ?? [])];
    const i = els.indexOf(document.activeElement as HTMLElement);
    if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); close(true); }
    else if (e.key === 'ArrowDown') { e.preventDefault(); els[(i + 1) % els.length]?.focus(); }
    else if (e.key === 'ArrowUp') { e.preventDefault(); els[(i - 1 + els.length) % els.length]?.focus(); }
    else if (e.key === 'Tab') close();
  }
  function pick(it: Extract<MenuItem, { id: string }>) { close(true); it.onselect(); }
</script>

<svelte:window onpointerdowncapture={outside} onkeydowncapture={key} />

<button class="mbtn" bind:this={btn} aria-haspopup="menu" aria-expanded={open} aria-label={label} title={label} data-testid={testid} onclick={() => (open ? close() : show())}><Icon name={icon} size={14} /></button>
{#if open}
  <div class="menu pop" role="menu" aria-label={label} bind:this={menu} style:left="{pos.x}px" style:top="{pos.y}px">
    {#each items as it, i (i)}
      {#if 'sep' in it}<div class="sep"></div>
      {:else}
        <button class="mi" class:danger={it.danger} role="menuitem" disabled={it.disabled} onclick={() => pick(it)}>{#if it.icon}<Icon name={it.icon} size={14} />{/if}<span class="lbl">{it.label}</span></button>
      {/if}
    {/each}
  </div>
{/if}

<style>
  .mbtn { all: unset; box-sizing: border-box; width: 32px; height: 32px; display: grid; place-items: center; border-radius: var(--r); color: var(--tx3); cursor: pointer; flex: none; }
  .mbtn:hover, .mbtn:focus-visible, .mbtn[aria-expanded='true'] { background: var(--sel); color: var(--tx); }
  .menu { position: fixed; z-index: 70; min-width: 200px; max-width: 320px; padding: var(--s1); border-radius: var(--r-lg); background: var(--panel); border: 1px solid var(--line-strong); box-shadow: var(--sh); display: flex; flex-direction: column; }
  .mi { all: unset; box-sizing: border-box; display: flex; align-items: center; gap: var(--s2); height: 32px; padding: 0 var(--s3); border-radius: var(--r); font-size: 13px; color: var(--tx); cursor: pointer; min-width: 0; }
  .mi:hover, .mi:focus-visible { background: var(--hv); outline: none; }
  .mi[disabled] { opacity: 0.4; pointer-events: none; }
  .danger { color: #ff8f90; }
  .sep { height: 1px; background: var(--line); margin: var(--s1) 0; }
</style>
