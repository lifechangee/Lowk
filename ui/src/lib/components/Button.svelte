<script lang="ts">
  import type { Snippet } from 'svelte';
  import Icon, { type IconName } from './Icon.svelte';

  type Props = {
    variant?: 'default' | 'primary' | 'ghost' | 'stop' | 'danger';
    size?: 'sm' | 'md' | 'lg';
    icon?: IconName;
    disabled?: boolean;
    pressed?: boolean;
    block?: boolean;
    type?: 'button' | 'submit';
    title?: string;
    onclick?: (e: MouseEvent) => void;
    children?: Snippet;
    trailing?: Snippet;
  };
  let { variant = 'default', size = 'md', icon, disabled = false, pressed, block = false, type = 'button', title, onclick, children, trailing }: Props = $props();
</script>

<button class="btn {variant} {size}" class:block class:on={pressed} {type} {disabled} {title} aria-pressed={pressed} {onclick}>
  {#if icon}<Icon name={icon} size={14} />{/if}
  {#if children}<span class="lbl bl">{@render children()}</span>{/if}
  {#if trailing}{@render trailing()}{/if}
</button>

<style>
  .btn {
    all: unset; box-sizing: border-box; cursor: pointer; display: inline-flex; gap: var(--s2); align-items: center; justify-content: center;
    height: 32px; padding: 0 var(--s3); border-radius: var(--r); border: 1px solid var(--line);
    font: 500 13px/20px var(--font); color: var(--tx); background: var(--panel); white-space: nowrap; max-width: 100%; min-width: 0;
  }
  .btn > :global(.bl) { display: inline-flex; align-items: center; gap: var(--s2); min-width: 0; }
  .btn:hover { border-color: var(--tx3); }
  .btn:focus-visible { outline: 2px solid var(--ac); outline-offset: 1px; }
  .btn[disabled] { pointer-events: none; color: var(--tx3) !important; background: var(--hv) !important; border-color: var(--line) !important; filter: none !important; }
  .block { width: 100%; justify-content: flex-start; }
  .sm { height: 28px; padding: 0 var(--s3); font-size: 12px; }
  .lg { height: 44px; padding: 0 var(--s6); font-size: 14px; border-radius: var(--r-lg); }
  .primary { background: var(--ac); border-color: var(--ac); color: var(--acfg); font-weight: 600; }
  .primary:hover { filter: brightness(1.08); border-color: var(--ac); }
  .ghost { background: transparent; }
  .stop { border-color: rgba(255, 77, 79, 0.5); color: #ff8f90; }
  .danger { color: #ff8f90; }
  .danger:hover { border-color: rgba(255, 77, 79, 0.5); }
  .on { border-color: var(--ac); color: var(--ac); }
  .primary.on { color: var(--acfg); filter: brightness(1.12); }
</style>
