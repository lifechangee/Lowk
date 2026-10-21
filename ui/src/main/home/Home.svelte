<script lang="ts">
  import Button from '../../lib/components/Button.svelte';
  import Icon from '../../lib/components/Icon.svelte';
  import RecDot from '../../lib/components/RecDot.svelte';
  import Section from '../../lib/components/Section.svelte';
  import Tag from '../../lib/components/Tag.svelte';
  import { locale, tPlural } from '../../lib/i18n/locale.svelte';
  import { api, folderKey } from '../api';
  import { bindFor, triggerText } from '../binds';
  import { mouseNames } from '../names';
  import { clock, duration, greeting, when } from '../fmt';
  import { tm } from '../i18n';
  import { hrefs } from '../router.svelte';
  import { callRow, dnd } from '../dnd.svelte';
  import { library, prefs, session, ui } from '../state.svelte';
  import { tour } from '../tour/tour.svelte';

  const last = $derived(library.recent.find((c) => c.ended !== null));
  const folders = $derived(
    [...library.folders].sort((a, b) => (library.lastActivity(folderKey(b.id)) ?? '').localeCompare(library.lastActivity(folderKey(a.id)) ?? '')),
  );
  const folderName = (c: (typeof library.calls)[number]) =>
    c.folder.kind === 'user' ? (library.folder(folderKey(c.folder))?.title ?? '') : tm('main.unsorted');
  const ask = $derived(bindFor(prefs.value.binds, 'ask_brief'));
  const askKey = $derived(ask ? triggerText(ask.trigger, api.platform(), mouseNames()) : '');
  const empty = $derived(library.loaded && library.active.length === 0 && !library.live);
</script>

<div class="view">
  <div class="hin">
    <h1>{greeting()}</h1>
    <p class="lead lbl">
      {tm('main.libraryStats', { calls: library.active.length, folders: library.folders.length, last: last ? `${when(last.started, new Date(), false).toLocaleLowerCase(locale.lang)}, ${duration(last.duration_s)}` : '—' })}
    </p>

    {#if empty}
      <section class="empty" data-testid="home-empty">
        <div class="eic"><Icon name="mic" size={20} /></div>
        <h2>{tm('main.empty.title')}</h2>
        <p>{tm('main.empty.body', { key: askKey })}</p>
        <div class="eact">
          <span data-np-trigger style="display: contents"><Button variant="primary" onclick={() => { ui.newCallFolder = null; ui.newCallOpen = true; }}><RecDot onprimary />{tm('main.newCall')}</Button></span>
          <Button onclick={() => tour.start()}>{tm('main.empty.tour')}</Button>
        </div>
      </section>
    {:else}
    <Section title={tm('main.recentCalls')} />
    <div class="tbl">
      {#if library.live}
        <a class="tr livec" href={session.callId ? hrefs.call(session.callId) : '#/'}>
          <div class="c1"><b class="lbl"><RecDot pulse />{tm('m.newCallTitle')}</b><small class="lbl">{tm('m.recordingIn', { folder: library.live.folder.kind === 'user' ? (library.folder(folderKey(library.live.folder))?.title ?? '') : tm('m.noFolder') })}</small></div>
          <span class="mono">{clock(session.elapsed)}</span><span></span><span class="st lbl">{tm('m.recording')}</span><span></span>
        </a>
      {/if}
      {#each library.recent.filter((c) => c.ended !== null).slice(0, 6) as c (c.id)}
        <div class="tr" class:fresh={ui.justFinished === c.id} use:callRow={() => ({ id: c.id, title: c.title })}>
          <div class="c1"><a class="lk" href={hrefs.call(c.id)} draggable="false"><b class="lbl">{c.title}</b></a><small class="lbl">{folderName(c)}{c.tags.length ? ' · ' + c.tags.map((t) => '#' + t).join(' ') : ''}</small></div>
          <span class="lbl">{when(c.started)}</span><span class="lbl">{duration(c.duration_s)}</span>
          <span class="st lbl" class:ok={library.summarized.includes(c.id)}>{library.summarized.includes(c.id) ? tm('main.summaryReady') : tm('main.noSummary')}</span>
          <button class="more" aria-haspopup="menu" aria-label={tm('m.callActions')} title={tm('m.moveMenu')} onclick={(e) => { const r = e.currentTarget.getBoundingClientRect(); ui.openMove(c.id, r.right, r.bottom); }}><Icon name="more" size={14} /></button>
        </div>
      {/each}
    </div>
    <Section title={tm('main.folders')}><span>{tm('main.sortActivity')}</span></Section>
    <div class="grid">
      {#each folders as f (folderKey(f.id))}
        {@const key = folderKey(f.id)}
        <a class="fc" class:dropping={dnd.over === key} data-drop={key} href={hrefs.folder(key)}>
          <div class="ic">{f.title.slice(0, 1).toUpperCase()}</div>
          <h4 class="lbl">{f.title}</h4>
          <small class="lbl">{tPlural('main.calls', f.call_count)}</small>
          <small class="lbl">{#if library.lastActivity(key)}{tm('m.lastCall', { when: when(library.lastActivity(key)!, new Date(), false).toLocaleLowerCase(locale.lang) })}{:else}{tm('m.noCallsYet')}{/if}</small>
          <div class="tgs">{#each f.tags.slice(0, 2) as t (t)}<Tag text={'# ' + t} />{/each}</div>
        </a>
      {/each}
      <button class="fc new" onclick={() => (ui.newFolderOpen = true)}><Icon name="plus" size={14} /><span class="lbl">{tm('main.newFolder')}</span></button>
    </div>
    {/if}
  </div>
</div>

<style>
  .view { height: 100%; overflow: auto; }
  .hin { max-width: 960px; margin: 0 auto; padding: var(--s8) var(--s8) 64px; }
  h1 { font-size: 28px; line-height: 36px; letter-spacing: -0.025em; margin: 0; font-weight: 650; }
  .lead { color: var(--tx2); margin: var(--s1) 0 var(--s6); }

  .empty { display: flex; flex-direction: column; align-items: center; text-align: center; gap: var(--s3); border: 1px dashed var(--line-strong); border-radius: var(--r-xl); padding: var(--s12) var(--s8); margin-top: var(--s6); }
  .eic { width: 44px; height: 44px; border-radius: var(--r-lg); background: var(--hv); border: 1px solid var(--line); display: grid; place-items: center; color: var(--tx2); }
  .empty h2 { margin: 0; font: 600 var(--fs-lg) var(--font); }
  .empty p { margin: 0; max-width: 480px; color: var(--tx2); }
  .eact { display: flex; gap: var(--s2); margin-top: var(--s2); flex-wrap: wrap; justify-content: center; }
  .grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(216px, 1fr)); gap: var(--s4); margin-bottom: var(--s8); }
  .fc { all: unset; box-sizing: border-box; border: 1px solid var(--line); background: var(--panel); border-radius: var(--r-lg); padding: var(--s4); cursor: pointer; display: flex; flex-direction: column; gap: var(--s1); min-height: 136px; min-width: 0; transition: border-color 0.15s, transform 0.15s; }
  .fc:hover { border-color: var(--tx3); transform: translateY(-1px); }
  .ic { width: 32px; height: 32px; border-radius: var(--r); background: var(--hv); border: 1px solid var(--line); display: grid; place-items: center; font-weight: 650; font-size: 13px; color: var(--tx2); margin-bottom: var(--s1); }
  h4 { margin: 0; font: 600 var(--fs) var(--font); }
  small { color: var(--tx2); font: var(--fs-sm) var(--font); }
  .tgs { margin-top: auto; display: flex; gap: var(--s1); flex-wrap: wrap; min-width: 0; }
  .fc.new { border-style: dashed; background: transparent; flex-direction: row; align-items: center; justify-content: center; gap: var(--s2); color: var(--tx3); }
  .tbl { border: 1px solid var(--line); border-radius: var(--r-lg); background: var(--panel); overflow: hidden; margin-bottom: var(--s8); }
  .tr { display: grid; grid-template-columns: minmax(0, 1fr) 112px 96px 128px 32px; gap: var(--s4); align-items: center; padding: var(--s3) var(--s4); border-top: 1px solid var(--line); cursor: pointer; color: inherit; text-decoration: none; position: relative; }
  .tr.fresh { background: var(--acbg); }
  .lk { color: inherit; text-decoration: none; display: block; min-width: 0; }
  .lk::after { content: ""; position: absolute; inset: 0; }
  .more { all: unset; box-sizing: border-box; position: relative; z-index: 1; width: 32px; height: 32px; display: grid; place-items: center; border-radius: var(--r); color: var(--tx3); cursor: pointer; }
  .more:hover, .more:focus-visible { background: var(--sel); color: var(--tx); }
  .tr:first-child { border-top: none; }
  .tr:hover { background: var(--hv); }
  .c1 { min-width: 0; }
  .c1 b { font-weight: 550; display: block; }
  .c1 small { display: block; }
  .livec .c1 b { display: flex; gap: var(--s2); align-items: center; }
  .st { font-size: 12px; color: var(--tx2); }
  .st.ok { color: var(--ac); }
  .mono { font: 600 12px var(--mono); }
  @media (max-width: 760px) { .tr { grid-template-columns: minmax(0, 1fr) 96px; } .tr > :nth-child(n + 3):not(.more) { display: none; } .tr { grid-template-columns: minmax(0, 1fr) 96px 32px; } }
</style>
