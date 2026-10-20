<script lang="ts">
  import Button from '../../lib/components/Button.svelte';
  import Icon from '../../lib/components/Icon.svelte';
  import Kbd from '../../lib/components/Kbd.svelte';
  import RecDot from '../../lib/components/RecDot.svelte';
  import Tag from '../../lib/components/Tag.svelte';
  import { api, folderKey, SYS_TRASH, SYS_UNSORTED } from '../api';
  import { duration, when } from '../fmt';
  import { tm } from '../i18n';
  import { hrefs, router } from '../router.svelte';
  import { callRow, dnd } from '../dnd.svelte';
  import { library, ui } from '../state.svelte';

  let { folder, callId, onnewfolder }: { folder: string; callId: string | null; onnewfolder: () => void } = $props();

  let open = $state<Record<string, boolean>>({});
  const isOpen = (key: string) => open[key] ?? key === folder;
  const mod = api.platform() === 'macos' ? '⌘K' : 'Ctrl K';
</script>

<aside class="left">
  <div class="top">
    <Button variant="ghost" block icon="arrow-left" onclick={() => router.go(hrefs.home)}>{tm('m.backLibrary')}</Button>
    <Button variant="primary" block onclick={() => ui.newCall()}><RecDot onprimary />{tm('main.newCall')}</Button>
    <button class="search" onclick={() => (ui.paletteOpen = true)}><Icon name="search" size={14} /><span class="lbl">{tm('common.search')}</span><Kbd text={mod} /></button>
  </div>

  <nav class="tree" aria-label={tm('main.folders')}>
    {#each library.folders as f (folderKey(f.id))}
      {@const key = folderKey(f.id)}
      <div class="f" class:cur={folder === key} class:open={isOpen(key)} class:dropping={dnd.over === key} data-drop={key}>
        <button class="chv" aria-label={tm('main.folders')} onclick={() => (open[key] = !isOpen(key))}><Icon name="chevron-right" size={12} /></button>
        <a class="fl" href={hrefs.folder(key)}><Icon name="folder" /><span class="lbl">{f.title}</span><span class="n">{f.call_count}</span></a>
      </div>
      {#if isOpen(key)}
        <div class="kids">
          {#each library.callsIn(key) as c (c.id)}
            <a class="c" class:on={callId === c.id} href={hrefs.call(c.id)} draggable="false" use:callRow={() => ({ id: c.id, title: c.title, movable: c.ended !== null })}>
              {#if c.ended === null}<b class="live"><RecDot pulse /><span class="lbl">{c.title}</span></b><small class="lbl">{tm('m.recording')}</small>
              {:else}<span class="lbl t">{c.title}</span><small class="lbl">{when(c.started, new Date(), false)} · {duration(c.duration_s)}</small>{/if}
            </a>
          {:else}
            <small class="empty lbl">{tm('m.noCallsYet')}</small>
          {/each}
        </div>
      {/if}
    {/each}

    <div class="sysh"></div>
    <a class="f sysf" class:cur={folder === SYS_UNSORTED} class:dropping={dnd.over === SYS_UNSORTED} data-drop={SYS_UNSORTED} href={hrefs.folder(SYS_UNSORTED)}><span class="fl"><Icon name="inbox" /><span class="lbl">{tm('main.unsorted')}</span><span class="n">{library.unsorted.length}</span></span></a>
    <a class="f sysf" class:cur={folder === SYS_TRASH} href={hrefs.folder(SYS_TRASH)}><span class="fl"><Icon name="trash" /><span class="lbl">{tm('main.trash')}</span><span class="n">{library.trashed.length}</span></span></a>
  </nav>

  {#if library.tags.length}
    <div class="lt">
      <div class="h6">{tm('main.tags')}</div>
      {#each library.tags.slice(0, 6) as [t, n], i (t)}<Tag accent={i === 0} text={`# ${t} · ${n}`} />{/each}
    </div>
  {/if}
  <div class="foot">
    <button onclick={onnewfolder}><Icon name="plus" size={12} /><span class="lbl">{tm('main.newFolder')}</span></button>
    <span class="sp"></span>
    <button onclick={() => router.go(hrefs.settings())}><Icon name="settings" size={12} /><span class="lbl">{tm('common.settings')}</span></button>
  </div>
</aside>

<style>
  .left { background: var(--side); border-right: 1px solid var(--line); display: flex; flex-direction: column; min-height: 0; min-width: 0; }
  .top { padding: var(--s3); display: flex; flex-direction: column; gap: var(--s2); }
  .search { all: unset; box-sizing: border-box; height: 32px; padding: 0 var(--s3); border-radius: var(--r); background: var(--hv); border: 1px solid var(--line); display: flex; gap: var(--s2); align-items: center; color: var(--tx2); cursor: pointer; font-size: 13px; }
  .search:hover { border-color: var(--tx3); }
  .search span { flex: 1; text-align: left; }
  .tree { padding: 0 var(--s2); flex: 1; overflow: auto; min-height: 0; }
  .f { display: flex; align-items: center; gap: var(--s1); height: 32px; padding: 0 var(--s2) 0 0; border-radius: var(--r); font-weight: 500; color: var(--tx); text-decoration: none; }
  .f:hover { background: var(--hv); }
  .f.cur { background: var(--hv); }
  .chv { all: unset; cursor: pointer; width: 24px; height: 32px; display: grid; place-items: center; color: var(--tx3); transition: transform 0.15s; flex: none; }
  .open .chv { transform: rotate(90deg); }
  .fl { display: flex; align-items: center; gap: var(--s2); flex: 1; min-width: 0; height: 32px; color: inherit; text-decoration: none; }
  .fl :global(svg) { color: var(--tx2); }
  .sysf { padding-left: var(--s2); cursor: pointer; }
  .n { margin-left: auto; color: var(--tx3); font-size: 12px; font-weight: 400; flex: none; }
  .kids { margin: 0 0 var(--s2) var(--s4); border-left: 1px solid var(--line); padding-left: var(--s2); }
  .c { display: block; padding: var(--s1) var(--s2); border-radius: var(--r); color: var(--tx2); line-height: 20px; text-decoration: none; min-width: 0; }
  .c:hover { background: var(--hv); }
  .c.on { background: var(--sel); color: var(--tx); }
  .c .t { display: block; }
  .c small, .empty { display: block; color: var(--tx3); font: var(--fs-xs) var(--font); }
  .empty { padding: var(--s1) var(--s2); }
  .live { display: flex; gap: var(--s2); align-items: center; font-weight: 500; color: var(--tx); min-width: 0; }
  .sysh { height: 1px; background: var(--line); margin: var(--s2) var(--s2); }
  .lt { padding: var(--s3) var(--s4); border-top: 1px solid var(--line); display: flex; flex-wrap: wrap; gap: var(--s1); }
  .h6 { width: 100%; margin: 0 0 var(--s1); font: 600 var(--fs-xs) var(--font); letter-spacing: 0.08em; text-transform: uppercase; color: var(--tx3); }
  .foot { padding: var(--s2) var(--s3); border-top: 1px solid var(--line); display: flex; gap: var(--s2); color: var(--tx2); font-size: 12px; align-items: center; height: 40px; min-width: 0; }
  .foot button { all: unset; cursor: pointer; display: inline-flex; gap: var(--s1); align-items: center; min-width: 0; }
  .foot button:hover { color: var(--tx); }
  .sp { flex: 1; }
</style>
