<script lang="ts">
  // ⌘K palette: one line over calls, documents and screenshots (hybrid search). ↵ opens the hit, ⌘↵ shows all results.
  import Icon from '../../lib/components/Icon.svelte';
  import Modal from '../../lib/components/Modal.svelte';
  import { api, type SearchResults } from '../api';
  import { errText } from '../errors';
  import { when } from '../fmt';
  import { tm } from '../i18n';
  import { hrefs, router, NO_SEARCH } from '../router.svelte';
  import { library, ui } from '../state.svelte';
  import HitRow from './HitRow.svelte';
  import ModeNotice from './ModeNotice.svelte';
  import { buildQuery, groupHits, hitHref, isHybrid } from './util';

  const LIMIT = 8;
  let q = $state('');
  let res = $state<SearchResults | null>(null);
  let error = $state('');
  let sel = $state(0);
  let input: HTMLInputElement | undefined = $state();
  let timer: ReturnType<typeof setTimeout> | undefined;
  let seq = 0;

  const groups = $derived(groupHits(res?.hits ?? []));
  const flat = $derived(groups.flatMap((g) => g.hits));
  const recent = $derived(library.recent.filter((c) => c.ended !== null).slice(0, 5));
  const mod = api.platform() === 'macos' ? '⌘' : 'Ctrl';

  $effect(() => {
    if (!ui.paletteOpen) return;
    const text = q.trim();
    clearTimeout(timer);
    if (!text) { res = null; error = ''; return; }
    const mine = ++seq;
    timer = setTimeout(async () => {
      try {
        const r = await api.searchQuery(buildQuery({ ...NO_SEARCH, q: text }, LIMIT));
        if (mine === seq) { res = r; error = ''; sel = 0; }
      } catch (e) { if (mine === seq) { res = null; error = errText(e); } }
    }, 120);
  });
  $effect(() => { if (ui.paletteOpen) { q = ''; res = null; error = ''; sel = 0; queueMicrotask(() => input?.focus()); } });

  function close() { ui.paletteOpen = false; }
  function openHit(i: number) {
    const h = flat[i];
    if (!h) return;
    close();
    router.go(hitHref(h));
  }
  function all() { close(); router.go(hrefs.search({ q: q.trim() })); }
  function onkey(e: KeyboardEvent) {
    if (e.key === 'ArrowDown') { e.preventDefault(); sel = Math.min(flat.length - 1, sel + 1); }
    else if (e.key === 'ArrowUp') { e.preventDefault(); sel = Math.max(0, sel - 1); }
    else if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) { e.preventDefault(); all(); }
    else if (e.key === 'Enter') openHit(sel);
  }
  $effect(() => { sel; queueMicrotask(() => document.querySelector('.pal .hit.on')?.scrollIntoView({ block: 'nearest' })); });
</script>

<Modal open={ui.paletteOpen} align="top" width={640} closeLabel={tm('common.close')} onclose={close}>
  <div class="pal" data-testid="palette">
    <div class="inrow"><Icon name="search" size={16} /><input bind:this={input} bind:value={q} placeholder={tm('search.placeholder')} autocomplete="off" onkeydown={onkey} aria-label={tm('common.search')} /></div>
    <div class="res">
      {#if error}
        <p class="g err" role="alert">{error}</p>
      {:else if !q.trim()}
        {#if recent.length}
          <div class="g">{tm('search.recent')}</div>
          {#each recent as c (c.id)}
            <a class="rc" href={hrefs.call(c.id)} onclick={close}><Icon name="mic" size={14} /><span class="lbl">{c.title}</span><small class="lbl">{when(c.started, new Date(), false)}</small></a>
          {/each}
        {:else}
          <p class="g">{tm('search.hint')}</p>
        {/if}
      {:else if res && res.total === 0}
        <p class="empty" data-testid="palette-empty">{tm('search.noResults', { q: q.trim() })}</p>
      {:else}
        {#each groups as g (g.kind)}
          <div class="g">{tm(`search.group.${g.kind}` as 'search.group.call')}</div>
          {#each g.hits as h (h.chunk_id)}
            {@const i = flat.indexOf(h)}
            <HitRow hit={h} selected={i === sel} onhover={() => (sel = i)} onpick={close} />
          {/each}
        {/each}
        {#if res && res.total > flat.length}<button class="more" onclick={all}>{tm('search.showAll', { n: res.total })}</button>{/if}
      {/if}
    </div>
    <div class="ft">
      <span>{tm('main.keyHintsMove')}</span><span>{tm('main.keyHintsOpen')}</span><span>{mod}↵ {tm('search.allResults')}</span><span>{tm('main.keyHintsClose')}</span>
      {#if res && !isHybrid(res)}<span class="r"><ModeNotice compact /></span>{/if}
    </div>
  </div>
</Modal>

<style>
  .pal { margin: calc(-1 * var(--s4)) calc(-1 * var(--s6)) calc(-1 * var(--s6)); }
  .inrow { display: flex; align-items: center; gap: var(--s3); padding: 0 var(--s6); border-bottom: 1px solid var(--line); color: var(--tx3); }
  input { all: unset; display: block; flex: 1; min-width: 0; box-sizing: border-box; padding: var(--s4) 0; font-size: 16px; line-height: 24px; color: var(--tx); }
  .res { padding: var(--s2); max-height: 400px; overflow: auto; }
  .g { font: 600 var(--fs-xs) var(--font); letter-spacing: 0.08em; text-transform: uppercase; color: var(--tx3); padding: var(--s2) var(--s2) var(--s1); margin: 0; }
  p.g { text-transform: none; letter-spacing: 0; font: 13px/20px var(--font); }
  .err { color: #ff8f90 !important; }
  .empty { margin: 0; padding: var(--s6) var(--s4); color: var(--tx2); text-align: center; overflow-wrap: anywhere; }
  .rc { display: flex; align-items: center; gap: var(--s3); padding: var(--s2) var(--s3); border-radius: var(--r); color: var(--tx); text-decoration: none; font-size: 13px; min-width: 0; }
  .rc:hover { background: var(--sel); }
  .rc :global(svg) { color: var(--tx3); flex: none; }
  .rc .lbl { flex: 1; }
  .rc small { color: var(--tx3); font-size: 11px; flex: none; max-width: 96px; }
  .more { all: unset; box-sizing: border-box; display: block; width: 100%; text-align: center; padding: var(--s2); color: var(--ac); font-size: 12px; cursor: pointer; border-radius: var(--r); }
  .more:hover, .more:focus-visible { background: var(--hv); }
  .ft { padding: var(--s2) var(--s4); border-top: 1px solid var(--line); color: var(--tx3); font-size: 11px; display: flex; gap: var(--s4); align-items: center; min-width: 0; min-height: 40px; }
  .r { margin-left: auto; min-width: 0; }
</style>
