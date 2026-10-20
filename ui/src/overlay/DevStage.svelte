<script lang="ts">
  // Dev-only harness (vite dev, outside Tauri): test backgrounds, controls, mock demo feed.
  // Query params for deterministic screenshots: bg=dark|light|color|live, t=0..90, lang=ru|en,
  // mode=passive|interactive|moving, menu=1, view=pill, still=1 (preloaded frame, no demo).
  import { onMount, untrack } from 'svelte';
  import { OverlayStore } from './store.svelte';
  import { MockSource, fakeThumb } from './mock';
  import Overlay from './Overlay.svelte';
  import { PRESETS, type ToneEvent } from './tone';
  import type { OverlayMode, Tone, ToneKind } from '../lib/ipc/bindings';
  import { setLang } from '../lib/i18n';

  const q = new URLSearchParams(location.search);
  // bare=1: the overlay fills the viewport exactly like the real 600x384 window (QA layout checks).
  const bare = q.get('bare') === '1';
  const store = new OverlayStore();
  store.isMac = /mac/i.test(navigator.platform);
  const source = new MockSource({ autoplay: q.get('still') !== '1', duplicate: q.get('dup') === '1' });

  let bg = $state<'dark' | 'light' | 'color' | 'live' | 'split'>((q.get('bg') as 'dark') ?? 'dark');
  store.transparency = Number(q.get('t') ?? 60);
  store.lang = q.get('lang') === 'en' ? 'en' : 'ru';
  let view = $state<'overlay' | 'pill'>(q.get('view') === 'pill' ? 'pill' : 'overlay');
  let pos = $state({ x: 0, y: 0 });
  let moving = false;
  let last = { x: 0, y: 0 };
  let grab = { x: 0, y: 0 };
  let dragging = false;
  let priorMode: OverlayMode = 'passive';
  let liveIdx = 0;

  const kind = $derived<ToneKind>(bg === 'live' ? (['dark', 'light', 'color'] as const)[liveIdx % 3]! : bg === 'split' ? 'dark' : bg);
  const toneOf = (k: ToneKind): Tone => ({ tone: k, rgb: PRESETS[k].rgb, fg: PRESETS[k].fg, inv: PRESETS[k].inv, int: PRESETS[k].int, mv: PRESETS[k].mv, luma: null, busy: null });
  // Same payload shape as the real `overlay://tone` event, including the per-region grid (F5).
  // `split` = dark code on the left, light document on the right, bright slide strip at the bottom.
  // grid=0 drops the grid to exercise the global-tone fallback.
  const toneEvent = $derived.by<ToneEvent>(() => {
    const t = toneOf(kind);
    if (bg !== 'split' || q.get('grid') === '0') return t;
    const cells = Array.from({ length: 12 }, (_, i) => toneOf(Math.floor(i / 4) === 2 ? 'color' : i % 4 < 2 ? 'dark' : 'light'));
    return { ...t, grid: { cols: 4, rows: 3, cells } };
  });
  // Applied once synchronously (first paint is already in the right tone: no start-up fade), then on change.
  store.applyTone(untrack(() => toneEvent));
  $effect(() => {
    store.applyTone(toneEvent);
  });
  $effect(() => {
    store.visibility = view === 'pill' ? 'pill' : 'recording_only';
    if (view === 'pill') store.phase = 'idle';
    else if (untrack(() => store.phase) === 'idle') store.phase = 'recording';
  });
  $effect(() => setLang(store.lang));

  function preload() {
    store.phase = 'recording';
    store.elapsedMs = (47 * 60 + 12) * 1000;
    const ru = store.lang === 'ru';
    store.applyStt({ stream: 'system', utt: 1, rev: 0, kind: 'final', text: ru ? 'А что у нас по EBITDA margin и по customer acquisition cost за третий квартал?' : 'And how risky are the one-off items for the Q4 forecast?', lang: null, t_start_ns: 0, t_audio_end_ns: 0, t_emit_ns: 0 });
    store.applyLlm({ type: 'start', req_id: 'r', qa_id: 'q' });
    store.applyLlm({ type: 'delta', req_id: 'r', seq: 0, text: ru ? '**EBITDA margin** вырос с 21,3% до **23,4%** (+2,1 п.п.). **CAC** снизился на 11%, с 86 до 77 тыс. ₽, за счёт partner-канала. Стоит упомянуть one-off эффект в марже.' : '**One-off items** added about **1.2 pp** to the Q3 margin, so the Q4 forecast should exclude them. Underlying margin is roughly 22.2%.' });
    store.applyLlm({ type: 'done', req_id: 'r', truncated: false, usage: null });
    store.applyStt({ stream: 'me', utt: 2, rev: 0, kind: 'final', text: ru ? 'EBITDA margin вырос до двадцати трёх и четырёх, а CAC упал на одиннадцать процентов.' : 'The one-offs added around one point two, so I would exclude them.', lang: null, t_start_ns: 0, t_audio_end_ns: 0, t_emit_ns: 0 });
    store.setQueue(2, [fakeThumb(0), fakeThumb(1)]);
  }

  onMount(() => {
    source.start(store);
    if (q.get('still') === '1') preload();
    if (view === 'pill') store.phase = 'idle';
    const m = q.get('mode') as OverlayMode | null;
    if (m) store.setMode(m);
    if (q.get('menu') === '1') store.menuOpen = true;
    const live = setInterval(() => liveIdx++, 4000);
    return () => {
      source.stop();
      clearInterval(live);
    };
  });

  const typing = (e: Event) => (e.target as HTMLElement)?.tagName === 'INPUT';

  function onKeyDown(e: KeyboardEvent) {
    // G6: hold Ctrl = clickable overlay (the real app does this natively; the stage mimics the mode).
    if (e.key === 'Control' && !moving && store.mode === 'passive' && view === 'overlay') {
      store.setMode('hover');
      return;
    }
    if (e.key === 'Alt' && !moving && view === 'overlay') {
      moving = true;
      priorMode = store.mode;
      grab = { x: pos.x - last.x, y: pos.y - last.y };
      store.setMode('moving');
      return;
    }
    if (typing(e) && e.key !== 'Escape') return;
    if (e.key === 'Escape') {
      if (store.menuOpen) store.menuOpen = false;
      else store.setMode('passive');
    } else if (e.key.toLowerCase() === 'i') store.setMode(store.mode === 'interactive' ? 'passive' : 'interactive');
    else if (e.key === 'Enter') source.ask();
    else if (e.key.toLowerCase() === 's') source.screenshot();
  }
  function onKeyUp(e: KeyboardEvent) {
    if (e.key === 'Control' && store.mode === 'hover') store.setMode('passive');
    if (e.key === 'Alt' && moving) {
      moving = false;
      store.setMode(priorMode);
    }
  }
  // Hold-to-move magnet: no click, no cursor change; the overlay follows the pointer.
  function onMove(e: PointerEvent) {
    last = { x: e.clientX, y: e.clientY };
    if (moving) pos = { x: e.clientX + grab.x, y: e.clientY + grab.y };
    else if (dragging) pos = { x: e.clientX + grab.x, y: e.clientY + grab.y };
  }
  // Interactive mode: drag by header.
  function onDown(e: PointerEvent) {
    const t = e.target as HTMLElement;
    if (store.mode === 'interactive' && t.closest('[data-drag]') && !t.closest('button')) {
      dragging = true;
      grab = { x: pos.x - e.clientX, y: pos.y - e.clientY };
    }
  }
</script>

<svelte:window onkeydown={onKeyDown} onkeyup={onKeyUp} onpointermove={onMove} onpointerdown={onDown} onpointerup={() => (dragging = false)} />

<div class="stage bg-{bg === 'live' ? kind : bg}" class:bare>
  {#if bg === 'split'}
    <div class="splitbg"></div>
  {:else if bg === 'dark' || (bg === 'live' && kind === 'dark')}
    <pre class="code">// Считаем EBITDA margin по сегментам
export async function buildReport(ctx: Ctx) &#123;
  const rows = await ctx.db.query(`
    SELECT segment, revenue, opex, one_off
    FROM finance_q WHERE quarter = $1`, [ctx.q])
  return rows.map(r =&gt; (&#123;
    segment: r.segment,
    margin: (r.revenue - r.opex + r.one_off) / r.revenue,
    cac: calcCac(r, ctx.partners),
  &#125;))
&#125;
export const fmt = (x: number) =&gt; (x * 100).toFixed(1) + '%'</pre>
  {:else if bg === 'light' || (bg === 'live' && kind === 'light')}
    <div class="doc"><h2>Q3 2026 report</h2><p>Revenue grew 18% quarter over quarter. EBITDA margin improved by 2.1 pp, CAC fell 11%.</p><table><tbody><tr><th>Metric</th><th>Q2</th><th>Q3</th></tr><tr><td>Revenue</td><td>349</td><td>412</td></tr><tr><td>EBITDA margin</td><td>21,3%</td><td>23,4%</td></tr><tr><td>CAC</td><td>86</td><td>77</td></tr></tbody></table></div>
  {:else}
    <div class="slide"><h2>Выручка +38% год к году</h2><p>Q3 2026 · итоги и приоритеты на Q4</p></div>
  {/if}

  <div class="frame" class:pillf={view === 'pill'} style:left="{pos.x}px" style:top="{pos.y}px">
    <Overlay {store} {source} />
  </div>

  <div class="bar" hidden={bare}>
    <label>transparency <input type="range" min="0" max="90" bind:value={store.transparency} /> <b>{store.transparency}%</b></label>
    <select bind:value={bg}><option value="dark">dark</option><option value="light">light</option><option value="color">color</option><option value="live">live</option><option value="split">split</option></select>
    <select bind:value={store.lang}><option value="ru">RU</option><option value="en">EN</option></select>
    <select bind:value={view}><option value="overlay">overlay</option><option value="pill">pill</option></select>
    <select bind:value={store.fontPx}><option value={12}>12px</option><option value={13}>13px</option><option value={15}>15px</option></select>
    <button onclick={() => store.setMode(store.mode === 'interactive' ? 'passive' : 'interactive')}>I interactive</button>
    <button onclick={() => source.ask()}>↵ ask</button>
    <button onclick={() => source.screenshot()}>S shot</button>
    <span class="tip">Alt hold = move (magnet), I = interactive, header drag in interactive</span>
  </div>
</div>

<style>
  .stage { position: fixed; inset: 0; overflow: hidden; font: 12px system-ui, sans-serif; }
  .bg-dark { background: #17181c; color: #cfd0d8; }
  .bg-light { background: #e9eaee; }
  .bg-color { background: radial-gradient(900px 500px at 15% 10%, #ff8a5c 0, transparent 55%), radial-gradient(700px 500px at 90% 90%, #2dd4bf 0, transparent 55%), linear-gradient(135deg, #7c3aed, #be185d); color: #fff; }
  .splitbg { position: absolute; inset: 0; background: linear-gradient(180deg, transparent 66.67%, #7c3aed 66.67%, #be185d), linear-gradient(90deg, #17181c 50%, #f1f2f5 50%); }
  .code { margin: 0; padding: 24px; font: 12px/1.6 ui-monospace, Menlo, monospace; color: #9aa0b4; }
  .doc { width: 520px; margin: 24px auto 0; background: #fff; padding: 32px 40px; color: #23242a; font-family: Georgia, serif; box-shadow: 0 2px 14px rgba(0, 0, 0, 0.12); }
  .doc table { width: 100%; border-collapse: collapse; margin-top: 8px; }
  .doc td, .doc th { border-bottom: 1px solid #e3e4e8; padding: 8px 4px; text-align: left; }
  .slide { padding: 200px 40px 0; color: #fff; }
  .slide h2 { font: 700 40px/1.1 sans-serif; max-width: 420px; margin: 0 0 8px; }
  .frame { position: absolute; width: 600px; height: 384px; margin: 16px 0 0 calc(50vw - 300px); }
  .frame.pillf { width: 140px; height: 22px; margin-left: calc(100vw - 160px); margin-top: 300px; }
  .bar { position: absolute; left: 0; right: 0; bottom: 0; display: flex; gap: 12px; align-items: center; flex-wrap: wrap; padding: 8px 16px; background: #111; color: #eee; }
  .tip { color: #999; }
  .bare .frame, .bare .frame.pillf { inset: 0; margin: 0; width: auto; height: auto; left: 0 !important; top: 0 !important; }
  .bare .bar { display: none; }
</style>
