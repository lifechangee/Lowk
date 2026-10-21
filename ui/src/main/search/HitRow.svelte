<script lang="ts">
  import type { SearchHit } from '../../lib/ipc';
  import Icon, { type IconName } from '../../lib/components/Icon.svelte';
  import { t } from '../../lib/i18n/locale.svelte';
  import { mmss } from '../../lib/sourceLabel';
  import { folderKey } from '../api';
  import { when } from '../fmt';
  import { tm } from '../i18n';
  import { library } from '../state.svelte';
  import { groupOf, hitHref, segments } from './util';

  let { hit, selected = false, onhover, onpick }: { hit: SearchHit; selected?: boolean; onhover?: () => void; onpick?: () => void } = $props();

  const icons: Record<ReturnType<typeof groupOf>, IconName> = { call: 'mic', doc: 'doc', image: 'monitor' };
  const folderTitle = $derived.by(() => {
    const k = folderKey(hit.source.folder);
    return k === 'sys:unsorted' ? tm('main.unsorted') : k === 'sys:trash' ? tm('main.trash') : (library.folder(k)?.title ?? hit.folder_title);
  });
  const where = $derived(hit.source.kind === 'doc' ? (hit.source.page ? t('source.page', { n: hit.source.page }) : '') : hit.source.at_s !== null ? mmss(hit.source.at_s) : '');
  const parts = $derived(segments(hit.snippet, hit.highlights));
</script>

<a class="hit" class:on={selected} href={hitHref(hit)} data-kind={groupOf(hit)} onmouseenter={onhover} onclick={onpick}>
  <span class="ic"><Icon name={icons[groupOf(hit)]} size={14} /></span>
  <span class="main">
    <span class="t1"><b class="lbl">{hit.source.title}</b>{#if where}<span class="wh">{where}</span>{/if}{#if hit.kind === 'qa'}<span class="wh">{tm('m.aiHint')}</span>{/if}</span>
    <span class="sn">{#each parts as p, i (i)}{#if p.m}<mark>{p.s}</mark>{:else}{p.s}{/if}{/each}</span>
  </span>
  <span class="meta lbl">{folderTitle}{#if hit.when} · {when(hit.when, new Date(), false)}{/if}</span>
</a>

<style>
  .hit { display: grid; grid-template-columns: 24px minmax(0, 1fr) auto; gap: var(--s3); align-items: start; padding: var(--s2) var(--s3); border-radius: var(--r); text-decoration: none; color: inherit; min-width: 0; }
  .hit.on, .hit:hover { background: var(--sel); }
  .ic { width: 24px; height: 24px; display: grid; place-items: center; color: var(--tx3); }
  .main { display: flex; flex-direction: column; gap: var(--s0); min-width: 0; }
  .t1 { display: flex; align-items: center; gap: var(--s2); min-width: 0; }
  .t1 b { font-weight: 550; font-size: 13px; line-height: 20px; }
  .wh { flex: none; font: 600 11px/16px var(--mono); color: var(--tx3); padding: 0 var(--s1); border-radius: var(--s1); background: var(--hv); }
  .sn { color: var(--tx2); font-size: 12px; line-height: 16px; overflow: hidden; display: -webkit-box; -webkit-line-clamp: 2; line-clamp: 2; -webkit-box-orient: vertical; overflow-wrap: anywhere; }
  mark { background: none; color: var(--ac); font-weight: 650; text-decoration: underline; text-underline-offset: 2px; }
  .meta { font-size: 11px; line-height: 20px; color: var(--tx3); max-width: 200px; text-align: right; }
</style>
