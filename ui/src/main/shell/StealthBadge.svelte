<script lang="ts">
  // «Скрыт от демонстрации» / «Может быть виден»: the overlay-protection state in the sidebar; a click explains the check.
  import Button from '../../lib/components/Button.svelte';
  import Icon from '../../lib/components/Icon.svelte';
  import { api } from '../api';
  import { tm } from '../i18n';
  import { hrefs, router } from '../router.svelte';
  import { stealth } from '../stealth.svelte';

  let open = $state(false);
  let el = $state<HTMLElement | null>(null);
  const safe = $derived(stealth.level === 'safe');
  const reason = $derived(stealth.status && stealth.status.state !== 'verified' ? stealth.status.reason : '');
  const mac = api.platform() === 'macos';

  $effect(() => stealth.start());
  function outside(e: PointerEvent) {
    if (open && el && !el.contains(e.target as Node)) open = false;
  }
</script>

<svelte:window onpointerdowncapture={outside} onkeydown={(e) => e.key === 'Escape' && (open = false)} />

<div class="wrap" bind:this={el}>
  <button class="badge" class:safe class:risk={!safe} data-testid="stealth-badge" data-level={stealth.level} aria-haspopup="dialog" aria-expanded={open} onclick={() => (open = !open)}>
    <Icon name={safe ? 'shield' : 'alert'} size={14} /><span class="lbl">{safe ? tm('m.stealth.safe') : tm('m.stealth.risk')}</span>
  </button>
  {#if open}
    <div class="pop" role="dialog" aria-label={tm('m.stealth.title')} data-testid="stealth-popover">
      <b>{tm('m.stealth.title')}</b>
      <p class="st" class:risk={!safe} role="status">
        {#if stealth.status?.state === 'verified'}{tm('m.stealth.stVerified')}
        {:else if stealth.status?.state === 'degraded'}{tm('m.stealth.stDegraded', { reason })}
        {:else}{tm('m.stealth.stUnavailable', { reason })}{/if}
      </p>
      {#if stealth.report}
        <p class="st" class:risk={!stealth.report.overlay_hidden} data-testid="stealth-report">{stealth.report.overlay_hidden ? tm('m.stealth.pxHidden') : tm('m.stealth.pxVisible')}</p>
      {:else if stealth.error}
        <p class="st risk" role="alert">{tm('m.stealth.failed')}</p>
      {/if}
      <p class="sub">{tm('m.stealth.checked')}</p>
      <p class="sub">{tm('m.stealth.main')}</p>
      {#if !safe}
        <p class="todo"><b>{tm('m.stealth.todoTitle')}</b> {tm('m.stealth.todo')}</p>
      {/if}
      <div class="acts">
        <Button size="sm" variant="primary" disabled={stealth.busy} onclick={() => stealth.check()}>{stealth.busy ? tm('m.stealth.checking') : tm('m.stealth.recheck')}</Button>
        {#if mac}<Button size="sm" variant="ghost" onclick={() => { open = false; router.go(hrefs.settings('permissions')); }}>{tm('m.stealth.perms')}</Button>{/if}
      </div>
    </div>
  {/if}
</div>

<style>
  .wrap { position: relative; padding: 0 var(--s1) var(--s2); }
  .badge { all: unset; box-sizing: border-box; cursor: pointer; width: 100%; display: flex; align-items: center; gap: var(--s2); height: 28px; padding: 0 var(--s2); border-radius: var(--r); font-size: 12px; font-weight: 550; min-width: 0; }
  .badge .lbl { flex: 1; min-width: 0; }
  .badge.safe { color: var(--ac); background: var(--acbg); }
  .badge.risk { color: var(--warn); background: var(--warnbg); }
  .badge:focus-visible { outline: 2px solid var(--ac); outline-offset: 1px; }
  .pop { position: absolute; z-index: 45; left: var(--s2); bottom: calc(100% + var(--s1)); width: 320px; max-width: calc(100vw - 32px); box-sizing: border-box; padding: var(--s4); display: flex; flex-direction: column; gap: var(--s2); border-radius: var(--r-lg); background: var(--panel); border: 1px solid var(--line-strong); box-shadow: var(--sh); font-size: 13px; }
  .pop p { margin: 0; }
  .st { color: var(--ac); font-weight: 550; }
  .st.risk, .todo { color: var(--warn); }
  .sub { color: var(--tx2); font-size: 12px; line-height: 16px; }
  .todo { font-size: 12px; line-height: 16px; }
  .todo b { color: var(--tx); }
  .acts { display: flex; gap: var(--s2); margin-top: var(--s1); flex-wrap: wrap; }
</style>
