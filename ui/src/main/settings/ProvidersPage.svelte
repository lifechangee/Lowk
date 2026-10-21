<script lang="ts">
  import { onMount } from 'svelte';
  import type { EngineDescriptor, LlmProvider } from '../../lib/ipc';
  import SettingRow from '../../lib/components/SettingRow.svelte';
  import Select from '../../lib/components/Select.svelte';
  import TextInput from '../../lib/components/TextInput.svelte';
  import Toggle from '../../lib/components/Toggle.svelte';
  import { api } from '../api';
  import { tm } from '../i18n';
  import { prefs } from '../state.svelte';
  import EngineCard from './EngineCard.svelte';
  import KeyCard from './KeyCard.svelte';

  let engines = $state<EngineDescriptor[]>([]);
  let recommended = $state('');
  onMount(async () => {
    engines = await api.engines();
    recommended = (await api.engineStatus()).find((i) => i.recommended)?.descriptor.id ?? '';
  });

  const llm = $derived(prefs.value.llm ?? {});
  // empty setting = the hardware check's pick, which is what a call would use
  const engineId = $derived(prefs.value.stt?.engine || recommended);
  const engine = $derived(engines.find((e) => e.id === engineId));
  const providers: LlmProvider[] = ['openrouter', 'ollama', 'lmstudio', 'llama_server', 'custom'];
  let model = $state('');
  let summaryModel = $state('');
  let pinned = $state('');
  let localUrl = $state('');
  let localModel = $state('');
  $effect(() => { localUrl = prefs.value.llm?.local_base_url ?? ''; localModel = prefs.value.llm?.local_model ?? ''; });
  $effect(() => { model = prefs.value.llm?.model ?? ''; });
  $effect(() => { summaryModel = prefs.value.llm?.summary_model ?? ''; pinned = prefs.value.llm?.pinned_provider ?? ''; });
  const pickEngine = (id: string) => {
    const e = engines.find((x) => x.id === id);
    prefs.patch({ stt: e?.kind === 'cloud' ? { engine: id, cloud_vendor: id } : { engine: id } });
  };
  const sizeLabel = (e: EngineDescriptor) => (e.kind === 'cloud' ? tm('m.cloud') : `${tm('m.local')} · ${(e.size_mb / 1000).toFixed(1)} GB`);
</script>

<h2>{tm('m.set.llm')}</h2>
<SettingRow label={tm('m.llmProvider')}>
  <Select label={tm('m.llmProvider')} value={llm.provider ?? 'openrouter'} options={providers.map((p) => ({ value: p, label: p === 'openrouter' ? 'OpenRouter' : p === 'ollama' ? 'Ollama' : p === 'lmstudio' ? 'LM Studio' : p === 'llama_server' ? 'llama-server' : tm('m.customEndpoint') }))} onchange={(v) => prefs.patch({ llm: { provider: v } })} />
</SettingRow>
<SettingRow label={tm('m.llmModel')} hint={tm('m.llmModelHint')}>
  <TextInput bind:value={model} mono label={tm('m.llmModel')} onenter={() => prefs.patch({ llm: { model } })} oninput={() => {}} />
</SettingRow>
<SettingRow label={tm('m.reasoning')} hint={tm('m.reasoningHint')}>
  <Toggle checked={llm.reasoning ?? false} label={tm('m.reasoning')} onchange={(v) => prefs.patch({ llm: { reasoning: v } })} />
</SettingRow>
<SettingRow label={tm('m.noDataCollection')} hint={tm('m.noDataCollectionHint')}>
  <Toggle checked={llm.data_collection_deny ?? true} label={tm('m.noDataCollection')} onchange={(v) => prefs.patch({ llm: { data_collection_deny: v } })} />
</SettingRow>
<SettingRow label={tm('summary.settings.model')} hint={tm('summary.settings.modelHint')}>
  <TextInput bind:value={summaryModel} mono label={tm('summary.settings.model')} onenter={() => prefs.patch({ llm: { summary_model: summaryModel.trim() } })} oninput={() => {}} />
</SettingRow>
<SettingRow label={tm('summary.settings.pinned')} hint={tm('summary.settings.pinnedHint')}>
  <TextInput bind:value={pinned} mono label={tm('summary.settings.pinned')} placeholder={tm('summary.settings.pinnedAuto')} onenter={() => prefs.patch({ llm: { pinned_provider: pinned.trim() || null } })} oninput={() => {}} />
</SettingRow>
<SettingRow label={tm('summary.settings.localUrl')} hint={tm('summary.settings.localUrlHint')}>
  <TextInput bind:value={localUrl} mono label={tm('summary.settings.localUrl')} placeholder="http://localhost:11434/v1" onenter={() => prefs.patch({ llm: { local_base_url: localUrl.trim() } })} oninput={() => {}} />
</SettingRow>
<SettingRow label={tm('summary.settings.localModel')}>
  <TextInput bind:value={localModel} mono label={tm('summary.settings.localModel')} onenter={() => prefs.patch({ llm: { local_model: localModel.trim() } })} oninput={() => {}} />
</SettingRow>
<KeyCard provider="openrouter" title={tm('settings.apiKey')} placeholder="sk-or-v1-…" />

<h2 class="gap">{tm('m.set.stt')}</h2>
<SettingRow label={tm('settings.stt')} hint={engine ? sizeLabel(engine) : undefined}>
  <Select label={tm('settings.stt')} value={engineId} options={engines.map((e) => ({ value: e.id, label: e.name }))} onchange={pickEngine} />
</SettingRow>
{#if engine?.kind === 'local'}
  <EngineCard engine={engine.id} />
{:else if engine?.kind === 'cloud'}
  <KeyCard provider="stt-cloud" title={tm('m.sttKey', { vendor: engine.name })} placeholder={tm('m.sttKeyPlaceholder')} note={tm('m.sttCloudNote')} />
{/if}

<style>
  h2 { font: 600 var(--fs-lg) var(--font); margin: 0 0 var(--s2); }
  .gap { margin-top: var(--s8); }
</style>
