<script lang="ts">
  import Modal from '../lib/components/Modal.svelte';
  import { api, type SearchHit } from './api';
  import { tm } from './i18n';
  import { hrefs, router } from './router.svelte';
  import { ui } from './state.svelte';

  let q = $state('');
  let hits = $state<SearchHit[]>([]);
  let sel = $state(0);
  let input: HTMLInputElement | undefined = $state();

  $effect(() => {
    if (!ui.paletteOpen) return;
    const query = q;
    api.search(query).then((h) => { if (query === q) { hits = h; sel = 0; } });
  });
  $effect(() => { if (ui.paletteOpen) { q = ''; queueMicrotask(() => input?.focus()); } });

  function open(h: SearchHit | undefined) {
    if (!h) return;
    ui.paletteOpen = false;
    router.go(h.call ? hrefs.call(h.call) : hrefs.folder(h.folder));
  }
  function onkey(e: KeyboardEvent) {
    if (e.key === 'ArrowDown') { e.preventDefault(); sel = Math.min(hits.length - 1, sel + 1); }
    else if (e.key === 'ArrowUp') { e.preventDefault(); sel = Math.max(0, sel - 1); }
    else if (e.key === 'Enter') open(hits[sel]);
  }
  const parts = (s: string) => {
    const t = q.trim();
    if (!t) return [{ m: false, s }];
    const i = s.toLowerCase().indexOf(t.toLowerCase());
    return i < 0 ? [{ m: false, s }] : [{ m: false, s: s.slice(0, i) }, { m: true, s: s.slice(i, i + t.length) }, { m: false, s: s.slice(i + t.length) }];
  };
</script>

<Modal open={ui.paletteOpen} align="top" width={600} closeLabel={tm('common.close')} onclose={() => (ui.paletteOpen = false)}>
  <div class="pal">
    <input bind:this={input} bind:value={q} placeholder={tm('m.searchPalette')} autocomplete="off" onkeydown={onkey} aria-label={tm('common.search')} />
    <div class="res">
      {#each hits as h, i (h.group + h.title + i)}
        {#if i === 0 || hits[i - 1]!.group !== h.group}<div class="g">{h.group === 'calls' ? tm('m.groupCalls') : tm('m.groupDocs')}</div>{/if}
        <button class="it" class:on={i === sel} onmouseenter={() => (sel = i)} onclick={() => open(h)}>
          <span class="lbl">{#each parts(h.title) as p}{#if p.m}<mark>{p.s}</mark>{:else}{p.s}{/if}{/each}</span>
          <small class="lbl">{#each parts(h.snippet) as p}{#if p.m}<mark>{p.s}</mark>{:else}{p.s}{/if}{/each}</small>
        </button>
      {:else}
        <div class="g">{tm('main.searchEmpty')}</div>
      {/each}
    </div>
    <div class="ft"><span>{tm('main.keyHintsMove')}</span><span>{tm('main.keyHintsOpen')}</span><span>{tm('main.keyHintsClose')}</span><span class="r lbl">{tm('main.semanticSearchLocal')}</span></div>
  </div>
</Modal>

<style>
  .pal { margin: calc(-1 * var(--s4)) calc(-1 * var(--s6)) calc(-1 * var(--s6)); }
  input { all: unset; display: block; width: 100%; box-sizing: border-box; padding: var(--s4) var(--s6); font-size: 16px; line-height: 24px; border-bottom: 1px solid var(--line); color: var(--tx); }
  .res { padding: var(--s2); max-height: 360px; overflow: auto; }
  .g { font: 600 var(--fs-xs) var(--font); letter-spacing: 0.08em; text-transform: uppercase; color: var(--tx3); padding: var(--s2) var(--s2) var(--s1); }
  .it { all: unset; box-sizing: border-box; width: 100%; padding: var(--s2); border-radius: var(--r); cursor: pointer; display: flex; flex-direction: column; min-width: 0; }
  .it.on { background: var(--sel); }
  small { color: var(--tx2); font: var(--fs-sm) var(--font); }
  mark { background: var(--acbg); color: var(--ac); border-radius: 3px; padding: 0 1px; }
  .ft { padding: var(--s2) var(--s4); border-top: 1px solid var(--line); color: var(--tx3); font-size: 11px; display: flex; gap: var(--s4); min-width: 0; }
  .r { margin-left: auto; }
</style>
