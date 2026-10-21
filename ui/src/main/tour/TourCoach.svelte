<script lang="ts">
  // First-run tour: 6 steps, skippable (button + Esc), progress «2 / 6», coach marks over the real UI.
  // Steps 2-5 show a labelled miniature of the overlay (the real one is a hidden window); their «try it» demos
  // are local only: no recording starts and nothing touches the network.
  import { tick } from 'svelte';
  import type { ActionId } from '../../lib/ipc';
  import Button from '../../lib/components/Button.svelte';
  import Icon from '../../lib/components/Icon.svelte';
  import { api } from '../api';
  import { bindFor, defaultBinds, triggerEquals, triggerFromKeyEvent, triggerText } from '../binds';
  import { tm } from '../i18n';
  import { mouseNames } from '../names';
  import { prefs } from '../state.svelte';
  import OverlayThumb from './OverlayThumb.svelte';
  import { TOUR, TOUR_STEPS, tour } from './tour.svelte';

  const os = api.platform();
  const bindText = (action: ActionId): string => {
    const b = bindFor(prefs.value.binds, action) ?? bindFor(defaultBinds(os), action);
    return b ? triggerText(b.trigger, os, mouseNames()) : '';
  };
  const keys = $derived({
    rec: bindText('toggle_record'), ask: bindText('ask_brief'), complement: bindText('ask_complement'),
    move: bindText('move_hold'), click: bindText('click_hold'), shot: bindText('screenshot'), area: bindText('screenshot_area'),
  });

  const def = $derived(TOUR[tour.step]!);
  let rect = $state<{ x: number; y: number; w: number; h: number } | null>(null);
  let answer = $state('');
  let shots = $state(0);
  let moving = $state(false);
  let pos = $state({ x: 0, y: 0 });
  let done = $state(false);
  let stage: HTMLElement | undefined = $state();
  let timer: ReturnType<typeof setInterval> | undefined;

  function measure() {
    const t = def.target;
    const el = t ? document.querySelector<HTMLElement>(`[data-tour="${t}"]`) : null;
    if (!el) { rect = null; return; }
    const z = Number(getComputedStyle(document.documentElement).getPropertyValue('--z')) || 1;
    const r = el.getBoundingClientRect();
    rect = { x: r.left / z, y: r.top / z, w: r.width / z, h: r.height / z };
  }
  $effect(() => {
    if (!tour.open) return;
    const s = tour.step;
    answer = ''; shots = 0; moving = false; done = false; pos = { x: 0, y: 0 };
    clearInterval(timer);
    void tick().then(measure);
    return () => { void s; clearInterval(timer); };
  });

  // «Try it» 1: the ask bind streams a demo answer into the miniature (a local timer).
  function runAsk() {
    clearInterval(timer);
    const words = tm('tour.demo.answer').split(' ');
    answer = '';
    let i = 0;
    timer = setInterval(() => {
      answer = words.slice(0, ++i).join(' ');
      if (i >= words.length) { clearInterval(timer); done = true; }
    }, 90);
  }
  // «Try it» 2: hold the move key: the miniature follows the cursor.
  function moveTo(e: MouseEvent) {
    if (!moving || !stage) return;
    const r = stage.getBoundingClientRect();
    pos = { x: Math.max(0, Math.min(1, (e.clientX - r.left) / r.width)), y: Math.max(0, Math.min(1, (e.clientY - r.top) / r.height)) };
  }
  const holdMods = (action: ActionId) => {
    const b = bindFor(prefs.value.binds, action) ?? bindFor(defaultBinds(os), action);
    return b?.trigger.kind === 'modifier_only' ? b.trigger.mods : null;
  };
  function onkeydown(e: KeyboardEvent) {
    if (e.key === 'Escape' && (tour.open || tour.offer)) { e.preventDefault(); e.stopPropagation(); void tour.finish(); return; }
    if (!tour.open) return;
    const t = triggerFromKeyEvent(e);
    const matches = (action: ActionId) => {
      const b = bindFor(prefs.value.binds, action) ?? bindFor(defaultBinds(os), action);
      return !!(t && b && triggerEquals(t, b.trigger));
    };
    if (def.tryIt === 'ask' && matches('ask_brief')) { e.preventDefault(); runAsk(); }
    else if (def.tryIt === 'shot' && matches('screenshot')) { e.preventDefault(); shots += 1; done = true; }
    else if (def.tryIt === 'hold') {
      const m = holdMods('move_hold');
      if (m && ((m.alt && e.key === 'Alt') || (m.ctrl && e.key === 'Control') || (m.shift && e.key === 'Shift') || (m.meta && e.key === 'Meta'))) { moving = true; done = true; }
    }
  }
  function onkeyup(e: KeyboardEvent) {
    if (def.tryIt === 'hold' && ['Alt', 'Control', 'Shift', 'Meta'].includes(e.key)) moving = false;
  }

  const params = $derived({ rec: keys.rec, ask: keys.ask, complement: keys.complement, move: keys.move, click: keys.click, shot: keys.shot, area: keys.area });
  const tryKey = $derived(def.tryIt === 'ask' ? keys.ask : def.tryIt === 'hold' ? keys.move : keys.shot);
  const last = $derived(tour.step === TOUR_STEPS - 1);
  const cardStyle = $derived(rect ? `left: ${rect.x + rect.w + 16}px; top: ${Math.max(8, rect.y)}px;` : '');
</script>

<svelte:window onkeydowncapture={onkeydown} onkeyup={onkeyup} onmousemove={moveTo} onresize={measure} />

{#if tour.offer && !tour.open}
  <div class="card offer" role="dialog" aria-label={tm('tour.offer.title')} data-testid="tour-offer">
    <b>{tm('tour.offer.title')}</b>
    <p>{tm('tour.offer.body')}</p>
    <div class="foot"><Button variant="primary" onclick={() => tour.start()}>{tm('tour.offer.show')}</Button><Button onclick={() => tour.finish()}>{tm('common.skip')}</Button></div>
  </div>
{/if}

{#if tour.open}
  {#if rect}<div class="ring" style:left="{rect.x - 4}px" style:top="{rect.y - 4}px" style:width="{rect.w + 8}px" style:height="{rect.h + 8}px" aria-hidden="true"></div>{/if}
  <div class="card step" class:docked={!rect} style={cardStyle} role="dialog" aria-label={tm('tour.title')} data-testid="tour-card" data-step={tour.step}>
    <div class="top"><span class="cnt" data-testid="tour-progress">{tm('tour.progress', { n: tour.step + 1, total: TOUR_STEPS })}</span>
      <button class="x" aria-label={tm('tour.skip')} title={tm('tour.skip')} onclick={() => tour.finish()}><Icon name="close" size={14} /></button></div>
    <h3>{tm(`tour.step.${tour.step + 1}.title` as 'tour.step.1.title')}</h3>
    <p>{tm(`tour.step.${tour.step + 1}.body` as 'tour.step.1.body', params)}</p>
    {#if def.thumb}
      <div bind:this={stage}><OverlayThumb focus={def.thumb} {answer} {shots} {moving} {pos} clickable={false} /></div>
    {/if}
    {#if def.tryIt}
      <div class="try" class:ok={done} data-testid="tour-try">
        <span class="lbl"><b>{tm('tour.try')}</b> {tm(`tour.try.${def.tryIt}` as 'tour.try.ask', { key: tryKey })}</span>
        {#if def.tryIt === 'ask'}<Button size="sm" onclick={runAsk}>{tm('tour.try.run')}</Button>{/if}
        {#if def.tryIt === 'shot'}<Button size="sm" onclick={() => { shots += 1; done = true; }}>{tm('tour.try.run')}</Button>{/if}
        {#if done}<Icon name="check" size={14} />{/if}
      </div>
    {/if}
    <div class="foot">
      <Button variant="ghost" onclick={() => tour.finish()}>{tm('common.skip')}</Button>
      <span class="sp"></span>
      {#if tour.step > 0}<Button onclick={() => tour.back()}>{tm('common.back')}</Button>{/if}
      <Button variant="primary" onclick={() => tour.next()}>{last ? tm('common.done') : tm('common.next')}</Button>
    </div>
  </div>
{/if}

<style>
  .ring { position: fixed; z-index: 90; border-radius: var(--r-lg); box-shadow: 0 0 0 2px var(--ac), 0 0 0 6px var(--acbg); pointer-events: none; }
  .card { position: fixed; z-index: 95; width: 360px; max-width: calc(100vw - 32px); display: flex; flex-direction: column; gap: var(--s3); padding: var(--s4); border-radius: var(--r-xl); background: var(--panel); border: 1px solid var(--line-strong); box-shadow: var(--sh); }
  .card.docked, .offer { right: var(--s6); bottom: var(--s6); }
  .top { display: flex; align-items: center; justify-content: space-between; }
  .cnt { font: 600 var(--fs-xs) var(--font); letter-spacing: 0.08em; text-transform: uppercase; color: var(--tx3); }
  .x { all: unset; cursor: pointer; width: 24px; height: 24px; display: grid; place-items: center; border-radius: var(--r-sm); color: var(--tx2); }
  .x:hover, .x:focus-visible { background: var(--hv); color: var(--tx); }
  h3 { margin: 0; font: 600 var(--fs-lg) var(--font); }
  p { margin: 0; color: var(--tx2); font-size: 13px; line-height: 20px; }
  b { font-weight: 600; }
  .offer b { font-size: 15px; }
  .try { display: flex; align-items: center; gap: var(--s2); padding: var(--s2) var(--s3); border: 1px dashed var(--line-strong); border-radius: var(--r); font-size: 12px; line-height: 16px; color: var(--tx2); }
  .try .lbl { flex: 1; white-space: normal; overflow: visible; }
  .try.ok { border-color: var(--ac); color: var(--tx); }
  .try :global(svg) { color: var(--ac); flex: none; }
  .foot { display: flex; gap: var(--s2); align-items: center; }
  .sp { flex: 1; }
</style>
