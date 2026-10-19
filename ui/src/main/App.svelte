<script lang="ts">
  import '../lib/tokens.css';
  import { onMount } from 'svelte';
  import Button from '../lib/components/Button.svelte';
  import Icon from '../lib/components/Icon.svelte';
  import Kbd from '../lib/components/Kbd.svelte';
  import RecDot from '../lib/components/RecDot.svelte';
  import { loadSystemLocale, locale } from '../lib/i18n/locale.svelte';
  import { api, folderKey } from './api';
  import { bindFor, triggerText } from './binds';
  import { clock } from './fmt';
  import { tm } from './i18n';
  import { mouseNames } from './names';
  import Onboarding from './onboarding/Onboarding.svelte';
  import { hrefs, router } from './router.svelte';
  import SearchPalette from './SearchPalette.svelte';
  import SettingsView from './settings/SettingsView.svelte';
  import { library, prefs, session, ui } from './state.svelte';
  import Home from './home/Home.svelte';
  import Workspace from './workspace/Workspace.svelte';

  let ready = $state(false);
  const route = $derived(router.route);

  onMount(async () => {
    locale.init(await loadSystemLocale());
    await Promise.all([prefs.load(), library.reload()]);
    if (!prefs.value.onboarding?.done && router.route.name !== 'onboarding') router.go(hrefs.onboarding(prefs.value.onboarding?.step ?? 0));
    ready = true;
  });

  // Mouse side buttons must not navigate history (WebView2 does that by default).
  function onmouseup(e: MouseEvent) { if (e.button === 3 || e.button === 4) e.preventDefault(); }
  function onkeydown(e: KeyboardEvent) {
    if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'k') { e.preventDefault(); ui.paletteOpen = !ui.paletteOpen; }
  }

  const crumbs = $derived.by<{ label: string; href?: string }[]>(() => {
    const lib = { label: tm('main.title'), href: hrefs.home };
    if (route.name === 'folder') return [lib, { label: folderLabel(route.id) }];
    if (route.name === 'call') {
      const c = library.call(route.id);
      return [lib, ...(c ? [{ label: folderLabel(c.deleted ? 'sys:trash' : folderKey(c.folder)), href: hrefs.folder(c.deleted ? 'sys:trash' : folderKey(c.folder)) }, { label: c.title }] : [])];
    }
    if (route.name === 'settings') return [lib, { label: tm('settings.title') }, { label: tm(`m.nav.${route.page}`) }];
    return [{ label: tm('main.title') }];
  });
  function folderLabel(key: string): string {
    return key === 'sys:unsorted' ? tm('main.unsorted') : key === 'sys:trash' ? tm('main.trash') : (library.folder(key)?.title ?? '');
  }
  const recBind = $derived(bindFor(prefs.value.binds, 'toggle_record'));
  const recKey = $derived(recBind ? triggerText(recBind.trigger, api.platform(), mouseNames()) : '');
</script>

<svelte:window {onmouseup} {onkeydown} />

{#if ready}
  {#if route.name === 'onboarding'}
    <Onboarding step={route.step} />
  {:else}
    <div class="app">
      <header class="tb">
        <a class="wm" href={hrefs.home} title={tm('m.toLibrary')}><i></i><span class="lbl">{tm('app.name')}</span></a>
        <nav class="crumb" aria-label="breadcrumb">
          {#each crumbs as c, i (i)}
            {#if i > 0}<span class="s">/</span>{/if}
            {#if c.href && i < crumbs.length - 1}<a class="lbl" href={c.href}>{c.label}</a>{:else}<b class="lbl">{c.label}</b>{/if}
          {/each}
        </nav>
        <span class="sp"></span>
        {#if session.phase === 'recording'}
          <span class="recpill"><RecDot pulse /><span class="t">{clock(session.elapsed)}</span><span class="m lbl">{library.live ? '' : ''}{session.sourceLabel}</span>
            <Button size="sm" onclick={() => session.callId && router.go(hrefs.call(session.callId))}>{tm('common.open')}</Button>
            <Button size="sm" variant="stop" onclick={() => session.stop()}>{tm('common.stop')}</Button></span>
        {:else}
          <Button variant="primary" onclick={() => ui.newCall()}><RecDot onprimary />{tm('main.newCall')}{#if recKey}<Kbd text={recKey} onprimary />{/if}</Button>
        {/if}
        <a class="gear" href={hrefs.settings()} aria-label={tm('common.settings')} title={tm('common.settings')}><Icon name="settings" /></a>
      </header>
      <div class="content">
        {#if route.name === 'home'}<Home flash={ui.flashNew} />
        {:else if route.name === 'settings'}<SettingsView page={route.page} />
        {:else}<Workspace {route} />{/if}
      </div>
    </div>
  {/if}
  <SearchPalette />
  {#if ui.toast}<div class="toast" role="status">{ui.toast}</div>{/if}
{/if}

<style>
  :global(body) { overflow: hidden; }
  .app { display: grid; grid-template-rows: 48px minmax(0, 1fr); height: 100vh; background: var(--bg); }
  .tb { display: flex; align-items: center; gap: var(--s4); padding: 0 var(--s4); background: var(--side); border-bottom: 1px solid var(--line); min-width: 0; }
  .wm { display: inline-flex; gap: var(--s2); align-items: center; height: 32px; padding: 0 var(--s2); border-radius: var(--r); font-weight: 650; font-size: 14px; letter-spacing: -0.01em; color: var(--tx); text-decoration: none; flex: none; }
  .wm:hover { background: var(--hv); }
  .wm i { width: 16px; height: 16px; border: 1.5px solid var(--tx2); border-radius: var(--s1); display: block; }
  .crumb { display: flex; gap: var(--s2); align-items: center; color: var(--tx2); font-size: 13px; min-width: 0; overflow: hidden; }
  .crumb a { color: var(--tx2); text-decoration: none; flex: none; max-width: 200px; }
  .crumb a:hover { color: var(--tx); }
  .crumb b { color: var(--tx); font-weight: 550; }
  .s { color: var(--tx3); flex: none; }
  .sp { flex: 1; }
  .recpill { display: inline-flex; align-items: center; gap: var(--s2); height: 32px; padding: 0 var(--s1) 0 var(--s3); border-radius: var(--r); border: 1px solid rgba(255, 77, 79, 0.4); background: var(--recbg); font-size: 13px; white-space: nowrap; min-width: 0; flex: none; }
  .recpill .t { font: 600 12px/16px var(--mono); }
  .recpill .m { color: var(--tx2); font-size: 12px; max-width: 160px; }
  .gear { width: 32px; height: 32px; display: grid; place-items: center; border-radius: var(--r); color: var(--tx2); flex: none; }
  .gear:hover { background: var(--hv); color: var(--tx); }
  .content { min-height: 0; min-width: 0; overflow: hidden; }
  .toast { position: fixed; bottom: var(--s6); left: 50%; transform: translateX(-50%); background: var(--panel); border: 1px solid var(--line-strong); border-radius: var(--r); padding: var(--s2) var(--s4); font-size: 13px; box-shadow: var(--sh); z-index: 60; max-width: calc(100% - 32px); }
</style>
