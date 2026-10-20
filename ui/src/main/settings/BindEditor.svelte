<script lang="ts">
  import { onMount } from 'svelte';
  import type { ActionId, BindReport, BindSpec, Mods, Trigger } from '../../lib/ipc';
  import Button from '../../lib/components/Button.svelte';
  import Icon from '../../lib/components/Icon.svelte';
  import { api } from '../api';
  import { bindFor, bindWarnings, defaultBinds, formatTrigger, hasMods, isModifierCode, modsOf, triggerFromKeyEvent, triggerFromMouse, withBind } from '../binds';
  import { tm } from '../i18n';
  import type { MKey } from '../i18n';
  import { mouseNames } from '../names';
  import { prefs } from '../state.svelte';

  let { actions, showReset = true, hints = true }: { actions: ActionId[]; showReset?: boolean; hints?: boolean } = $props();

  const os = api.platform();
  let rec = $state<ActionId | null>(null);
  let peak: Mods = {};
  let report = $state<BindReport>({ entries: [] });

  const binds = $derived<BindSpec[]>(prefs.value.binds ?? []);
  onMount(async () => { report = await api.bindReport(); });

  const label = (a: ActionId) => tm(`m.act.${a}` as MKey);
  const hint = (a: ActionId): string => (hints && HINTS.has(a) ? tm(`m.act.${a}.hint` as MKey) : '');
  const HINTS = new Set<ActionId>(['ask', 'screenshot', 'toggle_interactive', 'move_hold', 'next_template', 'toggle_overlay', 'panic_hide']);

  async function commit(a: ActionId, tr: Trigger | null) {
    rec = null;
    peak = {};
    await prefs.patch({ binds: withBind(binds, a, tr) });
    report = await api.bindReport();
  }
  function stopRec() { rec = null; peak = {}; }

  function onkeydown(e: KeyboardEvent) {
    if (!rec) return;
    e.preventDefault();
    e.stopPropagation();
    if (e.code === 'Escape') return stopRec();
    if (isModifierCode(e.code)) { peak = { ...peak, ...modsOf(e) }; return; }
    const tr = triggerFromKeyEvent(e);
    if (tr) commit(rec, tr);
  }
  function onkeyup(e: KeyboardEvent) {
    if (!rec || !isModifierCode(e.code) || !hasMods(peak)) return;
    e.preventDefault();
    commit(rec, { kind: 'modifier_only', mods: peak });
  }
  function onmousedown(e: MouseEvent) {
    if (!rec) return;
    const tr = triggerFromMouse(e.button, modsOf(e));
    if (!tr) return;
    e.preventDefault();
    commit(rec, tr);
  }
  const reset = () => prefs.patch({ binds: defaultBinds(os) });
  const status = (a: ActionId) => report.entries.find((x) => x.action === a)?.status;
</script>

<svelte:window onkeydowncapture={onkeydown} onkeyupcapture={onkeyup} onmousedowncapture={onmousedown} oncontextmenu={(e) => rec && e.preventDefault()} onauxclick={(e) => rec && e.preventDefault()} />

<div class="be">
  {#each actions as a (a)}
    {@const b = bindFor(binds, a)}
    {@const warns = b ? bindWarnings(b, binds, os) : []}
    {@const st = status(a)}
    <div class="hk">
      <span class="l"><span class="n lbl">{label(a)}</span>{#if hint(a)}<small class="lbl">{hint(a)}</small>{/if}</span>
      <div class="ctl">
        <button class="key" class:rec={rec === a} class:m={b?.trigger.kind === 'mouse'} onclick={(e) => { e.stopPropagation(); rec = rec === a ? null : a; peak = {}; }} aria-label={label(a)}>
          {#if rec === a}<span class="lbl">{tm('m.pressKey')}</span>
          {:else if b}{#each formatTrigger(b.trigger, os, mouseNames()) as part, i (i)}<kbd>{part}</kbd>{/each}{#if b.trigger.kind === 'modifier_only' && a === 'move_hold'}<span class="hold lbl">{tm('m.hold')}</span>{/if}
          {:else}<span class="unset lbl">{tm('m.notSet')}</span>{/if}
        </button>
        {#if b && rec !== a}<button class="clr" aria-label={tm('m.clearBind')} title={tm('m.clearBind')} onclick={() => commit(a, null)}><Icon name="close" size={12} /></button>{:else}<span class="clr"></span>{/if}
      </div>
      {#if st && st.state !== 'active' && b}
        <p class="wn">{st.state === 'degraded' ? tm('m.bindDegraded', { perm: tm(`m.perm.${st.reason}` as MKey) }) : tm('m.bindProblem', { reason: st.reason })}</p>
      {/if}
      {#each warns as w (w)}<p class="wn">{tm(`m.warn.${w}` as MKey)}</p>{/each}
    </div>
  {/each}
  {#if showReset}<div class="foot"><Button size="sm" icon="restore" onclick={reset}>{tm('settings.hotkeys.reset')}</Button></div>{/if}
  <p class="note">{tm('onboarding.hotkeys.rebind')}</p>
</div>

<style>
  .hk { display: grid; grid-template-columns: minmax(0, 1fr) auto; align-items: center; gap: var(--s1) var(--s3); padding: var(--s2) 0; border-bottom: 1px solid var(--line); min-height: 56px; }
  .l { min-width: 0; display: block; }
  .n { display: block; }
  small { display: block; color: var(--tx3); font: var(--fs-sm) var(--font); }
  .ctl { display: grid; grid-template-columns: 144px 24px; align-items: center; gap: var(--s1); }
  .key { all: unset; box-sizing: border-box; width: 144px; height: 32px; padding: 0 var(--s3); border-radius: var(--r); border: 1px solid var(--line); background: var(--panel); display: inline-flex; align-items: center; justify-content: center; gap: var(--s1); cursor: pointer; color: var(--tx); overflow: hidden; }
  .key:hover { border-color: var(--tx3); }
  .key.rec { border-color: var(--ac); color: var(--ac); animation: bl 1.2s infinite; font-size: 12px; }
  @keyframes bl { 50% { box-shadow: 0 0 0 4px var(--acbg); } }
  kbd { font: 600 12px/16px var(--mono); padding: 0 var(--s1); }
  .key.m kbd { color: var(--ac); }
  .hold, .unset { font-size: 12px; color: var(--tx3); }
  .clr { all: unset; display: block; cursor: pointer; width: 24px; height: 24px; display: grid; place-items: center; border-radius: var(--r-sm); color: var(--tx3); }
  .clr:hover { background: var(--hv); color: var(--tx); }
  .wn { grid-column: 1 / -1; margin: 0; font-size: 12px; line-height: 16px; color: var(--warn); }
  .foot { padding: var(--s3) 0; }
  .note { color: var(--tx2); font-size: 12px; margin: var(--s2) 0 0; }
  @media (prefers-reduced-motion: reduce) { .key.rec { animation: none; } }
</style>
