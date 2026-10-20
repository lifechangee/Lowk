<script lang="ts">
  import { onMount } from 'svelte';
  import type { AudioApp, Target } from '../../lib/ipc';
  import Button from '../../lib/components/Button.svelte';
  import Kbd from '../../lib/components/Kbd.svelte';
  import RecDot from '../../lib/components/RecDot.svelte';
  import Select from '../../lib/components/Select.svelte';
  import { api, folderKey, SYS_UNSORTED } from '../api';
  import { bindFor, triggerText } from '../binds';
  import { clock } from '../fmt';
  import { tm } from '../i18n';
  import { hrefs, router } from '../router.svelte';
  import { library, prefs, session } from '../state.svelte';
  import { mouseNames } from '../names';
  import { resolveSource } from '../source';

  let { flash = false }: { flash?: boolean } = $props();

  let apps = $state<AudioApp[]>([]);
  let folder = $state<string>(SYS_UNSORTED);
  let source = $state<string>('system');
  const os = api.platform();

  onMount(async () => {
    apps = await api.audioApps();
    // F12: the onboarding choice (stored default) wins; nothing stored yet = the recommended single app.
    source = resolveSource(prefs.value.audio?.default_source, apps, prefs.value.capture?.default_source);
  });

  const folderOptions = $derived([
    ...library.folders.map((f) => ({ value: folderKey(f.id), label: f.title })),
    { value: SYS_UNSORTED, label: tm('m.noFolder') },
  ]);
  const sourceOptions = $derived([
    ...apps.map((a, i) => ({ value: `app:${a.id}`, label: tm(i === 0 ? 'm.sourceOnlyRecommended' : 'm.sourceOnly', { app: a.name }) })),
    { value: 'system', label: tm('m.sourceSystem') },
  ]);
  const rec = $derived(bindFor(prefs.value.binds, 'toggle_record'));
  const recKey = $derived(rec ? triggerText(rec.trigger, os, mouseNames()) : '');
  const folderTitle = $derived(session.folder === SYS_UNSORTED || !session.folder ? tm('m.noFolder') : (library.folder(session.folder)?.title ?? ''));

  async function start() {
    const target: Target = source.startsWith('app:') ? { kind: 'app', id: source.slice(4) } : { kind: 'whole_system' };
    const label = source.startsWith('app:') ? (apps.find((a) => `app:${a.id}` === source)?.name ?? '') : tm('m.sourceSystemShort');
    await session.start(folder, target, label);
  }
</script>

<section class="nc" class:flash>
  <div class="ttl">
    <h2 class="lbl">{session.phase === 'recording' ? tm('main.recordingNow') : tm('main.newCall')}</h2>
    <small class="lbl">{session.phase === 'recording' ? tm('m.overlayShows') : recKey ? tm('m.startHint', { key: recKey }) : ''}</small>
  </div>
  {#if session.phase === 'recording'}
    <div class="recrow">
      <RecDot pulse />
      <div class="timer">
        <div class="bigt">{clock(session.elapsed)}</div>
        <div class="info lbl">{tm('main.recordingSource', { folder: folderTitle, source: session.sourceLabel })}</div>
      </div>
      <span class="sp"></span>
      <Button size="lg" onclick={() => session.callId && router.go(hrefs.call(session.callId))}>{tm('main.openCall')}</Button>
      <Button size="lg" variant="stop" icon="stop" onclick={() => session.stop()}>{tm('main.stopRecording')}</Button>
    </div>
  {:else}
    <div class="fld"><label for="nc-folder">{tm('m.folder')}</label><div id="nc-folder"><Select size="lg" label={tm('m.folder')} value={folder} options={folderOptions} onchange={(v) => (folder = v)} /></div></div>
    <div class="fld"><label for="nc-src">{tm('m.audioSource')}</label><div id="nc-src"><Select size="lg" label={tm('m.audioSource')} value={source} options={sourceOptions} onchange={(v) => (source = v)} /></div></div>
    <Button variant="primary" size="lg" onclick={start}>
      <RecDot onprimary />
      {tm('main.startRecording')}
      {#if recKey}<Kbd text={recKey} onprimary />{/if}
    </Button>
  {/if}
</section>

<style>
  .nc { border: 1px solid var(--line); background: var(--panel); border-radius: var(--r-xl); padding: var(--s6); display: flex; gap: var(--s6); align-items: flex-end; flex-wrap: wrap; margin-bottom: var(--s6); transition: border-color 0.3s, box-shadow 0.3s; }
  .nc.flash { border-color: var(--ac); box-shadow: 0 0 0 4px var(--acbg); }
  .ttl { width: 100%; display: flex; justify-content: space-between; align-items: center; gap: var(--s4); min-width: 0; }
  h2 { margin: 0; font: 600 var(--fs-lg) var(--font); min-width: 0; }
  small { color: var(--tx2); font: var(--fs-sm) var(--font); }
  .fld { display: flex; flex-direction: column; gap: var(--s1); flex: 1; min-width: 200px; }
  .fld label { font: 600 var(--fs-xs) var(--font); text-transform: uppercase; letter-spacing: 0.08em; color: var(--tx3); }
  .recrow { width: 100%; display: flex; align-items: center; gap: var(--s6); flex-wrap: wrap; }
  .timer { min-width: 0; }
  .bigt { font: 600 28px/36px var(--mono); letter-spacing: -0.02em; }
  .info { color: var(--tx2); font: var(--fs-sm) var(--font); max-width: 360px; }
  .sp { flex: 1; }
</style>
