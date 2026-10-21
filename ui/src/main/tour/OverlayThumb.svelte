<script lang="ts">
  // A miniature of the overlay for the tour (the real overlay lives in another window and is hidden from screen sharing).
  // Everything here is a labelled example: static text or a local timer, never a network request.
  import RecDot from '../../lib/components/RecDot.svelte';
  import { tm } from '../i18n';

  type Props = {
    focus: 'subtitles' | 'answer' | 'header' | 'shot';
    /** Streamed demo answer (step 3). */
    answer?: string;
    /** Screenshots queued (step 5). */
    shots?: number;
    /** The overlay follows the cursor (step 4); position inside the stage in px. */
    moving?: boolean;
    /** Position as fractions 0..1 of the free space inside the stage. */
    pos?: { x: number; y: number };
    /** Hold-Ctrl makes it clickable (step 4). */
    clickable?: boolean;
  };
  let { focus, answer = '', shots = 0, moving = false, pos = { x: 0, y: 0 }, clickable = false }: Props = $props();
  let sw = $state(0), sh = $state(0), ow = $state(0), oh = $state(0);
  const tx = $derived(Math.max(0, (sw - ow - 24) * pos.x));
  const ty = $derived(Math.max(0, (sh - oh - 24) * pos.y));
</script>

<div class="exrow"><span class="ex lbl">{tm('tour.example')}</span></div>
<div class="stage" data-testid="overlay-thumb" data-moving={moving || undefined} bind:clientWidth={sw} bind:clientHeight={sh}>
  <div class="ov" class:moving class:clickable bind:clientWidth={ow} bind:clientHeight={oh} style:transform={moving ? `translate(${tx}px, ${ty}px)` : undefined}>
    <div class="head" class:foc={focus === 'header'}>
      <RecDot pulse /><span class="tm">00:12:07</span>
      <span class="sp"></span>
      {#if moving}<span class="mode mv lbl">{tm('tour.mode.moving')}</span>{:else if clickable}<span class="mode lbl">{tm('tour.mode.clickable')}</span>{/if}
      {#if shots > 0}<span class="chip lbl" data-testid="thumb-shots">{tm('tour.shots', { n: shots })}</span>{/if}
    </div>
    <div class="sub" class:foc={focus === 'subtitles'}>
      <p><b>{tm('tour.demo.who')}</b> {tm('tour.demo.line1')}</p>
      <p class="dim"><b>{tm('main.speakerMe')}</b> {tm('tour.demo.line2')}</p>
    </div>
    <div class="ans" class:foc={focus === 'answer'}>
      {#if answer}<p data-testid="thumb-answer">{answer}</p>{:else}<p class="dim">{tm('tour.demo.answerHint')}</p>{/if}
    </div>
  </div>
</div>

<style>
  .exrow { display: flex; justify-content: flex-end; margin-bottom: var(--s1); }
  .stage { position: relative; height: 208px; border-radius: var(--r); background: linear-gradient(135deg, #2b2f3a, #14161c); overflow: hidden; border: 1px solid var(--line); }
  .ex { max-width: 100%; font: 600 10px/16px var(--font); letter-spacing: 0.08em; text-transform: uppercase; color: #ffc24a; background: rgba(0, 0, 0, 0.55); padding: 0 var(--s2); border-radius: var(--r-sm); }
  .ov { position: absolute; left: var(--s3); top: var(--s3); width: calc(100% - 24px); border-radius: var(--r); background: rgba(9, 9, 10, 0.78); border: 1px solid rgba(255, 255, 255, 0.16); padding: var(--s2); display: flex; flex-direction: column; gap: var(--s2); font-size: 11px; line-height: 16px; color: #ececee; }
  .ov.moving { border-color: #ffc24a; width: min(260px, calc(100% - 24px)); transition: none; }
  .ov.clickable { border-color: #8ba6c4; }
  .head { display: flex; align-items: center; gap: var(--s2); padding: 2px var(--s1); border-radius: var(--r-sm); }
  .tm { font: 600 10px/16px var(--mono); }
  .sp { flex: 1; }
  .mode { font-size: 10px; line-height: 16px; padding: 0 var(--s1); border-radius: var(--s1); background: rgba(139, 166, 196, 0.25); color: #c4d4e6; }
  .mode.mv { background: rgba(255, 194, 74, 0.25); color: #ffd98a; }
  .chip { font: 600 10px/16px var(--font); padding: 0 var(--s2); border-radius: 99px; background: rgba(255, 255, 255, 0.16); }
  .sub p, .ans p { margin: 0; overflow-wrap: anywhere; }
  .sub, .ans { padding: 2px var(--s1); border-radius: var(--r-sm); }
  .dim { color: rgba(236, 236, 238, 0.72); }
  .ans { border-top: 1px solid rgba(255, 255, 255, 0.12); min-height: 40px; }
  .foc { outline: 1px solid #8ba6c4; outline-offset: 1px; background: rgba(139, 166, 196, 0.12); }
</style>
