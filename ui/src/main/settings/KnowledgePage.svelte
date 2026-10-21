<script lang="ts">
  // Settings «Документы и поиск»: the embedder card and the documents card (pinned budget, slow-file warning, OCR).
  import SettingRow from '../../lib/components/SettingRow.svelte';
  import Toggle from '../../lib/components/Toggle.svelte';
  import { fmtTokens } from '../docs/util';
  import { tm } from '../i18n';
  import { prefs } from '../state.svelte';
  import EmbedderCard from './EmbedderCard.svelte';

  const docs = $derived(prefs.value.docs ?? {});
  const BUDGETS = [3000, 6000, 12000];
  const budget = $derived(docs.pinned_budget_tokens ?? 6000);
  const slow = $derived(docs.warn_slow_mb ?? 50);
  const retrieved = $derived(docs.retrieved_budget_tokens ?? 3000);

  const num = (e: Event, min: number, max: number): number => Math.min(max, Math.max(min, Math.round(Number((e.currentTarget as HTMLInputElement).value) || min)));
</script>

<h2>{tm('knowledge.search.title')}</h2>
<EmbedderCard />

<h2 class="gap">{tm('knowledge.docs.title')}</h2>
<SettingRow stack label={tm('knowledge.pinnedBudget')} hint={tm('knowledge.pinnedBudgetHint')}>
  <div class="seg" role="radiogroup" aria-label={tm('knowledge.pinnedBudget')} data-testid="pinned-budget">
    {#each BUDGETS as b (b)}<button class="sg" class:on={budget === b} role="radio" aria-checked={budget === b} onclick={() => prefs.patch({ docs: { pinned_budget_tokens: b } })}>{fmtTokens(b)}</button>{/each}
    <label class="num"><input type="number" min="500" max="60000" step="500" value={budget} aria-label={tm('knowledge.pinnedBudget')} onchange={(e) => prefs.patch({ docs: { pinned_budget_tokens: num(e, 500, 60000) } })} /><span>{tm('knowledge.tokens')}</span></label>
  </div>
</SettingRow>
<SettingRow label={tm('knowledge.retrieved')} hint={tm('knowledge.retrievedHint')}>
  <label class="num"><input type="number" min="500" max="60000" step="500" value={retrieved} aria-label={tm('knowledge.retrieved')} onchange={(e) => prefs.patch({ docs: { retrieved_budget_tokens: num(e, 500, 60000) } })} /><span>{tm('knowledge.tokens')}</span></label>
</SettingRow>
<SettingRow label={tm('knowledge.slow')} hint={tm('knowledge.slowHint')}>
  <label class="num"><input type="number" min="1" max="2000" step="5" value={slow} aria-label={tm('knowledge.slow')} onchange={(e) => prefs.patch({ docs: { warn_slow_mb: num(e, 1, 2000) } })} /><span>{tm('knowledge.mb')}</span></label>
</SettingRow>
<SettingRow label={tm('knowledge.ocr')} hint={tm('knowledge.ocrHint')}>
  <Toggle checked={docs.ocr ?? true} label={tm('knowledge.ocr')} onchange={(v) => prefs.patch({ docs: { ocr: v } })} />
</SettingRow>

<style>
  h2 { font: 600 var(--fs-lg) var(--font); margin: 0 0 var(--s2); }
  .gap { margin-top: var(--s8); }
  .seg { display: flex; gap: var(--s2); align-items: center; flex-wrap: wrap; }
  .sg { all: unset; box-sizing: border-box; cursor: pointer; height: 32px; padding: 0 var(--s3); border-radius: var(--r); border: 1px solid var(--line); background: var(--panel); font-size: 13px; line-height: 30px; color: var(--tx2); }
  .sg:hover { border-color: var(--tx3); color: var(--tx); }
  .sg:focus-visible { outline: 2px solid var(--focus); outline-offset: 1px; }
  .sg.on { border-color: var(--ac); color: var(--ac); background: var(--acbg); font-weight: 600; }
  .num { display: inline-flex; align-items: center; gap: var(--s2); color: var(--tx3); font-size: 12px; }
  .num input { box-sizing: border-box; width: 96px; height: 32px; border-radius: var(--r); border: 1px solid var(--line); background: var(--bg); color: var(--tx); padding: 0 var(--s3); font: 13px/20px var(--font); outline: none; }
  .num input:focus { border-color: var(--ac); }
</style>
