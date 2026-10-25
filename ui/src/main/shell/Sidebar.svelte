<script lang="ts">
  // Variant A: one permanent sidebar on every screen except onboarding.
  // Logo · New call · Home · Search · Folders tree · Unsorted · Trash · Settings · recording indicator.
  import Button from '../../lib/components/Button.svelte';
  import Icon from '../../lib/components/Icon.svelte';
  import Kbd from '../../lib/components/Kbd.svelte';
  import Mark from '../../lib/components/Mark.svelte';
  import Menu from '../../lib/components/Menu.svelte';
  import RecDot from '../../lib/components/RecDot.svelte';
  import { api, folderKey, SYS_TRASH, SYS_UNSORTED } from '../api';
  import { bindFor, triggerText } from '../binds';
  import { clock } from '../fmt';
  import { tm } from '../i18n';
  import { mouseNames } from '../names';
  import { dnd } from '../dnd.svelte';
  import { hrefs, router } from '../router.svelte';
  import { library, prefs, session, ui } from '../state.svelte';
  import { tour } from '../tour/tour.svelte';
  import NewCallPopover from '../NewCallPopover.svelte';
  import StealthBadge from './StealthBadge.svelte';

  let { folder }: { folder: string } = $props();

  const route = $derived(router.route);
  const mod = api.platform() === 'macos' ? '⌘K' : 'Ctrl K';
  const recBind = $derived(bindFor(prefs.value.binds, 'toggle_record'));
  const recKey = $derived(recBind ? triggerText(recBind.trigger, api.platform(), mouseNames()) : '');
  const folderHint = $derived(ui.newCallFolder ?? (folder && folder !== SYS_TRASH ? folder : null));
</script>

<aside class="side" aria-label={tm('main.nav.label')}>
  <div class="brand"><Mark size={22} /><span class="lbl">{tm('app.name')}</span></div>

  <div class="nc" data-np-trigger data-tour="newcall">
    {#if session.phase === 'recording'}
      <div class="recpill" role="status">
        <div class="r1"><RecDot pulse /><span class="t">{clock(session.elapsed)}</span><span class="m lbl">{session.sourceLabel}</span></div>
        <div class="r2">
          <Button size="sm" onclick={() => session.callId && router.go(hrefs.call(session.callId))}>{tm('common.open')}</Button>
          <Button size="sm" variant="stop" onclick={() => session.stop()}>{tm('common.stop')}</Button>
        </div>
      </div>
    {:else}
      <Button variant="primary" block pressed={ui.newCallOpen || undefined} onclick={() => { ui.newCallFolder = null; ui.newCallOpen = !ui.newCallOpen; }}>
        <RecDot onprimary />{tm('main.newCall')}{#if recKey}<Kbd text={recKey} onprimary />{/if}
      </Button>
    {/if}
  </div>

  <nav class="nav" aria-label={tm('main.nav.label')}>
    <a class="ni" class:on={route.name === 'home'} href={hrefs.home}><Icon name="home" /><span class="lbl">{tm('main.nav.home')}</span></a>
    <a class="ni" class:on={route.name === 'search'} href={hrefs.search()} data-tour="search"><Icon name="search" /><span class="lbl">{tm('common.search')}</span><Kbd text={mod} /></a>
  </nav>

  <div class="sech"><span class="lbl">{tm('main.folders')}</span><button class="plus" aria-label={tm('main.newFolder')} title={tm('main.newFolder')} onclick={() => (ui.newFolderOpen = true)}><Icon name="plus" size={14} /></button></div>
  <nav class="tree" aria-label={tm('main.folders')} data-tour="folders">
    {#each library.folders as f (folderKey(f.id))}
      {@const key = folderKey(f.id)}
      <div class="frow" data-testid="folder-row">
        <a class="f" class:cur={folder === key} class:dropping={dnd.over === key} data-drop={key} href={hrefs.folder(key)}>
          <Icon name="folder" /><span class="lbl">{f.title}</span><span class="n">{f.call_count}</span>
        </a>
        <span class="fm"><Menu label={tm('main.folderMenuOf', { title: f.title })} testid="folder-row-menu" items={[
          { id: 'delete', label: tm('m.folderDelete'), icon: 'trash', danger: true, onselect: () => (ui.deleteFolderKey = key) },
        ]} /></span>
      </div>
    {:else}
      <p class="none">{tm('main.nav.noFolders')}</p>
    {/each}
    <div class="sysh"></div>
    <a class="f" class:cur={folder === SYS_UNSORTED} class:dropping={dnd.over === SYS_UNSORTED} data-drop={SYS_UNSORTED} href={hrefs.folder(SYS_UNSORTED)}><Icon name="inbox" /><span class="lbl">{tm('main.unsorted')}</span><span class="n">{library.unsorted.length}</span></a>
    <a class="f" class:cur={folder === SYS_TRASH} href={hrefs.folder(SYS_TRASH)}><Icon name="trash" /><span class="lbl">{tm('main.trash')}</span><span class="n">{library.trashCount}</span></a>
  </nav>

  <StealthBadge />

  <div class="foot">
    <a class="ni" class:on={route.name === 'settings'} href={hrefs.settings()}><Icon name="settings" /><span class="lbl">{tm('common.settings')}</span></a>
    <button class="help" aria-label={tm('main.nav.tour')} title={tm('main.nav.tour')} data-testid="tour-open" onclick={() => tour.start()}><Icon name="help" /></button>
  </div>

  {#if ui.newCallOpen && session.phase !== 'recording'}
    <NewCallPopover {folderHint} onclose={() => { ui.newCallOpen = false; ui.newCallFolder = null; }} />
  {/if}
</aside>

<style>
  .side { position: relative; z-index: 30; display: flex; flex-direction: column; min-height: 0; min-width: 0; background: var(--side); border-right: 1px solid var(--line); padding: var(--s3) var(--s2) var(--s2); gap: var(--s2); }
  .brand { display: flex; align-items: center; gap: var(--s2); height: 32px; padding: 0 var(--s2); font-weight: 650; font-size: 14px; letter-spacing: -0.01em; }
  .nc { padding: 0 var(--s1); }
  .nc :global(.btn.block) { justify-content: center; }
  .recpill { display: flex; flex-direction: column; justify-content: center; gap: var(--s2); height: 80px; box-sizing: border-box; padding: 0 var(--s3); border-radius: var(--r); border: 1px solid rgba(255, 77, 79, 0.4); background: var(--recbg); min-width: 0; }
  .r1 { display: flex; align-items: center; gap: var(--s2); height: 20px; min-width: 0; font-size: 13px; }
  .r1 .t { font: 600 12px/16px var(--mono); flex: none; }
  .r1 .m { color: var(--tx2); font-size: 12px; min-width: 0; }
  .r2 { display: flex; gap: var(--s2); }
  .nav { display: flex; flex-direction: column; }
  .ni { display: flex; align-items: center; gap: var(--s2); height: 32px; padding: 0 var(--s2); border-radius: var(--r); color: var(--tx2); text-decoration: none; font-size: 13px; min-width: 0; }
  .ni .lbl { flex: 1; min-width: 0; }
  .ni:hover { background: var(--hv); color: var(--tx); }
  .ni.on { background: var(--sel); color: var(--tx); font-weight: 550; }
  .sech { display: flex; align-items: center; justify-content: space-between; height: 28px; padding: 0 var(--s1) 0 var(--s2); margin-top: var(--s2); font: 600 var(--fs-xs) var(--font); letter-spacing: 0.08em; text-transform: uppercase; color: var(--tx3); }
  .plus { all: unset; cursor: pointer; width: 24px; height: 24px; display: grid; place-items: center; border-radius: var(--r-sm); color: var(--tx2); }
  .plus:hover, .plus:focus-visible { background: var(--hv); color: var(--tx); }
  .tree { flex: 1; min-height: 0; overflow: auto; display: flex; flex-direction: column; }
  .frow { position: relative; flex: none; }
  .frow .f { width: 100%; box-sizing: border-box; }
  .fm { position: absolute; right: 0; top: 0; display: none; }
  .fm :global(.mbtn) { width: 32px; height: 32px; background: var(--hv); }
  .frow:hover .fm, .frow:focus-within .fm { display: block; }
  .frow:hover .n, .frow:focus-within .n { visibility: hidden; }
  .f { display: flex; align-items: center; gap: var(--s2); height: 32px; padding: 0 var(--s2); border-radius: var(--r); font-size: 13px; font-weight: 500; color: var(--tx); text-decoration: none; flex: none; min-width: 0; }
  .f :global(svg) { color: var(--tx2); }
  .f .lbl { flex: 1; min-width: 0; }
  .f:hover { background: var(--hv); }
  .f.cur { background: var(--sel); }
  .n { margin-left: auto; color: var(--tx3); font-size: 12px; font-weight: 400; flex: none; }
  .sysh { height: 1px; background: var(--line); margin: var(--s2) var(--s2); flex: none; }
  .none { margin: var(--s1) var(--s2); color: var(--tx3); font-size: 12px; line-height: 16px; }
  .foot { display: flex; align-items: center; gap: var(--s1); border-top: 1px solid var(--line); padding-top: var(--s2); }
  .foot .ni { flex: 1; }
  .help { all: unset; cursor: pointer; width: 32px; height: 32px; display: grid; place-items: center; border-radius: var(--r); color: var(--tx2); flex: none; }
  .help:hover, .help:focus-visible { background: var(--hv); color: var(--tx); }
</style>
