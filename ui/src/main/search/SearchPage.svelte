<script lang="ts">
  // Full search results page: filters folder / tag / date / kind in the URL, grouped hits with highlights.
  import Button from '../../lib/components/Button.svelte';
  import Icon from '../../lib/components/Icon.svelte';
  import Select from '../../lib/components/Select.svelte';
  import { api, folderKey, type SearchResults } from '../api';
  import { errText } from '../errors';
  import { tm } from '../i18n';
  import { hrefs, router, SEARCH_KINDS, type SearchKind, type SearchParams } from '../router.svelte';
  import { library } from '../state.svelte';
  import HitRow from './HitRow.svelte';
  import ModeNotice from './ModeNotice.svelte';
  import { buildQuery, groupHits, hasFilters, isHybrid } from './util';

  let { params }: { params: SearchParams } = $props();

  const PAGE = 20;
  let text = $state('');
  let res = $state<SearchResults | null>(null);
  let more = $state<SearchResults['hits']>([]);
  let loading = $state(false);
  let error = $state('');
  let input: HTMLInputElement | undefined = $state();
  let timer: ReturnType<typeof setTimeout> | undefined;
  let seq = 0;

  $effect(() => { text = params.q; });
  $effect(() => { queueMicrotask(() => { if (!params.q) input?.focus(); }); });

  // run whenever the address (query or filters) changes
  $effect(() => {
    const p = params;
    const mine = ++seq;
    more = [];
    if (!p.q.trim()) { res = null; error = ''; loading = false; return; }
    loading = true;
    api.searchQuery(buildQuery(p, PAGE)).then(
      (r) => { if (mine === seq) { res = r; error = ''; loading = false; } },
      (e) => { if (mine === seq) { res = null; error = errText(e); loading = false; } },
    );
  });

  const hits = $derived([...(res?.hits ?? []), ...more]);
  const groups = $derived(groupHits(hits));
  const facet = $derived(new Map((res?.facets ?? []).map((f) => [folderKey(f.folder), f.count])));
  const withCounts = $derived(!params.folder);

  function go(patch: Partial<SearchParams>) { router.replace(hrefs.search({ ...params, ...patch })); }
  function typed() {
    clearTimeout(timer);
    timer = setTimeout(() => go({ q: text }), 250);
  }
  async function showMore() {
    if (!res) return;
    loading = true;
    try {
      const r = await api.searchQuery(buildQuery(params, PAGE, hits.length));
      more = [...more, ...r.hits];
    } catch (e) { error = errText(e); }
    loading = false;
  }
  const folderOptions = $derived([
    { value: '', label: tm('search.filter.allFolders') },
    ...library.folders.map((f) => ({ value: folderKey(f.id), label: withCounts && facet.has(folderKey(f.id)) ? `${f.title} (${facet.get(folderKey(f.id))})` : f.title })),
    { value: 'sys:unsorted', label: tm('main.unsorted') },
  ]);
  const tagOptions = $derived([{ value: '', label: tm('search.filter.allTags') }, ...library.tags.map(([t, n]) => ({ value: t, label: `# ${t} (${n})` }))]);
  const kindLabel = (k: SearchKind | '') => (k === '' ? tm('search.filter.allKinds') : tm(`search.group.${k}` as 'search.group.call'));
</script>

<div class="view">
  <div class="in">
    <h1>{tm('common.search')}</h1>
    <div class="box"><Icon name="search" size={16} /><input bind:this={input} bind:value={text} placeholder={tm('search.placeholder')} autocomplete="off" aria-label={tm('common.search')} oninput={typed} onkeydown={(e) => e.key === 'Enter' && go({ q: text })} /></div>

    <div class="filters" data-testid="search-filters" role="group" aria-label={tm('search.filters')}>
      <label><span>{tm('m.folder')}</span><Select label={tm('m.folder')} value={params.folder} options={folderOptions} onchange={(v) => go({ folder: v })} /></label>
      <label><span>{tm('main.tags')}</span><Select label={tm('main.tags')} value={params.tag} options={tagOptions} onchange={(v) => go({ tag: v })} /></label>
      <label><span>{tm('search.filter.from')}</span><input class="date" type="date" value={params.from} max={params.to || undefined} aria-label={tm('search.filter.from')} onchange={(e) => go({ from: e.currentTarget.value })} /></label>
      <label><span>{tm('search.filter.to')}</span><input class="date" type="date" value={params.to} min={params.from || undefined} aria-label={tm('search.filter.to')} onchange={(e) => go({ to: e.currentTarget.value })} /></label>
    </div>
    <div class="kinds" role="radiogroup" aria-label={tm('search.filter.kind')}>
      {#each ['', ...SEARCH_KINDS] as const as k (k)}
        <button class="kb" class:on={params.kind === k} role="radio" aria-checked={params.kind === k} onclick={() => go({ kind: k })}>{kindLabel(k)}</button>
      {/each}
      {#if hasFilters(params)}<button class="clear" onclick={() => router.replace(hrefs.search({ q: params.q }))}>{tm('search.filter.clear')}</button>{/if}
    </div>

    {#if error}
      <p class="err" role="alert"><Icon name="alert" size={14} /><span>{error}</span></p>
    {:else if !params.q.trim()}
      <section class="empty" data-testid="search-empty">
        <div class="eic"><Icon name="search" size={20} /></div>
        <h2>{tm('search.empty.title')}</h2>
        <p>{tm('search.empty.body')}</p>
      </section>
    {:else if res}
      <div class="sum">
        <span class="lbl" data-testid="search-count">{tm('search.count', { n: res.total, ms: res.took_ms })}</span>
        {#if !isHybrid(res)}<ModeNotice />{/if}
      </div>
      {#if res.total === 0}
        <section class="empty" data-testid="search-none">
          <div class="eic"><Icon name="search" size={20} /></div>
          <h2>{tm('search.noResults', { q: params.q.trim() })}</h2>
          <p>{tm('search.none.body')}</p>
          {#if hasFilters(params)}<Button onclick={() => router.replace(hrefs.search({ q: params.q }))}>{tm('search.filter.clear')}</Button>{/if}
        </section>
      {:else}
        {#each groups as g (g.kind)}
          <section class="grp" data-group={g.kind}>
            <h3 class="h6">{tm(`search.group.${g.kind}` as 'search.group.call')} · {g.hits.length}</h3>
            <div class="list">{#each g.hits as h (h.chunk_id)}<HitRow hit={h} />{/each}</div>
          </section>
        {/each}
        {#if res.total > hits.length}<div class="mw"><Button disabled={loading} onclick={showMore}>{tm('search.showMore', { n: res.total - hits.length })}</Button></div>{/if}
      {/if}
    {:else if loading}
      <p class="muted">{tm('common.loading')}</p>
    {/if}
  </div>
</div>

<style>
  .view { height: 100%; overflow: auto; }
  .in { max-width: 840px; margin: 0 auto; padding: var(--s6) var(--s8) 64px; }
  h1 { font: 650 24px/32px var(--font); letter-spacing: -0.025em; margin: 0 0 var(--s4); }
  .box { display: flex; align-items: center; gap: var(--s3); height: 44px; padding: 0 var(--s4); border-radius: var(--r-lg); background: var(--panel); border: 1px solid var(--line); color: var(--tx3); }
  .box:focus-within { border-color: var(--ac); }
  .box input { all: unset; flex: 1; min-width: 0; font-size: 15px; line-height: 24px; color: var(--tx); }
  .filters { display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); gap: var(--s3); margin-top: var(--s4); }
  .filters label { display: flex; flex-direction: column; gap: var(--s1); min-width: 0; }
  .filters span { font: 600 var(--fs-xs) var(--font); letter-spacing: 0.08em; text-transform: uppercase; color: var(--tx3); }
  .date { box-sizing: border-box; height: 32px; width: 100%; min-width: 0; border-radius: var(--r); border: 1px solid var(--line); background: var(--bg); color: var(--tx); padding: 0 var(--s3); font: 13px/20px var(--font); color-scheme: dark; }
  .date:focus { border-color: var(--ac); outline: none; }
  .kinds { display: flex; gap: var(--s2); margin-top: var(--s3); flex-wrap: wrap; align-items: center; }
  .kb { all: unset; box-sizing: border-box; cursor: pointer; height: 28px; padding: 0 var(--s3); border-radius: 99px; border: 1px solid var(--line); color: var(--tx2); font-size: 12px; line-height: 26px; }
  .kb:hover { border-color: var(--tx3); color: var(--tx); }
  .kb:focus-visible { outline: 2px solid var(--focus); outline-offset: 1px; }
  .kb.on { background: var(--acbg); border-color: transparent; color: var(--ac); font-weight: 600; }
  .clear { all: unset; cursor: pointer; color: var(--ac); font-size: 12px; margin-left: auto; }
  .clear:hover, .clear:focus-visible { text-decoration: underline; }
  .sum { display: flex; align-items: center; justify-content: space-between; gap: var(--s3); margin: var(--s6) 0 var(--s3); color: var(--tx2); font-size: 13px; min-width: 0; }
  .grp { margin-bottom: var(--s6); }
  .h6 { margin: 0 0 var(--s2); font: 600 var(--fs-xs) var(--font); letter-spacing: 0.08em; text-transform: uppercase; color: var(--tx3); }
  .list { border: 1px solid var(--line); border-radius: var(--r-lg); background: var(--panel); padding: var(--s1); }
  .mw { display: flex; justify-content: center; }
  .muted { color: var(--tx3); }
  .err { display: flex; gap: var(--s2); align-items: center; margin: var(--s6) 0 0; color: #ff8f90; }
  .empty { display: flex; flex-direction: column; align-items: center; text-align: center; gap: var(--s2); padding: var(--s10) var(--s8); color: var(--tx2); }
  .empty h2 { margin: 0; font: 600 var(--fs-lg) var(--font); color: var(--tx); overflow-wrap: anywhere; }
  .empty p { margin: 0 0 var(--s2); max-width: 460px; }
  .eic { width: 44px; height: 44px; border-radius: var(--r-lg); background: var(--hv); border: 1px solid var(--line); display: grid; place-items: center; color: var(--tx2); margin-bottom: var(--s1); }
  @media (max-width: 900px) { .filters { grid-template-columns: repeat(2, minmax(0, 1fr)); } }
</style>
