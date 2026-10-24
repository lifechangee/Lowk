<script lang="ts">
  import { tick } from 'svelte';
  import Button from '../../lib/components/Button.svelte';
  import Icon from '../../lib/components/Icon.svelte';
  import RecDot from '../../lib/components/RecDot.svelte';
  import { clock } from '../fmt';
  import { tm } from '../i18n';
  import { library, session } from '../state.svelte';

  const folder = $derived(session.folder && session.folder !== 'sys:unsorted' ? (library.folder(session.folder)?.title ?? '') : tm('m.noFolder'));
  let box: HTMLElement | undefined = $state();
  $effect(() => {
    session.lines.length;
    tick().then(() => box?.scrollTo({ top: box.scrollHeight, behavior: 'smooth' }));
  });
</script>

<div class="page">
  <div class="cin chead">
    <h1 class="lbl">{tm('m.newCallTitle')}</h1>
    <div class="meta lbl">{tm('main.recordingSource', { folder, source: session.sourceLabel })}</div>
  </div>
  <div class="doc" bind:this={box}>
    <div class="cin">
      <div class="livebar"><RecDot pulse /><span class="t">{clock(session.elapsed)}</span><span class="m lbl">{session.sttLoading ? tm('main.sttLoading') : tm('main.liveNotice')}</span><span class="sp"></span><button class="mic" class:off={!session.micOn} type="button" data-testid="mic-toggle" aria-pressed={session.micOn} title={session.micOn ? tm('m.micOff') : tm('m.micOn')} aria-label={session.micOn ? tm('m.micOff') : tm('m.micOn')} onclick={() => void session.setMic(!session.micOn).catch(() => undefined)}><Icon name={session.micOn ? 'mic' : 'mic-off'} size={16} /><span class="lbl">{session.micOn ? '' : tm('m.micMuted')}</span></button><Button variant="stop" icon="stop" onclick={() => session.stop()}>{tm('main.stopRecording')}</Button></div>
      {#each session.lines as l, i (i)}
        <div class="ln" class:sys={l.speaker === 'system'}><span class="ts">{clock(l.at_s)}</span><span class={l.speaker === 'me' ? 'sp-me' : 'sp-sys'}>{l.speaker === 'me' ? tm('main.speakerMe') : tm('main.speakerSystem')}</span><span class="tx">{l.text}</span></div>
      {/each}
    </div>
  </div>
</div>

<style>
  .page { display: flex; flex-direction: column; min-height: 0; flex: 1; }
  .cin { max-width: 768px; width: 100%; margin: 0 auto; padding: 0 var(--s8); }
  .chead { padding-top: var(--s6); padding-bottom: var(--s4); }
  h1 { font-size: 24px; line-height: 32px; margin: 0; font-weight: 650; letter-spacing: -0.025em; }
  .meta { color: var(--tx2); font-size: 12px; line-height: 20px; margin-top: var(--s1); }
  .doc { flex: 1; overflow: auto; padding: 0 0 64px; min-height: 0; }
  .livebar { display: flex; align-items: center; gap: var(--s3); padding: var(--s3) var(--s4); border: 1px solid rgba(255, 77, 79, 0.35); background: var(--recbg); border-radius: var(--r-lg); margin-bottom: var(--s4); min-width: 0; }
  .t { font: 600 14px/20px var(--mono); }
  .m { color: var(--tx2); font-size: 12px; }
  .sp { flex: 1; }
  .mic { all: unset; box-sizing: border-box; display: inline-flex; align-items: center; gap: var(--s2); height: 32px; min-width: 32px; padding: 0 var(--s2); justify-content: center; border-radius: var(--r); border: 1px solid var(--line); color: var(--tx); cursor: pointer; font-size: 12px; }
  .mic:hover { background: var(--hv); }
  .mic.off { color: var(--rec); border-color: rgba(255, 77, 79, 0.45); }
  .mic:focus-visible { outline: 2px solid var(--ac); outline-offset: 2px; }
  .ln { display: grid; grid-template-columns: 64px 72px minmax(0, 1fr); gap: var(--s3); padding: var(--s1) 0; animation: fi 0.4s; }
  @keyframes fi { from { opacity: 0; transform: translateY(4px); } }
  .ts { font: 500 12px/20px var(--mono); color: var(--tx3); }
  .sp-me { font-size: 12px; font-weight: 700; }
  .sp-sys { font-size: 12px; font-weight: 600; color: var(--tx2); }
  .sys .tx { color: var(--tx2); }
</style>
