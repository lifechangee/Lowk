<script lang="ts">
  // Settings: one row per downloadable model (speech recognition, search by meaning) with its current state.
  import { onMount } from 'svelte';
  import type { EmbedderInfo, EngineInfo } from '../../lib/ipc';
  import { api } from '../api';
  import { Dl } from '../dl.svelte';
  import { fmtMbBytes } from '../fmt';
  import { tm } from '../i18n';
  import { prefs } from '../state.svelte';
  import ModelChip, { type ModelState } from './ModelChip.svelte';

  let engines = $state<EngineInfo[]>([]);
  let embedder = $state<EmbedderInfo | null>(null);
  const dls = { stt: new Dl(), embedder: new Dl() };
  let failed = $state({ stt: false, embedder: false });

  const sttId = $derived(prefs.value.stt?.engine || engines.find((e) => e.recommended)?.descriptor.id);
  const stt = $derived(engines.find((e) => e.descriptor.id === sttId && e.descriptor.kind === 'local') ?? null);

  async function refresh() {
    engines = await api.engineStatus();
    embedder = await api.embedderInfo().catch(() => null);
  }
  const stateOf = (key: 'stt' | 'embedder', installed: boolean | undefined): ModelState => {
    const v = dls[key].view;
    if (v.phase === 'downloading') return 'downloading';
    if (v.phase === 'verifying') return 'verifying';
    if (v.phase === 'failed' || failed[key]) return 'error';
    return installed ? 'ready' : 'missing';
  };
  onMount(() => {
    void refresh();
    return api.onEngineProgress((p) => {
      const key = p.engine === 'embedder' ? 'embedder' : p.engine === sttId ? 'stt' : null;
      if (!key) return;
      dls[key].feed(p);
      failed[key] = p.phase === 'failed';
      if (p.phase === 'ready' || p.phase === 'failed') void refresh();
    });
  });
</script>

<div class="rows" data-testid="model-rows">
  <div class="row" data-testid="model-row-stt">
    <div class="nm"><b>{tm('m.models.stt')}</b><span class="sub">{stt ? `${stt.descriptor.name} · ${tm('knowledge.model.size', { mb: fmtMbBytes((stt.descriptor.size_mb + (stt.extra_mb ?? 0)) * 1048576) })}` : tm('m.models.cloud')}</span></div>
    {#if stt}<ModelChip state={stateOf('stt', stt.installed)} />{/if}
  </div>
  <div class="row" data-testid="model-row-embedder">
    <div class="nm"><b>{tm('m.models.embed')}</b><span class="sub">{embedder ? `${embedder.name} · ${tm('knowledge.model.size', { mb: fmtMbBytes(embedder.size_mb * 1048576) })}` : ''}</span></div>
    {#if embedder}<ModelChip state={stateOf('embedder', embedder.installed)} />{/if}
  </div>
  <p class="hint">{tm('m.models.hint')}</p>
</div>

<style>
  .rows { border: 1px solid var(--line); background: var(--panel); border-radius: var(--r-lg); padding: var(--s2) var(--s4); margin: var(--s3) 0; }
  .row { display: flex; align-items: center; justify-content: space-between; gap: var(--s3); min-height: 48px; padding: var(--s2) 0; min-width: 0; }
  .row + .row { border-top: 1px solid var(--line); }
  .nm { min-width: 0; display: flex; flex-direction: column; gap: 2px; }
  .nm b { font-weight: 550; font-size: 13px; }
  .sub { color: var(--tx3); font-size: 12px; line-height: 16px; overflow-wrap: anywhere; }
  .hint { margin: 0; padding: var(--s2) 0; color: var(--tx3); font-size: 12px; border-top: 1px solid var(--line); }
</style>
