<script lang="ts">
  // The sidebar «Новый созвон» button opens this popover on every screen (folder + source + languages + Start):
  // the one and only way to start a call from the UI.
  import type { Target } from '../lib/ipc';
  import Button from '../lib/components/Button.svelte';
  import RecDot from '../lib/components/RecDot.svelte';
  import Select from '../lib/components/Select.svelte';
  import { folderKey, SYS_TRASH, SYS_UNSORTED } from './api';
  import { tm } from './i18n';
  import { nameFromId, resolveSource } from './source';
  import SourcePicker from './SourcePicker.svelte';
  import LangPicker from './LangPicker.svelte';
  import { callLangs, library, prefs, session, sources } from './state.svelte';

  let { folderHint, onclose }: { folderHint: string | null; onclose: () => void } = $props();

  let el: HTMLElement | undefined = $state();
  let folderChoice = $state<string | null>(null);
  let sourceChoice = $state<string | null>(null);
  let langChoice = $state<string[] | null>(null);
  const langs = $derived(langChoice ?? callLangs.initial);
  const folder = $derived(folderChoice ?? (folderHint && folderHint !== SYS_TRASH ? folderHint : SYS_UNSORTED));
  const source = $derived(sourceChoice ?? resolveSource(prefs.value.audio?.default_source, sources.running, prefs.value.capture?.default_source));
  const folderOptions = $derived([
    ...library.folders.map((f) => ({ value: folderKey(f.id), label: f.title })),
    { value: SYS_UNSORTED, label: tm('m.noFolder') },
  ]);

  async function start() {
    const target: Target = source.startsWith('app:') ? { kind: 'app', id: source.slice(4) } : { kind: 'whole_system' };
    const label = source.startsWith('app:') ? (sources.find(source.slice(4))?.name ?? nameFromId(source.slice(4))) : tm('m.sourceSystemShort');
    onclose();
    await session.start(folder, target, label, langs);
  }
  function outside(e: PointerEvent) {
    const t = e.target as HTMLElement;
    // clicks inside the popover (and its source picker modal) or on the header button itself are not "outside"
    if (el && !el.contains(t) && !t.closest('.modal, .scrim, [data-np-trigger]')) onclose();
  }
  function key(e: KeyboardEvent) {
    if (e.key === 'Escape') onclose();
  }
</script>

<svelte:window onpointerdowncapture={outside} onkeydown={key} />

<div class="np pop" role="dialog" aria-label={tm('main.newCall')} bind:this={el}>
  <div class="fld"><label for="np-folder">{tm('m.folder')}</label><div id="np-folder"><Select label={tm('m.folder')} value={folder} options={folderOptions} onchange={(v) => (folderChoice = v)} /></div></div>
  <div class="fld"><label for="np-src">{tm('m.audioSource')}</label><div id="np-src"><SourcePicker size="md" label={tm('m.audioSource')} value={source} onchange={(v) => (sourceChoice = v)} /></div></div>
  <div class="fld"><span class="flbl">{tm('m.callLangsShort')}</span><LangPicker value={langs} supported={callLangs.supported} label={tm('m.callLangs')} onchange={(v) => (langChoice = v)} /></div>
  <Button variant="primary" block onclick={start}><RecDot onprimary />{tm('main.startRecording')}</Button>
</div>

<style>
  .np { position: absolute; z-index: 45; top: 92px; left: var(--s2); width: 320px; max-width: calc(100vw - 32px); padding: var(--s4); display: flex; flex-direction: column; gap: var(--s3); border-radius: var(--r-lg); background: var(--panel); border: 1px solid var(--line-strong); box-shadow: var(--sh); }
  .fld { display: flex; flex-direction: column; gap: var(--s1); min-width: 0; }
  .flbl, .fld label { font: 600 var(--fs-xs) var(--font); text-transform: uppercase; letter-spacing: 0.08em; color: var(--tx3); }
  .np :global(.btn.block) { justify-content: center; }
</style>
