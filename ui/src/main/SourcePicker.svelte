<script lang="ts">
  // G3: the source picker. Running apps that produce audio (icon + name), presets, «Другое…» (installed
  // apps) and the whole system. The user never types an identifier.
  import { onMount } from 'svelte';
  import type { SourceInfo } from '../lib/ipc';
  import Icon from '../lib/components/Icon.svelte';
  import Modal from '../lib/components/Modal.svelte';
  import TextInput from '../lib/components/TextInput.svelte';
  import { api } from './api';
  import { tm } from './i18n';
  import { nameFromId, presetInfo, presetsFor, toValue, type SourceValue } from './source';
  import { sources } from './state.svelte';

  type Props = {
    value: SourceValue;
    label: string;
    onchange: (v: SourceValue, info: SourceInfo | null) => void;
    /** Offer «Весь звук системы» in the list (onboarding has it as its own card). */
    allowSystem?: boolean;
    size?: 'md' | 'lg';
  };
  let { value, label, onchange, allowSystem = true, size = 'lg' }: Props = $props();

  const os = api.platform();
  let open = $state(false);
  let otherOpen = $state(false);
  let query = $state('');
  let root: HTMLElement | undefined = $state();

  onMount(() => { void sources.refresh(); });

  const running = $derived(sources.running.filter((s) => s.kind === 'app' && s.producing_audio));
  const runningIds = $derived(new Set(running.map((s) => s.id)));
  const presets = $derived(
    presetsFor(os)
      .map((p) => presetInfo(p, os, sources.pool))
      .filter((s, i, a) => !runningIds.has(s.id) && a.findIndex((x) => x.id === s.id) === i),
  );
  const current = $derived.by<SourceInfo | null>(() => {
    if (value === 'system' || value === 'app:') return null;
    const id = value.slice(4);
    return sources.find(id) ?? { id, name: presetsFor(os).find((p) => p.mac === id || p.win === id)?.name ?? nameFromId(id), bundle_id: null, pid: null, icon_png_b64: null, producing_audio: false, kind: 'app' };
  });
  const installed = $derived(
    (sources.installed ?? []).filter((s) => !query.trim() || s.name.toLowerCase().includes(query.trim().toLowerCase())),
  );

  function toggle() {
    open = !open;
    if (open) void sources.refresh();
  }
  function pick(s: SourceInfo | null) {
    open = false;
    otherOpen = false;
    if (s) sources.remember(s);
    onchange(s ? toValue(s) : 'system', s);
  }
  async function showOther() {
    open = false;
    query = '';
    otherOpen = true;
    await sources.loadInstalled();
  }
  function outside(e: PointerEvent) {
    if (open && root && !root.contains(e.target as Node)) open = false;
  }
  function key(e: KeyboardEvent) {
    if (open && e.key === 'Escape') {
      e.stopPropagation();
      open = false;
    }
  }
</script>

<svelte:window onpointerdowncapture={outside} onkeydown={key} />

{#snippet ico(s: SourceInfo | null)}
  {#if s?.icon_png_b64}
    <img class="ic" src={`data:image/png;base64,${s.icon_png_b64}`} alt="" />
  {:else if s}
    <span class="ic tile">{s.name.slice(0, 1).toUpperCase()}</span>
  {:else}
    <span class="ic tile sys"><Icon name="monitor" size={12} /></span>
  {/if}
{/snippet}

<div class="sp" bind:this={root} data-source-value={value}>
  <button class="trig {size}" type="button" aria-haspopup="listbox" aria-expanded={open} aria-label={label} onclick={toggle}>
    {@render ico(current)}
    <span class="lbl">{current ? current.name : value === 'app:' ? tm('m.src.choose') : tm('m.sourceSystem')}</span>
    <Icon name="chevron-down" size={14} />
  </button>
  {#if open}
    <div class="panel pop" role="listbox" aria-label={label}>
      {#if running.length}
        <div class="h">{tm('m.src.running')}</div>
        {#each running as s (s.id)}
          <button class="it" role="option" aria-selected={value === toValue(s)} class:on={value === toValue(s)} type="button" onclick={() => pick(s)}>
            {@render ico(s)}<span class="lbl">{s.name}</span><i class="live" title={tm('m.src.playing')}></i>
          </button>
        {/each}
      {/if}
      <div class="h">{tm('m.src.apps')}</div>
      {#each presets as s (s.id)}
        <button class="it" role="option" aria-selected={value === toValue(s)} class:on={value === toValue(s)} type="button" onclick={() => pick(s)}>
          {@render ico(s)}<span class="lbl">{s.name}</span>
        </button>
      {/each}
      <button class="it other" role="option" aria-selected="false" type="button" onclick={showOther}><span class="ic tile"><Icon name="plus" size={12} /></span><span class="lbl">{tm('m.src.more')}</span></button>
      {#if allowSystem}
        <div class="sep"></div>
        <button class="it" role="option" aria-selected={value === 'system'} class:on={value === 'system'} type="button" onclick={() => pick(null)}>
          {@render ico(null)}<span class="lbl">{tm('m.sourceSystem')}</span>
        </button>
      {/if}
    </div>
  {/if}
</div>

<Modal open={otherOpen} title={tm('m.src.otherTitle')} width={420} closeLabel={tm('common.close')} onclose={() => (otherOpen = false)}>
  <TextInput bind:value={query} label={tm('m.src.search')} placeholder={tm('m.src.search')} autofocus />
  <div class="list">
    {#if sources.installed === null}
      <p class="none">{tm('m.src.loading')}</p>
    {:else if installed.length === 0}
      <p class="none">{tm('m.src.nothing')}</p>
    {:else}
      {#each installed as s (s.id)}
        <button class="it" type="button" onclick={() => pick(s)}>{@render ico(s)}<span class="lbl">{s.name}</span></button>
      {/each}
    {/if}
  </div>
</Modal>

<style>
  .sp { position: relative; min-width: 0; width: 100%; }
  .trig { all: unset; box-sizing: border-box; display: flex; align-items: center; gap: var(--s2); width: 100%; height: 32px; padding: 0 var(--s3); border-radius: var(--r); border: 1px solid var(--line); background: var(--bg); color: var(--tx); font: 13px/20px var(--font); cursor: pointer; }
  .trig.lg { height: 44px; border-radius: var(--r-lg); padding: 0 var(--s4); font-size: 14px; }
  .trig:hover, .trig:focus-visible, .trig[aria-expanded='true'] { border-color: var(--ac); }
  .trig .lbl { flex: 1; min-width: 0; }
  .panel { position: absolute; z-index: 40; top: calc(100% + var(--s1)); left: 0; right: 0; min-width: 240px; max-height: 360px; overflow: auto; padding: var(--s1); border-radius: var(--r-lg); background: var(--panel); border: 1px solid var(--line-strong); box-shadow: var(--sh); }
  .h { padding: var(--s2) var(--s3) var(--s1); font: 600 var(--fs-xs) var(--font); text-transform: uppercase; letter-spacing: 0.08em; color: var(--tx3); }
  .it { all: unset; box-sizing: border-box; width: 100%; display: flex; align-items: center; gap: var(--s3); height: 36px; padding: 0 var(--s3); border-radius: var(--r); font: 13px/20px var(--font); color: var(--tx); cursor: pointer; }
  .it:hover, .it:focus-visible { background: var(--hv); }
  .it.on { background: var(--sel); }
  .it .lbl { flex: 1; min-width: 0; }
  .ic { width: 20px; height: 20px; border-radius: var(--s1); flex: none; display: grid; place-items: center; object-fit: contain; }
  .tile { background: var(--hv); border: 1px solid var(--line); font: 600 11px/16px var(--font); color: var(--tx2); }
  .live { width: 8px; height: 8px; border-radius: 50%; background: var(--ac); flex: none; }
  .sep { height: 1px; background: var(--line); margin: var(--s1) 0; }
  .list { margin-top: var(--s3); max-height: 320px; overflow: auto; }
  .none { color: var(--tx2); margin: var(--s3) 0 0; font-size: 13px; }
</style>
