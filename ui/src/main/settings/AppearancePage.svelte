<script lang="ts">
  import type { OverlayVisibility, PillEdge, ToneMode } from '../../lib/ipc';
  import OptionCard from '../../lib/components/OptionCard.svelte';
  import SettingRow from '../../lib/components/SettingRow.svelte';
  import Select from '../../lib/components/Select.svelte';
  import Slider from '../../lib/components/Slider.svelte';
  import { tm } from '../i18n';
  import type { MKey } from '../i18n';
  import { prefs } from '../state.svelte';

  const o = $derived(prefs.value.overlay ?? {});
  const opacity = $derived(o.opacity ?? 60);
  const vis = $derived(o.visibility ?? 'recording_only');
  const modes: OverlayVisibility[] = ['recording_only', 'always', 'manual', 'pill'];
  const edges: PillEdge[] = ['right', 'left', 'top', 'bottom'];
  const tones: ToneMode[] = ['auto', 'light', 'dark'];
  const patch = (p: object) => prefs.patch({ overlay: p });
</script>

<h2>{tm('m.set.interface')}</h2>
<SettingRow label={tm('m.uiFontSize')} hint={tm('m.uiFontSizeHint')}>
  <Slider value={prefs.value.ui?.font_px ?? 14} min={12} max={20} unit=" px" label={tm('m.uiFontSize')} onchange={(v) => prefs.patch({ ui: { font_px: v } })} />
</SettingRow>

<h2 class="gap">{tm('m.set.overlay')}</h2>
<SettingRow label={tm('m.opacity')} hint={tm('m.opacityHint')}>
  <Slider value={opacity} min={0} max={90} label={tm('m.opacity')} onchange={(v) => patch({ opacity: v })} />
</SettingRow>

<div class="prev" aria-hidden="true">
  <div class="ov" style:--k={(100 - opacity) / 100 + 0.1}>
    <div class="sub"><i class="d"></i>{tm('m.previewSub')}</div>
    <div class="ans">{tm('m.previewAnswer')}</div>
  </div>
</div>

<SettingRow label={tm('m.fontSize')}>
  <Slider value={o.font_px ?? 14} min={12} max={22} unit=" px" label={tm('m.fontSize')} onchange={(v) => patch({ font_px: v })} />
</SettingRow>
<SettingRow label={tm('m.toneMode')} hint={tm('m.toneModeHint')}>
  <Select label={tm('m.toneMode')} value={o.tone_mode ?? 'auto'} options={tones.map((t) => ({ value: t, label: tm(`m.tone.${t}` as MKey) }))} onchange={(v) => patch({ tone_mode: v })} />
</SettingRow>

<h2 class="gap">{tm('m.visibility')}</h2>
<p class="lead">{tm('m.visibilityHint')}</p>
{#each modes as m (m)}
  <OptionCard selected={vis === m} title={tm(`m.vis.${m}` as MKey)} hint={tm(`m.vis.${m}.hint` as MKey)} badge={m === 'recording_only' ? tm('m.default') : undefined} onselect={() => patch({ visibility: m })} />
{/each}
{#if vis === 'pill'}
  <SettingRow label={tm('m.pillEdge')} hint={tm('m.pillEdgeHint')}>
    <Select label={tm('m.pillEdge')} value={o.pill_edge ?? 'right'} options={edges.map((e) => ({ value: e, label: tm(`m.edge.${e}` as MKey) }))} onchange={(v) => patch({ pill_edge: v })} />
  </SettingRow>
{/if}

<style>
  h2 { font: 600 var(--fs-lg) var(--font); margin: 0 0 var(--s2); }
  .gap { margin-top: var(--s8); }
  .lead { color: var(--tx2); margin: 0 0 var(--s3); font-size: 13px; }
  .prev { margin: var(--s3) 0; border-radius: var(--r-lg); border: 1px solid var(--line); padding: var(--s6); background: repeating-conic-gradient(#2a2c33 0 25%, #1f2026 0 50%) 0 0 / 16px 16px; }
  .ov { --k: 0.5; border-radius: var(--r-lg); padding: var(--s3) var(--s4); background: rgb(9 9 10 / calc(var(--k) * 0.9)); border: 1px solid rgb(255 255 255 / calc(var(--k) * 0.2)); color: #ececee; max-width: 420px; margin: 0 auto; }
  .sub { font-size: 12px; opacity: 0.66; display: flex; gap: var(--s2); align-items: center; }
  .d { width: 6px; height: 6px; border-radius: 50%; background: var(--rec); flex: none; }
  .ans { margin-top: var(--s2); font-size: 13px; line-height: 20px; }
</style>
