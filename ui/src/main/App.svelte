<script lang="ts">
  import '../lib/tokens.css';
  import { onMount } from 'svelte';
  import Icon from '../lib/components/Icon.svelte';
  import { locale } from '../lib/i18n/locale.svelte';
  import { api, folderKey } from './api';
  import { tm } from './i18n';
  import Onboarding from './onboarding/Onboarding.svelte';
  import { hrefs, router } from './router.svelte';
  import SearchPalette from './search/SearchPalette.svelte';
  import SearchPage from './search/SearchPage.svelte';
  import EngineNeededDialog from './EngineNeededDialog.svelte';
  import MoveMenu from './MoveMenu.svelte';
  import { dnd } from './dnd.svelte';
  import SettingsView from './settings/SettingsView.svelte';
  import NewFolderDialog from './shell/NewFolderDialog.svelte';
  import Sidebar from './shell/Sidebar.svelte';
  import { attachEvents, callLangs, index, library, prefs, session, ui } from './state.svelte';
  import TourCoach from './tour/TourCoach.svelte';
  import UpdateNotice from './UpdateNotice.svelte';
  import { updates } from './updates.svelte';
  import Home from './home/Home.svelte';
  import CallPage from './workspace/CallPage.svelte';
  import DocPage from './workspace/DocPage.svelte';
  import FolderView from './workspace/FolderView.svelte';
  import LivePage from './workspace/LivePage.svelte';

  let ready = $state(false);
  const route = $derived(router.route);

  onMount(() => {
    attachEvents();
    // The overlay's source chips and «no API key» link route the main window.
    if (!api.isMock) {
      void import('@tauri-apps/api/event').then(({ listen }) => {
        void listen<{ kind: string; id: string; page: number | null; at_s: number | null }>('nav://source', (e) => {
          const s = e.payload;
          router.go(s.kind === 'doc' ? hrefs.doc(s.id, s.page) : s.kind === 'instructions' ? hrefs.folder(s.id, 'instructions') : hrefs.call(s.id, s.at_s));
        });
        void listen<{ page: string }>('nav://settings', (e) => router.go(hrefs.settings((e.payload.page as never) ?? 'providers')));
      });
    }
    void (async () => {
      locale.init(await api.localeGet().catch(() => 'en'));
      await Promise.all([prefs.load(), library.reload()]);
      void callLangs.load();
      void index.refresh();
      if (!prefs.value.onboarding?.done && router.route.name !== 'onboarding') router.go(hrefs.onboarding(prefs.value.onboarding?.step ?? 0));
      ready = true;
    })();
    const stopUpdates = updates.start();
    const stopIndex = api.onIndexStatus((s) => { index.status = s; });
    return () => { stopUpdates(); stopIndex(); };
  });

  // F6: interface text size (settings ui.font_px, 14 = 100%) scales the whole main window live.
  const zoom = $derived(Math.min(1.6, Math.max(0.8, (prefs.value.ui?.font_px ?? 14) / 14)));
  $effect(() => {
    const el = document.getElementById('app');
    if (el) el.style.zoom = String(zoom);
    document.documentElement.style.setProperty('--z', String(zoom));
  });

  // Mouse side buttons must not navigate history (WebView2 does that by default).
  function onmouseup(e: MouseEvent) { if (e.button === 3 || e.button === 4) e.preventDefault(); }
  function onkeydown(e: KeyboardEvent) {
    if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'k') { e.preventDefault(); ui.paletteOpen = !ui.paletteOpen; }
  }

  // The folder a document lives in (for the sidebar highlight and the breadcrumbs).
  let docFolder = $state<string>('');
  let docTitle = $state<string>('');
  $effect(() => {
    if (route.name !== 'doc') return;
    const id = route.id;
    void api.docGet(id).then((d) => { if (route.name === 'doc' && route.id === id) { docFolder = d ? folderKey(d.folder) : ''; docTitle = d?.title ?? ''; } });
  });

  const call = $derived(route.name === 'call' ? library.call(route.id) : undefined);
  const folder = $derived(route.name === 'folder' ? route.id : call ? (call.deleted ? 'sys:trash' : folderKey(call.folder)) : route.name === 'doc' ? docFolder : '');

  function folderLabel(key: string): string {
    return key === 'sys:unsorted' ? tm('main.unsorted') : key === 'sys:trash' ? tm('main.trash') : (library.folder(key)?.title ?? '');
  }
  const isSys = (key: string) => key === 'sys:unsorted' || key === 'sys:trash';
  const crumbs = $derived.by<{ label: string; href?: string }[]>(() => {
    if (route.name === 'folder') {
      if (isSys(route.id)) return [{ label: folderLabel(route.id) }];
      return [{ label: tm('main.folders') }, { label: folderLabel(route.id), href: hrefs.folder(route.id) }, { label: tm(`main.tab.${route.tab}` as 'main.tab.calls') }];
    }
    if (route.name === 'call') {
      if (!call) return [];
      const key = call.deleted ? 'sys:trash' : folderKey(call.folder);
      return [...(isSys(key) ? [] : [{ label: tm('main.folders') }]), { label: folderLabel(key), href: hrefs.folder(key) }, { label: call.title }];
    }
    if (route.name === 'doc') return [...(docFolder && !isSys(docFolder) ? [{ label: tm('main.folders') }] : []), { label: folderLabel(docFolder), href: hrefs.folder(docFolder, 'documents') }, { label: docTitle }];
    if (route.name === 'search') return [{ label: tm('common.search') }];
    if (route.name === 'settings') return [{ label: tm('settings.title') }, { label: tm(`m.nav.${route.page}` as 'm.nav.general') }];
    return [{ label: tm('main.nav.home') }];
  });
</script>

<svelte:window {onmouseup} {onkeydown} />

{#if ready}
  {#if route.name === 'onboarding'}
    <Onboarding step={route.step} />
  {:else}
    <div class="app">
      <Sidebar {folder} />
      <div class="mainc" class:ld={session.phase === 'recording' && session.sttLoading}>
        <header class="tb">
          <nav class="crumb" aria-label="breadcrumb">
            {#each crumbs as c, i (i)}
              {#if i > 0}<span class="s"><Icon name="chevron-right" size={12} /></span>{/if}
              {#if c.href && i < crumbs.length - 1}<a class="lbl" href={c.href}>{c.label}</a>{:else}<b class="lbl">{c.label}</b>{/if}
            {/each}
          </nav>
        </header>
        {#if session.phase === 'recording' && session.sttLoading}
          <div class="loadbar lbl" role="status" data-testid="stt-loading"><i class="spin"></i>{tm('main.sttLoading')}</div>
        {/if}
        <div class="content">
          {#if route.name === 'home'}<Home />
          {:else if route.name === 'settings'}<SettingsView page={route.page} />
          {:else if route.name === 'search'}<SearchPage params={route} />
          {:else}
            <main class="ctr">
              {#if route.name === 'call'}
                {#if call && call.ended === null}<LivePage />{:else if call || !library.loaded}{#key route.id}<CallPage id={route.id} at={route.at} match={route.match ?? null} />{/key}{/if}
              {:else if route.name === 'doc'}{#key route.id}<DocPage id={route.id} page={route.page} />{/key}
              {:else if route.name === 'folder'}<FolderView folder={route.id} tab={route.tab} />{/if}
            </main>
          {/if}
        </div>
      </div>
    </div>
  {/if}
  <SearchPalette />
  <EngineNeededDialog />
  <MoveMenu />
  <NewFolderDialog />
  <TourCoach />
  {#if route.name !== 'onboarding'}<UpdateNotice />{/if}
  {#if dnd.call}
    <div class="ghost pop" style:left="{(dnd.x + 12) / zoom}px" style:top="{(dnd.y + 12) / zoom}px"><Icon name="doc" size={14} /><span class="lbl">{dnd.call.title}</span></div>
  {/if}
  {#if ui.toast}<div class="toast" role="status"><span class="lbl">{ui.toast}</span>{#if ui.toastAction}{@const act = ui.toastAction}<button class="ta" onclick={() => { act.run(); ui.toast = null; ui.toastAction = null; }}>{act.label}</button>{/if}</div>{/if}
{/if}

<style>
  .loadbar { display: flex; align-items: center; gap: var(--s2); padding: var(--s2) var(--s4); background: var(--acbg); color: var(--ac); font-size: 12px; line-height: 20px; }
  .loadbar .spin { width: 10px; height: 10px; border-radius: 50%; border: 2px solid currentColor; border-top-color: transparent; animation: ldspin 0.9s linear infinite; flex: none; }
  @keyframes ldspin { to { transform: rotate(360deg); } }
  @media (prefers-reduced-motion: reduce) { .loadbar .spin { animation: none; } }
  :global(body) { overflow: hidden; }
  :global(html.dnd-on), :global(html.dnd-on *) { user-select: none; -webkit-user-select: none; cursor: grabbing !important; }
  :global(html.dnd-on [data-drop]) { outline: 1px dashed var(--tx3); outline-offset: -1px; }
  :global([data-drop].dropping) { outline: 2px solid var(--ac) !important; outline-offset: -2px; background: var(--acbg) !important; }
  .ghost { position: fixed; z-index: 80; pointer-events: none; display: flex; align-items: center; gap: var(--s2); max-width: 280px; height: 32px; padding: 0 var(--s3); border-radius: var(--r); background: var(--panel); border: 1px solid var(--ac); box-shadow: var(--sh); font-size: 13px; color: var(--tx); }
  .app { display: grid; grid-template-columns: 248px minmax(0, 1fr); height: calc(100vh / var(--z, 1)); background: var(--bg); }
  .mainc { display: grid; grid-template-rows: 48px minmax(0, 1fr); min-width: 0; min-height: 0; }
  .mainc.ld { grid-template-rows: 48px auto minmax(0, 1fr); }
  .tb { display: flex; align-items: center; gap: var(--s4); padding: 0 var(--s6); border-bottom: 1px solid var(--line); min-width: 0; }
  .crumb { display: flex; gap: var(--s1); align-items: center; color: var(--tx2); font-size: 13px; min-width: 0; overflow: hidden; }
  .crumb a { color: var(--tx2); text-decoration: none; flex: none; max-width: 200px; }
  .crumb a:hover { color: var(--tx); }
  .crumb b { color: var(--tx); font-weight: 550; }
  .s { color: var(--tx3); flex: none; display: grid; }
  .content { min-height: 0; min-width: 0; overflow: hidden; }
  .ctr { display: flex; flex-direction: column; overflow: hidden; background: var(--bg); min-width: 0; min-height: 0; height: 100%; }
  .toast { display: flex; align-items: center; gap: var(--s3); position: fixed; bottom: var(--s6); left: 50%; transform: translateX(-50%); background: var(--panel); border: 1px solid var(--line-strong); border-radius: var(--r); padding: var(--s2) var(--s4); font-size: 13px; box-shadow: var(--sh); z-index: 60; max-width: calc(100% - 32px); }
  .ta { all: unset; cursor: pointer; color: var(--ac); font-weight: 600; flex: none; }
  .ta:hover, .ta:focus-visible { text-decoration: underline; }
</style>
