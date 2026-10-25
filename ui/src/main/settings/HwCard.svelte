<script lang="ts">
  // What Lowk found on this computer (onboarding hardware step + Settings). Numbers follow the UI language;
  // long names (CPU, GPU) wrap instead of being cut.
  import type { HwInfo } from '../../lib/ipc';
  import { fmtGb } from '../fmt';
  import { tm } from '../i18n';

  let { hw }: { hw: HwInfo | null } = $props();

  const gpu = $derived.by(() => {
    const g = [...(hw?.gpus ?? [])].sort((a, b) => b.vram_mb - a.vram_mb)[0];
    if (!g) return null;
    const name = g.vendor && !g.name.toLowerCase().includes(g.vendor.toLowerCase()) ? `${g.vendor} ${g.name}` : g.name;
    return { name, vram: g.vram_mb > 0 ? tm('m.hw.vram', { n: fmtGb(g.vram_mb / 1024) }) : '' };
  });
  const cpuLabel = $derived(hw?.os === 'macos' ? tm('m.hw.chip') : tm('m.hw.cpu'));
  const showGpu = $derived(hw ? hw.os !== 'macos' || gpu !== null : true);
</script>

<div class="hw" data-testid="hw-card">
  <div data-testid="hw-cpu"><small>{cpuLabel}</small><b title={hw?.chip}>{hw?.chip ?? '…'}</b></div>
  <div data-testid="hw-ram"><small>{tm('m.hw.ram')}</small><b>{hw?.ram_gb ? fmtGb(hw.ram_gb) : '…'}</b></div>
  {#if showGpu}
    <div data-testid="hw-gpu"><small>{tm('m.hw.gpu')}</small><b title={gpu?.name}>{hw ? (gpu?.name ?? tm('m.hw.noGpu')) : '…'}</b>{#if gpu?.vram}<span>{gpu.vram}</span>{/if}</div>
  {/if}
  <div data-testid="hw-disk"><small>{tm('m.hw.disk')}</small><b>{hw?.free_disk_gb ? tm('m.hw.free', { n: fmtGb(hw.free_disk_gb) }) : '…'}</b></div>
  <div data-testid="hw-os"><small>{tm('m.hw.os')}</small><b title={hw?.os_version}>{hw?.os_version ?? '…'}</b></div>
</div>

<style>
  .hw { display: grid; grid-template-columns: repeat(auto-fit, minmax(170px, 1fr)); gap: var(--s2); margin-bottom: var(--s4); }
  .hw div { border: 1px solid var(--line); border-radius: var(--r-lg); padding: var(--s3); background: var(--panel); min-width: 0; }
  .hw small { display: block; color: var(--tx3); font: 600 var(--fs-xs) var(--font); text-transform: uppercase; letter-spacing: 0.06em; margin-bottom: var(--s1); }
  .hw b { font-weight: 550; font-size: 13px; line-height: 18px; display: block; overflow-wrap: anywhere; }
  .hw span { display: block; color: var(--tx3); font-size: 12px; line-height: 16px; margin-top: 2px; }
</style>
